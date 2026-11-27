//! Named snapshots of one market's drawings, independent of the indicators and chart look.
use crate::i18n::tr;
use crate::model::Drawing;
use serde::{Deserialize, Serialize};

pub const KEY: &str = "chart.drawingSets.v1";
pub const MAX_SETS: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingSet {
    pub id: u64,
    pub name: String,
    pub market: String,
    #[serde(with = "crate::model::drawings::lenient")]
    pub drawings: Vec<Drawing>,
}

impl DrawingSet {
    pub fn fresh(&self, first: u64) -> Result<(Vec<Drawing>, u64), String> {
        let mut next = first;
        let mut out = Vec::new();
        for d in self.drawings.iter().filter(|d| !d.is_ruler()) {
            if matches!(d, Drawing::Unreadable(_)) {
                return Err(tr!("This version cannot safely replace unreadable drawings.").to_owned());
            }
            let id = next;
            next = next.checked_add(1).ok_or_else(|| tr!("No more drawing identifiers are available.").to_owned())?;
            out.push(d.clone().with_id(id));
        }
        Ok((out, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Tool};
    #[test]
    fn applying_a_set_allocates_new_ids_without_changing_its_coordinates() {
        let a = Anchor { time_ms: 1_000, price: 10.0 };
        let b = Anchor { time_ms: 2_000, price: 20.0 };
        let drawing = Drawing::build(Tool::TrendLine, 1, &[a,b], Tool::TrendLine.default_style()).unwrap();
        let set = DrawingSet { id: 1, name: "Weekly".into(), market: "BINANCE:BTCUSDT".into(), drawings: vec![drawing.clone()] };
        let back: DrawingSet = serde_json::from_str(&serde_json::to_string(&set).unwrap()).unwrap();
        assert_eq!(back, set);
        let (drawings, next) = back.fresh(50).unwrap();
        assert_eq!(next, 51);
        assert_eq!(drawings, vec![drawing.with_id(50)]);
        assert!(set.fresh(u64::MAX).is_err());
    }
}
