//! Repairing a capture-broken profile (ruling P3) and re-syncing a device by replace (ruling
//! T8-4): the backup that comes first, the capture retry, the drain and every way it can stop the
//! repair, and the re-join.

use super::join::{
    break_capture, device_of, repair_backups, repair_state, round, snapshot_of_a, validation_key,
    XorNative,
};
use super::runtime::op_devices;
use super::*;
use crate::services::backup;
use crate::services::sync::binding::{write_rejoin_marker, NoBinding, REJOIN_MARKER};
use crate::services::sync::capture::{self, guard};
use crate::services::sync::changeset::from_json;
use crate::services::sync::devices;
use crate::services::sync::engine::RoundMode;
use crate::services::sync::envelope::open_op;
use crate::services::sync::error::SyncError;
use crate::services::sync::join::join_by_replace;
use crate::services::sync::join::{repair_capture, resync_by_replace};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;
use crate::services::sync::test_support::{assert_converged, noise};

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
    // Audit H2: an edit while capture is off would never be queued, and the replace would discard
    // it. The guard refuses it instead, so the person knows it was not saved.
    assert_eq!(
        refused_institution(&vault.b.lock(), "b-uncaptured"),
        Some(guard::CHANGES_PAUSED)
    );
    let dir = tempfile::tempdir().expect("dir");

    let outcome = repair_capture(&vault.relay, &vault.b, dir.path(), &XorNative, NOW)
        .await
        .expect("repair");
    let zip = outcome.backup.clone();

    assert_eq!(zip, dir.path().join(format!("sync-repair-{NOW}-kept.zip")));
    assert_eq!(
        (
            outcome.unsent_items_in_backup_only,
            outcome.unsent_batches_in_backup_only,
            outcome.attachments.orphaned
        ),
        (0, 0, 0),
        "everything captured reached the relay"
    );
    assert!(
        outcome.kept,
        "a repair's backup may hold what a capture-broken session wrote uncaptured"
    );
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
        assert!(!guard::is_imposed(&b), "capture is back: the guard is gone");
        assert!(!has_institution(&b, "b-uncaptured"), "it was never written");
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
    // Every institution B ever wrote is on B and on the relay.
    assert!(relay_has_institution(&vault, "b1"));
    assert!(!relay_has_institution(&vault, "b-uncaptured"));
    institution(&vault.b.lock(), "b-after");
    assert_eq!(outbox_rows(&vault.b.lock(), "b-after"), 1, "captured");
    assert_eq!(
        repair_backups(dir.path()),
        vec![format!("sync-repair-{NOW}-kept.zip")]
    );
}

/// The key of the error an insert of institution `id` ends in, `None` when it went through.
fn refused_institution(conn: &Connection, id: &str) -> Option<&'static str> {
    let inserted = conn.execute(
        "INSERT INTO institutions (id, name) VALUES (?1, 'Bank')",
        [id],
    );
    match inserted.map_err(AppError::from) {
        Ok(_) => None,
        Err(AppError::Validation(key)) if key == guard::CHANGES_PAUSED => {
            Some(guard::CHANGES_PAUSED)
        }
        Err(other) => panic!("not the guard: {other:?}"),
    }
}

/// Whether any op on the relay carries institution `id`.
pub(super) fn relay_has_institution(vault: &Vault, id: &str) -> bool {
    vault.relay.ops().iter().any(|op| {
        let json = open_op(
            |epoch| vault.creds.keys.get(epoch),
            &vault.creds.vault_id.0,
            &op.ct,
        )
        .expect("open");
        from_json(&json)
            .expect("changeset")
            .rows
            .iter()
            .any(|row| row.table == "institutions" && row.key == RowKey::Single(id.into()))
    })
}

fn outbox_rows(conn: &Connection, id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'institutions' AND pk = ?1",
        [id],
        |r| r.get(0),
    )
    .expect("outbox")
}

