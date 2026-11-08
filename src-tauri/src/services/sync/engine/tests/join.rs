//! Joining a vault by replace (spec §8, §11.3 D22) and repairing a capture-broken profile (ruling
//! P3): the joiner's cursor, identity, binding and marks; what a refused or failed join leaves
//! behind; the repair's backup, drain and re-join, and every way the drain can stop it.

use std::path::Path;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use zeroize::Zeroizing;

use super::*;
use crate::services::backup;
use crate::services::sync::binding::{
    self, startup_check, BindingFailure, BindingNative, NoBinding, StartupCheck, REJOIN_MARKER,
};
use crate::services::sync::capture;
use crate::services::sync::devices;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::join::{join_by_replace, repair_capture};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;
use crate::services::sync::snapshot::RestoreReport;
use crate::services::sync::test_support::{assert_converged, noise};

/// Seals by XOR with one byte: a binding that opens on this "machine" only.
struct XorNative;

impl BindingNative for XorNative {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        Ok(secret.iter().map(|b| b ^ 0x5a).collect())
    }

    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        let mut out = Zeroizing::new([0u8; 32]);
        if blob.len() != out.len() {
            return Err(BindingFailure::Failed("wrong length".into()));
        }
        for (o, b) in out.iter_mut().zip(blob) {
            *o = b ^ 0x5a;
        }
        Ok(out)
    }
}

async fn round(vault: &Vault, device: &TestConn, mode: RoundMode) -> RoundReport {
    run_round(
        &vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        mode,
        NOW,
        0,
    )
    .await
    .expect("round")
}

/// `A` writes `id` and uploads its snapshot in a first-snapshot round; returns the snapshot's `S`.
async fn snapshot_of_a(vault: &Vault, id: &str) -> u64 {
    institution(&vault.a.lock(), id);
    let report = round(vault, &vault.a, RoundMode::FirstSnapshot).await;
    assert!(report.snapshot_uploaded, "{report:?}");
    let seq = vault.relay.snapshot_seq().expect("the vault's snapshot");
    assert_eq!(seq, cursor(&vault.a.lock()), "S is A's applied cursor");
    seq
}

async fn join(
    vault: &Vault,
    device: &TestConn,
    dir: &Path,
    native: &dyn BindingNative,
) -> SyncResult<RestoreReport> {
    join_by_replace(
        &vault.relay,
        device,
        vault.creds.clone(),
        "Laptop",
        dir,
        native,
        NOW,
    )
    .await
}

fn device_of(conn: &Connection) -> DeviceId {
    state::load(conn).expect("state").expect("row").device
}

fn validation_key(error: &SyncError) -> Option<&str> {
    match error {
        SyncError::Local(AppError::Validation(key)) => Some(key),
        _ => None,
    }
}

/// Everything a refused or failed join must leave as it was: the `sync_state` row, the heads,
/// the own log, the unsent envelopes, the outbox, the device rows and the institutions.
fn profile(conn: &Connection) -> Vec<String> {
    let mut out = sync_rows(conn);
    let mut stmt = conn
        .prepare("SELECT id, public_key, name FROM sync_devices ORDER BY id")
        .expect("devices");
    let rows = stmt
        .query_map([], |r| {
            Ok(format!(
                "sync_devices: {:?} {:?} {:?}",
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?
            ))
        })
        .expect("devices");
    out.extend(rows.map(|r| r.expect("device row")));
    out
}

/// What a stopped repair must leave as it was (ruling P3): the device and its chain, the cursor
/// and heads, the pending changes in the outbox and `sync_unsent`, the capture mark and every
/// synced row. The round's own bookkeeping (`last_error`) may change.
fn repair_state(conn: &Connection) -> Vec<String> {
    let mut out: Vec<String> = sync_rows(conn)
        .into_iter()
        .filter(|line| !line.starts_with("SELECT * FROM sync_state"))
        .collect();
    let sync = state::load(conn).expect("state").expect("row");
    let marks = state::marks(conn).expect("marks").expect("row");
    out.push(format!(
        "device {:?} next_seq {} last_hash {:?} cursor {} capture_broken_at {:?}",
        sync.device, sync.next_seq, sync.last_hash, sync.cursor, marks.capture_broken_at
    ));
    out
}

