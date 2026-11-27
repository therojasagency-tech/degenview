//! The countdown to the close of the forming bar, written under the last price on the axis.
//!
//! Pure: it is given the time, never reads it, so a frame, a snapshot and a test agree on one
//! answer. The close is read off the bucket grid every source cuts its candles on, from the clock
//! and not from the newest candle, so the count rolls over to the next bar on its own while the
//! next candle is still on its way and no candle is made up for it. What the countdown is for and
//! what it shows was checked on TradingView; what was not is listed in `docs/chart-canvas.md`.

use crate::i18n::tr;
use crate::model::chart::kind::ChartType;
use crate::model::market::bucket::{bucket_start_ms, next_bucket_start_ms};
use crate::model::{DataSource, Kline, Timeframe};
use std::time::Duration;

#[cfg(test)]
mod tests;

const SECOND_MS: i64 = 1_000;
const MINUTE_MS: i64 = 60 * SECOND_MS;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

/// How many refresh intervals may pass without an answer before the chart's data is too old to
/// say a bar is still forming. The refresh interval is the source's own, so a quiet market that
/// trades once an hour is not mistaken for a dead feed: what counts is when data last arrived,
/// not when the market last traded.
const STALE_AFTER_REFRESHES: i64 = 3;

/// The most the provider's own close and the bucket grid may differ before the grid is not
/// trusted for this market (a stamp the provider rounds to a second is not a disagreement).
const CLOSE_TOLERANCE_MS: i64 = SECOND_MS;

/// Whether a chart of `chart_type` on `source` has bars that close on a clock. A price-based type
/// (Renko, Kagi, Line Break, Point & Figure, Range) closes a bar when the price has moved, which
/// no countdown can promise. Heikin Ashi has one bar per source candle and counts down with it.
/// A source whose candles are not cut on the UTC grid, or are not real-time bars, has no
/// countdown either (see [`DataSource::counts_down_bars`]).
pub fn eligible(chart_type: ChartType, source: DataSource) -> bool {
    chart_type.is_time_based() && source.counts_down_bars()
}

/// What is known of one chart at one instant.
#[derive(Clone, Copy, Debug)]
pub struct CountdownInput<'a> {
    /// The type the chart draws (`ChartStyle::effective_type`).
    pub chart_type: ChartType,
    pub source: DataSource,
    pub timeframe: Timeframe,
    /// The newest candle of the market, which is the forming bar while the market is live.
    pub newest: Option<&'a Kline>,
    pub now_ms: i64,
    /// Milliseconds since data last arrived (a refresh or the live feed); `None` while none has,
    /// as on a first load.
    pub data_age_ms: Option<i64>,
    /// How often the source is asked for candles while it answers.
    pub refresh_ms: i64,
    /// A replay cuts the chart at a past bar: the wall clock says nothing about it.
    pub replay: bool,
}

/// The close of the forming bar and how far off it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Countdown {
    pub close_ms: i64,
    pub remaining_ms: i64,
}

/// Additional providers use native candle openings, not the UTC bucket grid.
pub fn provider_eligible(chart_type: ChartType, source: DataSource, symbol: &str, timeframe: Timeframe) -> bool {
    use crate::model::market::forex::pip_size;
    if !chart_type.is_time_based() { return false; }
    if pip_size(source,symbol).is_some() {
        return timeframe.nominal_ms() <= 12*HOUR_MS || timeframe == Timeframe::DAY_1;
    }
    // Regular US equities only: no guesses for arbitrary indices, CFDs or futures.
    source == DataSource::OpenChart && us_equity(symbol) && timeframe.nominal_ms() <= 30*MINUTE_MS
}

fn us_equity(symbol: &str) -> bool {
    symbol.split_once(':').is_some_and(|(venue,_)|
        matches!(venue.to_ascii_uppercase().as_str(),"NASDAQ"|"NYSE"|"AMEX"|"NYSEARCA"))
}

