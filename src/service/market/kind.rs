//! Provider-reported asset class, indexed by exact provider-qualified instrument identity.
//! The bounded in-memory catalog also supplies metadata for recently selected symbols.
use crate::model::DataSource;
use crate::service::SearchResult;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind { Stock, Crypto, Forex, Future, Index, Commodity, Cfd, Other }
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stock => "Stock / ETF", Self::Crypto => "Crypto", Self::Forex => "Forex",
            Self::Future => "Futures", Self::Index => "Index", Self::Commodity => "Commodity",
            Self::Cfd => "CFD", Self::Other => "Market",
        }
    }
}
pub fn tradingview(raw: &str) -> Kind {
    match raw.to_ascii_lowercase().as_str() {
        "stock" | "etf" | "fund" | "dr" => Kind::Stock,
        "crypto" => Kind::Crypto,
        "forex" => Kind::Forex,
        "futures" | "future" => Kind::Future,
        "index" => Kind::Index,
        "commodity" | "metal" => Kind::Commodity,
        "cfd" => Kind::Cfd,
        _ => Kind::Other,
    }
}
pub fn oanda(raw: &str) -> Kind {
    match raw {
        "CURRENCY" => Kind::Forex,
        "METAL" => Kind::Commodity,
        "CFD" => Kind::Cfd,
        _ => Kind::Other,
    }
}
type Key = (DataSource, String);
type Catalog = HashMap<Key, Kind>;
fn catalog() -> &'static Mutex<Catalog> {
    static CATALOG: OnceLock<Mutex<Catalog>> = OnceLock::new();
    CATALOG.get_or_init(Default::default)
}
pub fn remember(source: DataSource, symbol: &str, kind: Kind) {
    let mut map = crate::lock::lock(catalog());
    let key = (source, symbol.to_ascii_uppercase());
    if map.len() >= 8192 && !map.contains_key(&key) { map.clear(); }
    map.insert(key, kind);
}
pub fn of(result: &SearchResult) -> Kind {
    match result.source {
        DataSource::Alpaca => Kind::Stock,
        DataSource::OpenChart | DataSource::Oanda =>
            crate::lock::lock(catalog()).get(&(result.source, result.full_symbol.to_ascii_uppercase())).copied().unwrap_or(Kind::Other),
        DataSource::CoinMarketCap => Kind::Index,
        DataSource::Polymarket | DataSource::Kalshi => Kind::Other,
        _ => Kind::Crypto,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_types_not_symbol_guesses() {
        assert_eq!(tradingview("forex"), Kind::Forex);
        assert_eq!(tradingview("futures"), Kind::Future);
        assert_eq!(oanda("CFD"), Kind::Cfd);
        assert_eq!(oanda("METAL"), Kind::Commodity);
        assert_eq!(tradingview("unreported"), Kind::Other);
        let a = SearchResult::new(DataSource::OpenChart,"Fixture","TEST:EURUSD");
        remember(a.source,&a.full_symbol,Kind::Future);
        assert_eq!(of(&a),Kind::Future);
        assert_eq!(of(&SearchResult::new(DataSource::OpenChart,"Fixture","OTHER:EURUSD")),Kind::Other);
    }
}

pub fn market_precision(source: DataSource, symbol: &str) -> Option<u8> {
    if source == DataSource::Oanda {
        if let Some(precision) = crate::service::oanda_display_precision(symbol) { return Some(precision); }
    }
    if !matches!(source, DataSource::Oanda | DataSource::OpenChart) { return None; }
    let ticker = symbol.split_once(':').map_or(symbol, |(_, s)| s);
    let ticker = ticker.strip_suffix("_TEST").unwrap_or(ticker).replace('_', "").to_ascii_uppercase();
    const FX: &[&str] = &["USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "NOK", "SEK", "DKK", "SGD", "HKD", "PLN", "MXN", "ZAR", "TRY", "CNH", "CZK", "HUF"];
    if ticker.len() != 6 || !ticker.is_ascii() { return None; }
    let (base, quote) = ticker.split_at(3);
    if FX.contains(&base) && FX.contains(&quote) {
        Some(if quote == "JPY" { 3 } else { 5 })
    } else { None }
}
