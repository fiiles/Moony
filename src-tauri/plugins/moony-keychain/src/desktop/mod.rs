//! The desktop keychain: the master key is wrapped by a platform native that only the biometry can
//! unlock, and the wrapped blob is kept in a private file next to the database ([`blob_file`]).
//!
//! The platform natives plug in through [`NativeBiometrics`]: Touch ID on macOS ([`macos`]). Until
//! the Windows backend lands (and on Linux, which has no biometric unlock), every call is
//! [`KeychainError::Unsupported`] and the desktop keeps its password-only unlock.

mod blob_file;
#[cfg(target_os = "macos")]
mod macos;

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Manager, Runtime};
use zeroize::Zeroizing;

use crate::{KeychainError, Result, Status};

/// What a platform has to provide: a biometry-gated way to wrap and unwrap the 32-byte master key.
/// The blob [`wrap`](NativeBiometrics::wrap) returns is opaque to this crate; it only stores it.
pub trait NativeBiometrics {
    /// Whether wrapping and unwrapping can work right now, and which biometry the device offers.
    fn status(&self) -> Result<Status>;
    /// Wraps `secret` with a key that only the biometry (after a prompt showing `reason`) unlocks.
    fn wrap(&self, secret: &[u8; 32], reason: &str) -> Result<Vec<u8>>;
    /// Unwraps a blob [`wrap`](NativeBiometrics::wrap) made, after a biometric prompt.
    fn unwrap(&self, blob: &[u8], reason: &str) -> Result<Zeroizing<[u8; 32]>>;
    /// Deletes the biometry-gated key the blobs were wrapped with.
    fn forget(&self) -> Result<()>;
}

/// The native for a platform without a backend: every call is [`KeychainError::Unsupported`].
struct UnsupportedNative;

impl NativeBiometrics for UnsupportedNative {
    fn status(&self) -> Result<Status> {
        Err(KeychainError::Unsupported)
    }

    fn wrap(&self, _secret: &[u8; 32], _reason: &str) -> Result<Vec<u8>> {
        Err(KeychainError::Unsupported)
    }

    fn unwrap(&self, _blob: &[u8], _reason: &str) -> Result<Zeroizing<[u8; 32]>> {
        Err(KeychainError::Unsupported)
    }

    fn forget(&self) -> Result<()> {
        Err(KeychainError::Unsupported)
    }
}

/// The seam where a platform plugs in its native per `target_os`. The app identifier is passed
/// along because the Windows native names its credential after it; the data directory is for
/// natives that keep extra files next to the blob (the macOS one keeps none).
fn platform_native(
    _app_identifier: &str,
    _data_dir: &Path,
) -> Box<dyn NativeBiometrics + Send + Sync> {
    #[cfg(target_os = "macos")]
    let native = Box::new(macos::MacNative);
    #[cfg(not(target_os = "macos"))]
    let native = Box::new(UnsupportedNative);
    native
}

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Keychain<R> {
    let identifier = app.config().identifier.clone();
    match app.path().app_data_dir() {
        Ok(data_dir) => {
            let native = platform_native(&identifier, &data_dir);
            Keychain::new(data_dir, identifier, native)
        }
        Err(error) => {
            // Plugin setup must never fail the whole app over biometrics: without a data
            // directory there is nowhere to keep the blob, so biometrics are simply unsupported.
            log::warn!("biometric unlock disabled, the app data directory is unavailable: {error}");
            Keychain::new(PathBuf::new(), identifier, Box::new(UnsupportedNative))
        }
    }
}

/// The desktop keychain. `retrieve` blocks until the person answers the biometric prompt, so call
/// it from `spawn_blocking` or an async command, never from the main thread.
pub struct Keychain<R: Runtime> {
    /// The app data directory, the same one the database lives in.
    data_dir: PathBuf,
    /// The app identifier. Only the Windows native (it names its credential after it) will use it,
    /// so it is dead code on the other platforms until then.
    #[allow(dead_code)]
    app_identifier: String,
    native: Box<dyn NativeBiometrics + Send + Sync>,
    _runtime: PhantomData<fn() -> R>,
}

