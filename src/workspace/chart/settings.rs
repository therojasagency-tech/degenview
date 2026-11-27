//! The chart settings window and applying one chart's appearance to the others.

use crate::model::DataSource;
use crate::ui::chart::settings::{self, ChartSettingsAction, ChartSettingsContext};
use crate::workspace::{ChartWorkspace, WorkspaceAction};
use eframe::egui;

impl ChartWorkspace {
    /// The chart settings sheet over the focused window; returns what the workspace's host should
    /// do (opening the Pine Studio).
    pub(in crate::workspace) fn show_settings_window(
        &mut self,
        ctx: &egui::Context,
    ) -> Vec<WorkspaceAction> {
        let Some(chart_id) = self.settings_chart else {
            return Vec::new();
        };
        let Some(chart) = self.charts.iter_mut().find(|chart| chart.id == chart_id) else {
            self.settings_chart = None;
            return Vec::new();
        };
        let title = chart.display_title();
        let outcome = settings::show(
            ctx,
            &mut self.settings_state,
            ChartSettingsContext {
                title: &title,
                symbol: &chart.symbol,
                source: chart.source,
                indicators: &mut chart.indicators,
                card: &mut chart.chart,
                scale_mode: chart.scale_mode,
                store: &self.store,
            },
        );
        let mut actions = Vec::new();
        match outcome {
            Some(ChartSettingsAction::Done) => {
                self.settings_chart = None;
                self.script_popover = None;
                self.overlay_popover = None;
            }
            Some(ChartSettingsAction::RemoveChart) => {
                self.settings_chart = None;
                self.script_popover = None;
                self.overlay_popover = None;
                self.remove_chart(chart_id);
            }
            Some(ChartSettingsAction::OpenPineStudio) => {
                self.settings_chart = None;
                self.script_popover = None;
                self.overlay_popover = None;
                actions.push(WorkspaceAction::OpenPineScripts);
            }
            Some(ChartSettingsAction::ConfigureScript(id, anchor)) => {
                self.toggle_script_popover(chart_id, id, anchor);
            }
            Some(ChartSettingsAction::SetScaleMode(mode)) => self.set_scale_mode(chart_id, mode),
            Some(ChartSettingsAction::Overlay(command)) => {
                self.apply_overlay_command(chart_id, command);
            }
            Some(ChartSettingsAction::AddOverlay) => {
                // The picker is a modal of its own: the sheet gives way to it.
                self.settings_chart = None;
                self.script_popover = None;
                self.overlay_popover = None;
                self.focus_chart(chart_id);
                self.open_compare_picker();
            }
            Some(ChartSettingsAction::ApplyIndicatorTemplate(id)) => {
                match self.apply_indicator_template(chart_id, id) {
                    Ok(name) => {
                        self.script_popover = None;
                        self.settings_state.say(crate::i18n::tr!("Applied indicator template: {name}", name = name));
                    }
                    Err(message) => self.settings_state.say(message),
                }
            }
            Some(ChartSettingsAction::ApplyToAll(scope)) => {
                let appearance = self
                    .charts
                    .iter()
                    .find(|chart| chart.id == chart_id)
                    .map(|chart| chart.chart.appearance());
                if let Some(appearance) = appearance {
                    let message = self.apply_appearance_to_all(chart_id, &appearance, scope);
                    self.settings_state.say(message);
                }
            }
            None => {}
        }
        actions
    }

    /// Loads a frozen set without touching the market, drawings, viewport or appearance.
    fn apply_indicator_template(&mut self, chart_id: u64, id: u64) -> Result<String, String> {
        use crate::i18n::tr;
        use crate::model::chart::indicator_templates::{IndicatorTemplate, KEY};
        use crate::model::alert::pine::PineAlertSubscription;
        use crate::workspace::undo::Snapshot;
        let (template, subscriptions) = {
            let store = crate::lock::lock(&self.store);
            let templates = store.setting_strict::<Vec<IndicatorTemplate>>(KEY)
                .map_err(|e| e.to_string())?.unwrap_or_default();
            let template = templates.into_iter().find(|t| t.id == id)
                .ok_or_else(|| tr!("That template no longer exists.").to_owned())?;
            let subscriptions = store.documents_strict::<PineAlertSubscription>("pine_alert_subscription")
                .map_err(|e| e.to_string())?;
            (template, subscriptions)
        };
        let chart = self.charts.iter_mut().find(|c| c.id == chart_id)
            .ok_or_else(|| tr!("Chart no longer exists.").to_owned())?;
        if !chart.takes_indicators() {
            return Err(tr!("This chart does not support indicators.").to_owned());
        }
        // A bound alert must never be reassigned to a different script or silently paused.
        // Require the user to remove its binding explicitly before replacing this chart's scripts.
        if subscriptions.iter().any(|s| s.chart_id == chart_id && s.instance_id.is_some()) {
            return Err(tr!("This chart has linked Pine alerts. Remove their bindings before replacing its indicators.").to_owned());
        }
        let before = Snapshot::indicators_of(chart);
        let max = chart.chart.pine_instances().iter().map(|i| i.id).max().unwrap_or(0);
        let instances = template.fresh_instances(max)
            .ok_or_else(|| tr!("No more indicator instance identifiers are available.").to_owned())?;
        chart.indicators = template.indicators;
        chart.chart.set_pine_instances(instances);
        self.history.record(chart_id, before);
        Ok(template.name)
    }

