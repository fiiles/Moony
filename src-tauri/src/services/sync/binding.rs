//! The restore marker and the device binding (spec §9.3, §11.4; plan correction 10): the two ways
//! an unlock notices, before anything is pushed, that this database is a copy of a device and must
//! re-join under a new device ID.
//!
//! - `restore_backup` cannot edit `sync_state` (the restored database opens only with the archive's
//!   key), so it leaves [`REJOIN_MARKER`] next to the profile and the next unlock sees it.
//! - Where the platform can seal a value to this machine (a Secure Enclave key on macOS, DPAPI with
//!   machine scope on Windows), [`BINDING_FILE`] holds `SHA-256(BINDING_LABEL ‖ device_id)` sealed
//!   that way. A profile moved to another machine cannot open it, and a profile whose device ID
//!   changed underneath it does not match it. Linux has no binding; a copy on the same machine is
//!   left to the relay's own-log check (§9.3).
//!
//! Neither file is part of a backup or replaced by a restore (`backup::managed_names`); Delete
//! Account removes both (`auth::remove_account_files`), and so does [`forget_binding`].

use std::fs;
use std::io;
use std::path::Path;

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::hlc::DeviceId;
use super::state;
use crate::error::{AppError, Result};
use crate::services::crypto;

/// The sealed binding, next to the profile.
pub const BINDING_FILE: &str = "sync-binding.bin";
/// Left by `backup::restore_backup`: the restored database re-joins at the next unlock.
pub const REJOIN_MARKER: &str = "sync-rejoin.marker";
/// Domain separation of the bound value.
pub const BINDING_LABEL: &[u8] = b"moony sync binding v1";

/// Why a native could not seal or open the binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingFailure {
    /// The platform has no machine-bound sealing (Linux, tests).
    Unsupported,
    /// The blob was sealed on another machine (or by another key) and cannot be opened here.
    NotThisDevice,
    /// Anything else. The message is logged, so a native never puts a secret, a path or an ID
    /// in it.
    Failed(String),
}

/// Seals a 32-byte value to this machine and opens it again (Task 14 implements it per platform).
pub trait BindingNative: Send + Sync {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure>;
    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure>;
}

/// No binding: `Unsupported` everywhere (Linux, tests).
pub struct NoBinding;

impl BindingNative for NoBinding {
    fn seal(&self, _secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        Err(BindingFailure::Unsupported)
    }

    fn open(&self, _blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        Err(BindingFailure::Unsupported)
    }
}

/// What the unlock found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupCheck {
    Fine,
    /// The database is a copy: re-join under a new device ID before pushing anything.
    Rejoin(RejoinReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejoinReason {
    /// `restore_backup` replaced the profile.
    RestoreMarker,
    /// The binding is missing, was sealed on another machine, or binds another device ID.
    BindingMismatch,
}

/// Leaves the restore marker next to the profile (empty, flushed to disk).
pub fn write_rejoin_marker(data_dir: &Path) -> io::Result<()> {
    fs::File::create(data_dir.join(REJOIN_MARKER))?.sync_all()
}

/// Removes the restore marker; a missing one is fine.
pub fn clear_rejoin_marker(data_dir: &Path) -> io::Result<()> {
    remove_if_present(&data_dir.join(REJOIN_MARKER))
}

/// The check at unlock, before the engine's first round (and so before anything is pushed).
///
/// Only an enabled profile can re-join. A marker on a profile without sync (a restored backup of
/// a profile that never synced) is just cleared. With sync on: the marker asks for a re-join;
/// otherwise a binding written on this profile (`binding_written`) must be present, open on this
/// machine and equal `SHA-256(BINDING_LABEL ‖ device_id)` (compared in constant time). A platform
/// without a binding never asks, and a native failure is logged once and lets the start go on.
pub fn startup_check(
    conn: &Connection,
    data_dir: &Path,
    native: &dyn BindingNative,
) -> Result<StartupCheck> {
    let sync = state::load(conn)?.filter(|s| s.enabled);
    let Some(sync) = sync else {
        if let Err(e) = clear_rejoin_marker(data_dir) {
            log::warn!("[SYNC] Could not clear the restore marker: {e}");
        }
        return Ok(StartupCheck::Fine);
    };
    if data_dir.join(REJOIN_MARKER).try_exists()? {
        return Ok(StartupCheck::Rejoin(RejoinReason::RestoreMarker));
    }
    let written = state::marks(conn)?.is_some_and(|m| m.binding_written);
    if !written {
        return Ok(StartupCheck::Fine);
    }
    let blob = match fs::read(data_dir.join(BINDING_FILE)) {
        Ok(blob) => blob,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(StartupCheck::Rejoin(RejoinReason::BindingMismatch));
        }
        Err(e) => return Err(e.into()),
    };
    match native.open(&blob) {
        Ok(opened) => {
            let expected = bound_value(sync.device);
            if bool::from(opened[..].ct_eq(&expected[..])) {
                Ok(StartupCheck::Fine)
            } else {
                Ok(StartupCheck::Rejoin(RejoinReason::BindingMismatch))
            }
        }
        Err(BindingFailure::NotThisDevice) => {
            Ok(StartupCheck::Rejoin(RejoinReason::BindingMismatch))
        }
        Err(BindingFailure::Unsupported) => Ok(StartupCheck::Fine),
        Err(BindingFailure::Failed(reason)) => {
            log::warn!("[SYNC] The device binding could not be checked at this start: {reason}");
            Ok(StartupCheck::Fine)
        }
    }
}