impl<R: Runtime> Keychain<R> {
    fn new(
        data_dir: PathBuf,
        app_identifier: String,
        native: Box<dyn NativeBiometrics + Send + Sync>,
    ) -> Self {
        Self {
            data_dir,
            app_identifier,
            native,
            _runtime: PhantomData,
        }
    }

    /// Wraps the master key (given as hex) with the biometry-gated key and writes the blob.
    pub fn store(&self, key: &str, value_hex: &str, reason: &str) -> Result<()> {
        let secret = decode_secret(value_hex)?;
        let blob = self.native.wrap(&secret, reason)?;
        blob_file::write(&self.data_dir, key, &blob).map_err(io_failure)
    }

    /// Reads the blob and unwraps it behind a biometric prompt; `None` when nothing is stored.
    pub fn retrieve(&self, key: &str, reason: &str) -> Result<Option<String>> {
        let Some(blob) = blob_file::read(&self.data_dir, key).map_err(io_failure)? else {
            return Ok(None);
        };
        let secret = self.native.unwrap(&blob, reason)?;
        Ok(Some(hex::encode(*secret)))
    }

    /// Forgets the biometry-gated key and deletes the blob. Both are attempted even if the first
    /// fails, so a failing native never leaves the blob behind.
    pub fn remove(&self, key: &str) -> Result<()> {
        let forgotten = match self.native.forget() {
            Err(KeychainError::Unsupported) => Ok(()),
            other => other,
        };
        let removed = blob_file::remove(&self.data_dir, key).map_err(io_failure);
        forgotten.and(removed)
    }

    pub fn set_privacy_screen(&self, _enabled: bool) -> Result<()> {
        Err(KeychainError::Unsupported)
    }

    pub fn status(&self) -> Result<Status> {
        self.native.status()
    }

    /// Unsupported like the privacy screen: the desktop never calls it (device backups are a phone
    /// concern), and an `Ok(false)` would claim a path that was never touched is not excluded.
    pub fn exclude_from_backup(&self, _path: &Path) -> Result<bool> {
        Err(KeychainError::Unsupported)
    }
}

/// Hex (the app's wire form of the master key) to the fixed 32 bytes, wiped on drop.
fn decode_secret(value_hex: &str) -> Result<Zeroizing<[u8; 32]>> {
    let mut secret = Zeroizing::new([0u8; 32]);
    hex::decode_to_slice(value_hex, secret.as_mut_slice())
        .map_err(|_| KeychainError::Failed("the secret is not 32 bytes of hex".to_string()))?;
    Ok(secret)
}

