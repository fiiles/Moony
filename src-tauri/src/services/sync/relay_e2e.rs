//! End to end: the app's sync client (`HttpRelay`, setup, join, the engine's rounds) against a
//! real Moony sync server (Moony-relay-node, Plan 1). Every test is ignored; run them one at a time
//! (the admin login is attempt-limited and the tests share the server):
//!
//! ```text
//! # In Moony-relay-node (built with `npm run build`), a fresh data folder and a fixed setup code:
//! SETUP_CODE=moony-e2e-setup-code DATA_DIR=$(mktemp -d) npm start
//! # (or the image: docker run -e SETUP_CODE=moony-e2e-setup-code -p 8787:8787 ...)
//! cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8787 \
//!   cargo test --lib relay_e2e -- --ignored --test-threads=1
//! ```
//!
//! `MOONY_RELAY_E2E_SETUP_CODE` names another setup code (`relay_e2e_admin`). The suites go
//! through the server's admin path, as a person who runs a server does: the admin is set up once
//! (or signed in, on a server an earlier run set up), the server's address is saved, and every
//! device's person is added there; its connection code is what the app's setup takes
//! (`connect_own_server`). Devices are fresh migrated profiles behind a `TestConn`, with their
//! own data folder; the clock is the real one, as the server's is.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tempfile::TempDir;

use super::binding::NoBinding;
use super::capture;
use super::engine::snapshots::SnapshotPolicy;
use super::engine::tests::TestConn;
use super::engine::{run_round, RoundMode, RoundReport};
use super::error::{SyncError, SyncResult};
use super::join::{join_by_replace, repair_capture, resync_by_replace};
use super::keys::VaultAuth;
use super::keys::{self, Credentials};
use super::relay::http::HttpRelay;
use super::relay::{
    AccountStatus, CheckoutPlan, OutgoingOp, PullPage, PushReceipt, Relay, RelayError, RelayResult,
    ServerMode, SnapshotBlob, VaultInfo,
};
use super::relay_e2e_admin::{admin, Person};
use super::server::change_server_address;
use super::setup::{
    account_status, connect_own_server, delete_synced_data, finish_setup, note_kit_storage,
    prepare_kit, KitDraft,
};
use super::state;
use super::test_support::{assert_converged, migrated, noise};
use crate::error::AppError;
use crate::models::sync::SyncProblem;

const NEEDS_RELAY: &str = "set MOONY_RELAY_E2E to a running Moony sync server \
     (Moony-relay-node: SETUP_CODE=moony-e2e-setup-code DATA_DIR=$(mktemp -d) npm start)";

/// The sync server's address, or the failure that says how to start one.
pub(super) fn relay_url() -> String {
    match std::env::var("MOONY_RELAY_E2E") {
        Ok(value) if !value.is_empty() => value,
        _ => panic!("{NEEDS_RELAY}"),
    }
}

