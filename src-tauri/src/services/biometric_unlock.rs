//! Biometric unlock: Face ID / fingerprint on the phones (mobile spec §8.1), Touch ID on macOS and
//! Windows Hello on Windows (ADR 0021).
//!
//! The app keeps the master key in [`SessionKey`] after every unlock; biometric unlock stores a
//! copy behind biometry (the phone keychain, or on the desktop a blob file wrapped by the Secure
//! Enclave or a Windows Hello key) and reads it back from Rust only, behind [`KeychainLike`] (the
//! app's adapter wraps the `moony-keychain` plugin; tests use a stub). The key never crosses into
//! the webview.
//!
//! Whether biometric unlock is on is a marker file next to the key files ([`MARKER_FILE`], "1"
//! or "0"), not a row in the database: the lock screen needs it while the database is still
//! encrypted, and the keychain item cannot be probed without a biometric prompt.

use std::fs;
use std::path::Path;

use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::models::{BiometricStatus, BiometryType};
use crate::services::crypto::AUTH_KEY_FILES_CORRUPTED;
use crate::services::session_key::{SessionKey, KEY_LEN};

/// The keychain item that holds the master key (lowercase hex, 64 characters).
pub const MASTER_KEY_ITEM: &str = "moony.master-key";
/// The marker file in the app data dir: "1" while biometric unlock is on, "0" once it is off.
pub const MARKER_FILE: &str = "biometric-unlock";
/// The `reason` of a status whose biometrics cannot be used now (an i18n key).
pub const UNAVAILABLE_REASON: &str = "mobile:biometrics.unavailable";

/// The stored key cannot be read any more (enrolment changed, passcode removed, item gone):
/// biometric unlock is switched off and the person unlocks with the password.
pub const AUTH_BIOMETRIC_INVALIDATED: &str = "auth.biometricInvalidated";
/// The person dismissed the biometric prompt.
pub const AUTH_BIOMETRIC_CANCELLED: &str = "auth.biometricCancelled";
/// The prompt failed (not recognised, locked out): unlock with the password this time.
pub const AUTH_BIOMETRIC_FAILED: &str = "auth.biometricFailed";

/// What the keychain answered when it could not do what was asked. Mirrors the plugin's
/// `KeychainError`, kept as the service's own type so the service and its tests do not depend on
/// the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeychainFailure {
    /// The enrolment (or the passcode) changed since the key was stored: it is gone for good.
    Invalidated,
    /// The person or the system dismissed the prompt.
    Cancelled,
    /// No usable biometry right now (none enrolled, locked out, no hardware).
    Unavailable,
    /// Anything else, with the native side's message (never a secret).
    Failed(String),
    /// A platform without biometric unlock (Linux).
    Unsupported,
}

/// The keychain's `status` answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeychainStatus {
    pub available: bool,
    pub biometry_type: BiometryType,
}

/// The plugin's biometry-gated store as the biometric service uses it. Every call may block until the native
/// side (or the person, for a prompt) answers: call the service off the main thread.
pub trait KeychainLike {
    fn status(&self) -> std::result::Result<KeychainStatus, KeychainFailure>;
    fn store(
        &self,
        key: &str,
        value_hex: &str,
        reason: &str,
    ) -> std::result::Result<(), KeychainFailure>;
    fn retrieve(
        &self,
        key: &str,
        reason: &str,
    ) -> std::result::Result<Option<String>, KeychainFailure>;
    fn remove(&self, key: &str) -> std::result::Result<(), KeychainFailure>;
}

/// What the marker file says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// Never decided on this install (a fresh install, or a reinstall: iOS keeps keychain items
    /// across an uninstall, the app data dir does not).
    Absent,
    Enabled,
    Disabled,
}

/// Read the marker; anything but "1" is off.
pub fn read_marker(data_dir: &Path) -> Marker {
    match fs::read_to_string(data_dir.join(MARKER_FILE)) {
        Ok(text) if text.trim() == "1" => Marker::Enabled,
        Ok(_) => Marker::Disabled,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Marker::Absent,
        Err(e) => {
            log::warn!("[BIOMETRIC] Could not read the biometric unlock marker: {e}");
            Marker::Disabled
        }
    }
}

/// Write the marker ("1" on, "0" off).
pub fn write_marker(data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    fs::write(data_dir.join(MARKER_FILE), if enabled { "1" } else { "0" })
}

