//! The desktop keychain: the master key is wrapped by a platform native that only the biometry can
//! unlock, and the wrapped blob is kept in a private file next to the database ([`blob_file`]).
//!
//! The platform natives plug in through [`NativeBiometrics`]: Touch ID on macOS ([`macos`]) and
//! Windows Hello on Windows ([`windows`], wrapping with [`hello_wrap`]). On Linux, which has no
//! biometric unlock, every call is [`KeychainError::Unsupported`] and the desktop keeps its
//! password-only unlock.
//!
//! Device sync's two machine calls plug in through [`NativeMachine`]: the binding, a 32-byte value
//! sealed so that only this machine opens it (a Secure Enclave key without user presence on macOS,
//! DPAPI with machine scope on Windows, its answers classified in [`dpapi_errors`]), and a
//! clipboard copy that clears itself only while nobody wrote the clipboard since. Linux has
//! neither: [`KeychainError::Unsupported`].

mod blob_file;
#[cfg(any(target_os = "windows", test))]
mod dpapi_errors;
#[cfg(any(target_os = "windows", test))]
mod hello_wrap;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

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

/// What a platform offers device sync: a value sealed to this machine, and a clipboard copy of a
/// secret that can be taken back later. None of these calls prompts.
pub trait NativeMachine {
    /// Seals `secret` so that only this machine can open it. [`KeychainError::Unsupported`] where
    /// the machine has nothing to seal with.
    fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>>;
    /// Opens a blob [`bind_seal`](NativeMachine::bind_seal) made. [`KeychainError::Invalidated`]
    /// means the blob was not sealed on this machine (or is not a binding blob at all); a
    /// failure of the moment is [`KeychainError::Failed`] (or `Cancelled`/`Unavailable`).
    fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>>;
    /// Puts `text` on the clipboard, marked for clipboard managers and the clipboard history to
    /// leave alone, and returns the clipboard's change token after the write.
    fn copy_secret_text(&self, text: &str) -> Result<u64>;
    /// Empties the clipboard if its change token is still `token` (nothing was copied since);
    /// `true` when it emptied it.
    fn clear_clipboard_if_unchanged(&self, token: u64) -> Result<bool>;
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

impl NativeMachine for UnsupportedNative {
    fn bind_seal(&self, _secret: &[u8; 32]) -> Result<Vec<u8>> {
        Err(KeychainError::Unsupported)
    }

    fn bind_open(&self, _blob: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        Err(KeychainError::Unsupported)
    }

    fn copy_secret_text(&self, _text: &str) -> Result<u64> {
        Err(KeychainError::Unsupported)
    }

