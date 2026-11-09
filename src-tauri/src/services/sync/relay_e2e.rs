//! End to end: the app's sync client (`HttpRelay`, setup, join, the engine's rounds) against a
//! real Moony-relay running under `wrangler dev`. Every test is ignored; run them one at a time:
//!
//! ```text
//! # Moony-relay, branch feat/dev-grant: cp .dev.vars.example .dev.vars && npm run dev
//! cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8787 \
//!   cargo test --lib relay_e2e -- --ignored --test-threads=1
//! ```
//!
//! `a_self_hosted_relay_serves_only_allowed_accounts` needs a second relay set up as the relay's
//! README "Self-hosting" (b) describes, with one account on its `ALLOWED_ACCOUNTS`:
//! `MOONY_RELAY_E2E_SELF_HOSTED=<its address>` and `MOONY_RELAY_E2E_ALLOWED_ACCOUNT=<that number>`.
//! The account is minted on that relay first (`POST /v1/accounts`, as the app's setup does), then
//! added to `.dev.vars` and the relay restarted, as the README's step 6 says.
//!
//! Each test makes its own accounts and vaults (the relay limits account creation to 5 a minute
//! per address: `fresh_account` waits that out). Devices are fresh migrated profiles behind a
//! `TestConn`, with their own data folder; the clock is the real one, as the relay's is.

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
    AccountStatus, CheckoutPlan, OutgoingOp, PullPage, PushReceipt, Relay, RelayResult,
    SnapshotBlob, VaultInfo,
};
use super::setup::{
    account_status, checkout_url, create_account, delete_synced_data, finish_setup, recovery_kit,
};
use super::state;
use super::test_support::{assert_converged, migrated, noise};
use crate::error::AppError;
use crate::models::sync::{CheckoutTarget, SyncPlan, SyncProblem};

const NEEDS_RELAY: &str = "set MOONY_RELAY_E2E to a running wrangler dev of Moony-relay \
     (branch feat/dev-grant, npm run dev there, with DEV_GRANT=1 in .dev.vars)";
const NEEDS_SELF_HOSTED: &str = "set MOONY_RELAY_E2E_SELF_HOSTED to a running self-hosted \
     Moony-relay (README \"Self-hosting\" (b): SELF_HOSTED=1 in .dev.vars) and \
     MOONY_RELAY_E2E_ALLOWED_ACCOUNT to an account number minted on it and listed in its \
     ALLOWED_ACCOUNTS";

/// The local relay's address, or the failure that says how to start one.
fn relay_url() -> String {
    env_or("MOONY_RELAY_E2E", NEEDS_RELAY)
}

fn env_or(name: &str, missing: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => panic!("{missing}"),
    }
}