/// What the lock screen needs before the database is open.
pub fn biometric_status(keychain: &impl KeychainLike, data_dir: &Path) -> BiometricStatus {
    let enrolled = read_marker(data_dir) == Marker::Enabled;
    // `Unsupported` means this platform has no keychain at all, which is not something the person
    // can fix, so it carries no reason; every other failure is "supported, but not usable now".
    let (available, biometry_type, supported) = match keychain.status() {
        Ok(status) => (status.available, status.biometry_type, true),
        Err(KeychainFailure::Unsupported) => (false, BiometryType::None, false),
        Err(failure) => {
            log::warn!("[BIOMETRIC] Keychain status failed: {failure:?}");
            (false, BiometryType::None, true)
        }
    };
    BiometricStatus {
        available,
        enrolled,
        biometry_type,
        reason: (supported && !available).then(|| UNAVAILABLE_REASON.to_string()),
        supported,
    }
}

/// Store the session's master key behind biometry and switch biometric unlock on. `reason` is the
/// text of the biometric prompt both platforms show before they store.
pub fn enable(
    keychain: &impl KeychainLike,
    session: &SessionKey,
    data_dir: &Path,
    reason: &str,
) -> Result<()> {
    let key_hex = session.hex().ok_or_else(|| {
        AppError::Internal("No unlocked session to enable biometric unlock with".into())
    })?;

    if let Err(failure) = keychain.store(MASTER_KEY_ITEM, &key_hex, reason) {
        // A failed store leaves nothing usable behind (both platforms remove the old entry before
        // their prompt): biometric unlock is off now, whatever it was before.
        mark_off(data_dir);
        return Err(match failure {
            KeychainFailure::Cancelled => AppError::Auth(AUTH_BIOMETRIC_CANCELLED.into()),
            KeychainFailure::Invalidated => AppError::Auth(AUTH_BIOMETRIC_INVALIDATED.into()),
            // Not recognised, locked out, the permission refused, nothing enrolled: the sheet
            // says biometric unlock was not switched on.
            failure @ (KeychainFailure::Unavailable | KeychainFailure::Failed(_)) => {
                log::warn!("[BIOMETRIC] Could not switch biometric unlock on: {failure:?}");
                AppError::Auth(AUTH_BIOMETRIC_FAILED.into())
            }
            KeychainFailure::Unsupported => {
                AppError::Internal("Biometric unlock is not supported on this platform".into())
            }
        });
    }

    if let Err(e) = write_marker(data_dir, true) {
        // Without the marker the lock screen would never offer the stored key: take it back.
        if let Err(failure) = keychain.remove(MASTER_KEY_ITEM) {
            log::warn!(
                "[BIOMETRIC] Could not remove the key after a failed marker write: {failure:?}"
            );
        }
        return Err(AppError::Internal(format!(
            "Could not record that biometric unlock is on: {e}"
        )));
    }
    log::info!("[BIOMETRIC] Biometric unlock switched on");
    Ok(())
}

/// Switch biometric unlock off and remove the stored key.
///
/// The key is removed whether or not the marker could be written: a stale item behind a marker
/// stuck at "1" would offer the same invalidated answer on every lock screen. A failed marker
/// write is still the result; a failed removal is only logged.
pub fn disable(keychain: &impl KeychainLike, data_dir: &Path) -> Result<()> {
    // The marker decides: once it says off, the stored key is never used again.
    let marker = write_marker(data_dir, false);
    if let Err(failure) = keychain.remove(MASTER_KEY_ITEM) {
        // The next password unlock removes it (`remove_leftover_key`).
        log::warn!("[BIOMETRIC] Could not remove the stored key: {failure:?}");
    }
    marker.map_err(|e| {
        AppError::Internal(format!(
            "Could not record that biometric unlock is off: {e}"
        ))
    })?;
    log::info!("[BIOMETRIC] Biometric unlock switched off");
    Ok(())
}