impl Countdown {
    /// Native session candles have an exact opening stamp but an estimated closing
    /// stamp. Only count down while the latest actual candle is still in its supported
    /// interval; never synthesize a new countdown on an old/closed/delayed feed.
    pub fn resolve_provider(input: &CountdownInput, symbol: &str) -> Option<Self> {
        use crate::model::market::{forex::pip_size,session};
        if input.replay || !provider_eligible(input.chart_type,input.source,symbol,input.timeframe) { return None; }
        if input.data_age_ms?.abs() > input.refresh_ms.saturating_mul(STALE_AFTER_REFRESHES) { return None; }
        let bar = input.newest?;
        let now = input.now_ms;
        let open = bar.open_time;
        let nominal = input.timeframe.nominal_ms();
        if now < open { return None; }
        let close = if pip_size(input.source,symbol).is_some() {
            let day = session::local_day(now);
            let weekday = (day+3).rem_euclid(7); // Mon=0, Fri=4, Sun=6
            let boundary = session::forex_boundary_ms(day);
            if weekday == 5 || (weekday == 4 && now >= boundary) || (weekday == 6 && now < boundary) { return None; }
            let friday = day + (4-weekday).rem_euclid(7);
            let week_close = session::forex_boundary_ms(friday);
            if input.timeframe == Timeframe::DAY_1 {
                let start_day = session::local_day(open);
                if (open-session::forex_boundary_ms(start_day)).abs() > CLOSE_TOLERANCE_MS { return None; }
                session::forex_boundary_ms(start_day+1).min(week_close)
            } else {
                open.checked_add(nominal)?.min(week_close)
            }
        } else {
            let day = session::local_day(now);
            if !session::is_open(now) || open < session::open_ms(day) { return None; }
            open.checked_add(nominal)?.min(session::close_ms(day))
        };
        if now >= close { return None; }
        Some(Self { close_ms: close, remaining_ms: (close-now).min(close-open-1).max(0) })
    }

    /// The countdown to show, or `None` when it would mislead: a chart type or source without
    /// clock-cut bars, a replay, nothing loaded, data that stopped arriving, a provider whose
    /// candles are not on the grid.
    pub fn resolve(input: &CountdownInput) -> Option<Self> {
        if !eligible(input.chart_type, input.source) || input.replay {
            return None;
        }
        let newest = input.newest?;
        // A clock set back makes the last answer look as if it came from the future: as far from
        // now as it is out, it is not to be trusted either way.
        if input.data_age_ms?.abs() > input.refresh_ms * STALE_AFTER_REFRESHES {
            return None;
        }
        let close_ms = next_bucket_start_ms(input.timeframe, input.now_ms);
        // While the newest candle is the forming bar its own close must be where the grid puts
        // it; once the bar has rolled over, the candle is behind and only the clock speaks.
        let forming = bucket_start_ms(input.timeframe, input.now_ms) == newest.open_time;
        if forming && (newest.close_time + 1 - close_ms).abs() > CLOSE_TOLERANCE_MS {
            return None;
        }
        // A bar that has just opened has just under its length left, so the text of a bar of an
        // hour never reads `01:00:00` for the instant it opens in: the form a bar passes through
        // depends on its length alone.
        let opened = bucket_start_ms(input.timeframe, input.now_ms);
        Some(Self {
            close_ms,
            remaining_ms: (close_ms - input.now_ms).min(close_ms - opened - 1),
        })
    }

    /// What the axis tag says.
    pub fn text(&self) -> String {
        format(self.remaining_ms)
    }

    /// How long until the text changes, for `request_repaint_after`.
    pub fn next_repaint(&self) -> Duration {
        next_repaint(self.remaining_ms)
    }
}

/// Whole hours left, rounded up. `remaining_ms` is not negative.
fn hours_left(remaining_ms: i64) -> i64 {
    remaining_ms / HOUR_MS + i64::from(remaining_ms % HOUR_MS != 0)
}

