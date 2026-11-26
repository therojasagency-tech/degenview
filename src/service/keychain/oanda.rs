//! Local, non-shared OANDA market-data credential. Never serialized into application settings.
use super::api_keys::{KeyBackend, default_backend};
use super::cached_secret::CachedSecret;
use super::webhook_secrets::Scope;
use crate::service::webhook::delivery::SecretStoreError;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Serialize, Deserialize)]
pub struct OandaCredential {
    pub environment: String,
    pub account: String,
    pub token: String,
}
pub struct OandaStore {
    backend: Box<dyn KeyBackend>,
    secret: CachedSecret,
}
impl OandaStore {
    pub fn shared() -> &'static Self {
        static STORE: OnceLock<OandaStore> = OnceLock::new();
        STORE.get_or_init(|| Self {
            backend: default_backend(),
            secret: CachedSecret::new(Scope {
                service: "es.nico.degenview.custom.oanda", shared_group: false,
            }, "v20-market-data-v1"),
        })
    }
    pub fn is_saved(&self) -> bool { self.secret.is_saved(&*self.backend) }
    pub fn read(&self) -> Option<OandaCredential> {
        serde_json::from_str(&self.secret.read(&*self.backend)?).ok()
    }
    pub fn save(&self, value: &OandaCredential) -> Result<(), SecretStoreError> {
        let text = serde_json::to_string(value)
            .map_err(|_| SecretStoreError("Could not encode OANDA credentials".into()))?;
        self.secret.save(&*self.backend, &text)
    }
    pub fn remove(&self) -> Result<(), SecretStoreError> {
        self.secret.remove(&*self.backend)
    }
}