fn repair_backups(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("data dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("sync-repair-"))
        .collect();
    names.sort();
    names
}

/// Makes `device` a capture-broken profile as an unlock leaves it: capture not installed on the
/// connection, `capture_broken_at` set.
fn break_capture(device: &TestConn) {
    let conn = device.lock();
    capture::uninstall(&conn).expect("uninstall");
    state::mark_capture_broken(&conn, NOW - 60).expect("mark");
}

#[tokio::test]
async fn a_joined_device_matches_the_vault_and_starts_after_the_snapshot() {
    let vault = vault().await;
    let s = snapshot_of_a(&vault, "a1").await;
    institution(&vault.a.lock(), "a2");
    round(&vault, &vault.a, RoundMode::Normal).await;
    assert_eq!(vault.relay.ops().last().map(|op| op.seq), Some(s + 1));
    let joiner = TestConn::new(migrated());
    institution(&joiner.lock(), "local-only");
    let dir = tempfile::tempdir().expect("dir");

    join(&vault, &joiner, dir.path(), &NoBinding)
        .await
        .expect("join");

    {
        let c = joiner.lock();
        assert_eq!(cursor(&c), s, "the joiner starts at the snapshot's S");
        assert!(has_institution(&c, "a1"));
        assert!(!has_institution(&c, "a2"), "pulled after the join");
        assert!(!has_institution(&c, "local-only"), "replaced, not merged");
        assert!(state::load(&c).expect("state").expect("row").enabled);
        assert!(capture::is_installed(&c).expect("installed"));
    }
    // The relay drops everything up to S: a pull asking below S would need a snapshot (`410`).
    vault.relay.prune_to(s);
    let first = round(&vault, &joiner, RoundMode::Normal).await;
    assert!(!first.restored, "the first pull asked after S: {first:?}");
    assert_eq!(first.problem, None);
    assert_eq!(first.pull.as_ref().map(|p| p.applied), Some(1), "only a2");
    round(&vault, &vault.a, RoundMode::Normal).await;
    assert_converged(&[&*vault.a.lock(), &*joiner.lock()]);
}

#[tokio::test]
async fn the_joiner_records_itself_with_a_public_key_and_pushes_it() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let joiner = TestConn::new(migrated());
    let dir = tempfile::tempdir().expect("dir");

    join_by_replace(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "  Work laptop ",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");

    let (id, public_key) = {
        let c = joiner.lock();
        let id = device_of(&c).to_hex();
        let public_key = URL_SAFE_NO_PAD.encode(keys::ensure_device_keypair(&c).expect("key"));
        let row: (String, String, String, i64) = c
            .query_row(
                "SELECT public_key, name, platform, joined_at FROM sync_devices WHERE id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("the joiner's row");
        assert_eq!(
            row,
            (
                public_key.clone(),
                "Work laptop".to_string(),
                devices::platform().to_string(),
                NOW
            )
        );
        assert_eq!(
            count(
                &c,
                &format!(
                    "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'sync_devices' AND pk = '{id}'"
                )
            ),
            1,
            "queued for the next push"
        );
        (id, public_key)
    };
    let pushed = round(&vault, &joiner, RoundMode::Normal).await;
    assert_eq!(pushed.push.as_ref().map(|p| p.pushed), Some(1));
    round(&vault, &vault.a, RoundMode::Normal).await;
    let a = vault.a.lock();
    let seen: String = a
        .query_row(
            "SELECT public_key FROM sync_devices WHERE id = ?1",
            [&id],
            |r| r.get(0),
        )
        .expect("A sees the joiner");
    assert_eq!(seen, public_key);
}

#[tokio::test]
async fn joining_a_vault_without_a_snapshot_is_refused_and_changes_nothing() {
    let vault = vault().await;
    assert_eq!(vault.relay.snapshot_seq(), None);
    let joiner = TestConn::new(migrated());
    institution(&joiner.lock(), "local-only");
    let dir = tempfile::tempdir().expect("dir");
    binding::write_rejoin_marker(dir.path()).expect("marker");
    let before = profile(&joiner.lock());

    let error = join(&vault, &joiner, dir.path(), &NoBinding)
        .await
        .expect_err("no snapshot");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncVaultHasNoSnapshot"),
        "{error:?}"
    );
    let c = joiner.lock();
    assert_eq!(profile(&c), before);
    assert!(state::load(&c).expect("state").is_none(), "no sync row");
    assert!(keys::credentials(&c).expect("credentials").is_none());
    assert!(dir.path().join(REJOIN_MARKER).exists(), "the marker stays");
}

#[tokio::test]
async fn joining_clears_pauses_and_counters() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    {
        let b = vault.b.lock();
        institution(&b, "b1");
        seal_from(&b, &vault.creds);
        exec(
            &b,
            "UPDATE sync_state SET vault_gone_at = 11, capture_broken_at = 12, \
               snapshot_uploaded_at = 13, snapshot_too_large_at = 14, pulled_bytes = 500, \
               pulled_bytes_base = 7, binding_written = 1 WHERE id = 1",
        );
        assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_own_log"), 1);
    }
    let dir = tempfile::tempdir().expect("dir");

    join(&vault, &vault.b, dir.path(), &NoBinding)
        .await
        .expect("join");

    let b = vault.b.lock();
    let marks = state::marks(&b).expect("marks").expect("row");
    assert_eq!(
        (
            marks.vault_gone_at,
            marks.capture_broken_at,
            marks.snapshot_uploaded_at,
            marks.snapshot_too_large_at,
            marks.pulled_bytes,
            marks.pulled_bytes_base,
            marks.binding_written,
        ),
        (None, None, None, None, 0, 0, false)
    );
    let sync = state::load(&b).expect("state").expect("row");
    assert_ne!(sync.device, B, "a new device ID");
    assert_eq!((sync.next_seq, sync.last_hash), (1, None), "a new chain");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_own_log"), 0);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_unsent"), 0);
    assert!(
        !has_institution(&b, "b1"),
        "the unsent change went with the data"
    );
}

