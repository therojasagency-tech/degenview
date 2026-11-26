//! Official OANDA v20 market-data adapter; GET only, no order endpoints.
use crate::model::{DataSource, Kline, TimeUnit, Timeframe};
use crate::service::{MarketSource, ProviderError, SearchResult};
use crate::service::net::{agent::FollowingAgent, http, url::encode_component};
use serde::Deserialize;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

pub struct Oanda;
pub const OANDA: Oanda = Oanda;
static AGENT: LazyLock<FollowingAgent> = LazyLock::new(|| FollowingAgent::new(|config|
    config.timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false)
));
use crate::service::keychain::oanda::{OandaCredential, OandaStore};
struct Config { host: &'static str, account: String, token: String }
fn host_for(environment: &str) -> Result<&'static str, ProviderError> {
    match environment {
        "live" => Ok("https://api-fxtrade.oanda.com"),
        "practice" => Ok("https://api-fxpractice.oanda.com"),
        _ => Err(ProviderError::other("OANDA environment must be practice or live.")),
    }
}
fn config() -> Result<Config, ProviderError> {
    // Explicit legacy launcher credentials override the locally saved account.
    if std::env::var_os("DEGENVIEW_OANDA_TOKEN").is_none() {
        let saved = OandaStore::shared().read().ok_or_else(|| ProviderError::other("Configure OANDA in Settings."))?;
        return Ok(Config { host: host_for(&saved.environment)?, account: saved.account, token: saved.token });
    }
    let token = std::env::var("DEGENVIEW_OANDA_TOKEN").ok().filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or_else(|| ProviderError::other("Launch DegenView with your OANDA v20 API token."))?;
    let account = std::env::var("DEGENVIEW_OANDA_ACCOUNT").ok().filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'-'))
        .ok_or_else(|| ProviderError::other("OANDA v20 account ID is required."))?;
    let host = match std::env::var("DEGENVIEW_OANDA_ENV").as_deref() {
        Ok("live") => "https://api-fxtrade.oanda.com",
        Ok("practice") | Err(_) => "https://api-fxpractice.oanda.com",
        _ => return Err(ProviderError::other("OANDA environment must be practice or live.")),
    };
    Ok(Config { host, account, token })
}
fn get<T: serde::de::DeserializeOwned>(cfg: &Config, path: &str) -> Result<T, ProviderError> {
    let url = format!("{}/v3/accounts/{}{}", cfg.host, cfg.account, path);
    request(&url, &cfg.token)
}
fn request<T: serde::de::DeserializeOwned>(url: &str, token: &str) -> Result<T, ProviderError> {
    let agent = AGENT.get_for(url);
    let mut response = agent.get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept-Datetime-Format", "UNIX")
        .call().map_err(|_| ProviderError::from(http::Error::Network("OANDA request failed".into())))?;
    let status = response.status().as_u16();
    if status != 200 { return Err(ProviderError::from(http::Error::Status(status))); }
    let body = response.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string()
        .map_err(ProviderError::data)?;
    serde_json::from_str(&body).map_err(ProviderError::data)
}

#[derive(Deserialize)]
struct Accounts { accounts: Vec<Account> }
#[derive(Deserialize)]
struct Account { id: String }

pub fn discover_accounts(environment: &str, token: &str) -> Result<Vec<String>, ProviderError> {
    let token = token.trim();
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ProviderError::other("Enter a valid OANDA v20 token."));
    }
    let host = host_for(environment)?;
    let data: Accounts = request(&format!("{host}/v3/accounts"), token)?;
    let mut ids: Vec<_> = data.accounts.into_iter().map(|a| a.id).filter(|id| {
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    }).collect();
    ids.sort(); ids.dedup();
    if ids.is_empty() { return Err(ProviderError::other("No OANDA v20 accounts returned.")); }
    Ok(ids)
}