    /// Gives every other chart of this workspace `appearance`. The plan is made first, for every
    /// chart, so none is left half-restyled; each chart then takes it through its own card, so a
    /// new chart type resets that chart's scroll and bars the way a pick in its selector does.
    /// Other workspaces, saved layouts and everything else a chart holds are not touched.
    fn apply_appearance_to_all(
        &mut self,
        source_chart: u64,
        appearance: &crate::model::ChartAppearance,
        scope: crate::model::chart::template::ApplyScope,
    ) -> String {
        let markets: Vec<(u64, DataSource)> = self
            .charts
            .iter()
            .filter(|chart| chart.is_market())
            .map(|chart| (chart.id, chart.source))
            .collect();
        let plan = crate::model::chart::template::plan_apply_to_all(
            &markets,
            appearance,
            scope,
            source_chart,
        );
        for (id, step) in &plan.steps {
            if let Some(chart) = self.charts.iter_mut().find(|chart| chart.id == *id) {
                chart.chart.apply_template(appearance, *step);
            }
        }
        plan.message(appearance.style.chart_type.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::InstrumentId;
    use crate::workspace::test_support::empty_workspace;

    #[test]
    fn indicator_template_changes_only_indicators_and_can_be_undone() {
        use crate::model::chart::indicator_templates::{IndicatorTemplate, KEY};
        use crate::model::pine::instance::PineInstance;
        use crate::workspace::test_support::{chart_with_drawings, drawing_ids};
        let mut workspace = empty_workspace(21);
        let chart_id = chart_with_drawings(&mut workspace, "BTCUSDT", &[1, 2]);
        let other_id = chart_with_drawings(&mut workspace, "ETHUSDT", &[3]);
        let old_id = workspace.charts[0].chart.add_pine_instance(None, "Old", "indicator(\"Old\")\nplot(close)");
        let before = workspace.charts[0].chart.pine_instances().to_vec();
        let appearance = workspace.charts[0].chart.appearance();
        let viewport = workspace.charts[0].chart.saved_viewport();
        let mut script = PineInstance::new(old_id, None, "New", "indicator(\"New\")\nplot(close)");
        script.pane.share = Some(0.25);
        let mut config = workspace.charts[0].indicators;
        config.show_ema = !config.show_ema;
        let template = IndicatorTemplate { id: 1, name: "New".into(), indicators: config, scripts: vec![script.clone()] };
        crate::lock::lock(&workspace.store).set_setting(KEY, &vec![template]).unwrap();

        assert_eq!(workspace.apply_indicator_template(chart_id, 1).unwrap(), "New");
        let chart = &workspace.charts[0];
        assert_eq!(chart.symbol, "BTCUSDT");
        assert_eq!(chart.chart.appearance(), appearance);
        assert_eq!(chart.chart.saved_viewport(), viewport);
        assert_eq!(chart.indicators, config);
        assert_eq!(chart.chart.pine_instances()[0].pane.share, Some(0.25));
        assert_ne!(chart.chart.pine_instances()[0].id, old_id);
        assert_eq!(drawing_ids(&workspace, chart_id), [1, 2]);
        assert_eq!(drawing_ids(&workspace, other_id), [3]);
        assert!(workspace.charts[1].chart.pine_instances().is_empty());
        workspace.undo(&egui::Context::default());
        assert_eq!(workspace.charts[0].chart.pine_instances(), before.as_slice());
        assert_eq!(drawing_ids(&workspace, chart_id), [1, 2]);
    }

    #[test]
    fn a_linked_pine_alert_prevents_replacing_its_instance() {
        use crate::model::alert::pine::{PineAlertState, PineAlertSubscription};
        use crate::model::chart::indicator_templates::{IndicatorTemplate, KEY};
        use crate::workspace::test_support::chart_with_drawings;
        let mut workspace = empty_workspace(22);
        let chart_id = chart_with_drawings(&mut workspace, "BTCUSDT", &[1]);
        let instance_id = workspace.charts[0].chart.add_pine_instance(None, "Old", "indicator(\"Old\")\nplot(close)");
        let before = workspace.charts[0].chart.pine_instances().to_vec();
        {
            let store = crate::lock::lock(&workspace.store);
            store.set_setting(KEY, &vec![IndicatorTemplate {
                id: 1, name: "Empty".into(), indicators: Default::default(), scripts: Vec::new()
            }]).unwrap();
            store.replace_pine_subscriptions(&[PineAlertSubscription {
                id: 1, chart_id, instance_id: Some(instance_id), script_id: None,
                script_name: "Old".into(), source: String::new(), inputs: Default::default(),
                symbol_key: "BINANCE:BTCUSDT".into(), timeframe: "1d".into(), source_hash: "test".into(),
                note: String::new(), state: PineAlertState::Active, created_at_ms: 0,
                webhook_endpoint_ids: Vec::new(),
            }]).unwrap();
        }
        assert!(workspace.apply_indicator_template(chart_id, 1).is_err());
        assert_eq!(workspace.charts[0].chart.pine_instances(), before.as_slice());
        assert_eq!(crate::lock::lock(&workspace.store).pine_subscriptions()[0].instance_id, Some(instance_id));
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn apply_to_all_restyles_the_other_charts_of_this_workspace_only() {
        use crate::model::chart::canvas::BackgroundMode;
        use crate::model::chart::template::ApplyScope;
        let ctx = egui::Context::default();
        let mut workspace = empty_workspace(3);
        let mut control = empty_workspace(4);
        let ids: Vec<u64> = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
            .iter()
            .map(|symbol| {
                workspace
                    .add_chart_for(&InstrumentId::new(DataSource::Binance, *symbol), &ctx)
                    .chart_id
            })
            .collect();
        let other = control
            .add_chart_for(&InstrumentId::new(DataSource::Binance, "XRPUSDT"), &ctx)
            .chart_id;
        let untouched = control.charts[0].chart.appearance();
        // The second chart carries a script and a Kagi type of its own.
        workspace.charts[1].chart.add_pine_instance(
            None,
            "RSI",
            "//@version=6\nindicator(\"x\")\nplot(close)\n",
        );
        workspace.charts[1]
            .chart
            .set_chart_type(crate::model::ChartType::Kagi);
        workspace.mark_clean();

        let mut look = crate::model::ChartAppearance::default();
        look.bullish = [9, 8, 7];
        look.canvas.background.mode = BackgroundMode::Gradient;
        look.canvas.margins.right = 21;
        look.canvas.bar_countdown = false;
        look.style.chart_type = crate::model::ChartType::Renko;
        let message = workspace.apply_appearance_to_all(ids[0], &look, ApplyScope::ColorsAndCanvas);
        assert_eq!(message, "Applied the appearance to 2 charts.");
        for chart in &workspace.charts[1..] {
            let appearance = chart.chart.appearance();
            assert_eq!(appearance.bullish, [9, 8, 7]);
            assert_eq!(appearance.canvas, look.canvas);
        }
        // Colours and canvas only: each chart keeps the type it had.
        assert_eq!(
            workspace.charts[1].chart.appearance().style.chart_type,
            crate::model::ChartType::Kagi
        );
        assert_eq!(
            workspace.charts[2].chart.appearance().style.chart_type,
            crate::model::ChartType::Candles
        );
        assert_eq!(
            workspace
                .charts
                .iter()
                .map(|c| c.symbol.as_str())
                .collect::<Vec<_>>(),
            ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
        );
        assert_eq!(workspace.charts[1].chart.pine_instances().len(), 1);
        assert!(workspace.is_dirty(), "the layout notices the new look");
        // The source chart and the other workspace are untouched.
        assert_eq!(
            workspace.charts[0].chart.appearance().bullish,
            crate::model::ChartAppearance::default().bullish
        );
        assert_eq!(control.charts[0].chart.appearance(), untouched);
        assert_eq!(control.charts[0].id, other);

        // With the type too, charts whose market can draw it take it.
        workspace.apply_appearance_to_all(ids[0], &look, ApplyScope::AppearanceAndCanvas);
        assert_eq!(
            workspace.charts[2].chart.appearance().style.chart_type,
            crate::model::ChartType::Renko
        );
        let alone = control.apply_appearance_to_all(other, &look, ApplyScope::AppearanceAndCanvas);
        assert_eq!(alone, "There are no other charts in this workspace.");
    }
}
