//! The user's own TradingView session, local to this app. Never put it in settings or logs.
use super::api_keys::{KeyBackend, default_backend};
use super::webhook_secrets::Scope;
use std::sync::OnceLock;

const SCOPE: Scope = Scope { service: "es.nico.degenview.custom.tradingview", shared_group: false };
const ACCOUNT: &str = "browser-session-v1";
fn backend() -> &'static dyn KeyBackend {
    static STORE: OnceLock<Box<dyn KeyBackend>> = OnceLock::new();
    &**STORE.get_or_init(default_backend)
}
pub fn exists() -> bool { backend().exists(SCOPE, ACCOUNT) }
pub fn read() -> Option<String> { backend().read(SCOPE, ACCOUNT) }
pub fn save(session: &str) -> Result<(), String> {
    backend().write(SCOPE, ACCOUNT, session).map_err(|_| "No se pudo guardar la sesión en el almacenamiento seguro.".into())
}
pub fn remove() -> Result<(), String> {
    backend().delete(SCOPE, ACCOUNT).map_err(|_| "No se pudo eliminar la sesión guardada.".into())
}
