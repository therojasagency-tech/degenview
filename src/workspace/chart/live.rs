//! The live candle feeds: one per streamed source on the workspace, folding its candles into the
//! charts.

use crate::model::market::live::LiveMark;
use crate::model::{DataSource, InstrumentId, Kline, Timeframe};
use crate::service::market::live::{LiveFeed, PriceSample};
use crate::workspace::ChartWorkspace;
use eframe::egui;
use std::time::Instant;

impl ChartWorkspace {
    pub(in crate::workspace) fn update_live_feeds(&mut self, ctx: &egui::Context) {
        for source in DataSource::CANDLE_STREAMS {
            self.update_live_feed(source, ctx);
        }
        self.update_live_feed(DataSource::Oanda, ctx);
    }

    fn update_live_feed(&mut self, source: DataSource, ctx: &egui::Context) {
        let mut symbols: Vec<String> = self
            .charts
            .iter()
            .filter(|chart| chart.is_market() && chart.source == source)
            .map(|chart| InstrumentId::new(source, chart.symbol.as_str()).api_symbol())
            .collect();
        // The markets compared on a chart stream too, once however many overlays name them.
        if source != DataSource::Oanda {
            for symbol in self.overlay_hub.streamed_symbols(source) {
                if !symbols.contains(&symbol) { symbols.push(symbol); }
            }
        } else { symbols.sort(); symbols.dedup(); }
        // The stream is of the candle length the source serves the timeframe from (one-minute
        // klines for a 22-minute chart, monthly ones for a quarter); a timeframe it cannot reach
        // has none.
        let base = source
            .plan(self.timeframe)
            .map(|plan| plan.base())
            .filter(|_| !symbols.is_empty())
            // Only native intraday candles with reliable nominal boundaries in this first version.
            .filter(|base| source != DataSource::Oanda || (*base == self.timeframe && base.nominal_ms() <= 3_600_000));
        let wanted = base.map(|base| (symbols, base));
        let changed = match (self.live_feeds.get(&source), &wanted) {
            (Some(feed), Some(wanted)) => feed.signature() != wanted,
            (None, None) => false,
            _ => true,
        };
        if changed {
            let feed = wanted
                .and_then(|(symbols, base)| LiveFeed::connect(source, symbols, base, ctx.clone()));
            match feed {
                Some(feed) => {
                    self.live_feeds.insert(source, feed);
                }
                None => {
                    self.live_feeds.remove(&source);
                }
            }
        }

        if source == DataSource::Oanda {
            let prices = self.live_feeds.get(&source).map(|f| f.inbox.drain_prices()).unwrap_or_default();
            for (symbol, sample) in prices { self.apply_oanda_price(&symbol, sample); }
            return;
        }
        let updates = self
            .live_feeds
            .get(&source)
            .map(|feed| feed.inbox.drain())
            .unwrap_or_default();
        let Some(base) = base else {
            return;
        };
        let now = Instant::now();
        for (symbol, kline) in updates {
            self.overlay_hub
                .apply_live(source, self.timeframe, base, &symbol, &kline, now);
            self.apply_live_kline(source, base, &symbol, kline, now);
        }
    }

    fn apply_oanda_price(&mut self, symbol: &str, sample: PriceSample) {
        let timeframe = self.timeframe;
        let now = crate::clock::now_ms();
        // Do not replay a backlog after sleep or a hidden tab; REST can recover the full bar.
        if sample.time < now - 15_000 || sample.time > now + 5_000 { return; }
        for chart in self.charts.iter_mut().filter(|chart| chart.source == DataSource::Oanda
            && chart.data_timeframe == Some(timeframe) && chart.symbol.eq_ignore_ascii_case(symbol)) {
            if chart.last_data_ms.is_some_and(|received| sample.time < received) { continue; }
            if let Some(last) = chart.candles.last_mut() {
                apply_sample(last, sample);
            }
            // No LiveMark: the next official REST snapshot must be free to replace this
            // sampled update (including its high, low and volume). Never create a candle here.
        }
    }

    /// Every chart of `symbol` on `source` takes the kline of `base` candles, whatever else it
    /// shows. A chart's symbol may be spelled any way the source takes (`BTC`, `btc/usdt`): the
    /// feed names the market as the source does, so they are compared by `api_symbol`.
    fn apply_live_kline(
        &mut self,
        source: DataSource,
        base: Timeframe,
        symbol: &str,
        kline: Kline,
        now: Instant,
    ) {
        let timeframe = self.timeframe;
        for chart in self.charts.iter_mut().filter(|chart| {
            chart.source == source
                && InstrumentId::new(source, chart.symbol.as_str())
                    .api_symbol()
                    .eq_ignore_ascii_case(symbol)
        }) {
            // Do not append a bar of a newly selected timeframe to the previous history.
            if source == DataSource::OpenChart && chart.data_timeframe != Some(timeframe) {
                continue;
            }
            if let Some(open_time) =
                chart
                    .live_fold
                    .apply(timeframe, base, &mut chart.candles, kline.clone())
            {
                chart.live = Some(LiveMark::new(open_time, now));
                chart.note_arrival(timeframe, crate::clock::now_ms());
            }
        }
    }
}

