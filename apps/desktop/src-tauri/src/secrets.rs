//! API keys in the login keychain as generic passwords: service `Kongwe`,
//! account = the key reference stored in `config.json`.

use kongwe_core::{SecretError, SecretStore};

#[cfg(target_os = "macos")]
use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password, PasswordOptions,
};

pub const SERVICE: &str = "Kongwe";
/// `errSecItemNotFound`.
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
/// `errSecDuplicateItem`.
#[cfg(target_os = "macos")]
const ERR_SEC_DUPLICATE_ITEM: i32 = -25299;

#[derive(Default)]
pub struct KeychainStore;

impl KeychainStore {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for KeychainStore {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError> {
        match generic_password(PasswordOptions::new_generic_password(SERVICE, key_ref)) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) => {
                let message = format!("{e}");
                if e.code() == ERR_SEC_ITEM_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(SecretError::Unavailable(message))
                }
            }
        }
    }

    fn set(&self, key_ref: &str, value: &str) -> Result<(), SecretError> {
        match set_generic_password(SERVICE, key_ref, value.as_bytes()) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_DUPLICATE_ITEM => {
                delete_generic_password(SERVICE, key_ref)
                    .map_err(|e| SecretError::Unavailable(format!("{e}")))?;
                set_generic_password(SERVICE, key_ref, value.as_bytes())
                    .map_err(|e| SecretError::Unavailable(format!("{e}")))
            }
            Err(e) => Err(SecretError::Unavailable(format!("{e}"))),
        }
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        match delete_generic_password(SERVICE, key_ref) {
            Ok(()) => Ok(()),
            Err(e) => {
                let message = format!("{e}");
                if e.code() == ERR_SEC_ITEM_NOT_FOUND {
                    Ok(())
                } else {
                    Err(SecretError::Unavailable(message))
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for KeychainStore {
    fn get(&self, _key_ref: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError::Unavailable(
            "no keychain on this platform".into(),
        ))
    }
    fn set(&self, _key_ref: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable(
            "no keychain on this platform".into(),
        ))
    }
    fn delete(&self, _key_ref: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable(
            "no keychain on this platform".into(),
        ))
    }
}
