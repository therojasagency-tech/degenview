//! Read-only market data from a separately launched, authorized local OpenChart bridge.
use crate::model::{DataSource, Kline, Timeframe, TimeUnit};
use crate::service::{MarketSource, ProviderError, SearchResult};
use crate::service::net::{config_builder_for, url::encode_component};
use serde::Deserialize;
use std::time::Duration;

pub struct OpenChart;
pub const OPENCHART: OpenChart = OpenChart;

/// One stable price basis for history, paging and live data. The provider applies
/// stock splits where applicable; dividends are not included. Never silently fall back
/// to raw candles, which could mix incompatible prices in a single chart.
pub(super) const ADJUSTMENT: &str = "split";

#[derive(Deserialize)]
struct Search {
    provider: String,
    results: Vec<Symbol>,
}

#[derive(Deserialize)]
struct Symbol {
    id: String,
    exchange: String,
    symbol: String,
    name: String,
    #[serde(rename = "type", default)]
    market_type: String,
}

#[derive(Deserialize)]
struct History {
    provider: String,
    symbol: String,
    resolution: String,
    adjustment: String,
    bars: Vec<Bar>,
}

#[derive(Deserialize)]
pub(super) struct Bar {
    time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: Option<f64>,
}

fn get<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, ProviderError> {
    let (port, token) = super::openchart_manager::endpoint()?;
    let url = format!("http://127.0.0.1:{port}{path}");
    let agent = ureq::Agent::new_with_config(
        config_builder_for(&url)
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(45)))
            .build(),
    );
    let mut response = agent.get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .map_err(|_| ProviderError::other("OpenChart bridge request failed."))?;
    if response.status().as_u16() != 200 {
        return Err(ProviderError::other("OpenChart bridge rejected the request."));
    }
    let body = response.body_mut().with_config().limit(8 * 1024 * 1024)
        .read_to_string().map_err(ProviderError::data)?;
    serde_json::from_str(&body).map_err(ProviderError::data)
}

pub(super) fn resolution(tf: Timeframe) -> Result<&'static str, ProviderError> {
    let token = match tf {
        Timeframe::SECOND_1 => "1s",
        Timeframe::MINUTE_1 => "1m",
        Timeframe::MINUTE_5 => "5m",
        Timeframe::MINUTE_15 => "15m",
        Timeframe::MINUTE_30 => "30m",
        Timeframe::HOUR_1 => "1h",
        Timeframe::HOUR_4 => "4h",
        Timeframe::DAY_1 => "1d",
        Timeframe::WEEK_1 => "1W",
        Timeframe::MONTH_1 => "1M",
        _ => return Err(ProviderError::unsupported(DataSource::OpenChart, tf)),
    };
    Ok(token)
}

fn decode_history(history: History, symbol: &str, tf: Timeframe, token: &str) -> Result<Vec<Kline>, ProviderError> {
    if history.provider != "tradingview" || history.symbol != symbol
        || history.resolution != token || history.adjustment != ADJUSTMENT {
        return Err(ProviderError::data("OpenChart response identity mismatch"));
    }
    let bars = history.bars.into_iter().map(|bar| decode_bar(bar, tf)).collect::<Result<Vec<_>, _>>()?;
    Ok(super::candles::normalize(bars))
}

pub(super) fn decode_bar(bar: Bar, tf: Timeframe) -> Result<Kline, ProviderError> {
        if bar.time <= 0 || bar.time > 253_402_214_399_999
            || bar.high < bar.open.max(bar.close)
            || bar.low > bar.open.min(bar.close)
            || bar.volume.is_some_and(|v| v < 0.0) {
            return Err(ProviderError::data("Invalid OpenChart candle"));
        }
        // The bridge supplies the exact open time but no close time. Use a nominal display
        // duration until per-symbol session metadata is available; countdowns are disabled.
        let close_time = if tf.unit() == TimeUnit::Month {
            let offset = bar.time.rem_euclid(86_400_000);
            crate::model::market::bucket::next_bucket_start_ms(tf, bar.time) + offset - 1
        } else {
            bar.time + tf.nominal_ms() - 1
        };
        Ok(Kline {
            open_time: bar.time, close_time, open: bar.open, high: bar.high, low: bar.low,
            close: bar.close, volume: bar.volume.unwrap_or(0.0),
        })
}

/// A daily snapshot, not a tick quote. A closed market's last two sessions are valid too.
pub fn watchlist_quote(symbol: &str) -> Result<crate::model::watchlist::quote::SourceQuote, ProviderError> {
    let bars = OPENCHART.fetch_klines(symbol, Timeframe::DAY_1, 2)?;
    daily_quote(&bars)
}
fn daily_quote(bars: &[Kline]) -> Result<crate::model::watchlist::quote::SourceQuote, ProviderError> {
    let (last, previous) = bars.split_last().ok_or_else(|| ProviderError::other("No daily bars returned."))?;
    if !last.close.is_finite() { return Err(ProviderError::data("Invalid daily close")); }
    Ok(crate::model::watchlist::quote::SourceQuote {
        price: last.close,
        previous_day_price: previous.last().filter(|bar| bar.open_time < last.open_time).map(|bar| bar.close),
        // No tick timestamp is supplied. The book records the local retrieval time.
        timestamp_ms: None,
        ..Default::default()
    })
}