fn apply_sample(last: &mut Kline, sample: PriceSample) -> bool {
    if !sample.price.is_finite() || sample.price <= 0.0
        || sample.time < last.open_time || sample.time > last.close_time { return false; }
    last.high = last.high.max(sample.price);
    last.low = last.low.min(sample.price);
    last.close = sample.price;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::chart::Chart;
    use crate::workspace::test_support::{candle, empty_workspace};

    #[test]
    fn sampled_oanda_prices_never_change_volume_or_create_new_bars() {
        let mut bar = candle(1000,1.0);
        bar.close_time = 1999;
        let volume = bar.volume;
        let open = bar.open;
        assert!(apply_sample(&mut bar,PriceSample{time:1500,price:1.5}));
        assert_eq!(bar.close,1.5);
        assert_eq!(bar.open,open);
        assert_eq!(bar.volume,volume);
        assert!(!apply_sample(&mut bar,PriceSample{time:2000,price:2.0}));
        assert_eq!(bar.close,1.5);
    }
    #[test]
    fn every_chart_of_the_symbol_takes_the_live_bar() {
        let mut workspace = empty_workspace(1);
        for id in 1..=2 {
            let mut chart = Chart::blank(id, "BTCUSDT", DataSource::Binance);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(3, "ETHUSDT", DataSource::Binance);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Binance,
            base,
            "btcusdt",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 1]);
        assert!(workspace.charts[0].live.is_some() && workspace.charts[1].live.is_some());
    }

    /// A 1-minute kline of the bar that holds the clock now.
    fn minute_bar_now() -> Kline {
        let now = crate::clock::now_ms();
        let open = crate::model::market::bucket::bucket_start_ms(Timeframe::MINUTE_1, now);
        let mut bar = candle(open, 1.0);
        bar.close_time = open + 60_000 - 1;
        bar
    }

    #[test]
    fn a_live_bar_alone_gives_a_chart_its_countdown_while_the_history_is_still_coming() {
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::MINUTE_1;
        workspace
            .charts
            .push(Chart::blank(1, "BTCUSDT", DataSource::Binance));
        let now = crate::clock::now_ms();
        assert!(
            workspace.charts[0]
                .bar_countdown(Timeframe::MINUTE_1, false, now)
                .is_none(),
            "nothing has arrived"
        );

        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            minute_bar_now(),
            Instant::now(),
        );
        assert!(
            workspace.charts[0]
                .bar_countdown(Timeframe::MINUTE_1, false, crate::clock::now_ms())
                .is_some(),
            "the socket's bar is a bar of this timeframe, REST or no REST"
        );
    }

    #[test]
    fn a_live_bar_of_a_new_timeframe_counts_down_where_old_candles_do_not() {
        let mut workspace = empty_workspace(1);
        let mut chart = Chart::blank(1, "BTCUSDT", DataSource::Binance);
        // The chart was fetched on 5 minutes; the workspace has moved on to 1 minute.
        chart.candles = vec![candle(1, 1.0)];
        chart.note_arrival(Timeframe::MINUTE_5, crate::clock::now_ms());
        workspace.charts.push(chart);
        workspace.timeframe = Timeframe::MINUTE_1;
        let at = |workspace: &ChartWorkspace| {
            workspace.charts[0].bar_countdown(Timeframe::MINUTE_1, false, crate::clock::now_ms())
        };
        assert!(
            at(&workspace).is_none(),
            "the old candles are not a bar of it"
        );

        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            minute_bar_now(),
            Instant::now(),
        );
        assert!(at(&workspace).is_some());
    }

    #[test]
    fn a_live_bar_the_chart_drops_records_no_arrival() {
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::MINUTE_1;
        let mut chart = Chart::blank(1, "BTCUSDT", DataSource::Binance);
        chart.candles = vec![candle(1_000_000_000_000, 1.0)];
        workspace.charts.push(chart);
        // Older than the newest candle: dropped.
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            candle(1, 1.0),
            Instant::now(),
        );
        assert_eq!(workspace.charts[0].last_data_ms, None);
        assert_eq!(workspace.charts[0].data_timeframe, None);
    }

    #[test]
    fn a_kraken_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "xbt/usd"), (2, "BTC-USD"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Kraken);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETH/USD", DataSource::Kraken);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Kraken,
            base,
            "BTC/USD",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn an_okx_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTC-USDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Okx);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETH-USDT", DataSource::Okx);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Okx,
            base,
            "BTC-USDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_bybit_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTCUSDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Bybit);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETHUSDT", DataSource::Bybit);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Bybit,
            base,
            "BTCUSDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_gate_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTC_USDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Gate);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETH_USDT", DataSource::Gate);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Gate,
            base,
            "BTC_USDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_mexc_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTCUSDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Mexc);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETHUSDT", DataSource::Mexc);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Mexc,
            base,
            "BTCUSDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_late_tick_of_an_older_bar_does_not_unsort_the_chart() {
        let mut workspace = empty_workspace(1);
        let mut chart = Chart::blank(1, "BTCUSDT", DataSource::Binance);
        chart.candles = vec![candle(1, 1.0), candle(2, 2.0)];
        workspace.charts.push(chart);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Binance,
            base,
            "BTCUSDT",
            candle(1, 9.0),
            Instant::now(),
        );

        assert_eq!(
            workspace.charts[0].candles,
            [candle(1, 1.0), candle(2, 2.0)]
        );
        assert!(workspace.charts[0].live.is_none());
    }

    #[test]
    fn every_chart_of_the_symbol_folds_the_base_klines_into_its_own_bucket() {
        let minute = 60_000;
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::parse("5m").unwrap();
        for id in 1..=2 {
            workspace
                .charts
                .push(Chart::blank(id, "BTCUSDT", DataSource::Binance));
        }
        let live = |open: i64, volume: f64| Kline {
            volume,
            ..candle(open, 5.0)
        };

        for kline in [live(0, 1.0), live(0, 2.0), live(minute, 3.0)] {
            workspace.apply_live_kline(
                DataSource::Binance,
                Timeframe::MINUTE_1,
                "BTCUSDT",
                kline,
                Instant::now(),
            );
        }

        for chart in &workspace.charts {
            assert_eq!(chart.candles.len(), 1);
            assert_eq!(
                chart.candles[0].volume, 5.0,
                "each bar once, each chart its own"
            );
            assert_eq!(
                chart.live.map(|mark| mark.guard_for(Instant::now())),
                Some(None)
            );
        }
    }

    #[test]
    fn a_refresh_that_replaces_the_forming_bucket_makes_the_next_live_kline_start_from_it() {
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::parse("5m").unwrap();
        workspace
            .charts
            .push(Chart::blank(1, "BTCUSDT", DataSource::Binance));
        let mut kline = candle(0, 5.0);
        kline.volume = 4.0;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline.clone(),
            Instant::now(),
        );

        // A refresh with no live guard replaces the candle: the socket's account is dropped.
        let chart = &mut workspace.charts[0];
        chart.live = None;
        let (sender, receiver) = std::sync::mpsc::channel();
        let fetched = Kline {
            volume: 10.0,
            close_time: 5 * 60_000 - 1,
            ..candle(0, 5.0)
        };
        sender
            .send(Ok(crate::workspace::chart::Fetched {
                candles: vec![fetched],
                bars: Vec::new(),
                series: Vec::new(),
            }))
            .unwrap();
        chart.inbox = Some(receiver);
        chart.receive_fetch(workspace.timeframe);

        // The bar already counted in the refresh's volume adds only what it trades from here.
        kline.volume = 4.5;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline.clone(),
            Instant::now(),
        );
        assert_eq!(workspace.charts[0].candles[0].volume, 10.0);
        kline.volume = 5.0;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline,
            Instant::now(),
        );
        assert_eq!(workspace.charts[0].candles[0].volume, 10.5);
    }

    #[test]
    fn an_answer_with_the_buckets_base_bars_gives_the_forming_candle_what_the_socket_missed() {
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::parse("5m").unwrap();
        let mut chart = Chart::blank(1, "BTCUSDT", DataSource::Binance);
        // The socket was first: it saw only the bar at minute 3, 8 traded.
        chart.last_fetch = Instant::now();
        workspace.charts.push(chart);
        let mut kline = candle(3 * 60_000, 5.0);
        kline.volume = 8.0;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline,
            Instant::now(),
        );

        // REST answers later with the bucket's first four minutes, the last one cut at 3.
        let bars: Vec<Kline> = (0..4)
            .map(|minute| {
                let mut bar = candle(minute * 60_000, 4.0);
                bar.volume = if minute == 3 { 3.0 } else { 2.0 };
                bar
            })
            .collect();
        let chart = &mut workspace.charts[0];
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(Ok(crate::workspace::chart::Fetched {
                candles: vec![Kline {
                    volume: 9.0,
                    close_time: 5 * 60_000 - 1,
                    ..candle(0, 4.0)
                }],
                bars,
                series: Vec::new(),
            }))
            .unwrap();
        chart.inbox = Some(receiver);
        chart.live = Some(LiveMark::new(0, Instant::now()));
        chart.receive_fetch(workspace.timeframe);

        assert_eq!(workspace.charts[0].candles.len(), 1);
        assert_eq!(
            workspace.charts[0].candles[0].volume, 14.0,
            "three minutes of 2 and the socket's 8"
        );
    }
}
