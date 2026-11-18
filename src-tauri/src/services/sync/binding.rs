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
use std::io::{self, Read};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::hlc::DeviceId;
use super::snapshot::{self, RestoreReport};
use super::state;
use crate::error::{AppError, Result};
use crate::services::crypto;

/// The sealed binding, next to the profile.
pub const BINDING_FILE: &str = "sync-binding.bin";
/// Left by `backup::restore_backup`: the restored database re-joins at the next unlock.
pub const REJOIN_MARKER: &str = "sync-rejoin.marker";
/// Domain separation of the bound value.
pub const BINDING_LABEL: &[u8] = b"moony sync binding v1";

/// App starts in a row whose binding check answered a native `Failed` (Local `app_config` key
/// `sync.binding_failed_starts`); reset by any other answer. A start counts once however often the
/// engine retries inside it (the caller says so: see `startup_check_counted`). The third in a row
/// re-joins: a needless re-join is cheap, a check that never passes is not.
pub const FAILED_STARTS_BEFORE_REJOIN: u32 = 3;
/// The error a binding that could not be checked answers with; the engine shows it as its own
/// problem (`is_unchecked`).
const UNCHECKED_MESSAGE: &str = "sync binding: could not be checked at this start";
const FAILED_STARTS_KEY: &str = "sync.binding_failed_starts";

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

/// Leaves the restore marker next to the profile: the empty file is flushed to disk, and then so
/// is the folder, so the marker survives a power cut right after a restore.
pub fn write_rejoin_marker(data_dir: &Path) -> io::Result<()> {
    let marker = data_dir.join(REJOIN_MARKER);
    fs::File::create(&marker)?.sync_all()?;
    crypto::sync_parent_dir(&marker)
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
/// without a binding never writes one, so it asks only for a binding written on another OS (the
/// folder moved here); after that re-join `binding_written` stays 0 and it never asks again. A
/// native `Failed` is an error (no round runs and the next trigger tries again); it never changes
/// the failed-starts count here (the repair and a resumed setup are no app start). The engine's
/// start uses [`startup_check_counted`].
pub fn startup_check(
    conn: &Connection,
    data_dir: &Path,
    native: &dyn BindingNative,
) -> Result<StartupCheck> {
    startup_check_counted(conn, data_dir, native, false)
}

/// The check at an app start (audit M10: it fails closed). With `count_failure` (the first failure
/// of this process) a native `Failed` adds one to the failed-starts count and the
/// `FAILED_STARTS_BEFORE_REJOIN`-th in a row asks for a re-join instead and resets the count;
/// without it (a retry in the same process) the failure is an error and nothing is counted.
pub fn startup_check_counted(
    conn: &Connection,
    data_dir: &Path,
    native: &dyn BindingNative,
    count_failure: bool,
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
        set_failed_starts(conn, 0)?;
        return Ok(StartupCheck::Fine);
    }
    let blob = match fs::read(data_dir.join(BINDING_FILE)) {
        Ok(blob) => blob,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            set_failed_starts(conn, 0)?;
            return Ok(StartupCheck::Rejoin(RejoinReason::BindingMismatch));
        }
        Err(e) => return Err(e.into()),
    };
    let answer = native.open(&blob);
    if !matches!(answer, Err(BindingFailure::Failed(_))) {
        set_failed_starts(conn, 0)?;
    }
    match answer {
        Ok(opened) => {
            let expected = bound_value(sync.device);
            if bool::from(opened[..].ct_eq(&expected[..])) {
                Ok(StartupCheck::Fine)
            } else {
                Ok(StartupCheck::Rejoin(RejoinReason::BindingMismatch))
            }
        }
        // `binding_written = 1` and nothing here can open it: the folder moved from an OS that
        // binds.
        Err(BindingFailure::NotThisDevice | BindingFailure::Unsupported) => {
            Ok(StartupCheck::Rejoin(RejoinReason::BindingMismatch))
        }
        Err(BindingFailure::Failed(reason)) => {
            log::warn!("[SYNC] The device binding could not be checked at this start: {reason}");
            if count_failure {
                let failed = failed_starts(conn)?.saturating_add(1);
                if failed >= FAILED_STARTS_BEFORE_REJOIN {
                    set_failed_starts(conn, 0)?;
                    return Ok(StartupCheck::Rejoin(RejoinReason::BindingMismatch));
                }
                set_failed_starts(conn, failed)?;
            }
            Err(AppError::Internal(UNCHECKED_MESSAGE.into()))
        }
    }
}