/// A restored copy whose engine start never re-joined (its check failed, say) must not drain
/// under the original device's ID: the repair runs the start's check before the drain pushes.
#[tokio::test]
async fn a_repair_of_a_restored_copy_rejoins_before_the_drain_pushes() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let head = vault.relay.ops().last().map_or(0, |op| op.seq);
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    let dir = tempfile::tempdir().expect("dir");
    write_rejoin_marker(dir.path()).expect("marker");

    repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("repair");

    let drained = op_devices(&vault, head);
    assert_eq!(drained.len(), 1, "the drained envelope: {drained:?}");
    assert!(
        drained[0] > B,
        "pushed under the re-join's new ID, never under B's: {drained:?}"
    );
    assert!(
        !dir.path().join(REJOIN_MARKER).exists(),
        "the re-join cleared the marker"
    );
    round(&vault, &vault.a, RoundMode::Normal).await;
    assert!(has_institution(&vault.a.lock(), "b1"));
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
        .expect("repair")
        .backup;

    assert_eq!(
        zip,
        dir.path().join(format!("sync-repair-{NOW}-2-kept.zip"))
    );
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
/// in the drain's way; the repair must stop and leave `B` as it was, but for the backup. A gap
/// holds without stopping the drain's pushes (audit M3): `B`'s change is sent, nothing replaced.
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
        // Every drain round meets it: the repair drains while it makes progress.
        Fault::Network => fail_every_round(&vault, Route::Push, || {
            RelayError::Network("offline".into())
        }),
        Fault::Lapsed => fail_every_round(&vault, Route::Push, || RelayError::PaymentRequired),
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
    match fault {
        Fault::Gap => {
            assert!(
                relay_has_institution(&vault, "b1"),
                "the drain still pushed"
            );
            assert_eq!(device_of(&vault.b.lock()), B, "nothing replaced");
        }
        _ => assert_eq!(repair_state(&vault.b.lock()), before, "{fault:?}"),
    }
    assert_eq!(
        repair_backups(dir.path()),
        vec![format!("sync-repair-{NOW}-unused.zip")],
        "{fault:?}: the backup stays, marked unused"
    );
}

/// `route` fails in every drain round a repair may run.
pub(super) fn fail_every_round(vault: &Vault, route: Route, error: impl Fn() -> RelayError) {
    for _ in 0..crate::services::sync::join::MAX_DRAIN_ROUNDS {
        vault.relay.fail_next(route, error());
    }
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

/// Relay calls made after the first `from`, apart from none.
fn calls_since(vault: &Vault, from: usize) -> Vec<Route> {
    vault.relay.calls()[from..].to_vec()
}

fn capture_broken_at(conn: &Connection) -> Option<i64> {
    state::marks(conn)
        .expect("marks")
        .expect("row")
        .capture_broken_at
}

/// Ruling A: a profile whose capture failed at unlock has none installed; the repair installs it
/// again, so the drain can merge what other devices wrote meanwhile.
#[tokio::test]
async fn repair_reinstalls_capture_and_drains_what_arrived_meanwhile() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    institution(&vault.a.lock(), "a2");
    round(&vault, &vault.a, RoundMode::Normal).await;
    let dir = tempfile::tempdir().expect("dir");

    repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("repair");

    assert_ne!(device_of(&vault.b.lock()), B);
    assert_eq!(capture_broken_at(&vault.b.lock()), None);
    round(&vault, &vault.b, RoundMode::Normal).await;
    round(&vault, &vault.a, RoundMode::Normal).await;
    assert!(has_institution(&vault.a.lock(), "b1"));
    assert!(has_institution(&vault.b.lock(), "a2") && has_institution(&vault.b.lock(), "b1"));
    assert_converged(&[&*vault.a.lock(), &*vault.b.lock()]);
}

