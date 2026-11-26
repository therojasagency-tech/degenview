//! Native settings for the original OpenChart anonymous or browser-authorized session.
use crate::service::{keychain::openchart as credentials, market::openchart_manager as manager, work_queue::{self, Pending}};
use eframe::egui;
use std::path::PathBuf;
#[derive(Default)]
pub struct OpenChartForm {
    loaded: bool,
    checkout: String,
    authorized: bool,
    authenticated: bool,
    session_saved: bool,
    pending: Option<Pending<Result<(), String>>>,
    status: Option<(String, bool)>,
}
pub fn body(ui: &mut egui::Ui, form: &mut OpenChartForm) {
    if !cfg!(target_os = "macos") {
        ui.label("El arranque integrado está disponible en macOS.");
        return;
    }
    if !form.loaded {
        form.loaded = true;
        let saved = manager::saved_settings();
        form.authorized = saved.is_some();
        form.session_saved = credentials::exists();
        form.authenticated = saved.as_ref().is_some_and(|s| s.authenticated);
        form.checkout = saved.map(|s| s.checkout).unwrap_or_else(manager::default_checkout).to_string_lossy().into_owned();
    }
    if let Some(answer) = form.pending.as_mut().and_then(|job| job.poll_or(|| Err("La conexión se interrumpió.".into()))) {
        form.pending = None;
        form.session_saved = credentials::exists();
        if answer.is_ok() {
            form.authenticated = manager::saved_settings().is_some_and(|s| s.authenticated);
        }
        form.status = Some(match answer {
            Ok(()) => ("Configuración guardada. Reinicia DegenView.".into(), false),
            Err(e) => (e, true),
        });
    }
    ui.label(if form.authenticated { "Modo configurado: cuenta personal" } else { "Modo configurado: anónimo" });
    ui.label("La sesión personal se guarda en el llavero; no se solicita tu contraseña.");
    let embedded = manager::embedded_bridge().is_some();
    if embedded {
        ui.label("Motor OpenChart integrado. No necesita Bun ni una carpeta externa.");
    } else {
        ui.label("Modo de desarrollo: conserva el checkout de OpenChart y Bun instalados.");
    }
    ui.add_space(12.0);
    ui.add_enabled_ui(form.pending.is_none(), |ui| {
        if !embedded {
            ui.label("Ruta del checkout de OpenChart");
            ui.add(egui::TextEdit::singleline(&mut form.checkout).desired_width(340.0));
        }
        ui.checkbox(&mut form.authorized, "Dispongo de autorización para el acceso programático a estos datos.");
        let enabled = form.authorized && (embedded || !form.checkout.trim().is_empty());
        if ui.add_enabled(enabled, egui::Button::new("Activar modo anónimo")).clicked() {
            form.status = Some(match manager::save_settings(PathBuf::from(form.checkout.trim()), false) {
                Ok(()) => { form.authenticated = false; ("Modo anónimo guardado. Reinicia DegenView.".into(), false) },
                Err(e) => (e, true),
            });
        }
        if ui.add_enabled(enabled, egui::Button::new("Conectar cuenta en navegador")).clicked() {
            let checkout = PathBuf::from(form.checkout.trim());
            form.status = None;
            form.pending = Some(work_queue::start(Some(ui.ctx()), move || manager::connect_account(checkout)));
        }
        if form.authenticated || form.session_saved {
            if ui.button("Eliminar sesión guardada y usar modo anónimo").clicked() {
                form.pending = Some(work_queue::start(Some(ui.ctx()), manager::disconnect_account));
            }
        }
        if manager::saved_settings().is_some() && ui.button("Desactivar conexión automática").clicked() {
            form.status = Some(match manager::remove_settings() {
                Ok(()) => { form.authorized = false; form.authenticated = false; ("Desactivado. Reinicia DegenView.".into(), false) },
                Err(e) => (e, true),
            });
        }
    });
    if form.pending.is_some() {
        ui.spinner();
        ui.label("Completa la conexión en el navegador y en el diálogo local.");
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    if let Some((message, error)) = &form.status { super::window::status_message(ui, message, *error); }
}
