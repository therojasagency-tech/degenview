//! A reusable set of indicators, independent of market, appearance and drawings.
use crate::model::{IndicatorConfig, pine::instance::PineInstance};
use serde::{Deserialize, Serialize};

pub const KEY: &str = "chart.indicatorTemplates.v1";
pub const MAX_TEMPLATES: usize = 100;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndicatorTemplate {
    pub id: u64,
    pub name: String,
    pub indicators: IndicatorConfig,
    pub scripts: Vec<PineInstance>,
}

impl IndicatorTemplate {
    /// Runtime IDs belong to the target chart, not to the source template.
    pub fn fresh_instances(&self, after: u64) -> Option<Vec<PineInstance>> {
        self.scripts.iter().enumerate().map(|(index, script)| {
            let mut instance = script.clone();
            instance.id = after.checked_add(u64::try_from(index).ok()?.checked_add(1)?)?;
            Some(instance)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_keep_settings_and_order_but_allocate_fresh_ids() {
        let mut script = PineInstance::new(1, None, "Example", "indicator(\"Example\")\nplot(close)");
        script.visible = false;
        script.pane.share = Some(0.3);
        let template = IndicatorTemplate {
            id: 7, name: "Example".to_owned(), indicators: IndicatorConfig::default(),
            scripts: vec![script.clone(), script.clone()],
        };
        let decoded: IndicatorTemplate = serde_json::from_str(&serde_json::to_string(&template).unwrap()).unwrap();
        assert_eq!(decoded, template);
        let instances = decoded.fresh_instances(10).unwrap();
        assert_eq!(instances.iter().map(|i| i.id).collect::<Vec<_>>(), [11, 12]);
        let mut expected = script;
        expected.id = 11;
        assert_eq!(instances[0], expected);
        assert!(decoded.fresh_instances(u64::MAX).is_none());
    }
}