pub fn connect_account(environment: String, token: String, account: String) -> Result<(), ProviderError> {
    let ids = discover_accounts(&environment, &token)?;
    if !ids.contains(&account) { return Err(ProviderError::other("The selected account is not authorized.")); }
    let cfg = Config { host: host_for(&environment)?, account: account.clone(), token: token.trim().to_owned() };
    let _: Instruments = get(&cfg, "/instruments")?;
    OandaStore::shared().save(&OandaCredential { environment, account, token: cfg.token })
        .map_err(|_| ProviderError::other("Could not save OANDA credentials securely."))?;
    if let Ok(mut cache) = CATALOG.lock() { *cache = None; }
    Ok(())
}

pub fn disconnect_account() -> Result<(), ProviderError> {
    OandaStore::shared().remove().map_err(|_| ProviderError::other("Could not remove saved OANDA credentials."))?;
    if let Ok(mut cache) = CATALOG.lock() { *cache = None; }
    Ok(())
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Instrument { name: String, display_name: String, display_precision: u8 }
#[derive(Deserialize)]
struct Instruments { instruments: Vec<Instrument> }
type Catalog = (Instant, Arc<Vec<Instrument>>);
static CATALOG: Mutex<Option<Catalog>> = Mutex::new(None);
fn instruments(cfg: &Config) -> Result<Arc<Vec<Instrument>>, ProviderError> {
    let mut held = CATALOG.lock().map_err(|_| ProviderError::other("OANDA catalog unavailable"))?;
    if let Some((when, rows)) = held.as_ref() {
        if when.elapsed() < Duration::from_secs(3600) { return Ok(Arc::clone(rows)); }
    }
    let data: Instruments = get(cfg, "/instruments")?;
    let rows = Arc::new(data.instruments);
    *held = Some((Instant::now(), Arc::clone(&rows)));
    Ok(rows)
}
pub fn display_precision(symbol: &str) -> Option<u8> {
    let held = CATALOG.try_lock().ok()?;
    held.as_ref()?.1.iter().find(|r| r.name.eq_ignore_ascii_case(symbol)).map(|r| r.display_precision.min(12))
}

fn granularity(tf: Timeframe) -> Result<String, ProviderError> {
    if !DataSource::Oanda.natives().contains(&tf) {
        return Err(ProviderError::unsupported(DataSource::Oanda, tf));
    }
    Ok(match tf.unit() {
        TimeUnit::Second => format!("S{}", tf.count()),
        TimeUnit::Minute => format!("M{}", tf.count()),
        TimeUnit::Hour => format!("H{}", tf.count()),
        TimeUnit::Day => "D".into(),
        TimeUnit::Week => "W".into(),
        TimeUnit::Month => "M".into(),
    })
}
#[derive(Deserialize)]
struct History { instrument: String, granularity: String, candles: Vec<Candle> }
#[derive(Deserialize)]
struct Candle { time: String, mid: Mid, volume: u64 }
#[derive(Deserialize)]
struct Mid { o: String, h: String, l: String, c: String }
fn time_ms(text: &str) -> Result<i64, ProviderError> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let seconds: i64 = whole.parse().map_err(ProviderError::data)?;
    if !(0..=253_402_214_399).contains(&seconds) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ProviderError::data("Invalid OANDA UNIX timestamp"));
    }
    let mut ms = 0;
    for (i, digit) in fraction.bytes().take(3).enumerate() {
        ms += i64::from(digit - b'0') * [100, 10, 1][i];
    }
    Ok(seconds * 1000 + ms)
}
fn unix(ms: i64) -> String { format!("{}.{:03}", ms.div_euclid(1000), ms.rem_euclid(1000)) }
fn decode(data: History, symbol: &str, gran: &str, tf: Timeframe) -> Result<Vec<Kline>, ProviderError> {
    if data.instrument != symbol || data.granularity != gran {
        return Err(ProviderError::data("OANDA candle response identity mismatch"));
    }
    data.candles.into_iter().map(|c| {
        let open_time = time_ms(&c.time)?;
        let price = |s: &str| -> Result<f64, ProviderError> {
            let value: f64 = s.parse().map_err(ProviderError::data)?;
            if !value.is_finite() { return Err(ProviderError::data("Non-finite OANDA price")); }
            Ok(value)
        };
        let (open, high, low, close) = (price(&c.mid.o)?, price(&c.mid.h)?, price(&c.mid.l)?, price(&c.mid.c)?);
        if high < open.max(close) || low > open.min(close) { return Err(ProviderError::data("Invalid OANDA OHLC")); }
        // The opening timestamp is exact. Session close metadata is not yet wired to the chart.
        let close_time = if tf.unit() == TimeUnit::Month {
            crate::model::market::bucket::next_bucket_start_ms(tf, open_time) + open_time.rem_euclid(86_400_000) - 1
        } else { open_time + tf.nominal_ms() - 1 };
        Ok(Kline { open_time, close_time, open, high, low, close, volume: c.volume as f64 })
    }).collect()
}
fn history(symbol: &str, tf: Timeframe, count: usize, end: Option<i64>) -> Result<Vec<Kline>, ProviderError> {
    let gran = granularity(tf)?;
    if count == 0 { return Ok(Vec::new()); }
    let cfg = config()?;
    let symbol = symbol.to_ascii_uppercase();
    if !symbol.contains('_') || !symbol.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(ProviderError::other("OANDA requires an instrument such as EUR_USD."));
    }
    let count = count.min(5000);
    let mut path = format!("/instruments/{}/candles?price=M&smooth=false&granularity={gran}&count={count}&dailyAlignment=17&alignmentTimezone=America%2FNew_York&weeklyAlignment=Friday", encode_component(&symbol));
    if let Some(end) = end { path.push_str(&format!("&to={}", unix(end))); }
    let data = get(&cfg, &path)?;
    decode(data, &symbol, &gran, tf)
}
impl MarketSource for Oanda {
    fn fetch_klines(&self, symbol: &str, tf: Timeframe, limit: u32) -> Result<Vec<Kline>, ProviderError> {
        history(symbol, tf, limit as usize, None)
    }
    fn fetch_klines_range(&self, symbol: &str, tf: Timeframe, start: i64, end: i64, max: usize) -> Result<Vec<Kline>, ProviderError> {
        if start >= end || end <= 0 || max == 0 { return Ok(Vec::new()); }
        let mut rows = history(symbol, tf, max.saturating_add(1), Some(end))?;
        rows.retain(|bar| bar.open_time >= start && bar.open_time < end);
        if rows.len() > max { rows.drain(..rows.len() - max); }
        Ok(rows)
    }
    fn search_tickers(&self, query: &str) -> Result<Vec<SearchResult>, ProviderError> {
        let cfg = config()?;
        let needle = super::search_rank::normalize(query);
        if needle.is_empty() { return Ok(Vec::new()); }
        let rows = instruments(&cfg)?;
        let results = rows.iter().filter(|row| {
            super::search_rank::normalize(&row.name).contains(&needle)
                || super::search_rank::normalize(&row.display_name).contains(&needle)
        }).map(|row| SearchResult::new(DataSource::Oanda, format!("{} — {}", row.name, row.display_name), row.name.clone())).collect();
        Ok(super::search_rank::rank_results(results, query).into_iter().take(50).collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamp_and_granularity() {
        assert_eq!(time_ms("1767623400.123456789").unwrap(), 1767623400123);
        assert_eq!(unix(1767623400123), "1767623400.123");
        assert_eq!(granularity(Timeframe::MINUTE_5).unwrap(), "M5");
        assert_eq!(granularity(Timeframe::MONTH_1).unwrap(), "M");
        assert!(granularity(Timeframe::SECOND_1).is_err());
    }
    #[test]
    fn parse_realistic_candle_fixture() {
        let data: History = serde_json::from_str(r#"{"instrument":"EUR_USD","granularity":"M5","candles":[{"time":"1767623400.000000000","mid":{"o":"1.10225","h":"1.10300","l":"1.10100","c":"1.10250"},"volume":42,"complete":true}]}"#).unwrap();
        let rows = decode(data, "EUR_USD", "M5", Timeframe::MINUTE_5).unwrap();
        assert_eq!(rows[0].open_time, 1767623400000);
        assert_eq!(rows[0].close, 1.10250);
        assert_eq!(rows[0].volume, 42.0);
    }
}
