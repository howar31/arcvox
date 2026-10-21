//! API keys live in the OS keychain. Core only knows the trait; the client
//! supplies the platform implementation. Tests use the in-memory store.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret store unavailable: {0}")]
    Unavailable(String),
}

/// Where API keys live. `get` is called while a pipeline is being built
/// on an async worker, so it must answer in milliseconds, as a keychain
/// or a map does; a store that goes over the network belongs behind
/// `spawn_blocking`.
pub trait SecretStore: Send + Sync {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, key_ref: &str, value: &str) -> Result<(), SecretError>;
    fn delete(&self, key_ref: &str) -> Result<(), SecretError>;
}

#[derive(Default)]
pub struct MemorySecretStore {
    inner: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError> {
        Ok(self.inner.lock().unwrap().get(key_ref).cloned())
    }
    fn set(&self, key_ref: &str, value: &str) -> Result<(), SecretError> {
        self.inner
            .lock()
            .unwrap()
            .insert(key_ref.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        self.inner.lock().unwrap().remove(key_ref);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let s = MemorySecretStore::default();
        assert_eq!(s.get("gemini").unwrap(), None);
        s.set("gemini", "k1").unwrap();
        assert_eq!(s.get("gemini").unwrap(), Some("k1".into()));
        s.delete("gemini").unwrap();
        assert_eq!(s.get("gemini").unwrap(), None);
    }
}