/// Seals the binding of this device's ID, writes it next to the profile (owner-only, atomically:
/// `crypto::replace_secret_file`, as the keychain plugin's blob file) and then marks it written,
/// so a crash in between leaves a binding that is not checked rather than one that is checked and
/// missing. A platform without a binding leaves `binding_written = 0`.
pub fn write_binding(conn: &Connection, data_dir: &Path, native: &dyn BindingNative) -> Result<()> {
    let device = state::load(conn)?
        .ok_or_else(|| AppError::Internal("sync binding: sync is not set up".into()))?
        .device;
    let blob = match native.seal(&bound_value(device)) {
        Ok(blob) => blob,
        Err(BindingFailure::Unsupported) => return state::set_binding_written(conn, false),
        Err(BindingFailure::NotThisDevice) => {
            return Err(AppError::Internal(
                "sync binding: this machine refused to seal the binding".into(),
            ));
        }
        Err(BindingFailure::Failed(reason)) => {
            return Err(AppError::Internal(format!(
                "sync binding: could not seal the binding: {reason}"
            )));
        }
    };
    fs::create_dir_all(data_dir)?;
    crypto::replace_secret_file(&data_dir.join(BINDING_FILE), &blob)?;
    state::set_binding_written(conn, true)
}

/// Removes the binding file (and a staging copy an interrupted write left behind) and the restore
/// marker; missing ones are fine.
pub fn forget_binding(data_dir: &Path) -> Result<()> {
    let binding = data_dir.join(BINDING_FILE);
    remove_if_present(&binding)?;
    remove_if_present(&crypto::staging_path(&binding))?;
    clear_rejoin_marker(data_dir)?;
    Ok(())
}