/// The relay as the commands build it: from the stored address (ruling SH-2).
pub(super) fn relay_at(url: &str) -> HttpRelay {
    HttpRelay::new(url).expect("relay address")
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

/// One device: a fresh migrated profile and the folder next to it.
pub(super) struct Device {
    pub(super) db: TestConn,
    dir: TempDir,
}

impl Device {
    pub(super) fn new() -> Self {
        Device {
            db: TestConn::new(migrated()),
            dir: tempfile::tempdir().expect("data dir"),
        }
    }

    pub(super) fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub(super) fn exec(&self, sql: &str) {
        self.db.lock().execute_batch(sql).expect(sql);
    }

    pub(super) fn text(&self, sql: &str) -> Option<String> {
        self.db.lock().query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    pub(super) fn count(&self, sql: &str) -> i64 {
        self.db.lock().query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    pub(super) fn enabled(&self) -> bool {
        state::load(&self.db.lock())
            .expect("state")
            .is_some_and(|sync| sync.enabled)
    }

    pub(super) fn credentials(&self) -> Credentials {
        keys::credentials(&self.db.lock())
            .expect("credentials")
            .expect("stored")
    }

    /// One `Normal` round at the real time; an error or a problem fails the test.
    pub(super) async fn round(&self, relay: &impl Relay) -> RoundReport {
        let report = self.round_with_problem(relay).await;
        assert_eq!(report.problem, None, "{report:?}");
        report
    }

    async fn round_with_problem(&self, relay: &impl Relay) -> RoundReport {
        run_round(
            relay,
            &self.db,
            &mut SnapshotPolicy::default(),
            RoundMode::Normal,
            now(),
            0,
        )
        .await
        .expect("round")
    }

    /// Joins the vault of `creds` by replace under `name`.
    pub(super) async fn join(&self, relay: &HttpRelay, creds: Credentials, name: &str) {
        join_by_replace(relay, &self.db, creds, name, self.dir(), &NoBinding, now())
            .await
            .expect("join");
    }
}

/// The real relay, counting the pushes and pulls a round makes through it, and the pull pages
/// the relay cut short (`more`).
struct Counting<'a> {
    relay: &'a HttpRelay,
    pushes: AtomicUsize,
    pulls: AtomicUsize,
    cut_pages: AtomicUsize,
}

impl<'a> Counting<'a> {
    fn new(relay: &'a HttpRelay) -> Self {
        Counting {
            relay,
            pushes: AtomicUsize::new(0),
            pulls: AtomicUsize::new(0),
            cut_pages: AtomicUsize::new(0),
        }
    }

    fn pushes(&self) -> usize {
        self.pushes.load(Ordering::SeqCst)
    }

    fn pulls(&self) -> usize {
        self.pulls.load(Ordering::SeqCst)
    }

    fn cut_pages(&self) -> usize {
        self.cut_pages.load(Ordering::SeqCst)
    }
}

impl Relay for Counting<'_> {
    async fn server_info(&self) -> RelayResult<crate::services::sync::relay::ServerInfo> {
        self.relay.server_info().await
    }

    async fn create_account(&self) -> RelayResult<String> {
        self.relay.create_account().await
    }

    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        self.relay.account_status(account).await
    }

    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        self.relay.checkout(account, plan).await
    }

    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        self.relay.create_vault(account, vault).await
    }

    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        self.relay.vault_info(vault).await
    }

    async fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        self.pushes.fetch_add(1, Ordering::SeqCst);
        self.relay.push(vault, ops).await
    }

    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        self.pulls.fetch_add(1, Ordering::SeqCst);
        let page = self.relay.pull(vault, after, limit).await;
        if page.as_ref().is_ok_and(|page| page.more) {
            self.cut_pages.fetch_add(1, Ordering::SeqCst);
        }
        page
    }

    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        self.relay.put_snapshot(vault, seq, bytes).await
    }

    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        self.relay.get_snapshot(vault).await
    }

    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        self.relay.delete_vault(vault).await
    }
    async fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.relay.rotate_token(vault, new_token, bundle).await
    }
    async fn rotate_token_with_kit(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: crate::services::sync::relay::KitRotation<'_>,
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.relay
            .rotate_token_with_kit(vault, new_token, bundle, kit)
            .await
    }
    async fn put_kit(
        &self,
        vault: &VaultAuth,
        kit_token: &str,
        envelope: &[u8],
    ) -> RelayResult<bool> {
        self.relay.put_kit(vault, kit_token, envelope).await
    }
    async fn get_kit(
        &self,
        vault_id: &str,
        kit_token: &str,
    ) -> RelayResult<crate::services::sync::relay::KitRead> {
        self.relay.get_kit(vault_id, kit_token).await
    }
    async fn rekeys(
        &self,
        vault: &VaultAuth,
        after: u32,
        limit: u32,
    ) -> RelayResult<crate::services::sync::relay::RekeyPage> {
        self.relay.rekeys(vault, after, limit).await
    }
    async fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sealed_len: u64,
        body: crate::services::sync::relay::BlobBody,
    ) -> RelayResult<()> {
        self.relay.put_blob(vault, blob, sealed_len, body).await
    }
    async fn head_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<Option<u64>> {
        self.relay.head_blob(vault, blob).await
    }
    async fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sink: &mut (dyn crate::services::sync::relay::BlobSink + Send),
    ) -> RelayResult<crate::services::sync::relay::Delivery> {
        self.relay.get_blob(vault, blob, sink).await
    }
    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<uuid::Uuid>,
        limit: u32,
    ) -> RelayResult<crate::services::sync::relay::BlobPage> {
        self.relay.list_blobs(vault, after, limit).await
    }
    async fn delete_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<()> {
        self.relay.delete_blob(vault, blob).await
    }
}