#[tokio::test]
async fn repair_stops_when_capture_still_does_not_install() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    // Whatever broke capture at unlock still breaks it: the clock table is gone.
    exec(
        &vault.b.lock(),
        "ALTER TABLE sync_clock RENAME TO sync_clock_lost",
    );
    let before = repair_state(&vault.b.lock());
    let from = vault.relay.calls().len();
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("capture does not install");

    assert_eq!(validation_key(&error), Some(guard::CAPTURE_STILL_BROKEN));
    assert_eq!(calls_since(&vault, from), Vec::<Route>::new(), "no drain");
    let b = vault.b.lock();
    assert_eq!(repair_state(&b), before);
    assert!(!capture::is_installed(&b).expect("installed"));
    assert_eq!(
        repair_backups(dir.path()),
        vec![format!("sync-repair-{NOW}-unused.zip")],
        "no replace happened"
    );
}

/// The database inside a backup zip, opened from a copy in `dir`.
fn backed_up_database(zip: &std::path::Path, dir: &std::path::Path) -> Connection {
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(zip).expect("zip")).expect("archive");
    let mut entry = archive
        .by_name(backup::DB_FILE_NAME)
        .expect("database entry");
    let copy = dir.join("backed-up.db");
    let mut out = std::fs::File::create(&copy).expect("copy");
    std::io::copy(&mut entry, &mut out).expect("extract");
    Connection::open(&copy).expect("open the copy")
}

/// Offline (every push fails), the re-sync replaces anyway, as the person agreed: what it could not
/// send stays in the backup, which is kept, and the outcome counts it.
#[tokio::test]
async fn resync_replaces_a_capture_broken_profile_and_keeps_its_unsent_changes_in_the_backup() {
    let vault = vault().await;
    let s = snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let unsent_seq = {
        let b = vault.b.lock();
        institution(&b, "b-sealed");
        seal_from(&b, &vault.creds);
        institution(&b, "b-queued");
        count(&b, "SELECT MAX(seq) FROM sync_unsent")
    };
    break_capture(&vault.b);
    let consent = agreed(&vault.b);
    vault
        .relay
        .fail_next(Route::Push, RelayError::Network("offline".into()));
    let dir = tempfile::tempdir().expect("dir");

    let outcome = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &XorNative, NOW)
        .await
        .expect("resync");
    let zip = outcome.backup.clone();

    assert_eq!(zip, dir.path().join(format!("sync-repair-{NOW}-kept.zip")));
    assert_eq!(
        (
            outcome.unsent_items_in_backup_only,
            outcome.unsent_batches_in_backup_only
        ),
        (1, 1),
        "b-queued waits in the outbox, b-sealed in its batch"
    );
    assert!(
        !relay_has_institution(&vault, "b-sealed"),
        "the push failed: nothing was sent"
    );
    {
        let b = vault.b.lock();
        let sync = state::load(&b).expect("state").expect("row");
        assert_ne!(sync.device, B);
        assert_eq!(sync.cursor, s);
        assert_eq!(capture_broken_at(&b), None);
        assert!(capture::is_installed(&b).expect("installed"));
        assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_unsent"), 0);
        let mut stmt = b
            .prepare("SELECT tbl || ' ' || pk FROM sync_outbox ORDER BY tbl, pk")
            .expect("outbox");
        let outbox: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .expect("outbox")
            .map(|r| r.expect("row"))
            .collect();
        assert_eq!(
            outbox,
            vec![format!("sync_devices {}", sync.device.to_hex())],
            "only the new device's own row waits to go out"
        );
        assert!(!has_institution(&b, "b-sealed") && !has_institution(&b, "b-queued"));
    }
    let copy = backed_up_database(&zip, dir.path());
    assert_eq!(
        count(&copy, "SELECT seq FROM sync_unsent"),
        unsent_seq,
        "the unsent envelope is in the backup"
    );
    assert!(has_institution(&copy, "b-sealed") && has_institution(&copy, "b-queued"));
}

