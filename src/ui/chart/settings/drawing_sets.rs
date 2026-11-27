//! Independent saved drawing sets. Loading replaces only drawings; adding makes copies.
use super::ChartSettingsContext;
use crate::i18n::tr;
use crate::model::{Drawing, InstrumentId};
use crate::model::chart::drawing_sets::{DrawingSet, KEY, MAX_SETS};
use crate::model::chart::template::clean_name;
use crate::store::SharedStore;
use eframe::egui::Ui;

#[derive(Default)]
pub(super) struct Controls {
    current: Vec<Drawing>,
    name: String,
    rows: Option<Result<Vec<DrawingSet>, String>>,
    notice: Option<String>,
    replacing: Option<u64>,
    deleting: Option<u64>,
}

impl Controls {
    pub(super) fn set_current(&mut self, drawings: &[Drawing]) {
        self.current = drawings.iter().filter(|d| !d.is_ruler()).cloned().collect();
    }
    pub(super) fn refresh(&mut self) {
        self.rows = None;
        self.replacing = None;
        self.deleting = None;
    }
}

fn read(store: &SharedStore) -> Result<Vec<DrawingSet>, String> {
    crate::lock::lock(store).setting_strict(KEY).map(|v| v.unwrap_or_default()).map_err(|e| e.to_string())
}

fn change(store: &SharedStore, edit: impl FnOnce(&mut Vec<DrawingSet>) -> Result<(), String>) -> Result<Vec<DrawingSet>, String> {
    let store = crate::lock::lock(store);
    let mut rows: Vec<DrawingSet> = store.setting_strict(KEY).map_err(|e| e.to_string())?.unwrap_or_default();
    edit(&mut rows)?;
    store.set_setting(KEY, &rows).map_err(|e| e.to_string())?;
    Ok(rows)
}

pub(super) fn show(ui: &mut Ui, controls: &mut Controls, c: &ChartSettingsContext) -> Option<(u64,bool)> {
    let market = InstrumentId::new(c.source,c.symbol).key();
    if controls.rows.is_none() { controls.rows = Some(read(c.store)); }
    ui.heading(tr!("Drawing sets"));
    ui.label(tr!("Save or switch drawings for this market without changing indicators."));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut controls.name);
        if ui.button(tr!("Save current drawings")).clicked() {
            let result = change(c.store, |rows| {
                let name = clean_name(&controls.name).map_err(|e| e.message())?;
                if rows.len() >= MAX_SETS { return Err(tr!("Too many drawing sets.").to_owned()); }
                if rows.iter().any(|r| r.market == market && r.name.to_lowercase() == name.to_lowercase()) {
                    return Err(tr!("A template with that name already exists.").to_owned());
                }
                let id = rows.iter().map(|r| r.id).max().unwrap_or(0).checked_add(1)
                    .ok_or_else(|| tr!("No more template identifiers are available.").to_owned())?;
                rows.push(DrawingSet { id, name, market: market.clone(), drawings: controls.current.clone() });
                Ok(())
            });
            match result {
                Ok(rows) => {
                    controls.rows = Some(Ok(rows));
                    controls.name.clear();
                    controls.notice = Some(tr!("Drawing set saved.").to_owned());
                }
                Err(e) => controls.notice = Some(e),
            }
        }
        if ui.button(tr!("Refresh")).clicked() { controls.refresh(); }
    });
    let mut apply = None;
    let mut remove = None;
    if let Some(rows) = &controls.rows {
        match rows {
            Ok(rows) => {
                if !rows.iter().any(|r| r.market == market) { ui.label(tr!("None saved yet")); }
                for row in rows.iter().filter(|r| r.market == market) {
                    ui.push_id(row.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(&row.name);
                            ui.label(format!("({})", row.drawings.len()));
                            if ui.button(tr!("Load drawing set")).clicked() { controls.replacing = Some(row.id); }
                            if ui.button(tr!("Add drawing set")).clicked() { apply = Some((row.id,true)); }
                            if controls.deleting == Some(row.id) {
                                if ui.button(tr!("Confirm deletion")).clicked() { remove = Some(row.id); }
                                if ui.button(tr!("Cancel")).clicked() { controls.deleting = None; }
                            } else if ui.button(tr!("Delete")).clicked() { controls.deleting = Some(row.id); }
                        });
                        if controls.replacing == Some(row.id) {
                            ui.label(tr!("Replace current drawings? A recovery copy will be saved first."));
                            ui.horizontal(|ui| {
                                if ui.button(tr!("Load drawing set")).clicked() { apply = Some((row.id,false)); controls.replacing = None; }
                                if ui.button(tr!("Cancel")).clicked() { controls.replacing = None; }
                            });
                        }
                    });
                }
            }
            Err(e) => { ui.label(e); }
        }
    }
    if let Some(id) = remove {
        match change(c.store, |rows| { rows.retain(|r| r.id != id); Ok(()) }) {
            Ok(rows) => { controls.rows = Some(Ok(rows)); controls.notice = Some(tr!("Drawing set deleted.").to_owned()); }
            Err(e) => controls.notice = Some(e),
        }
        controls.deleting = None;
        controls.replacing = None;
    }
    if let Some(notice) = &controls.notice { ui.label(notice); }
    apply
}
