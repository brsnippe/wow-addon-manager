//! Secrets such as API tokens, kept in Windows Credential Manager and never in the
//! settings file.

use std::sync::Arc;

use keyring_core::Entry;
use keyring_core::api::CredentialStoreApi;

/// Service name the app's credentials are stored under.
const SERVICE: &str = "com.brsnippe.wowaddonmanager";

/// The GitHub personal access token.
pub const GITHUB_TOKEN: &str = "github";

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, name: &str, secret: &str) -> Result<(), SecretError>;
    /// Removing a secret that isn't stored is not an error.
    fn delete(&self, name: &str) -> Result<(), SecretError>;
}

#[derive(Debug, thiserror::Error)]
#[error("Windows Credential Manager failed: {0}")]
pub struct SecretError(String);

impl From<keyring_core::Error> for SecretError {
    fn from(error: keyring_core::Error) -> Self {
        Self(error.to_string())
    }
}

/// Generic credentials in Windows Credential Manager, named `<name>.com.brsnippe.wowaddonmanager`.
pub struct CredentialManager {
    store: Arc<windows_native_keyring_store::Store>,
}

impl CredentialManager {
    pub fn new() -> Result<Self, SecretError> {
        Ok(Self {
            store: windows_native_keyring_store::Store::new()?,
        })
    }

    fn entry(&self, name: &str) -> Result<Entry, SecretError> {
        Ok(self.store.build(SERVICE, name, None)?)
    }
}

impl SecretStore for CredentialManager {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        match self.entry(name)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), SecretError> {
        Ok(self.entry(name)?.set_password(secret)?)
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Keeps secrets in memory, for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        Ok(self.secrets.lock().unwrap().get(name).cloned())
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), SecretError> {
        self.secrets
            .lock()
            .unwrap()
            .insert(name.into(), secret.into());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        self.secrets.lock().unwrap().remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes to the real Credential Manager: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_credential_manager_round_trip() {
        let store = CredentialManager::new().unwrap();
        let name = "test-round-trip";
        store.delete(name).unwrap();
        assert_eq!(store.get(name).unwrap(), None);
        store.set(name, "secret one").unwrap();
        store.set(name, "secret two").unwrap();
        assert_eq!(store.get(name).unwrap().as_deref(), Some("secret two"));
        store.delete(name).unwrap();
        store.delete(name).unwrap();
        assert_eq!(store.get(name).unwrap(), None);
    }
}