/// Audit H3, as reproduced there: a policy with a contract PDF on `B`, then "Re-sync this
/// device". The policy comes back from the vault under its ID, and so do its document and blob.
#[tokio::test]
async fn a_resync_keeps_the_contract_pdf_of_a_policy_the_vault_has() {
    let vault = vault().await;
    exec(
        &vault.a.lock(),
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly')",
    );
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let blob = {
        let b = vault.b.lock();
        b.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        crate::db::files_schema::migrate(&b).expect("files schema");
        let blob = crate::services::attachment_store::put_bytes(&b, b"%PDF-1.7 contract")
            .expect("blob")
            .id;
        b.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
             VALUES ('d1', 'p1', 'Contract', ?1)",
            [&blob],
        )
        .expect("document");
        blob
    };
    let dir = tempfile::tempdir().expect("dir");
    let consent = agreed(&vault.b);

    resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &XorNative, NOW)
        .await
        .expect("resync");

    let b = vault.b.lock();
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM insurance_documents WHERE id = 'd1'"
        ),
        1
    );
    assert_eq!(
        crate::services::attachment_store::read_all(&b, &blob).expect("blob"),
        b"%PDF-1.7 contract".to_vec()
    );
}

/// The repair stops at a backup that fails: no relay call, no capture install, `B` unchanged.
async fn repair_with_a_failing_backup(
    spoil: impl FnOnce(&Connection),
    dir: &std::path::Path,
) -> SyncError {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    spoil(&vault.b.lock());
    let before = repair_state(&vault.b.lock());
    let marks = state::marks(&vault.b.lock()).expect("marks");
    let from = vault.relay.calls().len();

    let error = repair_capture(&vault.relay, &vault.b, dir, &NoBinding, NOW)
        .await
        .expect_err("the backup failed");

    assert_eq!(
        calls_since(&vault, from),
        Vec::<Route>::new(),
        "no drain, no join"
    );
    let b = vault.b.lock();
    assert_eq!(repair_state(&b), before);
    assert_eq!(state::marks(&b).expect("marks"), marks);
    assert!(!capture::is_installed(&b).expect("installed"));
    error
}