/// The relay as the commands build it: from the stored address (ruling SH-2).
fn relay_at(url: &str) -> HttpRelay {
    HttpRelay::new(url).expect("relay address")
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

/// One device: a fresh migrated profile and the folder next to it.
struct Device {
    db: TestConn,
    dir: TempDir,
}

impl Device {
    fn new() -> Self {
        Device {
            db: TestConn::new(migrated()),
            dir: tempfile::tempdir().expect("data dir"),
        }
    }

    fn dir(&self) -> &Path {
        self.dir.path()
    }

    fn exec(&self, sql: &str) {
        self.db.lock().execute_batch(sql).expect(sql);
    }

    fn text(&self, sql: &str) -> Option<String> {
        self.db.lock().query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.lock().query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    fn enabled(&self) -> bool {
        state::load(&self.db.lock())
            .expect("state")
            .is_some_and(|sync| sync.enabled)
    }

    fn credentials(&self) -> Credentials {
        keys::credentials(&self.db.lock())
            .expect("credentials")
            .expect("stored")
    }

    /// One `Normal` round at the real time; an error or a problem fails the test.
    async fn round(&self, relay: &impl Relay) -> RoundReport {
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
    async fn join(&self, relay: &HttpRelay, creds: Credentials, name: &str) {
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
}

/// Deletes a vault on the relay when dropped, so a test that panics after making it leaves no
/// vault behind (a self-hosted account owns at most 5). `Drop` cannot await, so it blocks on the
/// test's own runtime (`block_in_place`, which needs the multi-thread flavour): the relay
/// client's pooled connections belong to that runtime, and a request on another runtime would
/// wait on them until it times out. A vault already gone (`404`) is fine.
struct DeleteVaultOnDrop {
    url: String,
    vault: VaultAuth,
}

impl Drop for DeleteVaultOnDrop {
    fn drop(&mut self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            eprintln!("no runtime to delete the test's vault on");
            return;
        };
        let relay = relay_at(&self.url);
        let deleted = tokio::task::block_in_place(|| {
            runtime.block_on(async { relay.delete_vault(&self.vault).await })
        });
        match deleted {
            Ok(()) | Err(super::relay::RelayError::VaultNotFound) => {}
            Err(error) => eprintln!("could not delete the test's vault: {error}"),
        }
    }
}

/// `create_account` on `device`, waiting out the relay's limit of 5 new accounts a minute.
async fn fresh_account(relay: &HttpRelay, device: &Device, url: &str) -> String {
    for _ in 0..30 {
        match create_account(relay, &device.db, url).await {
            Ok(account) => return account.account_number,
            Err(SyncError::RateLimited) => tokio::time::sleep(Duration::from_secs(3)).await,
            Err(error) => panic!("create_account: {error:?}"),
        }
    }
    panic!("the relay kept refusing new accounts");
}

/// The validation key a command would answer for `result`'s error.
fn command_key<T: std::fmt::Debug>(result: SyncResult<T>) -> String {
    match result.expect_err("refused").to_app_error() {
        AppError::Validation(key) => key,
        other => panic!("not a validation key: {other:?}"),
    }
}

/// A first device with sync on: a new account, paid for 30 days with the dev grant, its Recovery
/// Kit, setup finished under `name`.
async fn first_device(relay: &HttpRelay, url: &str, name: &str) -> Device {
    let device = Device::new();
    let account = fresh_account(relay, &device, url).await;
    let paid_until = relay.dev_grant(&account, 30).await.expect("dev grant");
    assert!(paid_until > now() + 29 * 86_400, "{paid_until}");
    let status = account_status(relay, &device.db).await.expect("status");
    assert!(status.active, "{status:?}");
    recovery_kit(&device.db.lock()).expect("kit");
    finish_setup(
        relay,
        &device.db,
        name,
        true,
        device.dir(),
        &NoBinding,
        now(),
    )
    .await
    .expect("finish setup");
    assert!(device.enabled());
    device
}

/// A bank account, a category and 20 transactions (`tx-00` … `tx-19`).
fn add_finances(device: &Device) {
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

fn converged(a: &Device, b: &Device) {
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
#[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
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
#[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn an_unpaid_account_cannot_create_a_vault() {
    let url = relay_url();
    let relay = relay_at(&url);
    let device = Device::new();
    fresh_account(&relay, &device, &url).await;
    let status = account_status(&relay, &device.db).await.expect("status");
    assert!(!status.active, "{status:?}");
    assert_eq!(status.paid_until, None);
    // `wrangler dev` has no Polar products: the payment page is not available.
    assert_eq!(
        command_key(checkout_url(&relay, &device.db, SyncPlan::Monthly).await),
        "validation.syncCheckoutUnavailable"
    );
    recovery_kit(&device.db.lock()).expect("kit");

    let result = finish_setup(
        &relay,
        &device.db,
        "Laptop",
        true,
        device.dir(),
        &NoBinding,
        now(),
    )
    .await;
    assert!(matches!(result, Err(SyncError::Lapsed)), "{result:?}");
    assert_eq!(command_key(result), "validation.syncPaymentRequired");
    assert!(!device.enabled(), "sync stays off");
}

#[tokio::test]
#[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
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
#[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
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

// Multi-thread: `DeleteVaultOnDrop` blocks in place on this runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a self-hosted Moony-relay: MOONY_RELAY_E2E_SELF_HOSTED, MOONY_RELAY_E2E_ALLOWED_ACCOUNT"]
async fn a_self_hosted_relay_serves_only_allowed_accounts() {
    let url = env_or("MOONY_RELAY_E2E_SELF_HOSTED", NEEDS_SELF_HOSTED);
    let allowed = env_or("MOONY_RELAY_E2E_ALLOWED_ACCOUNT", NEEDS_SELF_HOSTED);
    let relay = relay_at(&url);

    // An account the operator has not listed: no payment page, never active, no vault.
    let stranger = Device::new();
    fresh_account(&relay, &stranger, &url).await;
    assert_eq!(
        checkout_url(&relay, &stranger.db, SyncPlan::Monthly)
            .await
            .expect("checkout"),
        CheckoutTarget::SelfHosted
    );
    let status = account_status(&relay, &stranger.db).await.expect("status");
    assert!(!status.active, "{status:?}");
    recovery_kit(&stranger.db.lock()).expect("kit");
    let result = finish_setup(
        &relay,
        &stranger.db,
        "Stranger",
        true,
        stranger.dir(),
        &NoBinding,
        now(),
    )
    .await;
    assert!(matches!(result, Err(SyncError::Lapsed)), "{result:?}");
    assert_eq!(command_key(result), "validation.syncPaymentRequired");
    assert!(!stranger.enabled());

    // The listed account, created earlier on this profile (README step 6: create it in the app,
    // add its number, restart the relay); setup resumes with it.
    let a = Device::new();
    keys::save_account(&a.db.lock(), &url, &allowed).expect("stored account");
    let account = create_account(&relay, &a.db, &url)
        .await
        .expect("the stored account");
    assert_eq!(account.account_number, allowed);
    assert!(account.active, "{account:?}");
    assert_eq!(
        checkout_url(&relay, &a.db, SyncPlan::Yearly)
            .await
            .expect("checkout"),
        CheckoutTarget::SelfHosted
    );
    let kit = recovery_kit(&a.db.lock()).expect("kit");
    assert_eq!(
        kit.relay_url.as_deref(),
        Some(url.trim_end_matches('/')),
        "the kit carries a self-hosted relay's address"
    );
    // From here on the vault may exist on the relay: it goes again however the test ends.
    let _cleanup = DeleteVaultOnDrop {
        url: url.clone(),
        vault: a.credentials().auth(),
    };
    finish_setup(&relay, &a.db, "A", true, a.dir(), &NoBinding, now())
        .await
        .expect("finish setup");
    add_finances(&a);
    a.round(&relay).await;

    let b = Device::new();
    b.join(&relay, a.credentials(), "B").await;
    b.round(&relay).await;
    b.exec("UPDATE bank_transactions SET description = 'from B' WHERE id = 'tx-05'");
    b.round(&relay).await;
    a.round(&relay).await;
    converged(&a, &b);
    assert_eq!(
        a.text("SELECT description FROM bank_transactions WHERE id = 'tx-05'")
            .as_deref(),
        Some("from B")
    );

    // The allowed account owns at most 5 vaults: this run's goes again (and `_cleanup` finds it
    // gone).
    delete_synced_data(&relay, &a.db, a.dir())
        .await
        .expect("delete synced data");
}

#[tokio::test]
#[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
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
#[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
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
        .expect("repair");
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
    let backup = resync_by_replace(&relay, &b.db, b.dir(), &NoBinding, now())
        .await
        .expect("re-sync");
    assert!(backup.exists());
    b.round(&relay).await;
    a.round(&relay).await;
    converged(&a, &b);
}