/// The validation key a command would answer for `result`'s error.
pub(super) fn command_key<T: std::fmt::Debug>(result: SyncResult<T>) -> String {
    match result.expect_err("refused").to_app_error() {
        AppError::Validation(key) => key,
        other => panic!("not a validation key: {other:?}"),
    }
}

/// The validation key a command on the person's own server would answer for `result`'s error
/// (`SyncError::to_own_server_app_error`: a blocked person is `syncAccountBlocked`).
pub(super) fn own_server_key<T: std::fmt::Debug>(result: SyncResult<T>) -> String {
    match result.expect_err("refused").to_own_server_app_error() {
        AppError::Validation(key) => key,
        other => panic!("not a validation key: {other:?}"),
    }
}

/// `call`, again after a pause while the server answers that this client asks too often: its
/// account lookups (a server check, an account's status, a vault's creation) are limited to 60 a
/// minute per client address, and one run's tests share the address. Any other answer is returned.
pub(super) async fn until_allowed<T, E, Fut>(
    mut call: impl FnMut() -> Fut,
    limited: impl Fn(&E) -> bool,
) -> Result<T, E>
where
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut waited = 0;
    loop {
        match call().await {
            Err(error) if limited(&error) && waited < 40 => {
                waited += 1;
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            other => return other,
        }
    }
}

/// `until_allowed` for the sync operations.
pub(super) async fn sync_until_allowed<T, Fut>(call: impl FnMut() -> Fut) -> SyncResult<T>
where
    Fut: std::future::Future<Output = SyncResult<T>>,
{
    until_allowed(call, |error| matches!(error, SyncError::RateLimited)).await
}

/// A name no other person on the server has: the admin adds one person per device.
fn unique_name(name: &str) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    format!(
        "e2e {name} {} {}",
        now(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    )
}

/// A device that adopted a person the admin just added, by their connection code, and has its
/// Recovery Kit: setup stops short of the vault. The server is at `url`.
async fn device_of_new_person(relay: &HttpRelay, url: &str, name: &str) -> (Device, Person) {
    let device = Device::new();
    let person = admin(url).await.add_person(&unique_name(name)).await;
    let account = sync_until_allowed(|| connect_own_server(relay, &device.db, &person.code))
        .await
        .expect("connect to the server");
    assert!(account.active, "{account:?}");
    let status = sync_until_allowed(|| account_status(relay, &device.db))
        .await
        .expect("status");
    assert!(status.active, "{status:?}");
    (device, person)
}

/// The Recovery Kit draft of setup, printed (revision §2.2, §2.7).
pub(super) fn printed_kit(device: &Device) -> KitDraft {
    let conn = device.db.lock();
    let mut draft = prepare_kit(&conn, None, now()).expect("kit");
    note_kit_storage(
        &conn,
        &mut draft,
        crate::services::sync::rekey::kit::KitStorage::Paper,
    )
    .expect("printed");
    draft
}

/// A first device with sync on: a person added by the admin, its connection code, its Recovery
/// Kit, setup finished under `name`. The person comes back with it, for the tests that block or
/// delete them.
pub(super) async fn first_device_of(relay: &HttpRelay, url: &str, name: &str) -> (Device, Person) {
    let (device, person, _) = first_device_with_kit(relay, url, name).await;
    (device, person)
}

