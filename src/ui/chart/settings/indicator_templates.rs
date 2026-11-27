//! Save and select indicator sets without saving chart drawings or appearance.
use super::ChartSettingsContext;
use crate::i18n::tr;
use crate::model::{IndicatorConfig, pine::instance::PineInstance};
use crate::model::chart::indicator_templates::{IndicatorTemplate, KEY, MAX_TEMPLATES};
use crate::model::chart::template::clean_name;
use crate::store::SharedStore;
use crate::ui::{kit, theme};
use eframe::egui::{self, Ui};

#[derive(Default)]
pub(super) struct Controls {
    name: String,
    rows: Option<Result<Vec<IndicatorTemplate>, String>>,
    notice: Option<String>,
    deleting: Option<u64>,
    updating: Option<u64>,
    editing: Option<NameEdit>,
    pub(super) focus_name: bool,
}

struct NameEdit { id: u64, name: String, duplicate: bool }
enum Edit { Save(String), Update(IndicatorTemplate), Remove(IndicatorTemplate), Name(IndicatorTemplate,String,bool) }

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

fn names(config: &IndicatorConfig, scripts: &[PineInstance]) -> Vec<String> {
    let mut names = Vec::new();
    for (enabled,name) in [(config.show_volume,tr!("Volume Bars")), (config.show_ema,tr!("EMA")),
        (config.show_bollinger,tr!("Bollinger Bands")), (config.show_trend_flips,tr!("Trend Flips"))] {
        if enabled { names.push(name.to_owned()); }
    }
    names.extend(scripts.iter().map(|s| s.name.clone()));
    names
}
fn summary(config: &IndicatorConfig, scripts: &[PineInstance]) -> String {
    let names = names(config,scripts);
    if names.is_empty() { return tr!("No indicators").to_owned(); }
    let count = names.len();
    format!("{} · {}",tr!("{count} indicators",count = count),names.join(", "))
}
fn checked_name(rows: &[IndicatorTemplate], name: &str, except: Option<u64>) -> Result<String,String> {
    let name = clean_name(name).map_err(|e| e.message())?;
    if rows.iter().any(|r| Some(r.id) != except && r.name.to_lowercase() == name.to_lowercase()) {
        return Err(tr!("A template with that name already exists.").to_owned());
    }
    Ok(name)
}
fn new_id(rows: &[IndicatorTemplate]) -> Result<u64,String> {
    if rows.len() >= MAX_TEMPLATES { return Err(tr!("Too many indicator templates.").to_owned()); }
    rows.iter().map(|r| r.id).max().unwrap_or(0).checked_add(1)
        .ok_or_else(|| tr!("No more template identifiers are available.").to_owned())
}
fn existing(rows: &[IndicatorTemplate], expected: &IndicatorTemplate) -> Result<usize,String> {
    let index = rows.iter().position(|r| r.id == expected.id)
        .ok_or_else(|| tr!("That template no longer exists.").to_owned())?;
    if &rows[index] != expected { return Err(tr!("The template has changed. Refresh and try again.").to_owned()); }
    Ok(index)
}