impl MarketSource for OpenChart {
    fn fetch_klines(&self, symbol: &str, timeframe: Timeframe, limit: u32) -> Result<Vec<Kline>, ProviderError> {
        let token = resolution(timeframe)?;
        if limit == 0 { return Ok(Vec::new()); }
        let symbol_arg = encode_component(symbol);
        let count = limit.min(5000);
        let history = get(&format!("/v1/history?symbol={symbol_arg}&resolution={token}&count={count}&adjustment={ADJUSTMENT}"))?;
        decode_history(history, symbol, timeframe, token)
    }
    fn fetch_klines_range(&self, symbol: &str, timeframe: Timeframe, start_ms: i64, end_ms: i64, max: usize) -> Result<Vec<Kline>, ProviderError> {
        let token = resolution(timeframe)?;
        if max == 0 || start_ms >= end_ms || end_ms <= 0 { return Ok(Vec::new()); }
        let start_ms = start_ms.max(0);
        let symbol_arg = encode_component(symbol);
        let history = get(&format!("/v1/history?symbol={symbol_arg}&resolution={token}&from={start_ms}&to={end_ms}&adjustment={ADJUSTMENT}"))?;
        let mut bars = decode_history(history, symbol, timeframe, token)?;
        bars.retain(|bar| bar.open_time >= start_ms && bar.open_time < end_ms);
        if bars.len() > max { bars.drain(..bars.len() - max); }
        Ok(bars)
    }
    fn search_tickers(&self, query: &str) -> Result<Vec<SearchResult>, ProviderError> {
        let search: Search = get(&format!("/v1/search?q={}&limit=50", encode_component(query)))?;
        if search.provider != "tradingview" {
            return Err(ProviderError::data("OpenChart search identity mismatch"));
        }
        let mut results = Vec::new();
        for row in search.results {
            if row.id != format!("{}:{}", row.exchange, row.symbol) { continue; }
            super::kind::remember(DataSource::OpenChart, &row.id, super::kind::tradingview(&row.market_type));
            let label = format!("{} — {} · {}", row.symbol, row.exchange, row.name);
            results.push(SearchResult::new(DataSource::OpenChart, label, row.id));
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchlist_uses_last_two_native_sessions() {
        let history: History = serde_json::from_str(r#"{"provider":"tradingview","symbol":"FX:EURUSD","resolution":"1d","adjustment":"split","bars":[{"time":1767537000000,"open":1.1,"high":1.2,"low":1.0,"close":1.1,"volume":null},{"time":1767623400000,"open":1.1,"high":1.2,"low":1.0,"close":1.15,"volume":null}]}"#).unwrap();
        let bars = decode_history(history,"FX:EURUSD",Timeframe::DAY_1,"1d").unwrap();
        let quote = daily_quote(&bars).unwrap();
        assert_eq!(quote.price,1.15);
        assert_eq!(quote.previous_day_price,Some(1.1));
        assert_eq!(quote.timestamp_ms,None);
        assert_eq!(daily_quote(&bars[1..]).unwrap().previous_day_price,None);
        assert!(daily_quote(&[]).is_err());
    }

    #[test]
    fn history_rejects_a_different_price_basis() {
        let fixture = r#"{"provider":"tradingview","symbol":"NASDAQ:AAPL","resolution":"1d","adjustment":"split","bars":[{"time":1598832000000,"open":125.0,"high":130.0,"low":120.0,"close":128.0,"volume":1000.0}]}"#;
        let adjusted = serde_json::from_str(fixture).unwrap();
        assert_eq!(decode_history(adjusted, "NASDAQ:AAPL", Timeframe::DAY_1, "1d").unwrap()[0].close, 128.0);
        let raw = serde_json::from_str(&fixture.replace("\"split\"", "\"raw\"")).unwrap();
        assert!(decode_history(raw, "NASDAQ:AAPL", Timeframe::DAY_1, "1d").is_err());
    }

    #[test]
    fn native_interval_tokens_preserve_case() {
        assert_eq!(resolution(Timeframe::MONTH_1).unwrap(), "1M");
        assert_eq!(resolution(Timeframe::WEEK_1).unwrap(), "1W");
        assert!(resolution(Timeframe::parse("3h").unwrap()).is_err());
    }

    #[test]
    fn candle_open_timestamp_and_prices_stay_native() {
        let history: History = serde_json::from_str(r#"{"provider":"tradingview","symbol":"OANDA:EURUSD","resolution":"1m","adjustment":"split","bars":[{"time":1767623400000,"open":1.1,"high":1.2,"low":1.0,"close":1.15,"volume":null}]}"#).unwrap();
        let bars = decode_history(history, "OANDA:EURUSD", Timeframe::MINUTE_1, "1m").unwrap();
        assert_eq!(bars[0].open_time, 1767623400000);
        assert_eq!(bars[0].close, 1.15);
        assert!(!DataSource::OpenChart.provides_volume());
        assert!(!DataSource::OpenChart.counts_down_bars());
    }
}
