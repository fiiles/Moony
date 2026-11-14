//! Audit M8 and the data behind H2–H4: a join, repair or re-sync checks what the person agreed
//! to, backs up and replaces in one connection hold so no write slips in between, and says what
//! stayed only in the backup. A re-sync sends what it can first; a repair drains until clean.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::join::{break_capture, device_of, profile, repair_backups, round, snapshot_of_a};
use super::repair::{fail_every_round, relay_has_institution};
use super::runtime::op_devices;
use super::*;
use crate::services::sync::backups::{self, BackupMark};
use crate::services::sync::binding::{write_rejoin_marker, NoBinding};
use crate::services::sync::engine::RoundMode;
use crate::services::sync::error::SyncError;
use crate::services::sync::impact::{self, DRAIN_INCOMPLETE, IMPACT_CHANGED};
use crate::services::sync::join::{join_new_device, repair_capture, resync_by_replace};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, CheckoutPlan, PullPage, PushReceipt, RelayError, RelayResult, SnapshotBlob,
    VaultInfo,
};

const DAY: i64 = 86_400;

fn key_of(error: &SyncError) -> Option<&str> {
    match error {
        SyncError::Local(AppError::Validation(key)) => Some(key),
        _ => None,
    }
}

/// What one `with_conn` saw: a backup name that was not there before, `sync_state.device_id`
/// changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HoldSeen {
    new_backup: bool,
    device_changed: bool,
}

/// A `TestConn` that records every hold (`HoldSeen`), and can prune the backups at `prune_at` at
/// the end of the hold that changed the device: a later prune that runs after the replace
/// committed, before the backup got its final mark.
#[derive(Clone)]
struct HoldLog {
    inner: TestConn,
    dir: PathBuf,
    seen: Arc<Mutex<Vec<HoldSeen>>>,
    prune_at: Option<i64>,
}

impl HoldLog {
    fn new(inner: TestConn, dir: &Path) -> Self {
        Self {
            inner,
            dir: dir.to_path_buf(),
            seen: Arc::new(Mutex::new(Vec::new())),
            prune_at: None,
        }
    }

    fn seen(&self) -> Vec<HoldSeen> {
        self.seen.lock().expect("log").clone()
    }
}

fn device_id(conn: &Connection) -> Option<Vec<u8>> {
    conn.query_row("SELECT device_id FROM sync_state", [], |r| r.get(0))
        .ok()
}

impl ConnAccess for HoldLog {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.inner.with_conn(|conn| {
            let names = repair_backups(&self.dir);
            let device = device_id(conn);
            let result = f(conn);
            let changed = device_id(conn) != device;
            let new_backup = repair_backups(&self.dir)
                .iter()
                .any(|name| !names.contains(name));
            if let (true, Some(at)) = (changed, self.prune_at) {
                backups::prune(&self.dir, at).expect("prune");
            }
            self.seen.lock().expect("log").push(HoldSeen {
                new_backup,
                device_changed: changed,
            });
            result
        })
    }

    fn round_lock(&self) -> &tokio::sync::Mutex<()> {
        self.inner.round_lock()
    }
}

/// The fake relay, with a write into a device's profile just before the first snapshot download
/// (after a re-sync's push, after a repair's drain), or (`drip`) pull pages of one envelope where
/// every second pull call fails: each round moves the cursor one envelope and ends in an error.
struct WritesBeforeSnapshot {
    relay: FakeRelay,
    write: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    drip: bool,
    pulls: std::sync::atomic::AtomicUsize,
}

