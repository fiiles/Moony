use std::marker::PhantomData;
use std::path::Path;

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{KeychainError, Result, Status};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Keychain<R> {
    Keychain(PhantomData)
}

/// Desktop stand-in: the desktop keeps its password-only unlock, so every call is
/// [`KeychainError::Unsupported`].
pub struct Keychain<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> Keychain<R> {
    pub fn store(&self, _key: &str, _value_hex: &str, _reason: &str) -> Result<()> {
        Err(KeychainError::Unsupported)
    }

    pub fn retrieve(&self, _key: &str, _reason: &str) -> Result<Option<String>> {
        Err(KeychainError::Unsupported)
    }

    pub fn remove(&self, _key: &str) -> Result<()> {
        Err(KeychainError::Unsupported)
    }

    pub fn set_privacy_screen(&self, _enabled: bool) -> Result<()> {
        Err(KeychainError::Unsupported)
    }

    pub fn status(&self) -> Result<Status> {
        Err(KeychainError::Unsupported)
    }

    /// Unsupported like every other call: the desktop never calls it (device backups are a phone
    /// concern), and an `Ok(false)` would claim a path that was never touched is not excluded.
    pub fn exclude_from_backup(&self, _path: &Path) -> Result<bool> {
        Err(KeychainError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_desktop_call_is_unsupported() {
        let keychain = Keychain::<tauri::Wry>(PhantomData);
        assert_eq!(
            keychain.store("k", "00", "reason"),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            keychain.retrieve("k", "reason"),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(keychain.remove("k"), Err(KeychainError::Unsupported));
        assert_eq!(
            keychain.set_privacy_screen(true),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(keychain.status(), Err(KeychainError::Unsupported));
        assert_eq!(
            keychain.exclude_from_backup(Path::new("/data")),
            Err(KeychainError::Unsupported)
        );
    }
}