fn io_failure(error: std::io::Error) -> KeychainError {
    KeychainError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "moony.master-key";
    const SECRET_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    /// Stands in for a platform native: XOR as the "wrap", so a round trip proves the plumbing.
    struct FakeNative;

    impl NativeBiometrics for FakeNative {
        fn status(&self) -> Result<Status> {
            Ok(Status {
                available: true,
                biometry_type: crate::BiometryType::Biometric,
            })
        }

        fn wrap(&self, secret: &[u8; 32], _reason: &str) -> Result<Vec<u8>> {
            Ok(secret.iter().map(|byte| byte ^ 0x5a).collect())
        }

        fn unwrap(&self, blob: &[u8], _reason: &str) -> Result<Zeroizing<[u8; 32]>> {
            let mut secret = Zeroizing::new([0u8; 32]);
            if blob.len() != 32 {
                return Err(KeychainError::Failed("bad blob".to_string()));
            }
            for (out, byte) in secret.iter_mut().zip(blob) {
                *out = byte ^ 0x5a;
            }
            Ok(secret)
        }

        fn forget(&self) -> Result<()> {
            Ok(())
        }
    }

    fn keychain(dir: &Path) -> Keychain<tauri::Wry> {
        Keychain::new(
            dir.to_path_buf(),
            "test.app".to_string(),
            Box::new(FakeNative),
        )
    }

    #[test]
    fn store_then_retrieve_returns_the_same_hex() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = keychain(dir.path());
        keychain.store(KEY, SECRET_HEX, "reason").expect("store");
        assert_eq!(
            keychain.retrieve(KEY, "reason"),
            Ok(Some(SECRET_HEX.to_string()))
        );
    }

    #[test]
    fn the_stored_file_is_the_wrapped_blob_not_the_secret() {
        let dir = tempfile::tempdir().expect("tempdir");
        keychain(dir.path())
            .store(KEY, SECRET_HEX, "reason")
            .expect("store");
        let blob = blob_file::read(dir.path(), KEY)
            .expect("read")
            .expect("blob");
        assert_eq!(blob.len(), 32);
        assert_ne!(blob, hex::decode(SECRET_HEX).expect("hex"));
    }

    #[test]
    fn uppercase_hex_is_accepted_and_retrieved_as_lowercase() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = keychain(dir.path());
        keychain
            .store(KEY, &SECRET_HEX.to_uppercase(), "reason")
            .expect("store");
        assert_eq!(
            keychain.retrieve(KEY, "reason"),
            Ok(Some(SECRET_HEX.to_string()))
        );
    }

    #[test]
    fn retrieve_without_a_file_is_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(keychain(dir.path()).retrieve(KEY, "reason"), Ok(None));
    }

    #[test]
    fn remove_deletes_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = keychain(dir.path());
        keychain.store(KEY, SECRET_HEX, "reason").expect("store");
        keychain.remove(KEY).expect("remove");
        assert!(!blob_file::path(dir.path(), KEY).exists());
        assert_eq!(keychain.retrieve(KEY, "reason"), Ok(None));
    }

    #[test]
    fn remove_with_nothing_stored_is_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(keychain(dir.path()).remove(KEY), Ok(()));
    }

    #[test]
    fn bad_hex_is_failed_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = keychain(dir.path());
        for bad in ["zz", "", "00", &SECRET_HEX[2..], &format!("{SECRET_HEX}00")] {
            assert!(
                matches!(
                    keychain.store(KEY, bad, "reason"),
                    Err(KeychainError::Failed(_))
                ),
                "{bad:?} must be rejected"
            );
        }
        assert!(!blob_file::path(dir.path(), KEY).exists());
    }

    #[test]
    fn status_comes_from_the_native() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            keychain(dir.path()).status().map(|status| status.available),
            Ok(true)
        );
    }

    #[test]
    fn privacy_screen_and_backup_exclusion_stay_unsupported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = keychain(dir.path());
        assert_eq!(
            keychain.set_privacy_screen(true),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            keychain.exclude_from_backup(Path::new("/data")),
            Err(KeychainError::Unsupported)
        );
    }

    #[test]
    fn linux_native_is_unsupported() {
        let native = UnsupportedNative;
        assert_eq!(native.status(), Err(KeychainError::Unsupported));
        assert_eq!(
            native.wrap(&[0; 32], "reason"),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            native.unwrap(&[0; 32], "reason").map(|_| ()),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(native.forget(), Err(KeychainError::Unsupported));
    }

    #[test]
    fn a_keychain_on_the_unsupported_native_never_stores() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = Keychain::<tauri::Wry>::new(
            dir.path().to_path_buf(),
            "test.app".to_string(),
            Box::new(UnsupportedNative),
        );
        assert_eq!(
            keychain.store(KEY, SECRET_HEX, "reason"),
            Err(KeychainError::Unsupported)
        );
        assert!(!blob_file::path(dir.path(), KEY).exists());
        assert_eq!(keychain.remove(KEY), Ok(()), "remove ignores Unsupported");
    }
}