#[tokio::test]
async fn a_join_writes_the_binding_of_the_new_device_and_clears_the_marker() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    binding::write_binding(&vault.b.lock(), dir.path(), &XorNative).expect("old binding");
    binding::write_rejoin_marker(dir.path()).expect("marker");

    join(&vault, &vault.b, dir.path(), &XorNative)
        .await
        .expect("join");

    let b = vault.b.lock();
    assert_ne!(device_of(&b), B);
    assert!(
        !dir.path().join(REJOIN_MARKER).exists(),
        "binding::join cleared it"
    );
    assert!(
        state::marks(&b)
            .expect("marks")
            .expect("row")
            .binding_written
    );
    assert_eq!(
        startup_check(&b, dir.path(), &XorNative).expect("check"),
        StartupCheck::Fine,
        "the binding matches the new device ID"
    );
}

#[tokio::test]
async fn joining_another_vault_starts_its_chain_and_cursor_over() {
    let vault = vault().await;
    // B is well into the first vault: a cursor of 3 and a chain of its own.
    let cts = {
        let a = vault.a.lock();
        institution(&a, "a1");
        let first = seal_from(&a, &vault.creds);
        institution(&a, "a2");
        vec![first, seal_from(&a, &vault.creds)]
    };
    push(&vault.relay, &vault.creds, &cts).await;
    institution(&vault.b.lock(), "b1");
    round(&vault, &vault.b, RoundMode::Normal).await;
    assert_eq!(cursor(&vault.b.lock()), 3);
    assert!(head(&vault.b.lock(), A).is_some());

    // A second vault, re-created with a lower head: its snapshot is at S = 1.
    let second = Credentials {
        relay_url: vault.creds.relay_url.clone(),
        account: vault.creds.account.clone(),
        vault_id: VaultId::random().expect("vault id"),
        keys: VaultKeys::new_random().expect("vault keys"),
    };
    assert!(vault
        .relay
        .create_vault(&second.account, &second.auth())
        .await
        .expect("vault"));
    let d = TestConn::new(migrated());
    {
        let conn = d.lock();
        keys::save_credentials(&conn, &second).expect("credentials D");
        state::enable(&conn, C).expect("enable D");
        exec(&conn, "DELETE FROM sync_outbox");
        institution(&conn, "d1");
    }
    let other = Vault {
        relay: vault.relay.clone(),
        creds: second.clone(),
        a: d.clone(),
        b: vault.b.clone(),
    };
    let first = round(&other, &d, RoundMode::FirstSnapshot).await;
    assert!(first.snapshot_uploaded);
    let s = cursor(&d.lock());
    assert_eq!(s, 1);

    join(
        &other,
        &vault.b,
        tempfile::tempdir().expect("dir").path(),
        &NoBinding,
    )
    .await
    .expect("join");

    {
        let b = vault.b.lock();
        assert_eq!(
            cursor(&b),
            1,
            "the cursor is the new vault's S, not the old one's"
        );
        assert_eq!(head(&b, A), None, "no head of the old vault");
        assert_eq!(
            keys::credentials(&b)
                .expect("credentials")
                .expect("bound")
                .vault_id,
            second.vault_id
        );
        assert!(has_institution(&b, "d1") && !has_institution(&b, "a1"));
    }
    institution(&d.lock(), "d2");
    round(&other, &d, RoundMode::Normal).await;
    let next = round(&other, &vault.b, RoundMode::Normal).await;
    assert_eq!(next.problem, None, "{next:?}");
    assert_eq!(next.pull.as_ref().map(|p| p.applied), Some(1));
    assert!(has_institution(&vault.b.lock(), "d2"));
}