impl WritesBeforeSnapshot {
    fn new(relay: &FakeRelay, write: impl FnOnce() + Send + 'static) -> Self {
        Self {
            relay: relay.clone(),
            write: Mutex::new(Some(Box::new(write))),
            drip: false,
            pulls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn drip(relay: &FakeRelay) -> Self {
        Self {
            drip: true,
            ..Self::new(relay, || {})
        }
    }
}

impl Relay for WritesBeforeSnapshot {
    fn server_info(
        &self,
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::ServerInfo>> + Send
    {
        self.relay.server_info()
    }
    fn create_account(&self) -> impl std::future::Future<Output = RelayResult<String>> + Send {
        self.relay.create_account()
    }
    fn account_status(
        &self,
        account: &str,
    ) -> impl std::future::Future<Output = RelayResult<AccountStatus>> + Send {
        self.relay.account_status(account)
    }
    fn checkout(
        &self,
        account: &str,
        plan: CheckoutPlan,
    ) -> impl std::future::Future<Output = RelayResult<String>> + Send {
        self.relay.checkout(account, plan)
    }
    fn create_vault(
        &self,
        account: &str,
        vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<bool>> + Send {
        self.relay.create_vault(account, vault)
    }
    fn vault_info(
        &self,
        vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<VaultInfo>> + Send {
        self.relay.vault_info(vault)
    }
    fn push(
        &self,
        vault: &VaultAuth,
        ops: &[OutgoingOp],
    ) -> impl std::future::Future<Output = RelayResult<PushReceipt>> + Send {
        self.relay.push(vault, ops)
    }
    fn pull(
        &self,
        vault: &VaultAuth,
        after: u64,
        limit: u32,
    ) -> impl std::future::Future<Output = RelayResult<PullPage>> + Send {
        let call = self.pulls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let fail = self.drip && call % 2 == 1;
        let page = self
            .relay
            .pull(vault, after, if self.drip { 1 } else { limit });
        async move {
            if fail {
                Err(RelayError::Network("dropped".into()))
            } else {
                page.await
            }
        }
    }
    fn put_snapshot(
        &self,
        vault: &VaultAuth,
        seq: u64,
        bytes: Vec<u8>,
    ) -> impl std::future::Future<Output = RelayResult<()>> + Send {
        self.relay.put_snapshot(vault, seq, bytes)
    }
    fn get_snapshot(
        &self,
        vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<SnapshotBlob>> + Send {
        if let Some(write) = self.write.lock().expect("write").take() {
            write();
        }
        self.relay.get_snapshot(vault)
    }
    fn delete_vault(
        &self,
        vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<()>> + Send {
        self.relay.delete_vault(vault)
    }
}

#[tokio::test]
async fn the_join_backs_up_and_replaces_in_one_connection_hold() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    let joiner = HoldLog::new(TestConn::new(migrated()), dir.path());
    institution(&joiner.inner.lock(), "local-only");
    bank_account(&joiner.inner.lock(), "local-account");
    let consent = agreed(&joiner.inner);

    let outcome = join_new_device(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "Laptop",
        consent,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");

    let seen = joiner.seen();
    let both = HoldSeen {
        new_backup: true,
        device_changed: true,
    };
    assert_eq!(
        seen.iter().filter(|hold| **hold == both).count(),
        1,
        "{seen:?}"
    );
    assert!(
        !seen
            .iter()
            .any(|hold| hold.new_backup != hold.device_changed),
        "no hold backs up without replacing or replaces without backing up: {seen:?}"
    );
    assert_eq!(
        outcome.backup,
        dir.path().join(format!("sync-repair-{NOW}-kept.zip"))
    );
    assert!(outcome.kept);
    assert!(outcome.backup.is_file());
}

/// Controller ruling: a join into a profile that holds something keeps its backup: it is the only
/// copy of what the join replaced.
#[tokio::test]
async fn a_join_into_a_profile_with_data_keeps_its_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    let joiner = TestConn::new(migrated());
    bank_account(&joiner.lock(), "b1");
    let consent = agreed(&joiner);

    let outcome = join_new_device(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "Laptop",
        consent,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");

    assert!(outcome.kept);
    let listed = backups::list(dir.path()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mark, BackupMark::Kept);
}

/// Spec 2026-11-14 §5.4: a join into a profile that holds nothing still writes its backup, and
/// leaves it plain (prunable). Institutions are not counted.
#[tokio::test]
async fn a_join_into_a_brand_new_profile_leaves_a_plain_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    let joiner = TestConn::new(migrated());
    institution(&joiner.lock(), "local-only");
    profile_created_at(&joiner.lock(), NOW - 3_600);
    let consent = agreed(&joiner);

    let outcome = join_new_device(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "Laptop",
        consent,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");

    assert!(!outcome.kept);
    assert_eq!(
        outcome.backup,
        dir.path().join(format!("sync-repair-{NOW}.zip"))
    );
    assert!(outcome.backup.is_file());
    let listed = backups::list(dir.path()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mark, BackupMark::Plain);
}

/// A join into a profile with nothing counted but something a person made (a watchlist row), or
/// one created more than a day ago, keeps its backup (`impact::brand_new_profile`).
async fn join_keeping_check(prepare: impl FnOnce(&Connection)) {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    let joiner = TestConn::new(migrated());
    prepare(&joiner.lock());
    let consent = agreed(&joiner);
    assert!(impact::replace_impact(&joiner.lock())
        .expect("impact")
        .holds_nothing());

    let outcome = join_new_device(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "Laptop",
        consent,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");

    assert!(outcome.kept);
    let listed = backups::list(dir.path()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mark, BackupMark::Kept);
}

#[tokio::test]
async fn a_join_into_a_profile_with_only_a_watchlist_row_keeps_its_backup() {
    join_keeping_check(|conn| {
        profile_created_at(conn, NOW - 3_600);
        exec(
            conn,
            "INSERT INTO watched_stocks (id, ticker) VALUES ('w1', 'AAPL')",
        );
    })
    .await;
}

#[tokio::test]
async fn a_join_into_an_empty_profile_created_a_day_ago_keeps_its_backup() {
    join_keeping_check(|conn| profile_created_at(conn, NOW - 25 * 3_600)).await;
}

/// The "brand new" decision is taken inside the hold: an account written after the person agreed
/// (a joining profile has no capture, so the consent watermark does not move and the join goes on)
/// makes the backup kept, not plain.
#[tokio::test]
async fn an_account_written_after_the_confirmation_keeps_the_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    let joiner = TestConn::new(migrated());
    profile_created_at(&joiner.lock(), NOW - 3_600);
    let consent = agreed(&joiner);
    assert!(impact::replace_impact(&joiner.lock())
        .expect("impact")
        .holds_nothing());
    let writer = joiner.clone();
    let relay = WritesBeforeSnapshot::new(&vault.relay, move || {
        bank_account(&writer.lock(), "late");
    });

    let outcome = join_new_device(
        &relay,
        &joiner,
        vault.creds.clone(),
        "Laptop",
        consent,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("the consent does not see an unsynced write, so the join goes on");

    assert!(outcome.kept);
    let listed = backups::list(dir.path()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mark, BackupMark::Kept);
}

/// Pre-flight I-6 with the Task 5 review: between the replace's commit and the backup's final
/// mark, a prune (a later run of the app, after a crash) must not remove a backup that may hold
/// the only copy of something.
#[tokio::test]
async fn a_backup_survives_a_prune_between_its_replace_and_its_mark() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    // Three plain backups newer than this one: at `later`, a plain backup made at NOW is the
    // fourth and older than 30 days, so a prune removes it.
    for days in [100, 200, 300] {
        std::fs::write(
            dir.path()
                .join(format!("sync-repair-{}.zip", NOW + days * DAY)),
            b"x",
        )
        .expect("newer backup");
    }
    let later = NOW + 400 * DAY;
    // A re-sync with nothing left unsent: its backup settles to plain after the replace.
    round(&vault, &vault.b, RoundMode::Normal).await;
    let mut resyncing = HoldLog::new(vault.b.clone(), dir.path());
    resyncing.prune_at = Some(later);
    let consent = agreed(&vault.b);

    let outcome = resync_by_replace(
        &vault.relay,
        &resyncing,
        consent,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("resync");

    assert!(
        outcome.backup.is_file(),
        "the backup was marked kept before the replace committed"
    );
    assert_eq!(
        outcome.backup,
        dir.path().join(format!("sync-repair-{NOW}.zip")),
        "and settled to plain after it"
    );
    assert!(!outcome.kept);
}

#[tokio::test]
async fn a_resync_pushes_what_it_can_before_it_replaces() {
    let vault = vault().await;
    let s = snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let head = vault.relay.ops().last().map_or(s, |op| op.seq);
    {
        let b = vault.b.lock();
        institution(&b, "b1");
        seal_from(&b, &vault.creds);
        institution(&b, "b2");
    }
    let consent = agreed(&vault.b);
    let dir = tempfile::tempdir().expect("dir");

    let outcome = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect("resync");

    assert_eq!(
        op_devices(&vault, head),
        vec![B, B],
        "both under B's old ID"
    );
    assert!(relay_has_institution(&vault, "b1") && relay_has_institution(&vault, "b2"));
    assert_eq!(
        (
            outcome.unsent_items_in_backup_only,
            outcome.unsent_batches_in_backup_only
        ),
        (0, 0)
    );
    assert_eq!(
        outcome.backup,
        dir.path().join(format!("sync-repair-{NOW}-kept.zip")),
        "nothing counted as only in the backup, but b1 and b2 never came back: kept (Task 16 I3)"
    );
    assert_ne!(device_of(&vault.b.lock()), B);
    round(&vault, &vault.b, RoundMode::Normal).await;
    let b = vault.b.lock();
    assert!(has_institution(&b, "b1") && has_institution(&b, "b2"));
}

#[tokio::test]
async fn an_offline_resync_keeps_unsent_changes_in_the_backup_and_says_how_many() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    {
        let b = vault.b.lock();
        institution(&b, "b-sealed");
        seal_from(&b, &vault.creds);
        institution(&b, "b1");
        institution(&b, "b2");
    }
    let consent = agreed(&vault.b);
    vault.relay.fail_next(Route::Push, RelayError::Server(503));
    let dir = tempfile::tempdir().expect("dir");

    let outcome = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect("resync");

    assert_eq!(outcome.unsent_items_in_backup_only, 2);
    assert_eq!(outcome.unsent_batches_in_backup_only, 1);
    assert_eq!(
        outcome.backup,
        dir.path().join(format!("sync-repair-{NOW}-kept.zip"))
    );
    let listed = backups::list(dir.path()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mark, BackupMark::Kept);
    assert!(!has_institution(&vault.b.lock(), "b1"), "replaced");
}

#[tokio::test]
async fn a_change_after_the_person_agreed_stops_the_resync() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    let consent = agreed(&vault.b);
    institution(&vault.b.lock(), "b-late");
    vault.relay.fail_next(Route::Push, RelayError::Server(503));
    let dir = tempfile::tempdir().expect("dir");

    let error = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("changed since the person agreed");

    assert_eq!(key_of(&error), Some(IMPACT_CHANGED), "{error:?}");
    let b = vault.b.lock();
    assert_eq!(device_of(&b), B);
    assert!(has_institution(&b, "b1") && has_institution(&b, "b-late"));
    assert!(has_institution(&b, "a1"));
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
}

/// Pre-flight Minor 4: a count would pass here (one pending item against two agreed), but the
/// item is not one the person saw.
#[tokio::test]
async fn a_different_change_after_consent_is_caught_by_the_watermark() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    {
        let b = vault.b.lock();
        institution(&b, "b1");
        institution(&b, "b2");
    }
    let consent = agreed(&vault.b);
    assert_eq!(
        impact::replace_impact(&vault.b.lock())
            .expect("impact")
            .pending_items,
        2
    );
    let writer = vault.b.clone();
    let relay = WritesBeforeSnapshot::new(&vault.relay, move || {
        institution(&writer.lock(), "b3");
    });
    let dir = tempfile::tempdir().expect("dir");

    let error = resync_by_replace(&relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("b3 came after consent");

    assert_eq!(key_of(&error), Some(IMPACT_CHANGED), "{error:?}");
    assert!(relay_has_institution(&vault, "b1") && relay_has_institution(&vault, "b2"));
    let b = vault.b.lock();
    assert_eq!(impact::replace_impact(&b).expect("impact").pending_items, 1);
    assert_eq!(device_of(&b), B);
    assert!(has_institution(&b, "b3"));
    assert!(repair_backups(dir.path()).is_empty());
}

#[tokio::test]
async fn a_join_with_more_attachments_than_agreed_stops() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let joiner = TestConn::new(migrated());
    exec(
        &joiner.lock(),
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly')",
    );
    let consent = agreed(&joiner);
    assert_eq!(consent.attachments, 0);
    exec(
        &joiner.lock(),
        "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
         VALUES ('d1', 'p1', 'Contract', 'blob-1')",
    );
    let before = profile(&joiner.lock());
    let dir = tempfile::tempdir().expect("dir");

    let error = join_new_device(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "Laptop",
        consent,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("a document the person was not told about");

    assert_eq!(key_of(&error), Some(IMPACT_CHANGED), "{error:?}");
    let c = joiner.lock();
    assert_eq!(profile(&c), before, "nothing replaced");
    assert!(state::load(&c).expect("state").is_none(), "sync stays off");
    assert_eq!(count(&c, "SELECT COUNT(*) FROM insurance_documents"), 1);
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
}

#[tokio::test]
async fn a_repair_with_a_change_after_its_drain_drains_again() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    // Capture is back by the time the drain ends, so this write is captured.
    let writer = vault.b.clone();
    let relay = WritesBeforeSnapshot::new(&vault.relay, move || {
        institution(&writer.lock(), "b-late");
    });
    let dir = tempfile::tempdir().expect("dir");

    let outcome = repair_capture(&relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("the repair drains again and completes");

    assert!(relay_has_institution(&vault, "b1") && relay_has_institution(&vault, "b-late"));
    assert_eq!(
        outcome.backup,
        dir.path().join(format!("sync-repair-{NOW}-kept.zip"))
    );
    assert_ne!(device_of(&vault.b.lock()), B);
    round(&vault, &vault.b, RoundMode::Normal).await;
    assert!(has_institution(&vault.b.lock(), "b-late"));
}

#[tokio::test]
async fn a_repair_that_stops_making_progress_ends() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    fail_every_round(&vault, Route::Push, || RelayError::Server(503));
    let from = vault.relay.calls().len();
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("no progress");

    assert_eq!(key_of(&error), Some(DRAIN_INCOMPLETE), "{error:?}");
    let pushes = vault.relay.calls()[from..]
        .iter()
        .filter(|route| **route == Route::Push)
        .count();
    assert_eq!(pushes, 2, "two rounds without progress end it");
    assert_eq!(
        repair_backups(dir.path()),
        vec![format!("sync-repair-{NOW}-unused.zip")],
        "the pre-drain backup of a repair that replaced nothing"
    );
    assert_eq!(device_of(&vault.b.lock()), B);
}

#[tokio::test]
async fn a_resync_of_a_copied_profile_rejoins_before_it_pushes() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let head = vault.relay.ops().last().map_or(0, |op| op.seq);
    institution(&vault.b.lock(), "b1");
    let dir = tempfile::tempdir().expect("dir");
    write_rejoin_marker(dir.path()).expect("marker");
    let consent = agreed(&vault.b);

    resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect("resync");

    let pushed = op_devices(&vault, head);
    assert!(!pushed.is_empty(), "the re-sync pushed first");
    assert!(
        pushed.iter().all(|device| *device != B),
        "never under the copied profile's original ID: {pushed:?}"
    );
    assert!(relay_has_institution(&vault, "b1"));
}

/// Review fix 3: a device several pull pages behind advances its cursor in every round, even
/// when each round ends in an error; that is progress, so the repair goes on draining.
#[tokio::test]
async fn a_repair_whose_pull_advances_keeps_draining() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let cts = {
        let a = vault.a.lock();
        (1..=4)
            .map(|i| {
                institution(&a, &format!("a-late-{i}"));
                seal_from(&a, &vault.creds)
            })
            .collect::<Vec<_>>()
    };
    push(&vault.relay, &vault.creds, &cts).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    let relay = WritesBeforeSnapshot::drip(&vault.relay);
    let dir = tempfile::tempdir().expect("dir");

    repair_capture(&relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("four rounds that each move the cursor, then a clean one");

    assert!(relay_has_institution(&vault, "b1"));
    assert_ne!(device_of(&vault.b.lock()), B);
}

/// Review fix 1: the outbox keeps a row's first `queued_at`, and nothing is sealed while an
/// envelope waits, so an edit after consent to an item already queued must be caught by its
/// clock.
#[tokio::test]
async fn an_edit_to_a_queued_item_after_consent_stops_an_offline_resync() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    {
        let b = vault.b.lock();
        institution(&b, "b-sealed");
        seal_from(&b, &vault.creds);
        institution(&b, "b1");
    }
    let consent = agreed(&vault.b);
    exec(
        &vault.b.lock(),
        "UPDATE institutions SET name = 'Renamed after consent' WHERE id = 'b1'",
    );
    vault.relay.fail_next(Route::Push, RelayError::Server(503));
    let dir = tempfile::tempdir().expect("dir");

    let error = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("b1 changed after the person agreed");

    assert_eq!(key_of(&error), Some(IMPACT_CHANGED), "{error:?}");
    let b = vault.b.lock();
    assert_eq!(device_of(&b), B, "nothing replaced");
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM institutions WHERE id = 'b1' AND name = 'Renamed after consent'"
        ),
        1
    );
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
}

/// Task 6 review: a copied profile re-joins inside the re-sync, between the consent and the hold,
/// so the hold's device filter on `sync_clock` names the new ID while the deletion after consent
/// keeps the old one on its tombstone. The re-join's wipe of `sync_unsent` has the failing push
/// seal the deletion again, and the hold sees its clock (`impact::check_consent`).
#[tokio::test]
async fn a_deletion_after_consent_stops_a_resync_that_rejoins_first() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    let dir = tempfile::tempdir().expect("dir");
    write_rejoin_marker(dir.path()).expect("marker");
    let consent = agreed(&vault.b);
    exec(&vault.b.lock(), "DELETE FROM institutions WHERE id = 'b1'");
    vault.relay.fail_next(Route::Push, RelayError::Server(503));

