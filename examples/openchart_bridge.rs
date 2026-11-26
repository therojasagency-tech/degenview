// SPDX-License-Identifier: GPL-3.0-only
//! Offline integration probe for the native OpenChart data bridge.
//! This example is intentionally not registered as a GUI market provider yet.

use serde::Deserialize;
use std::time::Duration;

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
#[serde(rename_all = "camelCase")]
struct Bar {
    time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: Option<f64>,
    received_sequence: u64,
}

fn get<T: serde::de::DeserializeOwned>(
    agent: &ureq::Agent,
    port: u16,
    token: &str,
    path: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let mut response = agent
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .call()?;
    if response.status().as_u16() != 200 {
        return Err("unexpected bridge response status".into());
    }
    let body = response.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string()?;
    Ok(serde_json::from_str(&body)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("DEGENVIEW_BRIDGE_FIXTURE_PORT")?.parse()?;
    let token = std::env::var("DEGENVIEW_BRIDGE_FIXTURE_TOKEN")?;
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );

    let search: Search = get(&agent, port, &token, "/v1/search?q=EURUSD")?;
    assert_eq!(search.provider, "tradingview");
    let ids: Vec<_> = search.results.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["OANDA:EURUSD", "FXCM:EURUSD"]);
    for row in search.results {
        assert_eq!(row.id, format!("{}:{}", row.exchange, row.symbol));
    }

    let start = 1_767_623_400_000_i64;
    let end = start + 60_000;
    let path = format!(
        "/v1/history?symbol=OANDA%3AEURUSD&resolution=1m&from={start}&to={end}"
    );
    let history: History = get(&agent, port, &token, &path)?;
    assert_eq!(history.provider, "tradingview");
    assert_eq!(history.symbol, "OANDA:EURUSD");
    assert_eq!(history.resolution, "1m");
    assert_eq!(history.adjustment, "raw");
    assert_eq!(history.bars.len(), 1);
    let bar = &history.bars[0];
    assert_eq!(bar.time, start);
    assert_eq!([bar.open, bar.high, bar.low, bar.close], [1.1, 1.2, 1.0, 1.15]);
    assert_eq!(bar.volume, None);
    assert!(bar.received_sequence > 0);

    println!("Rust + OpenChart HTTP bridge: offline smoke passed");
    Ok(())
}
