//! Non-secret configuration for the on-demand anonymous bridge.
use crate::service::market::openchart_manager as manager;
use eframe::egui;
use std::path::PathBuf;
#[derive(Default)]
pub struct OpenChartForm {
    loaded: bool,
    checkout: String,
    authorized: bool,
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
        form.checkout = saved.map(|s| s.checkout).unwrap_or_else(manager::default_checkout).to_string_lossy().into_owned();
    }
    ui.label("El motor se inicia al abrir un gráfico de TradingView. Modo público/anónimo.");
    ui.label("Conserva el checkout de OpenChart y Bun instalados; aún no es un DMG autónomo.");
    ui.add_space(12.0);
    ui.label("Ruta del checkout de OpenChart");
    ui.add(egui::TextEdit::singleline(&mut form.checkout).desired_width(340.0));
    ui.checkbox(&mut form.authorized, "Dispongo de autorización para el acceso programático a estos datos.");
    if ui.add_enabled(form.authorized && !form.checkout.trim().is_empty(), egui::Button::new("Activar conexión automática")).clicked() {
        form.status = Some(match manager::save_settings(PathBuf::from(form.checkout.trim())) {
            Ok(()) => ("Configuración guardada. Reinicia DegenView sin el lanzador de Terminal.".into(), false),
            Err(e) => (e, true),
        });
    }
    if manager::saved_settings().is_some() && ui.button("Desactivar conexión automática").clicked() {
        form.status = Some(match manager::remove_settings() {
            Ok(()) => { form.authorized = false; ("Desactivado. Reinicia DegenView.".into(), false) },
            Err(e) => (e, true),
        });
    }
    if let Some((message, error)) = &form.status {
        super::window::status_message(ui, message, *error);
    }
}
