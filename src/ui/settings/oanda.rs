//! Explicit OANDA account discovery and secure save. No trade endpoints.
use crate::model::DataSource;
use crate::service::{ProviderError, work_queue::{self, Pending}};
use crate::service::keychain::oanda::OandaStore;
use crate::service::market::oanda;
use eframe::egui;

enum Outcome { Accounts(Vec<String>), Saved, Removed }
#[derive(Default)]
pub struct OandaForm {
    live: bool,
    token: String,
    accounts: Vec<String>,
    selected: usize,
    pending: Option<Pending<Result<Outcome, ProviderError>>>,
    status: Option<(String, bool)>,
}
pub fn body(ui: &mut egui::Ui, form: &mut OandaForm) {
    if let Some(answer) = form.pending.as_mut().and_then(|job| job.poll_or(|| {
        Err(ProviderError::other("OANDA request stopped."))
    })) {
        form.pending = None;
        match answer {
            Ok(Outcome::Accounts(ids)) => {
                form.accounts = ids;
                form.selected = 0;
                form.status = Some(("Cuenta detectada. Pulsa Guardar conexión.".into(), false));
            }
            Ok(Outcome::Saved) => {
                form.token.clear(); form.accounts.clear();
                form.status = Some(("Conexión guardada. Reinicia DegenView sin el lanzador de OANDA.".into(), false));
            }
            Ok(Outcome::Removed) => {
                form.token.clear(); form.accounts.clear();
                form.status = Some(("Credencial guardada eliminada. Reinicia para cerrar la conexión anterior.".into(), false));
            }
            Err(err) => form.status = Some((err.to_string(), true)),
        }
    }
    let saved = OandaStore::shared().is_saved();
    ui.label(if saved { "Credencial configurada" } else { "Sin credencial guardada" });
    ui.label("El token se guarda en el almacenamiento seguro del sistema. No se enviarán órdenes.");
    ui.add_space(12.0);
    ui.add_enabled_ui(form.pending.is_none(), |ui| {
        ui.horizontal(|ui| {
            let a = ui.radio_value(&mut form.live, false, "Demo (practice)").changed();
            let b = ui.radio_value(&mut form.live, true, "Real (live)").changed();
            if a || b { form.accounts.clear(); }
        });
        ui.label("Token de API v20");
        if ui.add(egui::TextEdit::singleline(&mut form.token).password(true).desired_width(340.0)).changed() {
            form.accounts.clear();
        }
        let environment = if form.live { "live" } else { "practice" }.to_owned();
        if ui.add_enabled(!form.token.trim().is_empty(), egui::Button::new("Detectar cuentas")).clicked() {
            let token = form.token.clone();
            form.status = None; form.accounts.clear();
            let environment = environment.clone();
            form.pending = Some(work_queue::start_for(DataSource::Oanda, Some(ui.ctx()), move ||
                oanda::discover_accounts(&environment, &token).map(Outcome::Accounts)));
        }
        if !form.accounts.is_empty() {
            egui::ComboBox::from_id_salt("oanda-account")
                .selected_text(&form.accounts[form.selected])
                .show_ui(ui, |ui| {
                    for (i, account) in form.accounts.iter().enumerate() {
                        ui.selectable_value(&mut form.selected, i, account);
                    }
                });
            if ui.button("Guardar conexión").clicked() {
                let token = form.token.clone();
                let account = form.accounts[form.selected].clone();
                form.pending = Some(work_queue::start_for(DataSource::Oanda, Some(ui.ctx()), move ||
                    oanda::connect_account(environment, token, account).map(|_| Outcome::Saved)));
            }
        }
        if saved && form.pending.is_none() && ui.button("Eliminar credencial").clicked() {
            form.pending = Some(work_queue::start_for(DataSource::Oanda, Some(ui.ctx()), || 
                oanda::disconnect_account().map(|_| Outcome::Removed)));
        }
    });
    if form.pending.is_some() {
        ui.spinner();
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    if let Some((message, error)) = &form.status {
        super::window::status_message(ui, message, *error);
    }
}