    let error = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("b1 was deleted after the person agreed");

    assert_eq!(key_of(&error), Some(IMPACT_CHANGED), "{error:?}");
    let b = vault.b.lock();
    assert_ne!(device_of(&b), B, "the copy re-joined before the hold");
    assert!(!has_institution(&b, "b1"));
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
}

/// Task 7 review: a consent the UI never showed (a watermark past every clock, more attachments
/// than the device has) refuses and replaces nothing.
#[tokio::test]
async fn a_forged_consent_replaces_nothing() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    let dir = tempfile::tempdir().expect("dir");
    let everything = crate::services::sync::impact::ReplaceConsent {
        changes_through: u64::MAX,
        ..agreed(&vault.b)
    };

    let error = resync_by_replace(
        &vault.relay,
        &vault.b,
        everything,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("a watermark past every clock");

    assert_eq!(key_of(&error), Some(IMPACT_CHANGED), "{error:?}");
    assert_eq!(device_of(&vault.b.lock()), B);
    assert!(has_institution(&vault.b.lock(), "b1"));

    let joiner = TestConn::new(migrated());
    let before = profile(&joiner.lock());
    let inflated = crate::services::sync::impact::ReplaceConsent {
        attachments: 5,
        ..agreed(&joiner)
    };
    let error = join_new_device(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "Laptop",
        inflated,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("more attachments than the device has");

    assert_eq!(key_of(&error), Some(IMPACT_CHANGED), "{error:?}");
    assert_eq!(profile(&joiner.lock()), before);
    assert!(state::load(&joiner.lock()).expect("state").is_none());
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
}
