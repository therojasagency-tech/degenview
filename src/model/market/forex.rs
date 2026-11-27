//! Conventional FX pip units, not broker-specific minimum tick sizes.
use crate::model::DataSource;

pub fn pip_size(source: DataSource,symbol: &str)->Option<f64> {
    if !matches!(source,DataSource::OpenChart|DataSource::Oanda) { return None; }
    let raw = symbol.rsplit(':').next()?.to_ascii_uppercase();
    let pair: String = raw.chars().filter(|c| !matches!(c,'_'|'/'|'-')).collect();
    if pair.len()!=6 || !pair.is_ascii() { return None; }
    let (base,quote)=pair.split_at(3);
    const CURRENCIES: &[&str]=&["USD","EUR","GBP","JPY","CHF","CAD","AUD","NZD","SEK","NOK","DKK","PLN","CZK","HUF","SGD","HKD","CNH","CNY","MXN","ZAR","TRY","ILS","THB"];
    if base==quote || !CURRENCIES.contains(&base) || !CURRENCIES.contains(&quote) { return None; }
    Some(if quote=="JPY" {0.01} else {0.0001})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_recognized_currency_pairs_get_fx_units() {
        assert_eq!(pip_size(DataSource::OpenChart,"PEPPERSTONE:EURUSD"),Some(0.0001));
        assert_eq!(pip_size(DataSource::Oanda,"USD_JPY"),Some(0.01));
        assert_eq!(pip_size(DataSource::OpenChart,"NASDAQ:GOOGL"),None);
        assert_eq!(pip_size(DataSource::OpenChart,"OANDA:XAUUSD"),None);
        assert_eq!(pip_size(DataSource::Binance,"EURUSD"),None);
    }
}
