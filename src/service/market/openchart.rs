//! Read-only market data from a separately launched, authorized local OpenChart bridge.
use crate::model::{DataSource, Kline, Timeframe, TimeUnit};
use crate::service::{MarketSource, ProviderError, SearchResult};
use crate::service::net::{config_builder_for, url::encode_component};
use serde::Deserialize;
use std::time::Duration;

pub struct OpenChart;
pub const OPENCHART: OpenChart = OpenChart;

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
    let port = std::env::var("DEGENVIEW_BRIDGE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0);
    let token = std::env::var("DEGENVIEW_BRIDGE_TOKEN")
        .ok()
        .filter(|token| token.len() >= 32 && token.bytes().all(|b| b.is_ascii_graphic()));
    let (Some(port), Some(token)) = (port, token) else {
        return Err(ProviderError::other("Start the local OpenChart bridge before opening its charts."));
    };
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
        || history.resolution != token || history.adjustment != "raw" {
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

impl MarketSource for OpenChart {
    fn fetch_klines(&self, symbol: &str, timeframe: Timeframe, limit: u32) -> Result<Vec<Kline>, ProviderError> {
        let token = resolution(timeframe)?;
        if limit == 0 { return Ok(Vec::new()); }
        let symbol_arg = encode_component(symbol);
        let count = limit.min(5000);
        let history = get(&format!("/v1/history?symbol={symbol_arg}&resolution={token}&count={count}"))?;
        decode_history(history, symbol, timeframe, token)
    }
    fn fetch_klines_range(&self, symbol: &str, timeframe: Timeframe, start_ms: i64, end_ms: i64, max: usize) -> Result<Vec<Kline>, ProviderError> {
        let token = resolution(timeframe)?;
        if max == 0 || start_ms >= end_ms || end_ms <= 0 { return Ok(Vec::new()); }
        let start_ms = start_ms.max(0);
        let symbol_arg = encode_component(symbol);
        let history = get(&format!("/v1/history?symbol={symbol_arg}&resolution={token}&from={start_ms}&to={end_ms}"))?;
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
    fn native_interval_tokens_preserve_case() {
        assert_eq!(resolution(Timeframe::MONTH_1).unwrap(), "1M");
        assert_eq!(resolution(Timeframe::WEEK_1).unwrap(), "1W");
        assert!(resolution(Timeframe::parse("3h").unwrap()).is_err());
    }

    #[test]
    fn candle_open_timestamp_and_prices_stay_native() {
        let history: History = serde_json::from_str(r#"{"provider":"tradingview","symbol":"OANDA:EURUSD","resolution":"1m","adjustment":"raw","bars":[{"time":1767623400000,"open":1.1,"high":1.2,"low":1.0,"close":1.15,"volume":null}]}"#).unwrap();
        let bars = decode_history(history, "OANDA:EURUSD", Timeframe::MINUTE_1, "1m").unwrap();
        assert_eq!(bars[0].open_time, 1767623400000);
        assert_eq!(bars[0].close, 1.15);
        assert!(!DataSource::OpenChart.provides_volume());
        assert!(!DataSource::OpenChart.counts_down_bars());
    }
}
