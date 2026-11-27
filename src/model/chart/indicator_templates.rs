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
    /// Replace only the saved indicator configuration; preserve the template's identity.
    pub fn update_from_chart(&mut self, indicators: IndicatorConfig, scripts: &[PineInstance]) {
        self.indicators = indicators;
        self.scripts = scripts.to_vec();
    }

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
    fn updating_a_template_keeps_its_identity_and_replaces_the_entire_set() {
        let old = PineInstance::new(1, None, "Old", "indicator(\"Old\")\nplot(close)");
        let mut new = PineInstance::new(2, None, "New", "indicator(\"New\")\nplot(open)");
        new.visible = false;
        new.pane.share = Some(0.25);
        let mut config = IndicatorConfig::default();
        config.show_ema = !config.show_ema;
        let mut template = IndicatorTemplate {
            id: 7, name: "Sentiment".into(), indicators: IndicatorConfig::default(),
            scripts: vec![old.clone(), old],
        };
        template.update_from_chart(config, &[new.clone()]);
        assert_eq!(template.id, 7);
        assert_eq!(template.name, "Sentiment");
        assert_eq!(template.indicators, config);
        assert_eq!(template.scripts, vec![new]);
        let decoded: IndicatorTemplate = serde_json::from_str(&serde_json::to_string(&template).unwrap()).unwrap();
        assert_eq!(decoded, template);
        template.update_from_chart(IndicatorConfig::default(), &[]);
        assert!(template.scripts.is_empty());
    }

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