/// `MM:SS` under an hour, `HH:MM:SS` under a day and `2d 14h` from a day on, as TradingView
/// writes them. Seconds count down to `00:00`; the hours of the long form round up, so a week
/// bar with 4 days and 2.6 hours left reads `4d 3h`. Never negative.
pub fn format(remaining_ms: i64) -> String {
    let remaining = remaining_ms.max(0);
    if remaining >= DAY_MS {
        let hours = hours_left(remaining);
        return tr!("{days}d {hours}h", days = hours / 24, hours = hours % 24);
    }
    let seconds = remaining / SECOND_MS;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// How long until [`format`] gives another text: the next second, or the next hour once only
/// days and hours show. Never zero, so a repaint is not requested in a loop.
pub fn next_repaint(remaining_ms: i64) -> Duration {
    let remaining = remaining_ms.max(0);
    let until = if remaining >= DAY_MS {
        // The text changes when the hours left, rounded up, drop by one, or sooner, when the
        // bar has less than a day left and the form changes.
        (remaining - (hours_left(remaining) - 1) * HOUR_MS).min(remaining - (DAY_MS - 1))
    } else {
        remaining % SECOND_MS + 1
    };
    Duration::from_millis(until.max(1) as u64)
}

/// The texts [`format`] gives at their widest for a bar of `timeframe`, which the axis is measured
/// with so its width does not change as the digits do. A bar always has less than its length left
/// ([`Countdown::resolve`]), so a bar of an hour reads `59:59` at most, one of a day `23:59:59`,
/// and a longer one `6d 23h` as well.
pub fn widest_texts(timeframe: Timeframe) -> Vec<String> {
    let nominal = timeframe.nominal_ms();
    if nominal > DAY_MS {
        vec![
            format(DAY_MS - 1),
            format((nominal / DAY_MS - 1) * DAY_MS + 23 * HOUR_MS),
        ]
    } else if nominal > HOUR_MS {
        vec![format(DAY_MS - 1)]
    } else {
        vec![format(HOUR_MS - 1)]
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::format::parse_timestamp;
    fn ts(s: &str)->i64 { parse_timestamp(s).unwrap() }
    fn bar(open: i64,tf:Timeframe)->Kline { Kline { open_time:open, close_time:open+tf.nominal_ms()-1, open:1.1,high:1.1,low:1.1,close:1.1,volume:0.0 } }
    fn input(source:DataSource,tf:Timeframe,b:&Kline,now:i64)->CountdownInput<'_> {
        CountdownInput { chart_type:ChartType::Candles,source,timeframe:tf,newest:Some(b),now_ms:now,data_age_ms:Some(0),refresh_ms:5000,replay:false }
    }
    #[test]
    fn forex_intraday_counts_only_a_current_native_bar() {
        let open=ts("2026-11-27 13:00");
        let b=bar(open,Timeframe::MINUTE_15);
        for (source,symbol) in [(DataSource::OpenChart,"PEPPERSTONE:EURUSD"),(DataSource::Oanda,"EUR_USD")] {
            let mut i=input(source,Timeframe::MINUTE_15,&b,open+600_000);
            assert_eq!(Countdown::resolve_provider(&i,symbol).unwrap().remaining_ms,300_000);
            i.now_ms=open+900_000;
            assert!(Countdown::resolve_provider(&i,symbol).is_none());
        }
    }
    #[test]
    fn forex_friday_close_and_sunday_open_respect_new_york_dst() {
        let b=bar(ts("2026-07-10 20:30"),Timeframe::HOUR_1);
        let mut i=input(DataSource::Oanda,Timeframe::HOUR_1,&b,ts("2026-07-10 20:45"));
        assert_eq!(Countdown::resolve_provider(&i,"USD_JPY").unwrap().close_ms,ts("2026-07-10 21:00"));
        i.now_ms=ts("2026-07-10 21:01");
        assert!(Countdown::resolve_provider(&i,"USD_JPY").is_none());
        let b=bar(ts("2026-11-29 22:00"),Timeframe::DAY_1);
        let i=input(DataSource::Oanda,Timeframe::DAY_1,&b,ts("2026-11-29 22:15"));
        assert_eq!(Countdown::resolve_provider(&i,"EUR_USD").unwrap().close_ms,ts("2026-11-30 22:00"));
    }
    #[test]
    fn stock_countdown_needs_current_regular_session_and_fresh_data() {
        let b=bar(ts("2026-11-25 15:00"),Timeframe::MINUTE_15);
        let mut i=input(DataSource::OpenChart,Timeframe::MINUTE_15,&b,ts("2026-11-25 15:10"));
        assert_eq!(Countdown::resolve_provider(&i,"NASDAQ:AAPL").unwrap().remaining_ms,300_000);
        i.data_age_ms=Some(20_000);
        assert!(Countdown::resolve_provider(&i,"NASDAQ:AAPL").is_none());
        assert!(!provider_eligible(ChartType::Candles,DataSource::OpenChart,"CAPITALCOM:DXY",Timeframe::MINUTE_15));
    }
}