/// Whether `error` is the answer of a binding that could not be checked.
pub fn is_unchecked(error: &AppError) -> bool {
    matches!(error, AppError::Internal(m) if m == UNCHECKED_MESSAGE)
}

/// Removes the failed-starts count (turning sync off: `state::disable`, final review B minor 8),
/// so a later setup starts from none.
pub fn forget_failed_starts(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM app_config WHERE key = ?1", [FAILED_STARTS_KEY])?;
    Ok(())
}

fn failed_starts(conn: &Connection) -> Result<u32> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [FAILED_STARTS_KEY],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.and_then(|v| v.parse().ok()).unwrap_or(0))
}

/// Stores the count; zero removes the row, so a healthy profile writes nothing. Written only when
/// it changes (read first): a guard that closed all of `app_config` (its triggers failed) refuses
/// every write there, and a check with nothing to count must not fail on it (final review A
/// minor 4, B minor 3), or the repair's copy check would answer `syncChangesPaused`.
fn set_failed_starts(conn: &Connection, count: u32) -> Result<()> {
    if failed_starts(conn)? == count {
        return Ok(());
    }
    if count == 0 {
        forget_failed_starts(conn)?;
    } else {
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![FAILED_STARTS_KEY, count.to_string()],
        )?;
    }
    Ok(())
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

/// Whether the engine's start must (try to) write the binding: sync is on and no binding is
/// written. A seal that failed for a passing reason when sync was turned on, or after a re-join,
/// would otherwise leave the check off for good; every start tries again. A platform without a
/// binding answers `Unsupported` each time, which leaves `binding_written = 0` and costs nothing.
pub fn binding_due(conn: &Connection) -> Result<bool> {
    if !state::load(conn)?.is_some_and(|s| s.enabled) {
        return Ok(false);
    }
    Ok(!state::marks(conn)?.is_some_and(|m| m.binding_written))
}

/// Turns sync on for the first device of a vault (`state::enable`), then clears a restore marker
/// a restored never-synced backup left behind. `startup_check` runs only for an enabled profile
/// (G5), so without this the first unlock after setup would re-join a device that has nothing to
/// re-join from. Setup (Task 16) calls this, not `state::enable`.
///
/// The marker goes only once sync is on, so a failed setup keeps it. A marker that cannot be
/// removed is logged and left: at worst the next unlock re-joins under another fresh ID.
pub fn enable(conn: &Connection, data_dir: &Path, device: DeviceId) -> Result<()> {
    state::enable(conn, device)?;
    clear_marker_after_start(data_dir);
    Ok(())
}

/// Joins the vault from a snapshot (`snapshot::join`), then clears a restore marker, as
/// [`enable`] does: the joiner has just taken a fresh device ID, so a marker left from an earlier
/// restore has nothing to re-join. Joining (Task 15) calls this, not `snapshot::join`.
pub fn join(
    conn: &Connection,
    data_dir: &Path,
    device: DeviceId,
    input: &mut impl Read,
    now: i64,
) -> Result<RestoreReport> {
    let report = join_leaving_marker(conn, device, input, now)?;
    clear_marker_after_start(data_dir);
    Ok(report)
}

/// [`join`] without the marker: for a caller that runs the join inside its own transaction and
/// clears the marker (`clear_rejoin_marker`) only once that transaction has committed, so a join
/// rolled back with it leaves the marker where it was (`sync::join`'s `replace_profile`).
pub fn join_leaving_marker(
    conn: &Connection,
    device: DeviceId,
    input: &mut impl Read,
    now: i64,
) -> Result<RestoreReport> {
    snapshot::join(conn, device, input, now)
}