#[tokio::test]
async fn a_join_that_fails_leaves_the_profile_as_it_was() {
    const FAILING_NOW: i64 = 4_242;
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    binding::write_rejoin_marker(dir.path()).expect("marker");
    {
        let b = vault.b.lock();
        institution(&b, "b-local");
        // The last step of the join, recording the new device, fails.
        exec(
            &b,
            &format!(
                "CREATE TRIGGER fail_join BEFORE INSERT ON sync_devices \
                 WHEN NEW.joined_at = {FAILING_NOW} BEGIN SELECT RAISE(ABORT, 'injected'); END;"
            ),
        );
    }
    let before = profile(&vault.b.lock());

    let result = join_by_replace(
        &vault.relay,
        &vault.b,
        vault.creds.clone(),
        "Laptop",
        dir.path(),
        &XorNative,
        FAILING_NOW,
    )
    .await;

    assert!(result.is_err(), "{result:?}");
    let b = vault.b.lock();
    assert_eq!(profile(&b), before);
    assert!(
        dir.path().join(REJOIN_MARKER).exists(),
        "the marker is back"
    );
    assert!(!dir.path().join(binding::BINDING_FILE).exists());
    // Capture still runs under the old device.
    institution(&b, "after");
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'institutions' AND pk = 'after'"
        ),
        1
    );
}

#[tokio::test]
async fn a_join_keeps_the_relay_address_of_its_credentials() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let joiner = TestConn::new(migrated());
    let dir = tempfile::tempdir().expect("dir");
    let mut custom = vault.creds.clone();
    custom.relay_url = "https://sync.example.org".into();

    join_by_replace(
        &vault.relay,
        &joiner,
        custom.clone(),
        "Laptop",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");
    assert_eq!(
        keys::credentials(&joiner.lock())
            .expect("credentials")
            .expect("bound")
            .relay_url,
        "https://sync.example.org"
    );

    // An address the device could never sync with is refused before the relay is asked.
    let fresh = TestConn::new(migrated());
    let mut plain_http = vault.creds.clone();
    plain_http.relay_url = "http://sync.example.org".into();
    let gets = vault
        .relay
        .calls()
        .iter()
        .filter(|r| **r == Route::GetSnapshot)
        .count();
    let error = join_by_replace(
        &vault.relay,
        &fresh,
        plain_http,
        "Laptop",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("refused");
    assert_eq!(
        validation_key(&error),
        Some("validation.syncRelayUrlInvalid")
    );
    assert_eq!(
        vault
            .relay
            .calls()
            .iter()
            .filter(|r| **r == Route::GetSnapshot)
            .count(),
        gets
    );
    assert!(state::load(&fresh.lock()).expect("state").is_none());
}

#[tokio::test]
async fn repair_backs_up_drains_and_rejoins_under_a_new_device() {
    let vault = vault().await;
    let s = snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    {
        let b = vault.b.lock();
        devices::record_this_device(&b, "Phone", &[7; 32], NOW).expect("B's row");
        institution(&b, "b1");
    }
    break_capture(&vault.b);
    // Written while capture is off: not queued, kept only in the backup.
    institution(&vault.b.lock(), "b-uncaptured");
    let dir = tempfile::tempdir().expect("dir");

    let zip = repair_capture(&vault.relay, &vault.b, dir.path(), &XorNative, NOW)
        .await
        .expect("repair");

    assert_eq!(zip, dir.path().join(format!("sync-repair-{NOW}.zip")));
    let inspection =
        backup::inspect_backup(&zip, &crate::db::known_migration_names()).expect("readable");
    assert!(!inspection.newer_than_app);
    // B's captured rows reached the relay under its old ID before the replace.
    let pushed_by_b = vault.relay.ops().iter().filter(|op| op.seq > s).count();
    assert_eq!(pushed_by_b, 1, "the drained envelope");
    {
        let b = vault.b.lock();
        let sync = state::load(&b).expect("state").expect("row");
        assert_ne!(sync.device, B, "a new device ID");
        assert_eq!(sync.cursor, s);
        let marks = state::marks(&b).expect("marks").expect("row");
        assert_eq!(marks.capture_broken_at, None);
        assert!(marks.binding_written);
        assert!(capture::is_installed(&b).expect("installed"));
        assert!(
            !has_institution(&b, "b-uncaptured"),
            "replaced by the vault's data"
        );
        assert_eq!(
            keys::credentials(&b)
                .expect("credentials")
                .expect("bound")
                .relay_url,
            vault.creds.relay_url,
            "the stored relay address is kept"
        );
        assert_eq!(
            devices::this_device(&b).expect("row").map(|(name, _)| name),
            Some("Phone".to_string()),
            "the new device keeps the name"
        );
    }
    let again = round(&vault, &vault.b, RoundMode::Normal).await;
    assert_eq!(again.problem, None, "{again:?}");
    assert!(
        has_institution(&vault.b.lock(), "b1"),
        "pulled back from the relay"
    );
    round(&vault, &vault.a, RoundMode::Normal).await;
    assert!(has_institution(&vault.a.lock(), "b1"));
    assert_converged(&[&*vault.a.lock(), &*vault.b.lock()]);
    assert_eq!(
        repair_backups(dir.path()),
        vec![format!("sync-repair-{NOW}.zip")]
    );
}

#[tokio::test]
async fn a_second_repair_in_the_same_second_keeps_the_first_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(
        dir.path().join(format!("sync-repair-{NOW}.zip")),
        b"earlier",
    )
    .expect("zip");
    break_capture(&vault.b);

    let zip = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("repair");

    assert_eq!(zip, dir.path().join(format!("sync-repair-{NOW}-2.zip")));
    assert_eq!(
        std::fs::read(dir.path().join(format!("sync-repair-{NOW}.zip"))).expect("earlier"),
        b"earlier"
    );
}