/// Read the master key behind a biometric prompt (`reason`) and hand it to `open`, which opens
/// the database (`auth::unlock_with_master_key`). A key that can never be read again, or that
/// no longer opens this profile, switches biometric unlock off and asks for the password.
pub fn unlock_with_biometrics<T>(
    keychain: &impl KeychainLike,
    data_dir: &Path,
    reason: &str,
    open: impl FnOnce(&[u8; KEY_LEN]) -> Result<T>,
) -> Result<T> {
    if read_marker(data_dir) != Marker::Enabled {
        // Switched off (or never on): no prompt, the password it is.
        return Err(AppError::Auth(AUTH_BIOMETRIC_INVALIDATED.into()));
    }

    let stored = match keychain.retrieve(MASTER_KEY_ITEM, reason) {
        Ok(Some(hex)) => Zeroizing::new(hex),
        Ok(None) => return Err(invalidated(keychain, data_dir, "no key is stored")),
        Err(KeychainFailure::Invalidated) => {
            return Err(invalidated(keychain, data_dir, "the enrolment changed"))
        }
        Err(KeychainFailure::Cancelled) => {
            return Err(AppError::Auth(AUTH_BIOMETRIC_CANCELLED.into()))
        }
        Err(failure @ (KeychainFailure::Failed(_) | KeychainFailure::Unavailable)) => {
            log::warn!("[BIOMETRIC] Biometric unlock failed: {failure:?}");
            return Err(AppError::Auth(AUTH_BIOMETRIC_FAILED.into()));
        }
        Err(KeychainFailure::Unsupported) => {
            return Err(AppError::Internal(
                "Biometric unlock is not supported on this platform".into(),
            ))
        }
    };

    let mut master_key = Zeroizing::new([0u8; KEY_LEN]);
    if hex::decode_to_slice(stored.as_str(), master_key.as_mut_slice()).is_err() {
        return Err(invalidated(
            keychain,
            data_dir,
            "the stored key is unreadable",
        ));
    }

    match open(&master_key) {
        // The key no longer opens this profile (a restored backup, another profile).
        Err(AppError::Auth(key)) if key == AUTH_KEY_FILES_CORRUPTED => Err(invalidated(
            keychain,
            data_dir,
            "the stored key does not open this profile",
        )),
        other => other,
    }
}

/// After a password unlock: a stored key without the "on" marker is a leftover (iOS keeps
/// keychain items across an uninstall; a disable whose removal failed). Remove it, so a stale key
/// is never offered. Best effort: a failure is logged.
pub fn remove_leftover_key(keychain: &impl KeychainLike, data_dir: &Path) {
    if read_marker(data_dir) == Marker::Enabled {
        return;
    }
    if let Err(failure) = keychain.remove(MASTER_KEY_ITEM) {
        log::warn!("[BIOMETRIC] Could not remove a leftover stored key: {failure:?}");
    }
}

/// The stored key is gone for good: switch biometric unlock off and say so.
fn invalidated(keychain: &impl KeychainLike, data_dir: &Path, why: &str) -> AppError {
    log::warn!("[BIOMETRIC] Biometric unlock invalidated: {why}");
    if let Err(e) = disable(keychain, data_dir) {
        log::warn!("[BIOMETRIC] {e}");
    }
    AppError::Auth(AUTH_BIOMETRIC_INVALIDATED.into())
}