/// `first_device_of`, with the Recovery Kit its setup registered.
pub(super) async fn first_device_with_kit(
    relay: &HttpRelay,
    url: &str,
    name: &str,
) -> (Device, Person, KitDraft) {
    let (device, person) = device_of_new_person(relay, url, name).await;
    let kit = printed_kit(&device);
    sync_until_allowed(|| {
        finish_setup(
            relay,
            &device.db,
            name,
            &kit,
            device.dir(),
            &NoBinding,
            now(),
        )
    })
    .await
    .expect("finish setup");
    assert!(device.enabled());
    (device, person, kit)
}

pub(super) async fn first_device(relay: &HttpRelay, url: &str, name: &str) -> Device {
    first_device_of(relay, url, name).await.0
}

/// A bank account, a category and 20 transactions (`tx-00` … `tx-19`).
pub(super) fn add_finances(device: &Device) {
    let mut sql = String::from(
        "INSERT INTO bank_accounts (id, name, currency, balance) \
           VALUES ('acc-1', 'Main', 'CZK', '1000.00'); \
         INSERT INTO transaction_categories (id, name) VALUES ('cat-1', 'Sync test');",
    );
    for i in 0..20 {
        sql.push_str(&format!(
            "INSERT INTO bank_transactions \
               (id, bank_account_id, tx_type, amount, currency, booking_date, category_id, \
                description) \
             VALUES ('tx-{i:02}', 'acc-1', 'debit', '-{i}.50', 'CZK', {}, 'cat-1', 'note {i}');",
            1_767_225_600 + i * 86_400
        ));
    }
    // Text the wire must carry exactly: diacritics, an emoji, quotes and a newline.
    sql.push_str(
        "UPDATE bank_transactions SET counterparty_name = 'Příliš žluťoučký kůň 🐎 \"O''Neil\"' \
           || char(10) || 'řádek 2' WHERE id = 'tx-00';",
    );
    device.exec(&sql);
}

/// `n` more transactions (`big-0000` …) on `acc-1`, each with 1 KiB of incompressible text: a
/// history of many envelopes that the relay pages and the push splits.
fn add_bulk(device: &Device, n: usize) {
    let conn = device.db.lock();
    let tx = conn.unchecked_transaction().expect("transaction");
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO bank_transactions \
                   (id, bank_account_id, tx_type, amount, currency, booking_date, description) \
                 VALUES (?1, 'acc-1', 'credit', ?2, 'EUR', ?3, ?4)",
            )
            .expect("insert");
        for i in 0..n {
            insert
                .execute(rusqlite::params![
                    format!("big-{i:04}"),
                    format!("{i}.25"),
                    1_767_225_600 + i as i64 * 3_600,
                    noise(1024, i as u64),
                ])
                .expect("bulk row");
        }
    }
    tx.commit().expect("commit");
}

pub(super) fn converged(a: &Device, b: &Device) {
    let (a, b) = (a.db.lock(), b.db.lock());
    let devices: [&Connection; 2] = [&a, &b];
    assert_converged(&devices);
}