/// `SHA-256(BINDING_LABEL ‖ device_id)`: what the binding seals.
fn bound_value(device: DeviceId) -> Zeroizing<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(BINDING_LABEL);
    hasher.update(device.0);
    Zeroizing::new(hasher.finalize().into())
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::chain::rejoin_device_id;
    use crate::services::sync::test_support::migrated;

    /// Seals by XOR with a per-instance "machine" byte and tags the blob with it; another
    /// machine's blob is `NotThisDevice`.
    struct FakeNative {
        machine: u8,
    }

    impl BindingNative for FakeNative {
        fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
            let mut blob = vec![self.machine];
            blob.extend(secret.iter().map(|b| b ^ self.machine));
            Ok(blob)
        }

        fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
            let (&tag, sealed) = blob
                .split_first()
                .ok_or_else(|| BindingFailure::Failed("empty blob".into()))?;
            if tag != self.machine {
                return Err(BindingFailure::NotThisDevice);
            }
            let mut out = Zeroizing::new([0u8; 32]);
            if sealed.len() != out.len() {
                return Err(BindingFailure::Failed("wrong length".into()));
            }
            for (o, b) in out.iter_mut().zip(sealed) {
                *o = b ^ self.machine;
            }
            Ok(out)
        }
    }

    /// Opens nothing: every call fails without a reason the check can act on.
    struct BrokenNative;

    impl BindingNative for BrokenNative {
        fn seal(&self, _secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
            Err(BindingFailure::Failed("native error".into()))
        }

        fn open(&self, _blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
            Err(BindingFailure::Failed("native error".into()))
        }
    }

    const DEVICE: DeviceId = DeviceId([0x11; 16]);

    fn synced() -> Connection {
        let conn = migrated();
        state::ensure_row(&conn, DEVICE).expect("sync row");
        conn
    }

    fn binding_written(conn: &Connection) -> bool {
        state::marks(conn)
            .expect("marks")
            .expect("sync row")
            .binding_written
    }

    fn expected_binding(device: DeviceId) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"moony sync binding v1");
        hasher.update(device.0);
        hasher.finalize().into()
    }

    #[test]
    fn a_restore_marker_asks_for_a_rejoin() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        let native = FakeNative { machine: 7 };
        write_binding(&conn, dir.path(), &native).expect("binding");

        write_rejoin_marker(dir.path()).expect("marker");

        assert!(dir.path().join("sync-rejoin.marker").exists());
        assert_eq!(
            startup_check(&conn, dir.path(), &native).expect("check"),
            StartupCheck::Rejoin(RejoinReason::RestoreMarker),
            "the marker wins over a binding that still matches"
        );
        clear_rejoin_marker(dir.path()).expect("clear");
        assert!(!dir.path().join("sync-rejoin.marker").exists());
        clear_rejoin_marker(dir.path()).expect("clearing twice is fine");
        assert_eq!(
            startup_check(&conn, dir.path(), &native).expect("check"),
            StartupCheck::Fine
        );
    }

    #[test]
    fn a_marker_without_sync_is_just_cleared() {
        let conn = migrated();
        let dir = tempfile::tempdir().expect("dir");
        write_rejoin_marker(dir.path()).expect("marker");

        assert_eq!(
            startup_check(&conn, dir.path(), &FakeNative { machine: 7 }).expect("check"),
            StartupCheck::Fine
        );
        assert!(!dir.path().join("sync-rejoin.marker").exists());
    }

    #[test]
    fn a_binding_written_on_another_machine_asks_for_a_rejoin() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");

        let sealed = fs::read(dir.path().join("sync-binding.bin")).expect("binding file");
        let mut expected = vec![7u8];
        expected.extend(expected_binding(DEVICE).iter().map(|b| b ^ 7));
        assert_eq!(
            sealed, expected,
            "the file holds the sealed SHA-256 binding"
        );
        assert!(binding_written(&conn));
        assert_eq!(
            startup_check(&conn, dir.path(), &FakeNative { machine: 7 }).expect("check"),
            StartupCheck::Fine
        );

        assert_eq!(
            startup_check(&conn, dir.path(), &FakeNative { machine: 9 }).expect("check"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch)
        );
    }

    #[test]
    fn a_missing_binding_file_after_one_was_written_asks_for_a_rejoin() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        let native = FakeNative { machine: 7 };
        write_binding(&conn, dir.path(), &native).expect("binding");
        fs::remove_file(dir.path().join("sync-binding.bin")).expect("remove");

        assert_eq!(
            startup_check(&conn, dir.path(), &native).expect("check"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch)
        );
    }

    #[test]
    fn a_never_written_binding_is_fine() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        let native = FakeNative { machine: 7 };
        assert!(!binding_written(&conn));

        assert_eq!(
            startup_check(&conn, dir.path(), &native).expect("check"),
            StartupCheck::Fine
        );
        // A stray file from another profile is not this profile's binding.
        fs::write(dir.path().join("sync-binding.bin"), [9u8; 33]).expect("stray file");
        assert_eq!(
            startup_check(&conn, dir.path(), &native).expect("check"),
            StartupCheck::Fine
        );
    }

    #[test]
    fn unsupported_platforms_never_rejoin() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");

        write_binding(&conn, dir.path(), &NoBinding).expect("nothing to write");
        assert!(
            !binding_written(&conn),
            "an unsupported platform writes no binding"
        );
        assert!(!dir.path().join("sync-binding.bin").exists());
        assert_eq!(
            startup_check(&conn, dir.path(), &NoBinding).expect("check"),
            StartupCheck::Fine
        );

        // A profile bound on a supported machine, opened where nothing can open the binding.
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        assert_eq!(
            startup_check(&conn, dir.path(), &NoBinding).expect("check"),
            StartupCheck::Fine
        );
    }

    #[test]
    fn a_native_failure_is_fine() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");

        assert_eq!(
            startup_check(&conn, dir.path(), &BrokenNative).expect("check"),
            StartupCheck::Fine
        );
        let error = write_binding(&conn, dir.path(), &BrokenNative).expect_err("seal fails");
        assert!(matches!(error, AppError::Internal(_)), "{error:?}");
        assert!(
            !error.to_string().contains(&*dir.path().to_string_lossy()),
            "no path in the error"
        );
    }

    #[test]
    fn a_binding_of_the_old_device_id_asks_for_a_rejoin() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        let native = FakeNative { machine: 7 };
        write_binding(&conn, dir.path(), &native).expect("binding");

        // The device ID changed underneath the binding (another copy's row), on the same machine.
        let new = rejoin_device_id(DEVICE).expect("new id");
        state::ensure_row(&conn, new).expect("new device id");
        assert_eq!(
            startup_check(&conn, dir.path(), &native).expect("check"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch)
        );

        // Writing the binding again (after the re-join) binds the new ID.
        write_binding(&conn, dir.path(), &native).expect("binding again");
        assert_eq!(
            startup_check(&conn, dir.path(), &native).expect("check"),
            StartupCheck::Fine
        );
        let opened = native
            .open(&fs::read(dir.path().join("sync-binding.bin")).expect("file"))
            .expect("open");
        assert_eq!(*opened, expected_binding(new));
    }

    #[test]
    fn forget_binding_removes_the_file_and_the_marker() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        write_rejoin_marker(dir.path()).expect("marker");
        fs::write(dir.path().join("sync-binding.bin.tmp"), b"staged").expect("staging file");
        fs::write(dir.path().join("moony.db"), b"db").expect("other file");

        forget_binding(dir.path()).expect("forget");

        let mut left: Vec<String> = fs::read_dir(dir.path())
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["moony.db"]);
        forget_binding(dir.path()).expect("forgetting twice is fine");
    }

    #[cfg(unix)]
    #[test]
    fn the_binding_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        let mode = fs::metadata(dir.path().join("sync-binding.bin"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