/// Record "off" after a failed store. A failure is logged: the lock screen then still offers
/// biometrics, and the first biometric unlock finds nothing stored and switches them off.
fn mark_off(data_dir: &Path) {
    if let Err(e) = write_marker(data_dir, false) {
        log::warn!("[BIOMETRIC] Could not record that biometric unlock is off: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use tempfile::tempdir;

    const KEY: [u8; 32] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
        0x32, 0x10,
    ];
    const KEY_HEX: &str = "00112233445566778899aabbccddeeff0123456789abcdeffedcba9876543210";

    /// A keychain that answers what the test set and records every call.
    struct StubKeychain {
        status: std::result::Result<KeychainStatus, KeychainFailure>,
        store: std::result::Result<(), KeychainFailure>,
        retrieve: std::result::Result<Option<String>, KeychainFailure>,
        remove: std::result::Result<(), KeychainFailure>,
        calls: RefCell<Vec<String>>,
    }

    impl Default for StubKeychain {
        fn default() -> Self {
            Self {
                status: Ok(KeychainStatus {
                    available: true,
                    biometry_type: BiometryType::FaceId,
                }),
                store: Ok(()),
                retrieve: Ok(Some(KEY_HEX.to_string())),
                remove: Ok(()),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl StubKeychain {
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl KeychainLike for StubKeychain {
        fn status(&self) -> std::result::Result<KeychainStatus, KeychainFailure> {
            self.calls.borrow_mut().push("status".into());
            self.status.clone()
        }
        fn store(
            &self,
            key: &str,
            value_hex: &str,
            reason: &str,
        ) -> std::result::Result<(), KeychainFailure> {
            self.calls
                .borrow_mut()
                .push(format!("store {key} {value_hex} {reason}"));
            self.store.clone()
        }
        fn retrieve(
            &self,
            key: &str,
            reason: &str,
        ) -> std::result::Result<Option<String>, KeychainFailure> {
            self.calls
                .borrow_mut()
                .push(format!("retrieve {key} {reason}"));
            self.retrieve.clone()
        }
        fn remove(&self, key: &str) -> std::result::Result<(), KeychainFailure> {
            self.calls.borrow_mut().push(format!("remove {key}"));
            self.remove.clone()
        }
    }

    fn assert_auth(err: AppError, key: &str) {
        match err {
            AppError::Auth(got) => assert_eq!(got, key),
            other => panic!("expected AppError::Auth({key}), got {other:?}"),
        }
    }

    fn enabled_dir() -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        write_marker(dir.path(), true).expect("marker");
        dir
    }

    /// A data dir the marker cannot be written to: a path below a regular file. Keep the returned
    /// guard alive for the test.
    fn unwritable_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "x").expect("file");
        assert!(write_marker(&blocker, false).is_err());
        (dir, blocker)
    }

    /// An `open` that must never run.
    fn never_open(_: &[u8; 32]) -> Result<()> {
        panic!("the database must not be opened");
    }

    // ---- Marker ----

    #[test]
    fn marker_is_absent_until_written() {
        let dir = tempdir().expect("tempdir");
        assert_eq!(read_marker(dir.path()), Marker::Absent);
        write_marker(dir.path(), true).expect("marker");
        assert_eq!(read_marker(dir.path()), Marker::Enabled);
        write_marker(dir.path(), false).expect("marker");
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    #[test]
    fn a_marker_that_is_not_one_reads_as_off() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join(MARKER_FILE), "yes").expect("write");
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
        fs::write(dir.path().join(MARKER_FILE), "1\n").expect("write");
        assert_eq!(read_marker(dir.path()), Marker::Enabled);
    }

    // ---- Status ----

    #[test]
    fn status_reports_the_marker_and_the_keychain() {
        let dir = enabled_dir();
        let status = biometric_status(&StubKeychain::default(), dir.path());
        assert_eq!(
            status,
            BiometricStatus {
                available: true,
                enrolled: true,
                biometry_type: BiometryType::FaceId,
                reason: None,
                supported: true,
            }
        );
    }

    #[test]
    fn status_without_the_marker_is_not_enrolled() {
        let dir = tempdir().expect("tempdir");
        let status = biometric_status(&StubKeychain::default(), dir.path());
        assert!(status.available);
        assert!(!status.enrolled);
    }

    #[test]
    fn unavailable_biometry_carries_the_reason_key() {
        let dir = enabled_dir();
        let keychain = StubKeychain {
            status: Ok(KeychainStatus {
                available: false,
                biometry_type: BiometryType::TouchId,
            }),
            ..StubKeychain::default()
        };
        let status = biometric_status(&keychain, dir.path());
        assert!(!status.available);
        assert!(status.enrolled, "the marker still says on");
        assert_eq!(status.biometry_type, BiometryType::TouchId);
        assert_eq!(status.reason.as_deref(), Some(UNAVAILABLE_REASON));
    }

    #[test]
    fn a_failing_status_call_reads_as_unavailable() {
        let dir = tempdir().expect("tempdir");
        let keychain = StubKeychain {
            status: Err(KeychainFailure::Failed("boom".into())),
            ..StubKeychain::default()
        };
        let status = biometric_status(&keychain, dir.path());
        assert!(!status.available);
        assert_eq!(status.biometry_type, BiometryType::None);
        assert_eq!(status.reason.as_deref(), Some(UNAVAILABLE_REASON));
    }

    #[test]
    fn unsupported_platform_reports_supported_false() {
        let dir = enabled_dir();
        let keychain = StubKeychain {
            status: Err(KeychainFailure::Unsupported),
            ..StubKeychain::default()
        };
        let status = biometric_status(&keychain, dir.path());
        assert!(!status.supported);
        assert!(!status.available);
        assert!(
            status.enrolled,
            "the marker is read regardless of the platform"
        );
        assert_eq!(
            status.reason, None,
            "nothing to explain without the feature"
        );
    }

    #[test]
    fn unavailable_biometrics_are_supported_but_not_available() {
        let dir = tempdir().expect("tempdir");
        let keychain = StubKeychain {
            status: Ok(KeychainStatus {
                available: false,
                biometry_type: BiometryType::None,
            }),
            ..StubKeychain::default()
        };
        let status = biometric_status(&keychain, dir.path());
        assert!(status.supported);
        assert!(!status.available);
        assert_eq!(status.reason.as_deref(), Some(UNAVAILABLE_REASON));
    }

    #[test]
    fn keychain_failure_other_than_unsupported_is_supported() {
        let dir = tempdir().expect("tempdir");
        let keychain = StubKeychain {
            status: Err(KeychainFailure::Failed("x".into())),
            ..StubKeychain::default()
        };
        let status = biometric_status(&keychain, dir.path());
        assert!(status.supported);
        assert!(!status.available);
    }

    // ---- Enable ----

    #[test]
    fn enable_stores_the_session_key_and_sets_the_marker() {
        let dir = tempdir().expect("tempdir");
        let session = SessionKey::default();
        session.set(&KEY);
        let keychain = StubKeychain::default();

        enable(&keychain, &session, dir.path(), "Turn on").expect("enable");

        assert_eq!(
            keychain.calls(),
            vec![format!("store {MASTER_KEY_ITEM} {KEY_HEX} Turn on")]
        );
        assert_eq!(read_marker(dir.path()), Marker::Enabled);
        assert!(session.hex().is_some(), "the session keeps its key");
    }

    #[test]
    fn enable_without_an_unlocked_session_stores_nothing() {
        let dir = tempdir().expect("tempdir");
        let keychain = StubKeychain::default();
        let err = enable(&keychain, &SessionKey::default(), dir.path(), "r").unwrap_err();
        assert!(matches!(err, AppError::Internal(_)), "{err:?}");
        assert!(keychain.calls().is_empty());
        assert_eq!(read_marker(dir.path()), Marker::Absent);
    }

    #[test]
    fn a_cancelled_store_leaves_biometric_unlock_off() {
        let dir = enabled_dir();
        let session = SessionKey::default();
        session.set(&KEY);
        let keychain = StubKeychain {
            store: Err(KeychainFailure::Cancelled),
            ..StubKeychain::default()
        };
        let err = enable(&keychain, &session, dir.path(), "r").unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_CANCELLED);
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    #[test]
    fn an_invalidated_store_is_the_invalidated_auth_error_and_leaves_biometric_unlock_off() {
        let dir = enabled_dir();
        let session = SessionKey::default();
        session.set(&KEY);
        let keychain = StubKeychain {
            store: Err(KeychainFailure::Invalidated),
            ..StubKeychain::default()
        };
        let err = enable(&keychain, &session, dir.path(), "r").unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_INVALIDATED);
        assert_eq!(
            fs::read_to_string(dir.path().join(MARKER_FILE)).expect("marker"),
            "0"
        );
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    /// Final review I1: no usable biometry at the moment of "Zapnout" (permission denied, nothing
    /// enrolled) is an answer the sheet shows as "not switched on", not an internal error.
    #[test]
    fn an_unavailable_store_is_the_failed_auth_error_and_leaves_biometric_unlock_off() {
        let dir = enabled_dir();
        let session = SessionKey::default();
        session.set(&KEY);
        let keychain = StubKeychain {
            store: Err(KeychainFailure::Unavailable),
            ..StubKeychain::default()
        };
        let err = enable(&keychain, &session, dir.path(), "r").unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_FAILED);
        assert_eq!(
            fs::read_to_string(dir.path().join(MARKER_FILE)).expect("marker"),
            "0"
        );
    }

    /// Final review I1: a face that was not recognised (or a lockout) while switching on.
    #[test]
    fn a_failed_store_is_the_failed_auth_error_and_leaves_biometric_unlock_off() {
        let dir = enabled_dir();
        let session = SessionKey::default();
        session.set(&KEY);
        let keychain = StubKeychain {
            store: Err(KeychainFailure::Failed("authentication failed".into())),
            ..StubKeychain::default()
        };
        let err = enable(&keychain, &session, dir.path(), "r").unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_FAILED);
        assert_eq!(
            fs::read_to_string(dir.path().join(MARKER_FILE)).expect("marker"),
            "0"
        );
    }

    #[test]
    fn an_unsupported_store_is_an_internal_error_and_leaves_biometric_unlock_off() {
        let dir = tempdir().expect("tempdir");
        let session = SessionKey::default();
        session.set(&KEY);
        let keychain = StubKeychain {
            store: Err(KeychainFailure::Unsupported),
            ..StubKeychain::default()
        };
        let err = enable(&keychain, &session, dir.path(), "r").unwrap_err();
        assert!(matches!(err, AppError::Internal(_)), "{err:?}");
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    #[test]
    fn a_failed_marker_write_takes_the_stored_key_back() {
        let (_guard, data_dir) = unwritable_dir();
        let session = SessionKey::default();
        session.set(&KEY);
        let keychain = StubKeychain::default();

        let err = enable(&keychain, &session, &data_dir, "r").unwrap_err();

        assert!(matches!(err, AppError::Internal(_)), "{err:?}");
        assert_eq!(
            keychain.calls(),
            vec![
                format!("store {MASTER_KEY_ITEM} {KEY_HEX} r"),
                format!("remove {MASTER_KEY_ITEM}"),
            ],
            "the key stored a moment ago is removed again"
        );
    }

    // ---- Disable ----

    #[test]
    fn disable_removes_the_key_and_clears_the_marker() {
        let dir = enabled_dir();
        let keychain = StubKeychain::default();
        disable(&keychain, dir.path()).expect("disable");
        assert_eq!(keychain.calls(), vec![format!("remove {MASTER_KEY_ITEM}")]);
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    #[test]
    fn disable_stays_off_when_the_removal_fails() {
        let dir = enabled_dir();
        let keychain = StubKeychain {
            remove: Err(KeychainFailure::Failed("busy".into())),
            ..StubKeychain::default()
        };
        disable(&keychain, dir.path()).expect("off is off; the next password unlock retries");
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    /// Without this the stale item and a marker stuck at "1" would send every lock screen into
    /// the same invalidated answer.
    #[test]
    fn disable_removes_the_key_even_when_the_marker_cannot_be_written() {
        let (_guard, data_dir) = unwritable_dir();
        let keychain = StubKeychain::default();

        let err = disable(&keychain, &data_dir).unwrap_err();

        assert!(matches!(err, AppError::Internal(_)), "{err:?}");
        assert_eq!(keychain.calls(), vec![format!("remove {MASTER_KEY_ITEM}")]);
    }

    // ---- Unlock with biometrics ----

    #[test]
    fn unlock_hands_the_stored_key_to_open() {
        let dir = enabled_dir();
        let keychain = StubKeychain::default();
        let opened = unlock_with_biometrics(&keychain, dir.path(), "Unlock Moony", |key| {
            assert_eq!(key, &KEY);
            Ok("profile")
        })
        .expect("unlock");
        assert_eq!(opened, "profile");
        assert_eq!(
            keychain.calls(),
            vec![format!("retrieve {MASTER_KEY_ITEM} Unlock Moony")]
        );
        assert_eq!(read_marker(dir.path()), Marker::Enabled);
    }

    /// Review focus 3: an invalidated enrolment falls back to the password, never a loop.
    #[test]
    fn an_invalidated_key_is_the_invalidated_auth_error_and_switches_biometrics_off() {
        let dir = enabled_dir();
        let keychain = StubKeychain {
            retrieve: Err(KeychainFailure::Invalidated),
            ..StubKeychain::default()
        };
        let err = unlock_with_biometrics(&keychain, dir.path(), "r", never_open).unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_INVALIDATED);
        assert!(keychain
            .calls()
            .contains(&format!("remove {MASTER_KEY_ITEM}")));
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
        // The lock screen now offers the password only.
        assert!(!biometric_status(&keychain, dir.path()).enrolled);
    }

    #[test]
    fn a_marker_without_a_stored_key_counts_as_invalidated() {
        let dir = enabled_dir();
        let keychain = StubKeychain {
            retrieve: Ok(None),
            ..StubKeychain::default()
        };
        let err = unlock_with_biometrics(&keychain, dir.path(), "r", never_open).unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_INVALIDATED);
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    #[test]
    fn an_unreadable_stored_key_counts_as_invalidated() {
        let dir = enabled_dir();
        let keychain = StubKeychain {
            retrieve: Ok(Some("not-hex".into())),
            ..StubKeychain::default()
        };
        let err = unlock_with_biometrics(&keychain, dir.path(), "r", never_open).unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_INVALIDATED);
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    #[test]
    fn a_stored_key_that_no_longer_opens_the_profile_counts_as_invalidated() {
        let dir = enabled_dir();
        let keychain = StubKeychain::default();
        let err = unlock_with_biometrics(&keychain, dir.path(), "r", |_| -> Result<()> {
            Err(AppError::Auth(AUTH_KEY_FILES_CORRUPTED.into()))
        })
        .unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_INVALIDATED);
        assert_eq!(read_marker(dir.path()), Marker::Disabled);
    }

    #[test]
    fn other_open_errors_pass_through_and_keep_biometrics_on() {
        let dir = enabled_dir();
        let keychain = StubKeychain::default();
        let err = unlock_with_biometrics(&keychain, dir.path(), "r", |_| -> Result<()> {
            Err(AppError::Database("migration failed".into()))
        })
        .unwrap_err();
        assert!(matches!(err, AppError::Database(_)), "{err:?}");
        assert_eq!(read_marker(dir.path()), Marker::Enabled);
    }

    #[test]
    fn a_cancelled_prompt_keeps_biometrics_on() {
        let dir = enabled_dir();
        let keychain = StubKeychain {
            retrieve: Err(KeychainFailure::Cancelled),
            ..StubKeychain::default()
        };
        let err = unlock_with_biometrics(&keychain, dir.path(), "r", never_open).unwrap_err();
        assert_auth(err, AUTH_BIOMETRIC_CANCELLED);
        assert_eq!(read_marker(dir.path()), Marker::Enabled);
        assert!(!keychain.calls().iter().any(|c| c.starts_with("remove")));
    }

    /// Task 1.7 review: `Failed` (e.g. LAError.authenticationFailed) is a path a person can hit.
    #[test]
    fn a_failed_or_unavailable_prompt_is_the_failed_auth_error_and_keeps_biometrics_on() {
        for failure in [
            KeychainFailure::Failed("authentication failed".into()),
            KeychainFailure::Unavailable,
        ] {
            let dir = enabled_dir();
            let keychain = StubKeychain {
                retrieve: Err(failure),
                ..StubKeychain::default()
            };
            let err = unlock_with_biometrics(&keychain, dir.path(), "r", never_open).unwrap_err();
            assert_auth(err, AUTH_BIOMETRIC_FAILED);
            assert_eq!(read_marker(dir.path()), Marker::Enabled);
        }
    }

    #[test]
    fn unlock_while_switched_off_never_prompts() {
        for marker in [None, Some(false)] {
            let dir = tempdir().expect("tempdir");
            if let Some(enabled) = marker {
                write_marker(dir.path(), enabled).expect("marker");
            }
            let keychain = StubKeychain::default();
            let err = unlock_with_biometrics(&keychain, dir.path(), "r", never_open).unwrap_err();
            assert_auth(err, AUTH_BIOMETRIC_INVALIDATED);
            assert!(
                keychain.calls().is_empty(),
                "no prompt: {:?}",
                keychain.calls()
            );
        }
    }

    // ---- Leftovers ----

    #[test]
    fn a_password_unlock_removes_a_key_left_without_the_marker() {
        for marker in [None, Some(false)] {
            let dir = tempdir().expect("tempdir");
            if let Some(enabled) = marker {
                write_marker(dir.path(), enabled).expect("marker");
            }
            let keychain = StubKeychain::default();
            remove_leftover_key(&keychain, dir.path());
            assert_eq!(keychain.calls(), vec![format!("remove {MASTER_KEY_ITEM}")]);
        }
    }

    #[test]
    fn a_password_unlock_keeps_the_key_while_biometrics_are_on() {
        let dir = enabled_dir();
        let keychain = StubKeychain::default();
        remove_leftover_key(&keychain, dir.path());
        assert!(keychain.calls().is_empty());
        assert_eq!(read_marker(dir.path()), Marker::Enabled);
    }

    #[test]
    fn a_failing_leftover_removal_is_only_logged() {
        let dir = tempdir().expect("tempdir");
        let keychain = StubKeychain {
            remove: Err(KeychainFailure::Unsupported),
            ..StubKeychain::default()
        };
        // Returns (a unit, nothing to propagate); the failure only reaches the log.
        remove_leftover_key(&keychain, dir.path());
        assert_eq!(keychain.calls(), vec![format!("remove {MASTER_KEY_ITEM}")]);
        assert_eq!(read_marker(dir.path()), Marker::Absent);
    }
}