#[derive(Debug, Clone, Copy)]
enum Fault {
    Network,
    Lapsed,
    Gap,
}

/// A capture-broken `B` with one change already sealed and waiting in `sync_unsent`, and `fault`
/// in the drain's way; the repair must stop and leave `B` as it was, but for the backup.
async fn repair_with(fault: Fault) {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    {
        let b = vault.b.lock();
        institution(&b, "b1");
        seal_from(&b, &vault.creds);
    }
    break_capture(&vault.b);
    match fault {
        Fault::Network => vault
            .relay
            .fail_next(Route::Push, RelayError::Network("offline".into())),
        Fault::Lapsed => vault
            .relay
            .fail_next(Route::Push, RelayError::PaymentRequired),
        Fault::Gap => {
            let cts = {
                let a = vault.a.lock();
                institution(&a, "a2");
                let first = seal_from(&a, &vault.creds);
                institution(&a, "a3");
                vec![first, seal_from(&a, &vault.creds)]
            };
            push(&vault.relay, &vault.creds, &cts).await;
            let dropped = vault.relay.ops().last().expect("ops").seq - 1;
            vault.relay.drop_op(dropped);
        }
    }
    let before = repair_state(&vault.b.lock());
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("the drain did not finish");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncRepairDrainIncomplete"),
        "{fault:?}: {error:?}"
    );
    assert_eq!(repair_state(&vault.b.lock()), before, "{fault:?}");
    assert_eq!(
        repair_backups(dir.path()),
        vec![format!("sync-repair-{NOW}.zip")],
        "{fault:?}: the backup stays"
    );
}

#[tokio::test]
async fn repair_stops_when_the_drain_did_not_finish() {
    for fault in [Fault::Network, Fault::Lapsed, Fault::Gap] {
        repair_with(fault).await;
    }
}

#[tokio::test]
async fn repair_stops_while_a_row_is_left_in_the_outbox() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    {
        let b = vault.b.lock();
        exec(
            &b,
            "INSERT INTO watched_stocks (id, ticker) VALUES ('w1', 'AAPL')",
        );
        b.execute(
            "UPDATE watched_stocks SET notes = ?1 WHERE id = 'w1'",
            [noise(300 * 1024, 29)],
        )
        .expect("notes");
    }
    break_capture(&vault.b);
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("the oversized row is still queued");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncRepairDrainIncomplete")
    );
    let b = vault.b.lock();
    assert_eq!(device_of(&b), B);
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'watched_stocks' AND pk = 'w1'"
        ),
        1
    );
    assert!(state::marks(&b)
        .expect("marks")
        .expect("row")
        .capture_broken_at
        .is_some());
}