    fn clear_clipboard_if_unchanged(&self, _token: u64) -> Result<bool> {
        Err(KeychainError::Unsupported)
    }
}

/// The seam where a platform plugs in its native per `target_os`. The app identifier is passed
/// along because the Windows native names its Hello credential after it; the data directory is for
/// natives that keep extra files next to the blob (neither the macOS nor the Windows one does).
fn platform_native(
    #[cfg_attr(not(target_os = "windows"), allow(unused_variables))] app_identifier: &str,
    _data_dir: &Path,
) -> Box<dyn NativeBiometrics + Send + Sync> {
    #[cfg(target_os = "macos")]
    let native = Box::new(macos::MacNative);
    #[cfg(target_os = "windows")]
    let native = Box::new(windows::WinNative::new(app_identifier));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let native = Box::new(UnsupportedNative);
    native
}

/// The seam for [`NativeMachine`] per `target_os`.
fn platform_machine() -> Box<dyn NativeMachine + Send + Sync> {
    #[cfg(target_os = "macos")]
    let machine = Box::new(macos::MacNative);
    #[cfg(target_os = "windows")]
    let machine = Box::new(windows::WinMachine);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let machine = Box::new(UnsupportedNative);
    machine
}

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Keychain<R> {
    match app.path().app_data_dir() {
        Ok(data_dir) => {
            let native = platform_native(&app.config().identifier, &data_dir);
            Keychain::new(data_dir, native, platform_machine())
        }
        Err(error) => {
            // Plugin setup must never fail the whole app over biometrics: without a data
            // directory there is nowhere to keep the blob, so biometrics are simply unsupported.
            log::warn!("biometric unlock disabled, the app data directory is unavailable: {error}");
            // The machine calls keep no files, so they stay available.
            Keychain::new(
                PathBuf::new(),
                Box::new(UnsupportedNative),
                platform_machine(),
            )
        }
    }
}

/// The desktop keychain. `retrieve` blocks until the person answers the biometric prompt, so call
/// it from `spawn_blocking` or an async command, never from the main thread.
pub struct Keychain<R: Runtime> {
    /// The app data directory, the same one the database lives in.
    data_dir: PathBuf,
    native: Box<dyn NativeBiometrics + Send + Sync>,
    machine: Box<dyn NativeMachine + Send + Sync>,
    _runtime: PhantomData<fn() -> R>,
}

impl<R: Runtime> Keychain<R> {
    fn new(
        data_dir: PathBuf,
        native: Box<dyn NativeBiometrics + Send + Sync>,
        machine: Box<dyn NativeMachine + Send + Sync>,
    ) -> Self {
        Self {
            data_dir,
            native,
            machine,
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
        // Encoded from the `Zeroizing` buffer itself: `*secret` would copy the bytes to the stack
        // where nothing wipes them.
        Ok(Some(hex::encode(secret.as_slice())))
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

    /// Seals device sync's binding to this machine, without a prompt. The caller keeps the blob.
    /// [`KeychainError::Unsupported`] on Linux and on a Mac without a Secure Enclave.
    pub fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>> {
        self.machine.bind_seal(secret)
    }

    /// Opens a binding blob, without a prompt. [`KeychainError::Invalidated`]: it was sealed on
    /// another machine (or is not a binding blob).
    pub fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        self.machine.bind_open(blob)
    }

    /// Copies a secret (the Recovery Kit code) to the clipboard, hidden from clipboard managers
    /// and the clipboard history; returns the token [`clear_clipboard_if_unchanged`] takes.
    ///
    /// [`clear_clipboard_if_unchanged`]: Keychain::clear_clipboard_if_unchanged
    pub fn copy_secret_text(&self, text: &str) -> Result<u64> {
        self.machine.copy_secret_text(text)
    }

    /// Empties the clipboard only if nothing was copied since the copy that returned `token`;
    /// `true` when it did.
    pub fn clear_clipboard_if_unchanged(&self, token: u64) -> Result<bool> {
        self.machine.clear_clipboard_if_unchanged(token)
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
            Box::new(FakeNative),
            Box::new(UnsupportedNative),
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

    /// Stands in for a machine native: XOR with a "machine" byte as the seal (the byte leads the
    /// blob, so another machine's blob is `Invalidated`), and a clipboard that counts its writes.
    struct FakeMachine {
        machine: u8,
        clipboard: std::sync::Mutex<(u64, Option<String>)>,
    }

    impl FakeMachine {
        fn new(machine: u8) -> Self {
            Self {
                machine,
                clipboard: std::sync::Mutex::new((0, None)),
            }
        }
    }

    impl NativeMachine for FakeMachine {
        fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>> {
            let mut blob = vec![self.machine];
            blob.extend(secret.iter().map(|byte| byte ^ self.machine));
            Ok(blob)
        }

        fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
            match blob.split_first() {
                Some((&machine, sealed)) if machine == self.machine && sealed.len() == 32 => {
                    let mut secret = Zeroizing::new([0u8; 32]);
                    for (out, byte) in secret.iter_mut().zip(sealed) {
                        *out = byte ^ self.machine;
                    }
                    Ok(secret)
                }
                _ => Err(KeychainError::Invalidated),
            }
        }

        fn copy_secret_text(&self, text: &str) -> Result<u64> {
            let mut clipboard = self.clipboard.lock().expect("clipboard");
            *clipboard = (clipboard.0 + 1, Some(text.to_string()));
            Ok(clipboard.0)
        }

        fn clear_clipboard_if_unchanged(&self, token: u64) -> Result<bool> {
            let mut clipboard = self.clipboard.lock().expect("clipboard");
            if clipboard.0 != token {
                return Ok(false);
            }
            *clipboard = (clipboard.0 + 1, None);
            Ok(true)
        }
    }

    fn machine_keychain(dir: &Path, machine: u8) -> Keychain<tauri::Wry> {
        Keychain::new(
            dir.to_path_buf(),
            Box::new(FakeNative),
            Box::new(FakeMachine::new(machine)),
        )
    }

    #[test]
    fn the_binding_goes_through_the_machine_native() {
        let dir = tempfile::tempdir().expect("tempdir");
        let secret = [0x42u8; 32];
        let here = machine_keychain(dir.path(), 7);
        let blob = here.bind_seal(&secret).expect("seal");
        assert_eq!(blob.len(), 33);
        assert_eq!(*here.bind_open(&blob).expect("open"), secret);
        assert_eq!(
            machine_keychain(dir.path(), 9).bind_open(&blob).map(|_| ()),
            Err(KeychainError::Invalidated),
            "another machine's blob"
        );
    }

    #[test]
    fn the_clipboard_goes_through_the_machine_native() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = machine_keychain(dir.path(), 7);
        let token = keychain.copy_secret_text("code").expect("copy");
        assert_eq!(keychain.clear_clipboard_if_unchanged(token), Ok(true));
        let stale = keychain.copy_secret_text("code").expect("copy");
        keychain.copy_secret_text("something else").expect("copy");
        assert_eq!(keychain.clear_clipboard_if_unchanged(stale), Ok(false));
    }

    #[test]
    fn the_unsupported_native_has_no_binding_or_clipboard() {
        assert_unsupported_binding_and_clipboard(&UnsupportedNative);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_has_no_binding_or_clipboard() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = Keychain::<tauri::Wry>::new(
            dir.path().to_path_buf(),
            platform_native("com.moonyfinance.desktop", dir.path()),
            platform_machine(),
        );
        assert_eq!(
            keychain.bind_seal(&[0; 32]),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            keychain.bind_open(&[0; 33]).map(|_| ()),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            keychain.copy_secret_text("code"),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            keychain.clear_clipboard_if_unchanged(1),
            Err(KeychainError::Unsupported)
        );
    }

    fn assert_unsupported_binding_and_clipboard(native: &dyn NativeMachine) {
        assert_eq!(native.bind_seal(&[0; 32]), Err(KeychainError::Unsupported));
        assert_eq!(
            native.bind_open(&[0; 33]).map(|_| ()),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            native.copy_secret_text("code"),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            native.clear_clipboard_if_unchanged(1),
            Err(KeychainError::Unsupported)
        );
    }

    #[test]
    fn a_keychain_on_the_unsupported_native_never_stores() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keychain = Keychain::<tauri::Wry>::new(
            dir.path().to_path_buf(),
            Box::new(UnsupportedNative),
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
