//! Save and select indicator sets without saving chart drawings or appearance.
use super::ChartSettingsContext;
use crate::i18n::tr;
use crate::model::chart::indicator_templates::{IndicatorTemplate, KEY, MAX_TEMPLATES};
use crate::model::chart::template::clean_name;
use crate::store::SharedStore;
use eframe::egui::Ui;

#[derive(Default)]
pub(super) struct Controls {
    name: String,
    rows: Option<Result<Vec<IndicatorTemplate>, String>>,
    notice: Option<String>,
    deleting: Option<u64>,
}

fn read(store: &SharedStore) -> Result<Vec<IndicatorTemplate>, String> {
    crate::lock::lock(store).setting_strict(KEY).map(|v| v.unwrap_or_default()).map_err(|e| e.to_string())
}

fn change(store: &SharedStore, edit: impl FnOnce(&mut Vec<IndicatorTemplate>) -> Result<(), String>) -> Result<Vec<IndicatorTemplate>, String> {
    let store = crate::lock::lock(store);
    let mut rows: Vec<IndicatorTemplate> = store.setting_strict(KEY).map_err(|e| e.to_string())?.unwrap_or_default();
    edit(&mut rows)?;
    store.set_setting(KEY, &rows).map_err(|e| e.to_string())?;
    Ok(rows)
}

pub(super) fn show(ui: &mut Ui, controls: &mut Controls, c: &ChartSettingsContext) -> Option<u64> {
    ui.heading(tr!("Indicator templates"));
    ui.label(tr!("Switch indicators without changing the market or drawings."));
    if controls.rows.is_none() {
        controls.rows = Some(read(c.store));
    }
    let mut apply = None;
    let mut remove = None;
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut controls.name);
        if ui.button(tr!("Save current indicators")).clicked() {
            let result = change(c.store, |rows| {
                let name = clean_name(&controls.name).map_err(|e| e.message())?;
                if rows.len() >= MAX_TEMPLATES {
                    return Err(tr!("Too many indicator templates.").to_owned());
                }
                if rows.iter().any(|t| t.name.to_lowercase() == name.to_lowercase()) {
                    return Err(tr!("A template with that name already exists.").to_owned());
                }
                let id = rows.iter().map(|t| t.id).max().unwrap_or(0).checked_add(1)
                    .ok_or_else(|| tr!("No more template identifiers are available.").to_owned())?;
                rows.push(IndicatorTemplate { id, name, indicators: *c.indicators, scripts: c.card.pine_instances().to_vec() });
                Ok(())
            });
            match result {
                Ok(rows) => {
                    controls.rows = Some(Ok(rows));
                    controls.name.clear();
                    controls.notice = Some(tr!("Indicator template saved.").to_owned());
                }
                Err(error) => controls.notice = Some(error),
            }
        }
        if ui.button(tr!("Refresh")).clicked() {
            controls.rows = Some(read(c.store));
            controls.deleting = None;
        }
    });
    if let Some(rows) = &controls.rows {
        match rows {
            Ok(rows) => {
                if rows.is_empty() { ui.label(tr!("None saved yet")); }
                for row in rows {
                    ui.push_id(row.id, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button(&row.name).clicked() { apply = Some(row.id); }
                            if controls.deleting == Some(row.id) {
                                if ui.button(tr!("Confirm deletion")).clicked() { remove = Some(row.id); }
                                if ui.button(tr!("Cancel")).clicked() { controls.deleting = None; }
                            } else if ui.button(tr!("Delete")).clicked() { controls.deleting = Some(row.id); }
                        });
                    });
                }
            }
            Err(error) => { ui.label(error); }
        }
    }
    if let Some(id) = remove {
        match change(c.store, |rows| {
            rows.retain(|row| row.id != id);
            Ok(())
        }) {
            Ok(rows) => {
                controls.rows = Some(Ok(rows));
                controls.notice = Some(tr!("Indicator template deleted.").to_owned());
            }
            Err(error) => controls.notice = Some(error),
        }
        controls.deleting = None;
    }
    if let Some(notice) = &controls.notice { ui.label(notice); }
    apply
}
