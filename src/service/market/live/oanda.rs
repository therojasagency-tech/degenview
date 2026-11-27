//! OANDA's sampled HTTP price stream, not a candle stream.
//! REST remains authoritative for the chart's open, completed candles and volume.
use super::{LiveInbox, PriceSample};
use crate::service::market::oanda::{config, time_ms};
use crate::service::net::{agent::FollowingAgent, url::encode_component, ws::Backoff};
use eframe::egui;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, LazyLock, Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::{Duration, Instant};

static AGENT: LazyLock<FollowingAgent> = LazyLock::new(|| FollowingAgent::new(|c| c
    .timeout_connect(Some(Duration::from_secs(10)))
    .timeout_recv_response(Some(Duration::from_secs(15)))
    .timeout_per_read(Some(Duration::from_secs(12)))
    .http_status_as_error(false)));
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static LAST_CONNECT: Mutex<Option<Instant>> = Mutex::new(None);
const MAX_STREAMS: usize = 4;
struct Slot;
impl Slot {
    fn acquire() -> Option<Self> {
        ACTIVE.try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MAX_STREAMS).then_some(n+1)).ok()?;
        Some(Self)
    }
}
impl Drop for Slot {
    fn drop(&mut self) { ACTIVE.fetch_sub(1, Ordering::AcqRel); }
}
fn connection_slot(shutdown: &AtomicBool) -> bool {
    while !shutdown.load(Ordering::Acquire) {
        {
            let mut last = crate::lock::lock(&LAST_CONNECT);
            if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(600)) {
                *last = Some(Instant::now());
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}
#[derive(Deserialize)]
struct Level { price: String }
#[derive(Deserialize)]
struct Row {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    instrument: String,
    #[serde(default)]
    time: String,
    #[serde(default)]
    bids: Vec<Level>,
    #[serde(default)]
    asks: Vec<Level>,
    tradeable: Option<bool>,
    status: Option<String>,
}
fn parse(line: &[u8]) -> Option<(String, PriceSample)> {
    let row: Row = serde_json::from_slice(line).ok()?;
    if row.kind != "PRICE" || row.instrument.is_empty() || row.tradeable == Some(false)
        || row.status.as_deref().is_some_and(|s| s != "tradeable") { return None; }
    let price = |v: &Level| v.price.parse::<f64>().ok().filter(|p| p.is_finite() && *p > 0.0);
    let bid = row.bids.iter().filter_map(price).reduce(f64::max)?;
    let ask = row.asks.iter().filter_map(price).reduce(f64::min)?;
    if ask < bid { return None; }
    let time = time_ms(&row.time).ok()?;
    Some((row.instrument, PriceSample { time, price: bid + (ask-bid)/2.0 }))
}
fn stream(symbols: &[String], inbox: &LiveInbox, shutdown: &AtomicBool, ctx: &egui::Context, backoff: &mut Backoff) {
    let Ok(cfg) = config() else { return; };
    if !cfg.account.bytes().all(|b| b.is_ascii_digit() || b == b'-') || cfg.account.is_empty() { return; }
    let host = match cfg.host {
        "https://api-fxtrade.oanda.com" => "https://stream-fxtrade.oanda.com",
        "https://api-fxpractice.oanda.com" => "https://stream-fxpractice.oanda.com",
        _ => return,
    };
    let list = symbols.iter().map(|s| encode_component(s)).collect::<Vec<_>>().join("%2C");
    let url = format!("{host}/v3/accounts/{}/pricing/stream?instruments={list}&snapshot=true",cfg.account);
    let Some(_slot) = Slot::acquire() else { return; };
    if !connection_slot(shutdown) { return; }
    let agent = AGENT.get_for(&url);
    let Ok(mut response) = agent.get(&url)
        .header("Authorization",format!("Bearer {}",cfg.token))
        .header("Accept-Datetime-Format","UNIX")
        .call() else { return; };
    if response.status().as_u16() != 200 { return; }
    let mut reader = BufReader::new(response.body_mut().as_reader());
    let mut line = Vec::new();
    while !shutdown.load(Ordering::Acquire) {
        line.clear();
        // Limit the maximum length of a record, not the life of the stream.
        let Ok(n) = reader.by_ref().take(65_537).read_until(b'\n',&mut line) else { break; };
        if n == 0 || n > 65_536 || line.last() != Some(&b'\n') { break; }
        backoff.reset();
        if let Some((symbol, sample)) = parse(&line) {
            if symbols.contains(&symbol) {
                inbox.push_price(symbol,sample);
                ctx.request_repaint();
            }
        }
    }
}
pub(super) fn run(mut symbols: Vec<String>, inbox: Arc<LiveInbox>, shutdown: Arc<AtomicBool>, ctx: egui::Context) {
    symbols.retain(|s| s.contains('_') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    symbols.sort(); symbols.dedup(); symbols.truncate(50);
    if symbols.is_empty() { return; }
    let mut backoff = Backoff::new();
    while !shutdown.load(Ordering::Acquire) {
        stream(&symbols,&inbox,&shutdown,&ctx,&mut backoff);
        backoff.wait(&shutdown);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_use_midpoint_and_ignore_heartbeats_or_crossed_books() {
        let (symbol,sample) = parse(br#"{"type":"PRICE","instrument":"EUR_USD","time":"1767623400.123","tradeable":true,"bids":[{"price":"1.10000"}],"asks":[{"price":"1.10020"}]}"#).unwrap();
        assert_eq!(symbol,"EUR_USD");
        assert_eq!(sample.time,1767623400123);
        assert!((sample.price-1.10010).abs()<1e-10);
        assert!(parse(br#"{"type":"HEARTBEAT","time":"1767623400.123"}"#).is_none());
        assert!(parse(br#"{"type":"PRICE","instrument":"EUR_USD","time":"1767623400.123","bids":[{"price":"1.2"}],"asks":[{"price":"1.1"}]}"#).is_none());
    }
}