#[tokio::test]
async fn a_backup_that_cannot_be_written_stops_the_repair_before_the_drain() {
    let dir = tempfile::tempdir().expect("dir");
    let missing = dir.path().join("missing");

    let error = repair_with_a_failing_backup(|_| {}, &missing).await;

    assert_eq!(
        validation_key(&error),
        Some("validation.backupDestinationMissing"),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_backup_that_does_not_read_back_stops_the_repair_before_the_drain() {
    let dir = tempfile::tempdir().expect("dir");

    // A migration this build does not know: the archive reads as newer than the app.
    let error = repair_with_a_failing_backup(
        |b| {
            exec(
                b,
                "INSERT INTO _migrations (name) VALUES ('999_from_the_future')",
            )
        },
        dir.path(),
    )
    .await;

    let read_back = matches!(
        &error,
        SyncError::Local(AppError::Internal(m)) if m.contains("does not read back")
    );
    assert!(read_back, "{error:?}");
    assert_eq!(
        repair_backups(dir.path()),
        Vec::<String>::new(),
        "a backup that does not read back leaves no file"
    );
}

/// Audit H2 with pre-flight C-1: the guard is a connection setting no rollback restores, so a
/// replace that fails on a guarded profile must impose it again.
#[tokio::test]
async fn a_failed_replace_on_a_guarded_profile_still_refuses_writes() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    // The snapshot's row `a1` fails as the replace restores it.
    exec(
        &vault.b.lock(),
        "CREATE TRIGGER fail_restore BEFORE INSERT ON institutions WHEN NEW.id = 'a1' \
         BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    );
    break_capture(&vault.b);
    let dir = tempfile::tempdir().expect("dir");

    let result = join_by_replace(
        &vault.relay,
        &vault.b,
        vault.creds.clone(),
        "Laptop",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await;

    assert!(result.is_err(), "{result:?}");
    let b = vault.b.lock();
    assert_eq!(device_of(&b), B, "rolled back");
    assert!(!capture::is_installed(&b).expect("installed"));
    assert!(guard::is_imposed(&b));
    assert_eq!(
        refused_institution(&b, "after"),
        Some(guard::CHANGES_PAUSED)
    );
}

#[tokio::test]
async fn a_replace_on_a_guarded_profile_ends_with_capture() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    break_capture(&vault.b);
    let dir = tempfile::tempdir().expect("dir");

    join_by_replace(
        &vault.relay,
        &vault.b,
        vault.creds.clone(),
        "Laptop",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");

    let b = vault.b.lock();
    assert!(capture::is_installed(&b).expect("installed"));
    assert!(!guard::is_imposed(&b));
    assert_eq!(refused_institution(&b, "after"), None);
    assert_eq!(outbox_rows(&b, "after"), 1, "captured");
}

/// The repair keeps the guard while capture still fails (`syncCaptureStillBroken`, pointing to
/// Turn off sync), and lifts it as soon as capture installs: before the drain, so a drain that
/// fails leaves a profile that captures what it writes.
#[tokio::test]
async fn repair_lifts_the_guard_only_after_capture_installs() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    break_capture(&vault.b);
    exec(
        &vault.b.lock(),
        "ALTER TABLE sync_clock RENAME TO sync_clock_lost",
    );
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("capture still does not install");

    assert_eq!(validation_key(&error), Some(guard::CAPTURE_STILL_BROKEN));
    {
        let b = vault.b.lock();
        assert!(guard::is_imposed(&b));
        assert_eq!(
            refused_institution(&b, "b-refused"),
            Some(guard::CHANGES_PAUSED)
        );
    }

    exec(
        &vault.b.lock(),
        "ALTER TABLE sync_clock_lost RENAME TO sync_clock",
    );
    fail_every_round(&vault, Route::Pull, || {
        RelayError::Network("offline".into())
    });

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("the drain did not finish");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncRepairDrainIncomplete")
    );
    let b = vault.b.lock();
    assert!(capture::is_installed(&b).expect("installed"));
    assert!(!guard::is_imposed(&b));
    assert_eq!(refused_institution(&b, "b-after"), None);
    assert_eq!(outbox_rows(&b, "b-after"), 1, "captured");
}

/// Pre-flight I-1: the copy check of a restored, guarded profile re-joins, which records the new
/// device in `sync_devices`: the guard lets that through, and the repair completes.
#[tokio::test]
async fn a_repair_of_a_guarded_profile_carrying_the_restore_marker_succeeds() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let head = vault.relay.ops().last().map_or(0, |op| op.seq);
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    let dir = tempfile::tempdir().expect("dir");
    write_rejoin_marker(dir.path()).expect("marker");

    repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("repair");

    let drained = op_devices(&vault, head);
    assert_eq!(drained.len(), 1, "{drained:?}");
    assert!(drained[0] > B, "drained under the re-join's ID");
    assert!(!dir.path().join(REJOIN_MARKER).exists());
    let b = vault.b.lock();
    assert!(capture::is_installed(&b).expect("installed"));
    assert!(!guard::is_imposed(&b));
    institution(&b, "b-after");
    assert_eq!(outbox_rows(&b, "b-after"), 1, "captured");
}

/// Fix round 1 (1): under the read-only last resort nothing can be written, a backup included;
/// the repair says capture is still broken before it tries anything.
#[tokio::test]
async fn a_read_only_profile_is_told_to_turn_sync_off_before_any_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    break_capture(&vault.b);
    {
        // The guard could not be imposed: `guard_or_close` made the session read-only.
        let b = vault.b.lock();
        guard::lift(&b).expect("lift");
        exec(&b, "PRAGMA query_only = 1");
    }
    let from = vault.relay.calls().len();
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("read-only");

    assert_eq!(validation_key(&error), Some(guard::CAPTURE_STILL_BROKEN));
    let error = resync_by_replace(
        &vault.relay,
        &vault.b,
        agreed(&vault.b),
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("read-only");
    assert_eq!(validation_key(&error), Some(guard::CAPTURE_STILL_BROKEN));
    assert_eq!(calls_since(&vault, from), Vec::<Route>::new());
    assert_eq!(
        repair_backups(dir.path()),
        Vec::<String>::new(),
        "no backup"
    );
    assert!(guard::is_read_only(&vault.b.lock()));
}