pub(super) fn show(ui: &mut Ui, controls: &mut Controls, c: &ChartSettingsContext) -> Option<u64> {
    ui.heading(tr!("Indicator templates"));
    ui.label(tr!("Switch indicators without changing the market or drawings."));
    ui.add_space(8.0);
    let mut apply = None;
    let mut edit = None;
    if controls.rows.is_none() { controls.rows = Some(read(c.store)); }
    let width = ui.available_width();
    kit::card_frame(false).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width((width-26.0).max(100.0));
        ui.label(egui::RichText::new(tr!("Current indicators")).strong());
        ui.label(egui::RichText::new(summary(c.indicators,c.card.pine_instances())).color(theme::text_secondary()));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut controls.name)
                .hint_text(tr!("Template name")).desired_width(190.0));
            if controls.focus_name { field.request_focus(); controls.focus_name = false; }
            let valid = clean_name(&controls.name).is_ok();
            if ui.add_enabled(valid,egui::Button::new(tr!("Save new template"))).clicked()
                || (valid && field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                edit = Some(Edit::Save(controls.name.clone()));
            }
        });
    });
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("My templates")).strong());
        if ui.button(tr!("Refresh")).clicked() {
            controls.rows = Some(read(c.store));
            controls.deleting = None;
            controls.updating = None;
            controls.editing = None;
        }
    });
    if let Some(rows) = &controls.rows {
        match rows {
            Ok(rows) => {
                if rows.is_empty() { ui.label(tr!("None saved yet")); }
                let mut ordered: Vec<_> = rows.iter().collect();
                ordered.sort_by_key(|row| row.name.to_lowercase());
                for row in ordered {
                    let current = row.matches_chart(c.indicators,c.card.pine_instances());
                    ui.push_id(row.id, |ui| {
                        kit::card_frame(false).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                            ui.set_width((width-26.0).max(100.0));
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(&row.name).strong());
                                if current { ui.label(egui::RichText::new(format!("✓ {}",tr!("Matches current chart"))).color(kit::GREEN).small()); }
                            });
                            ui.label(egui::RichText::new(summary(&row.indicators,&row.scripts)).small().color(theme::text_secondary()));
                            ui.add_space(5.0);
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled(!current,egui::Button::new(tr!("Load"))).clicked() {
                                    apply = Some(row.id); controls.updating = None; controls.deleting = None; controls.editing = None;
                                }
                                if ui.button(tr!("Update from this chart")).clicked() {
                                    controls.updating = Some(row.id); controls.deleting = None; controls.editing = None;
                                }
                                ui.menu_button("⋯", |ui| {
                                    if ui.button(tr!("Rename")).clicked() {
                                        controls.editing = Some(NameEdit { id: row.id, name: row.name.clone(), duplicate: false });
                                        controls.updating = None; controls.deleting = None; ui.close();
                                    }
                                    if ui.button(tr!("Duplicate")).clicked() {
                                        controls.editing = Some(NameEdit { id: row.id, name: String::new(), duplicate: true });
                                        controls.updating = None; controls.deleting = None; ui.close();
                                    }
                                    if ui.button(tr!("Delete")).clicked() {
                                        controls.deleting = Some(row.id); controls.updating = None; controls.editing = None; ui.close();
                                    }
                                });
                            });
                            if controls.updating == Some(row.id) {
                                ui.separator();
                                ui.label(tr!("Replace the saved indicators and settings with those of this chart? Existing charts and drawings will not change."));
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button(tr!("Update Template")).clicked() { edit = Some(Edit::Update(row.clone())); }
                                    if ui.button(tr!("Cancel")).clicked() { controls.updating = None; }
                                });
                            }
                            if controls.deleting == Some(row.id) {
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button(tr!("Confirm deletion")).clicked() { edit = Some(Edit::Remove(row.clone())); }
                                    if ui.button(tr!("Cancel")).clicked() { controls.deleting = None; }
                                });
                            }
                            let mut cancel = false;
                            if let Some(name) = controls.editing.as_mut().filter(|n| n.id == row.id) {
                                ui.horizontal_wrapped(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut name.name).hint_text(tr!("Template name")).desired_width(160.0));
                                    if ui.add_enabled(clean_name(&name.name).is_ok(),egui::Button::new(tr!("Save"))).clicked() {
                                        edit = Some(Edit::Name(row.clone(),name.name.clone(),name.duplicate));
                                    }
                                    cancel = ui.button(tr!("Cancel")).clicked();
                                });
                            }
                            if cancel { controls.editing = None; }
                        });
                    });
                    ui.add_space(4.0);
                }
            }
            Err(error) => { ui.label(error); }
        }
    }
    if let Some(edit) = edit {
        let mut message = String::new();
        let result = change(c.store, |rows| {
            match edit {
                Edit::Save(name) => {
                    let name = checked_name(rows,&name,None)?;
                    let id = new_id(rows)?;
                    rows.push(IndicatorTemplate { id,name,indicators:*c.indicators,scripts:c.card.pine_instances().to_vec() });
                    message = tr!("Indicator template saved.").to_owned();
                }
                Edit::Update(expected) => {
                    let index = existing(rows,&expected)?;
                    rows[index].update_from_chart(*c.indicators,c.card.pine_instances());
                    message = tr!("Updated “{name}” from this chart.",name = expected.name.as_str());
                }
                Edit::Remove(expected) => {
                    let index = existing(rows,&expected)?;
                    rows.remove(index);
                    message = tr!("Indicator template deleted.").to_owned();
                }
                Edit::Name(expected,name,duplicate) => {
                    let index = existing(rows,&expected)?;
                    let name = checked_name(rows,&name,(!duplicate).then_some(expected.id))?;
                    if duplicate {
                        let id = new_id(rows)?;
                        let mut copy = rows[index].clone();
                        copy.id = id; copy.name = name;
                        rows.push(copy);
                        message = tr!("Indicator template duplicated.").to_owned();
                    } else {
                        rows[index].name = name;
                        message = tr!("Indicator template renamed.").to_owned();
                    }
                }
            }
            Ok(())
        });
        match result {
            Ok(rows) => {
                controls.rows = Some(Ok(rows)); controls.name.clear();
                controls.notice = Some(message); controls.updating = None; controls.deleting = None; controls.editing = None;
            }
            Err(error) => controls.notice = Some(error),
        }
    }
    if let Some(notice) = &controls.notice { ui.label(notice); }
    apply
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_validation_excludes_only_the_template_being_renamed() {
        let row = IndicatorTemplate { id: 7, name: "COT".into(), indicators: Default::default(), scripts: Vec::new() };
        assert!(checked_name(&[row.clone()],"cot",None).is_err());
        assert!(checked_name(&[row.clone()],"cot",Some(7)).is_ok());
        assert_eq!(new_id(&[row]).unwrap(),8);
    }
}