/// The sequence of the vault's newest snapshot, as the relay reports it.
async fn vault_snapshot_seq(relay: &HttpRelay, creds: &Credentials) -> Option<u64> {
    relay
        .vault_info(&creds.auth())
        .await
        .expect("vault info")
        .snapshot_seq
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn two_devices_sync_through_the_relay() {
    let url = relay_url();
    let relay = relay_at(&url);
    let a = first_device(&relay, &url, "A").await;
    add_finances(&a);
    let pushed = a.round(&relay).await;
    assert!(pushed.push.is_some_and(|push| push.pushed > 0));

    let b = Device::new();
    b.join(&relay, a.credentials(), "B").await;
    b.round(&relay).await;
    assert_eq!(b.count("SELECT COUNT(*) FROM bank_transactions"), 20);
    // A learns of B (its `sync_devices` row) in its next round.
    a.round(&relay).await;
    converged(&a, &b);

    // Both edit offline: the same transaction's note on both (B's edit is the newer one), and a
    // different column of another transaction on each.
    a.exec(
        "UPDATE bank_transactions SET description = 'from A' WHERE id = 'tx-03'; \
         UPDATE bank_transactions SET counterparty_name = 'Shop A' WHERE id = 'tx-07';",
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    b.exec(
        "UPDATE bank_transactions SET description = 'from B' WHERE id = 'tx-03'; \
         UPDATE bank_transactions SET remittance_info = 'ref B' WHERE id = 'tx-07';",
    );
    a.round(&relay).await;
    b.round(&relay).await;
    a.round(&relay).await;

    converged(&a, &b);
    for device in [&a, &b] {
        assert_eq!(
            device
                .text("SELECT description FROM bank_transactions WHERE id = 'tx-03'")
                .as_deref(),
            Some("from B"),
            "the newer note wins"
        );
        assert_eq!(
            device.text(
                "SELECT counterparty_name || ' / ' || remittance_info \
                 FROM bank_transactions WHERE id = 'tx-07'"
            ),
            Some("Shop A / ref B".to_string()),
            "edits of different columns both stay"
        );
    }
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_blocked_person_cannot_create_a_vault_until_unblocked() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (device, person) = device_of_new_person(&relay, &url, "Blocked").await;
    let kit = printed_kit(&device);
    let admin = admin(&url).await;
    admin.block(&person.id).await;

    let result = sync_until_allowed(|| {
        finish_setup(
            &relay,
            &device.db,
            "Laptop",
            &kit,
            device.dir(),
            &NoBinding,
            now(),
        )
    })
    .await;
    assert!(matches!(result, Err(SyncError::Lapsed)), "{result:?}");
    assert_eq!(own_server_key(result), "validation.syncAccountBlocked");
    assert!(!device.enabled(), "sync stays off");

    admin.unblock(&person.id).await;
    sync_until_allowed(|| {
        finish_setup(
            &relay,
            &device.db,
            "Laptop",
            &kit,
            device.dir(),
            &NoBinding,
            now(),
        )
    })
    .await
    .expect("finish setup after the unblock");
    assert!(device.enabled());
    delete_synced_data(&relay, &device.db, device.dir())
        .await
        .expect("delete synced data");
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_blocked_person_keeps_pulling_but_cannot_push_until_unblocked() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (a, person) = first_device_of(&relay, &url, "A").await;
    add_finances(&a);
    a.round(&relay).await;
    let b = Device::new();
    b.join(&relay, a.credentials(), "B").await;
    b.round(&relay).await;

    // B pushes a change before the block; A has one of its own waiting.
    b.exec("UPDATE bank_transactions SET description = 'from B' WHERE id = 'tx-01'");
    b.round(&relay).await;
    a.exec("UPDATE bank_transactions SET description = 'from A' WHERE id = 'tx-02'");

    let admin = admin(&url).await;
    let blocked_at = now();
    admin.block(&person.id).await;

    // A still pulls (the server's read grace), its push is refused as lapsed and waits.
    let report = a.round_with_problem(&relay).await;
    assert_eq!(report.problem, Some(SyncProblem::Lapsed), "{report:?}");
    assert_eq!(
        a.text("SELECT description FROM bank_transactions WHERE id = 'tx-01'")
            .as_deref(),
        Some("from B"),
        "pulls go on"
    );
    assert!(
        a.count("SELECT COUNT(*) FROM sync_unsent") > 0,
        "the push waits"
    );
    // The status carries the moment of the block.
    let status = sync_until_allowed(|| account_status(&relay, &a.db))
        .await
        .expect("status");
    assert!(!status.active, "{status:?}");
    let paid_until = status.paid_until.expect("a block time");
    assert!(
        (paid_until - blocked_at).abs() <= 120,
        "paid_until {paid_until} is about the block time {blocked_at}"
    );

    admin.unblock(&person.id).await;
    a.round(&relay).await;
    b.round(&relay).await;
    assert_eq!(
        b.text("SELECT description FROM bank_transactions WHERE id = 'tx-02'")
            .as_deref(),
        Some("from A"),
        "the push goes through after the unblock"
    );
    converged(&a, &b);
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn deleting_synced_data_makes_the_other_device_pause() {
    let url = relay_url();
    let relay = relay_at(&url);
    let a = first_device(&relay, &url, "A").await;
    add_finances(&a);
    a.round(&relay).await;
    let b = Device::new();
    b.join(&relay, a.credentials(), "B").await;
    b.round(&relay).await;

    delete_synced_data(&relay, &a.db, a.dir())
        .await
        .expect("delete synced data");
    assert!(!a.enabled(), "sync is off on the device that deleted");
    assert_eq!(
        a.count("SELECT COUNT(*) FROM bank_transactions"),
        20,
        "local data stays"
    );

    b.exec("UPDATE bank_transactions SET description = 'after' WHERE id = 'tx-01'");
    let paused = b.round_with_problem(&relay).await;
    assert_eq!(paused.problem, Some(SyncProblem::VaultGone), "{paused:?}");
    let marks = state::marks(&b.db.lock()).expect("marks").expect("row");
    assert!(marks.vault_gone_at.is_some(), "B is paused");
    assert!(b.enabled(), "B keeps sync on, paused");
    assert_eq!(b.count("SELECT COUNT(*) FROM bank_transactions"), 20);

    // The pause holds: the next round does not ask the relay.
    let again = b.round_with_problem(&relay).await;
    assert_eq!(again.problem, Some(SyncProblem::VaultGone));
    assert!(again.pull.is_none());
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn snapshot_round_trip_through_r2() {
    let url = relay_url();
    let relay = relay_at(&url);
    let a = first_device(&relay, &url, "A").await;
    let creds = a.credentials();
    let first = vault_snapshot_seq(&relay, &creds).await;
    assert!(first.is_some(), "setup uploaded the first snapshot");

    // A uploads a snapshot that holds its finances (forced, as after a barrier crossing).
    add_finances(&a);
    let report = run_round(
        &relay,
        &a.db,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        now(),
        0,
    )
    .await
    .expect("round");
    assert_eq!(report.problem, None, "{report:?}");
    assert!(report.snapshot_uploaded, "{report:?}");
    let seq = vault_snapshot_seq(&relay, &creds)
        .await
        .expect("a snapshot");
    assert!(
        Some(seq) > first,
        "the new snapshot is newer: {seq} vs {first:?}"
    );

    // B reads it back from R2: its data, before any round, is A's, and its cursor is `S`.
    let b = Device::new();
    b.join(&relay, creds, "B").await;
    assert_eq!(b.count("SELECT COUNT(*) FROM bank_transactions"), 20);
    assert_eq!(
        b.text("SELECT description FROM bank_transactions WHERE id = 'tx-19'")
            .as_deref(),
        Some("note 19")
    );
    let cursor = state::load(&b.db.lock())
        .expect("state")
        .expect("row")
        .cursor;
    assert_eq!(cursor, seq);
    b.round(&relay).await;
    a.round(&relay).await;
    converged(&a, &b);
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn registration_is_closed_and_a_deleted_persons_code_is_refused() {
    let url = relay_url();
    let relay = relay_at(&url);

    // What the server says it is, and that the app cannot mint an account on it.
    let info = until_allowed(|| relay.server_info(), |e| *e == RelayError::RateLimited)
        .await
        .expect("server info");
    assert_eq!(info.protocol, 3);
    assert_eq!(info.mode, ServerMode::SelfHosted);
    assert!(!info.version.is_empty());
    assert_eq!(
        until_allowed(|| relay.create_account(), |e| *e == RelayError::RateLimited).await,
        Err(RelayError::RegistrationClosed)
    );

    // A person with a synced device is deleted by the admin: the device's next round finds its
    // vault gone, and the person's code no longer connects a new profile.
    let (a, person) = first_device_of(&relay, &url, "Deleted").await;
    add_finances(&a);
    a.round(&relay).await;
    admin(&url).await.delete(&person.id).await;

    a.exec("UPDATE bank_transactions SET description = 'after' WHERE id = 'tx-01'");
    let paused = a.round_with_problem(&relay).await;
    assert_eq!(paused.problem, Some(SyncProblem::VaultGone), "{paused:?}");

    let fresh = Device::new();
    let result = sync_until_allowed(|| connect_own_server(&relay, &fresh.db, &person.code)).await;
    assert_eq!(
        own_server_key(result),
        "validation.syncConnectionCodeRevoked"
    );
    assert!(keys::account(&fresh.db.lock()).expect("account").is_none());
}

/// The address a device syncs through can change without touching its vault: the same server by
/// another name works; an address nobody answers is refused and the old one is kept.
#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn an_address_change_goes_through_a_real_server() {
    let url = relay_url();
    let port = reqwest::Url::parse(&url)
        .expect("address")
        .port_or_known_default()
        .expect("a port");
    let local = format!("http://127.0.0.1:{port}");
    let named = format!("http://localhost:{port}");
    let relay = relay_at(&local);
    let a = first_device(&relay, &local, "Mover").await;
    add_finances(&a);
    a.round(&relay).await;

    let stored = |device: &Device| {
        keys::credentials(&device.db.lock())
            .expect("credentials")
            .expect("stored")
            .relay_url
    };
    let build = |address: &str| HttpRelay::new(address).map_err(SyncError::Local);

    let changed = sync_until_allowed(|| change_server_address(build, &a.db, &named))
        .await
        .expect("change the address");
    assert_eq!(changed, named);
    assert_eq!(stored(&a), named);
    a.exec("UPDATE bank_transactions SET description = 'by name' WHERE id = 'tx-02'");
    a.round(&relay_at(&named)).await;

    let nobody = "http://127.0.0.1:1";
    let result = sync_until_allowed(|| change_server_address(build, &a.db, nobody)).await;
    assert_eq!(command_key(result), "validation.syncServerNotFound");
    assert_eq!(stored(&a), named, "the old address stays");
    a.round(&relay_at(&named)).await;
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_large_history_is_split_and_paged() {
    let url = relay_url();
    let relay = relay_at(&url);
    let a = first_device(&relay, &url, "A").await;
    add_finances(&a);
    // ~6 MiB of envelopes: more than one push (3 MiB of base64 each) and more than one pull page
    // (the relay cuts a page at 4 MiB).
    add_bulk(&a, 6_000);
    let counting = Counting::new(&relay);
    let pushed = a.round(&counting).await;
    let envelopes = pushed.push.as_ref().map_or(0, |push| push.pushed);
    assert!(envelopes > 60, "{pushed:?}");
    assert!(
        counting.pushes() >= 2,
        "{envelopes} envelopes went in {} push(es)",
        counting.pushes()
    );
    let info = relay
        .vault_info(&a.credentials().auth())
        .await
        .expect("vault info");
    assert!(
        info.bytes > 4 * 1024 * 1024,
        "more than one pull page: {info:?}"
    );
    let head = info.head;

    // B joins from setup's (empty) snapshot and pulls the whole history.
    let b = Device::new();
    b.join(&relay, a.credentials(), "B").await;
    let counting = Counting::new(&relay);
    let pulled = b.round(&counting).await;
    // The round pulls again after pushing B's device row, so the call count alone would pass
    // with one page of history: a page the relay cut short (`more`) proves the paging.
    assert!(
        counting.pulls() >= 2 && counting.cut_pages() >= 1,
        "the history came in {} pull call(s), {} of them cut short",
        counting.pulls(),
        counting.cut_pages()
    );
    assert!(
        pulled
            .pull
            .as_ref()
            .is_some_and(|pull| pull.done() && pull.applied > 60),
        "{pulled:?}"
    );
    let cursor = state::load(&b.db.lock())
        .expect("state")
        .expect("row")
        .cursor;
    assert!(cursor >= head, "cursor {cursor}, head {head}");
    assert_eq!(b.count("SELECT COUNT(*) FROM bank_transactions"), 6_020);
    a.round(&relay).await;
    converged(&a, &b);
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn repair_and_resync_go_through_the_relay() {
    let url = relay_url();
    let relay = relay_at(&url);
    let a = first_device(&relay, &url, "A").await;
    add_finances(&a);
    a.round(&relay).await;
    let b = Device::new();
    b.join(&relay, a.credentials(), "B").await;
    b.round(&relay).await;

    // B captures an edit, then its capture breaks (as at an unlock that could not install it).
    b.exec("UPDATE bank_transactions SET description = 'captured on B' WHERE id = 'tx-02'");
    {
        let conn = b.db.lock();
        capture::uninstall(&conn).expect("uninstall");
        state::mark_capture_broken(&conn, now()).expect("mark");
    }
    assert_eq!(
        b.round_with_problem(&relay).await.problem,
        Some(SyncProblem::CaptureBroken)
    );
    let old_device = state::load(&b.db.lock())
        .expect("state")
        .expect("row")
        .device;

    let zip = repair_capture(&relay, &b.db, b.dir(), &NoBinding, now())
        .await
        .expect("repair")
        .backup;
    assert!(zip.exists(), "the repair's backup stays");
    let repaired = state::load(&b.db.lock()).expect("state").expect("row");
    assert_ne!(
        repaired.device, old_device,
        "re-joined under a new device ID"
    );
    b.round(&relay).await;
    a.round(&relay).await;
    converged(&a, &b);
    assert_eq!(
        a.text("SELECT description FROM bank_transactions WHERE id = 'tx-02'")
            .as_deref(),
        Some("captured on B"),
        "the drained edit reached A"
    );

    // "Re-sync this device": a backup, then the vault's snapshot replaces B's data.
    let consent = super::engine::tests::agreed(&b.db);
    let backup = resync_by_replace(&relay, &b.db, consent, b.dir(), &NoBinding, now())
        .await
        .expect("re-sync")
        .backup;
    assert!(backup.exists());
    b.round(&relay).await;
    a.round(&relay).await;
    converged(&a, &b);
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_resync_pushes_unsent_changes_first() {
    let url = relay_url();
    let relay = relay_at(&url);
    let a = first_device(&relay, &url, "A").await;
    add_finances(&a);
    a.round(&relay).await;
    let b = Device::new();
    b.join(&relay, a.credentials(), "B").await;
    b.round(&relay).await;
    a.round(&relay).await;

    // B edits and has not synced: the re-sync sends that first, then replaces B's data.
    b.exec("UPDATE bank_transactions SET description = 'unsent on B' WHERE id = 'tx-05'");
    let consent = super::engine::tests::agreed(&b.db);
    let outcome = resync_by_replace(&relay, &b.db, consent, b.dir(), &NoBinding, now())
        .await
        .expect("re-sync");
    assert!(outcome.backup.exists());
    assert_eq!(
        outcome.unsent_items_in_backup_only, 0,
        "the edit reached the sync server before the replace"
    );

    // On the relay under B's old ID: A pulls the edit.
    a.round(&relay).await;
    assert_eq!(
        a.text("SELECT description FROM bank_transactions WHERE id = 'tx-05'")
            .as_deref(),
        Some("unsent on B")
    );
    // And on B again, after its next round.
    b.round(&relay).await;
    assert_eq!(
        b.text("SELECT description FROM bank_transactions WHERE id = 'tx-05'")
            .as_deref(),
        Some("unsent on B"),
        "the replaced profile got its own edit back"
    );
    a.round(&relay).await;
    converged(&a, &b);
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn an_envelope_stays_until_it_comes_back() {
    let url = relay_url();
    let relay = relay_at(&url);
    let a = first_device(&relay, &url, "A").await;
    add_finances(&a);
    let creds = a.credentials();

    let pushed = super::engine::push::push_all(&relay, &a.db, &creds)
        .await
        .expect("push");
    assert!(
        pushed.pushed > 0 && !pushed.acknowledged.is_empty(),
        "{pushed:?}"
    );
    assert!(
        a.count("SELECT COUNT(*) FROM sync_unsent") > 0,
        "the relay's answer removes nothing: the echo does"
    );

    super::engine::pull::pull_all(&relay, &a.db, &creds, now())
        .await
        .expect("pull");
    assert_eq!(
        a.count("SELECT COUNT(*) FROM sync_unsent"),
        0,
        "the envelopes came back in a pull"
    );
}