fn clear_marker_after_start(data_dir: &Path) {
    if let Err(e) = clear_rejoin_marker(data_dir) {
        log::warn!("[SYNC] Could not clear the restore marker after turning sync on: {e}");
    }
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
    use crate::services::sync::engine::pull::rejoin_as_new_device;
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

        // A stray file does not count while nothing was written here.
        fs::write(dir.path().join("sync-binding.bin"), [7u8; 33]).expect("stray file");
        assert_eq!(
            startup_check(&conn, dir.path(), &NoBinding).expect("check"),
            StartupCheck::Fine
        );
    }

    #[test]
    fn a_binding_from_an_os_that_binds_asks_an_unsupported_platform_for_a_rejoin() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        // Bound on a supported machine; the folder moved to one where nothing can open it.
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        assert!(binding_written(&conn));

        assert_eq!(
            startup_check(&conn, dir.path(), &NoBinding).expect("check"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch)
        );
    }

    /// The Linux no-loop property: a profile bound elsewhere re-joins once, and then never again.
    #[test]
    fn an_unsupported_platform_rejoins_once_and_then_is_fine() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        fs::remove_file(dir.path().join("sync-binding.bin")).expect("the file did not come along");

        assert_eq!(
            startup_check(&conn, dir.path(), &NoBinding).expect("check"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch)
        );
        rejoin_as_new_device(&conn, None, 1_800_000_000).expect("re-join");
        let device = state::load(&conn).expect("load").expect("row").device;
        assert!(device > DEVICE, "the re-join took an ID above the old one");
        write_binding(&conn, dir.path(), &NoBinding).expect("nothing to write");

        assert!(!binding_written(&conn));
        assert_eq!(
            startup_check(&conn, dir.path(), &NoBinding).expect("check"),
            StartupCheck::Fine
        );
        assert_eq!(
            startup_check(&conn, dir.path(), &NoBinding).expect("check"),
            StartupCheck::Fine,
            "and at every start after it"
        );
    }

    #[test]
    fn turning_sync_on_clears_a_marker_from_a_never_synced_backup() {
        let conn = migrated();
        let dir = tempfile::tempdir().expect("dir");
        write_rejoin_marker(dir.path()).expect("marker");

        enable(&conn, dir.path(), DEVICE).expect("enable");

        assert!(!dir.path().join("sync-rejoin.marker").exists());
        assert!(state::load(&conn).expect("load").expect("row").enabled);
        assert_eq!(
            startup_check(&conn, dir.path(), &FakeNative { machine: 7 }).expect("check"),
            StartupCheck::Fine,
            "the first unlock after setup does not re-join"
        );
    }

    #[test]
    fn joining_from_a_snapshot_clears_a_marker() {
        let source = synced();
        let mut bytes = Vec::new();
        snapshot::dump(&source, 1, &mut bytes).expect("dump");
        let joiner = migrated();
        let dir = tempfile::tempdir().expect("dir");
        write_rejoin_marker(dir.path()).expect("marker");

        let joined = DeviceId([0x22; 16]);
        join(&joiner, dir.path(), joined, &mut &bytes[..], 1_800_000_000).expect("join");

        assert!(!dir.path().join("sync-rejoin.marker").exists());
        assert_eq!(
            state::load(&joiner).expect("load").expect("row").device,
            joined
        );
        assert_eq!(
            startup_check(&joiner, dir.path(), &FakeNative { machine: 7 }).expect("check"),
            StartupCheck::Fine
        );
    }

    #[test]
    fn a_failed_join_keeps_the_marker() {
        let joiner = migrated();
        let dir = tempfile::tempdir().expect("dir");
        write_rejoin_marker(dir.path()).expect("marker");

        join(&joiner, dir.path(), DEVICE, &mut &b"not a snapshot"[..], 1).expect_err("refused");

        assert!(dir.path().join("sync-rejoin.marker").exists());
    }

    #[test]
    fn a_binding_is_due_while_sync_is_on_and_none_is_written() {
        let conn = migrated();
        assert!(!binding_due(&conn).expect("no sync row"), "sync is off");

        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        assert!(binding_due(&conn).expect("due"));
        write_binding(&conn, dir.path(), &BrokenNative).expect_err("seal fails");
        assert!(binding_due(&conn).expect("due"), "a failed seal stays due");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        assert!(!binding_due(&conn).expect("written"));
    }

    #[test]
    fn a_binding_that_cannot_be_opened_now_is_an_error() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");

        let error = startup_check(&conn, dir.path(), &BrokenNative).expect_err("fails closed");
        assert!(matches!(error, AppError::Internal(_)), "{error:?}");
        assert_eq!(failed_starts(&conn).expect("count"), 0, "not counted");
        let error = write_binding(&conn, dir.path(), &BrokenNative).expect_err("seal fails");
        assert!(matches!(error, AppError::Internal(_)), "{error:?}");
        assert!(
            !error.to_string().contains(&*dir.path().to_string_lossy()),
            "no path in the error"
        );
    }

    fn counted(conn: &Connection, dir: &Path, native: &dyn BindingNative) -> Result<StartupCheck> {
        startup_check_counted(conn, dir, native, true)
    }

    #[test]
    fn three_failed_starts_in_a_row_rejoin() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");

        for _ in 1..FAILED_STARTS_BEFORE_REJOIN {
            let error = counted(&conn, dir.path(), &BrokenNative).expect_err("fails closed");
            assert!(is_unchecked(&error), "{error:?}");
        }
        assert_eq!(
            counted(&conn, dir.path(), &BrokenNative).expect("third"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch)
        );
        assert_eq!(failed_starts(&conn).expect("count"), 0, "the count resets");
    }

    #[test]
    fn a_retry_in_the_same_start_and_the_repair_do_not_count() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");

        assert!(counted(&conn, dir.path(), &BrokenNative).is_err());
        for _ in 0..5 {
            assert!(startup_check(&conn, dir.path(), &BrokenNative).is_err());
        }
        assert_eq!(failed_starts(&conn).expect("count"), 1);
    }

    #[test]
    fn a_successful_check_resets_the_failed_count() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");

        assert!(counted(&conn, dir.path(), &BrokenNative).is_err());
        assert!(counted(&conn, dir.path(), &BrokenNative).is_err());
        assert_eq!(
            counted(&conn, dir.path(), &FakeNative { machine: 7 }).expect("check"),
            StartupCheck::Fine
        );
        assert_eq!(failed_starts(&conn).expect("count"), 0);
        assert!(counted(&conn, dir.path(), &BrokenNative).is_err());
        assert!(
            counted(&conn, dir.path(), &BrokenNative).is_err(),
            "the count started over: two failures are still errors"
        );
    }

    /// The guard with its triggers failed: the authorizer refuses every write to `app_config`.
    fn close_app_config(conn: &mut Connection) {
        conn.authorizer(Some(|ctx: rusqlite::hooks::AuthContext<'_>| {
            crate::services::sync::capture::guard::decide(&ctx.action, ctx.database_name, true)
        }))
        .expect("authorizer");
        assert!(
            conn.execute("DELETE FROM app_config WHERE key = 'none'", [])
                .is_err(),
            "app_config is closed"
        );
    }

    /// Final review A minor 4, B minor 3: a check with nothing to count writes nothing, so a guard
    /// that closed all of `app_config` does not fail the start check (or the repair's copy check)
    /// of a healthy profile.
    #[test]
    fn a_check_with_nothing_to_count_writes_nothing() {
        let mut conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        close_app_config(&mut conn);

        assert_eq!(
            startup_check(&conn, dir.path(), &FakeNative { machine: 7 }).expect("not written"),
            StartupCheck::Fine
        );

        let mut conn = synced();
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        close_app_config(&mut conn);
        assert_eq!(
            counted(&conn, dir.path(), &FakeNative { machine: 7 }).expect("matches"),
            StartupCheck::Fine
        );
        assert_eq!(
            counted(&conn, dir.path(), &FakeNative { machine: 9 }).expect("another machine"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch)
        );
    }

    /// Final review B, minor 8: turning sync off forgets the count, so a later setup starts at 0.
    #[test]
    fn turning_sync_off_forgets_the_failed_count() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");
        assert!(counted(&conn, dir.path(), &BrokenNative).is_err());
        assert_eq!(failed_starts(&conn).expect("count"), 1);

        state::disable(&conn).expect("off");

        assert_eq!(failed_starts(&conn).expect("count"), 0);
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM app_config WHERE key = 'sync.binding_failed_starts'",
                [],
                |r| r.get(0),
            )
            .expect("count rows");
        assert_eq!(rows, 0);
    }

    #[test]
    fn another_machines_answer_and_a_missing_file_reset_the_failed_count() {
        let conn = synced();
        let dir = tempfile::tempdir().expect("dir");
        write_binding(&conn, dir.path(), &FakeNative { machine: 7 }).expect("binding");

        assert!(counted(&conn, dir.path(), &BrokenNative).is_err());
        assert_eq!(failed_starts(&conn).expect("count"), 1);
        assert_eq!(
            counted(&conn, dir.path(), &FakeNative { machine: 9 }).expect("check"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch),
            "NotThisDevice"
        );
        assert_eq!(failed_starts(&conn).expect("count"), 0);

        assert!(counted(&conn, dir.path(), &BrokenNative).is_err());
        fs::remove_file(dir.path().join(BINDING_FILE)).expect("remove");
        assert_eq!(
            counted(&conn, dir.path(), &BrokenNative).expect("check"),
            StartupCheck::Rejoin(RejoinReason::BindingMismatch),
            "a missing file"
        );
        assert_eq!(failed_starts(&conn).expect("count"), 0);
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
