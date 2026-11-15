//! The daily pass (attachments design §5.5, ADR 0030): when it may run, which server blobs it
//! deletes (unreferenced here and older than seven days by the server's clock), which it puts back
//! (held here, confirmed once, no longer on the server), and that nothing it meets fails a round.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use uuid::Uuid;

use super::{document, elsewhere, exec, held, photo, profile, NOW as PROFILE_NOW};
use crate::services::attachment_store::REFERENCES;
use crate::services::sync::blobs::pass::{
    self, pass_allowed, run_if_due, run_with, Limits, PassReport, Skip, PASS_AT_KEY, PASS_INTERVAL,
};
use crate::services::sync::blobs::policy::{BlobPolicy, Lane};
use crate::services::sync::blobs::queue;
use crate::services::sync::blobs::transfer::run_uploads;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::tests::{vault, with_store, TestConn, Vault, NOW};
use crate::services::sync::engine::{run_round, PullReport, RoundMode};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::{FakeRelay, Route};
use crate::services::sync::relay::{
    AccountStatus, BlobBody, BlobPage, BlobSink, CheckoutPlan, Delivery, OutgoingOp, PullPage,
    PushReceipt, Relay, RelayError, RelayResult, ServerInfo, SnapshotBlob, VaultInfo,
    MIN_BLOB_SEALED,
};
use crate::services::sync::state;

const DAY: i64 = 86_400;
/// The server's clock in these tests: unrelated to the device's `NOW` unless a test says so.
const SERVER_NOW: i64 = 1_800_000_000;

// ---------------------------------------------------------------------------------------------
// When the pass may run.
// ---------------------------------------------------------------------------------------------

/// `pass_allowed` on a fresh caught-up profile after `setup`, with `pull`.
fn gate(setup: impl FnOnce(&Connection), pull: PullReport) -> Option<Skip> {
    let conn = profile();
    setup(&conn);
    pass_allowed(&conn, &pull, PROFILE_NOW).expect("gate")
}

fn done_pull() -> PullReport {
    PullReport::default()
}

fn nothing(_: &Connection) {}

/// A queue entry `up` with `last_error` (and its retry time) as given.
fn up_entry(conn: &Connection, blob: &str, last_error: Option<&str>, next_try_at: i64) {
    conn.execute(
        "INSERT INTO sync_blobs (blob_id, state, queued_at, last_error, next_try_at, attempts) \
         VALUES (?1, 'up', 0, ?2, ?3, ?4)",
        rusqlite::params![
            blob,
            last_error,
            next_try_at,
            i64::from(last_error.is_some())
        ],
    )
    .expect("entry");
}

#[test]
fn the_pass_refuses_on_a_device_that_is_behind() {
    assert_eq!(gate(nothing, done_pull()), None, "a caught-up device");

    let mut unfinished = done_pull();
    unfinished.unfinished = true;
    assert_eq!(gate(nothing, unfinished), Some(Skip::Behind));
    let mut held_pull = done_pull();
    held_pull.held = 1;
    assert_eq!(gate(nothing, held_pull), Some(Skip::Behind));
    let mut stopped = done_pull();
    stopped.stop = Some(crate::services::sync::engine::PullStop::Invalid);
    assert_eq!(gate(nothing, stopped), Some(Skip::Behind));
    let mut behind = done_pull();
    behind.behind = true;
    assert_eq!(gate(nothing, behind), Some(Skip::Behind));

    let outbox = |conn: &Connection| {
        exec(
            conn,
            "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i1', 0)",
        )
    };
    assert_eq!(gate(outbox, done_pull()), Some(Skip::Pending));
    let unsent = |conn: &Connection| {
        exec(
            conn,
            "INSERT INTO sync_unsent (seq, envelope) VALUES (1, x'00')",
        )
    };
    assert_eq!(gate(unsent, done_pull()), Some(Skip::Pending));
    let up = |conn: &Connection| up_entry(conn, &elsewhere(1), None, 0);
    assert_eq!(gate(up, done_pull()), Some(Skip::UploadsWaiting));
    let gone = |conn: &Connection| state::set_vault_gone(conn, Some(1)).expect("gone");
    assert_eq!(gate(gone, done_pull()), Some(Skip::Paused));
    let broken = |conn: &Connection| state::mark_capture_broken(conn, 1).expect("broken");
    assert_eq!(gate(broken, done_pull()), Some(Skip::Paused));
    let went_back = |conn: &Connection| state::note_vault_went_back(conn, 1).expect("back");
    assert_eq!(gate(went_back, done_pull()), Some(Skip::WentBack));
    let missed = |conn: &Connection| state::note_missed_below_barrier(conn, 3).expect("missed");
    assert_eq!(gate(missed, done_pull()), Some(Skip::Missed));
    let skipped = |conn: &Connection| state::note_skipped_miss(conn, 3).expect("skipped");
    assert_eq!(gate(skipped, done_pull()), Some(Skip::Missed));
    let quarantined = |conn: &Connection| {
        exec(
            conn,
            "INSERT INTO sync_heads (device, seq, hash, quarantined) \
             VALUES (x'03030303030303030303030303030303', 1, x'00', 1)",
        )
    };
    assert_eq!(gate(quarantined, done_pull()), Some(Skip::Quarantined));
    let recent = |conn: &Connection| {
        pass::note_passed(conn, PROFILE_NOW - PASS_INTERVAL + 1).expect("passed")
    };
    assert_eq!(gate(recent, done_pull()), Some(Skip::NotDue));
    let a_day_ago =
        |conn: &Connection| pass::note_passed(conn, PROFILE_NOW - PASS_INTERVAL).expect("passed");
    assert_eq!(
        gate(a_day_ago, done_pull()),
        None,
        "24 h since the last pass"
    );
}

/// Rule 1: content whose upload failed and will be tried again holds the pass back.
#[test]
fn an_upload_that_will_be_retried_holds_the_pass_back() {
    for error in ["network", "server", "noRoom", "notOnServer", "local"] {
        let retried = |conn: &Connection| {
            up_entry(conn, &elsewhere(1), Some(error), PROFILE_NOW + 600);
        };
        assert_eq!(
            gate(retried, done_pull()),
            Some(Skip::UploadsWaiting),
            "{error}"
        );
    }
}

/// Rule 1: content that never uploads by itself (refused, a conflict, an invalid ID) does not
/// hold the pass back for good.
#[test]
fn a_stuck_upload_does_not_hold_the_pass_back() {
    for error in ["refused", "conflict", "invalidId"] {
        let stuck = |conn: &Connection| up_entry(conn, &elsewhere(1), Some(error), i64::MAX);
        assert_eq!(gate(stuck, done_pull()), None, "{error}");
    }
}

/// Rule 3: the pass reads references from the registry's `requires` columns, which cover every
/// column the attachment store counts, plus the blobs a replace holds.
#[test]
fn the_pass_counts_every_attachment_column_and_every_hold_as_a_reference() {
    let sql = pass::references_sql();
    for (table, column) in REFERENCES {
        assert!(
            sql.contains(&format!("FROM main.\"{table}\""))
                && sql.contains(&format!("\"{column}\"")),
            "{table}.{column} missing from {sql}"
        );
    }
    assert!(sql.contains("sync_held_blobs"), "{sql}");
}

/// Rule 7: the pass's mark is a Local `app_config` key: written, it queues nothing.
#[test]
fn the_pass_mark_stays_on_this_device() {
    let conn = profile();
    pass::note_passed(&conn, PROFILE_NOW).expect("passed");
    assert_eq!(pass::passed_at(&conn).expect("read"), Some(PROFILE_NOW));
    let queued: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))
        .expect("outbox");
    assert_eq!(queued, 0);
    let stored: String = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [PASS_AT_KEY],
            |r| r.get(0),
        )
        .expect("stored");
    assert_eq!(stored, PROFILE_NOW.to_string());
}

// ---------------------------------------------------------------------------------------------
// The pass against the fake sync server.
// ---------------------------------------------------------------------------------------------

/// A vault whose device `A` has its store and the owners `p1` and `r1`, nothing waiting to push.
async fn device() -> Vault {
    let vault = vault().await;
    with_store(&vault.a);
    exec(
        &vault.a.lock(),
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
         INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat');",
    );
    pushed(&vault.a);
    vault
}

/// Everything captured counts as pushed (the gate wants an empty outbox).
fn pushed(device: &TestConn) {
    exec(&device.lock(), "DELETE FROM sync_outbox");
}

fn vault_id(vault: &Vault) -> String {
    vault.creds.vault_id.to_b64()
}

/// A document `id` with new content held on `A`, uploaded (its entry `done`); its blob ID.
async fn uploaded_document(vault: &Vault, policy: &BlobPolicy, id: &str, bytes: &[u8]) -> Uuid {
    let blob = {
        let conn = vault.a.lock();
        let blob = held(&conn, bytes);
        document(&conn, id, &blob, 1_767_225_600);
        queue::reconcile(&conn, NOW).expect("reconcile");
        blob
    };
    let sent = run_uploads(
        &vault.relay,
        &vault.a,
        &vault.creds,
        policy,
        NOW,
        None,
        None,
    )
    .await
    .expect("uploads");
    assert_eq!(sent.uploaded, 1);
    pushed(&vault.a);
    assert_eq!(entry_state(&vault.a, &blob).as_deref(), Some("done"));
    Uuid::parse_str(&blob).expect("uuid")
}

/// A blob only the server has (a device uploaded it and its row went), stored at `created_at`.
async fn server_blob(vault: &Vault, created_at: i64) -> Uuid {
    let id = Uuid::new_v4();
    let body: BlobBody = Box::new(std::iter::once(Ok(vec![0u8; MIN_BLOB_SEALED as usize])));
    vault
        .relay
        .put_blob(&vault.creds.auth(), id, MIN_BLOB_SEALED, body)
        .await
        .expect("put");
    vault
        .relay
        .set_blob_created_at(&vault_id(vault), id, created_at);
    id
}

fn entry_state(device: &TestConn, blob: &str) -> Option<String> {
    device
        .lock()
        .query_row(
            "SELECT state FROM sync_blobs WHERE blob_id = ?1",
            [blob],
            |r| r.get(0),
        )
        .ok()
}

fn on_server(vault: &Vault) -> Vec<Uuid> {
    vault.relay.blob_ids(&vault_id(vault))
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

fn passed_at(device: &TestConn) -> Option<i64> {
    pass::passed_at(&device.lock()).expect("mark")
}

fn lists(relay: &FakeRelay) -> usize {
    relay
        .calls()
        .into_iter()
        .filter(|route| *route == Route::ListBlobs)
        .count()
}

async fn run<R: Relay>(relay: &R, vault: &Vault, policy: &BlobPolicy, now: i64) -> PassReport {
    run_if_due(relay, &vault.a, &vault.creds, policy, &done_pull(), now)
        .await
        .expect("pass")
}

#[tokio::test]
async fn the_pass_deletes_only_unreferenced_blobs_older_than_seven_days() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let referenced = uploaded_document(&vault, &policy, "d1", b"contract").await;
    vault
        .relay
        .set_blob_created_at(&vault_id(&vault), referenced, SERVER_NOW - 30 * DAY);
    let eight_days = server_blob(&vault, SERVER_NOW - 8 * DAY).await;
    let six_days = server_blob(&vault, SERVER_NOW - 6 * DAY).await;
    vault.relay.set_server_now(Some(SERVER_NOW));

    let report = run(&vault.relay, &vault, &policy, NOW).await;

    assert_eq!(
        report,
        PassReport {
            deleted: 1,
            requeued: 0,
            skipped: None,
            complete: true
        }
    );
    assert_eq!(on_server(&vault), sorted(vec![referenced, six_days]));
    assert!(!on_server(&vault).contains(&eight_days));
    assert_eq!(passed_at(&vault.a), Some(NOW));
}

#[tokio::test]
async fn the_pass_reuploads_a_blob_the_server_lost() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let blob = uploaded_document(&vault, &policy, "d1", b"contract").await;
    vault.relay.lose_blob(&vault_id(&vault), blob);

    let report = run(&vault.relay, &vault, &policy, NOW).await;

    assert_eq!(report.requeued, 1);
    assert!(report.complete);
    assert_eq!(
        entry_state(&vault.a, &blob.to_string()).as_deref(),
        Some("up")
    );
    // The upload lane is woken.
    tokio::time::timeout(
        Duration::from_secs(1),
        policy.wait(Lane::Up, Duration::from_secs(3_600)),
    )
    .await
    .expect("the upload lane was woken");

    run_uploads(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &policy,
        NOW,
        None,
        None,
    )
    .await
    .expect("uploads");
    assert_eq!(on_server(&vault), vec![blob]);
    assert_eq!(
        entry_state(&vault.a, &blob.to_string()).as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn the_pass_ages_blobs_by_the_server_clock_only() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let eight_days = server_blob(&vault, SERVER_NOW - 8 * DAY).await;

    // A listing without the server's clock deletes nothing, however old the blob looks.
    vault.relay.set_server_now(None);
    let report = run(&vault.relay, &vault, &policy, SERVER_NOW).await;
    assert_eq!(report.deleted, 0);
    assert_eq!(on_server(&vault), vec![eight_days]);

    // Six days old on the server's clock, 36 on the device's: kept. A blob dated in the server's
    // future is never deleted. Only the eight-day-old one goes.
    let six_days = server_blob(&vault, SERVER_NOW - 6 * DAY).await;
    let ahead = server_blob(&vault, SERVER_NOW + 10 * DAY).await;
    vault.relay.set_server_now(Some(SERVER_NOW));
    let report = run(&vault.relay, &vault, &policy, SERVER_NOW + 30 * DAY).await;
    assert_eq!(report.deleted, 1);
    assert_eq!(on_server(&vault), sorted(vec![six_days, ahead]));
}

#[tokio::test]
async fn the_pass_runs_at_most_once_a_day() {
    let vault = device().await;
    let policy = BlobPolicy::default();

    let first = run(&vault.relay, &vault, &policy, NOW).await;
    assert!(first.complete);
    assert_eq!(lists(&vault.relay), 1);

    let again = run(&vault.relay, &vault, &policy, NOW + PASS_INTERVAL - 1).await;
    assert_eq!(again.skipped, Some(Skip::NotDue));
    assert!(!again.complete);
    assert_eq!(lists(&vault.relay), 1);
    assert_eq!(passed_at(&vault.a), Some(NOW));

    let next_day = run(&vault.relay, &vault, &policy, NOW + PASS_INTERVAL).await;
    assert!(next_day.complete);
    assert_eq!(lists(&vault.relay), 2);
    assert_eq!(passed_at(&vault.a), Some(NOW + PASS_INTERVAL));
    let queued: i64 = vault
        .a
        .lock()
        .query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))
        .expect("outbox");
    assert_eq!(queued, 0, "the mark is never captured");
}

#[tokio::test]
async fn a_failing_pass_does_not_fail_the_round() {
    let vault = device().await;
    vault
        .relay
        .set_failing(Route::ListBlobs, Some(RelayError::Server(503)));

    let report = run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("the round");

    assert_eq!(report.problem, None);
    assert_eq!(lists(&vault.relay), 1, "the round ran the pass");
    assert_eq!(passed_at(&vault.a), None);
    let marks = state::marks(&vault.a.lock()).expect("marks").expect("on");
    assert_eq!(marks.last_success_at, Some(NOW));
}

#[tokio::test]
async fn a_round_runs_the_pass_and_a_drain_does_not() {
    let vault = device().await;
    let stale = server_blob(&vault, 0).await;
    run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Drain,
        NOW,
        0,
    )
    .await
    .expect("the drain");
    assert_eq!(lists(&vault.relay), 0);

    run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("the round");
    assert_eq!(lists(&vault.relay), 1);
    assert!(!on_server(&vault).contains(&stale));
    assert_eq!(passed_at(&vault.a), Some(NOW));
}

/// Rule 2: content with an invalid ID is never put back (the server can never store it), and it
/// does not hold the pass back.
#[tokio::test]
async fn an_invalid_blob_id_is_never_requeued() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let invalid = "NOT-A-UUID";
    {
        let conn = vault.a.lock();
        conn.execute(
            "INSERT INTO files.blobs (id, data, size, sha256) VALUES (?1, x'0102', 2, \
             'a12871fee210fb8619291eaea194581cbd2531e4b23759d225f6806923f63222')",
            [invalid],
        )
        .expect("blob");
        document(&conn, "d1", invalid, 1_767_225_600);
        queue::reconcile(&conn, NOW).expect("reconcile");
    }
    pushed(&vault.a);
    assert_eq!(entry_state(&vault.a, invalid).as_deref(), Some("up"));

    let report = run(&vault.relay, &vault, &policy, NOW).await;

    assert_eq!(report.skipped, None);
    assert!(report.complete);
    assert_eq!(report.requeued, 0);
    let (state, error): (String, String) = vault
        .a
        .lock()
        .query_row(
            "SELECT state, last_error FROM sync_blobs WHERE blob_id = ?1",
            [invalid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("entry");
    assert_eq!((state.as_str(), error.as_str()), ("up", "invalidId"));
}

/// Rule 3: a blob a replace holds, and both blobs of a photo, are references.
#[tokio::test]
async fn a_held_blob_and_a_thumbnail_are_never_deleted() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let held_only = server_blob(&vault, SERVER_NOW - 30 * DAY).await;
    let full = server_blob(&vault, SERVER_NOW - 30 * DAY).await;
    let thumb = server_blob(&vault, SERVER_NOW - 30 * DAY).await;
    {
        let conn = vault.a.lock();
        conn.execute(
            "INSERT INTO sync_held_blobs (blob_id, held_at) VALUES (?1, 0)",
            [held_only.to_string()],
        )
        .expect("hold");
        photo(
            &conn,
            "ph1",
            &full.to_string(),
            &thumb.to_string(),
            1_767_225_600,
        );
    }
    pushed(&vault.a);
    vault.relay.set_server_now(Some(SERVER_NOW));

    let report = run(&vault.relay, &vault, &policy, NOW).await;

    assert!(report.complete);
    assert_eq!(report.deleted, 0);
    assert_eq!(on_server(&vault), sorted(vec![held_only, full, thumb]));
}

/// Rule 5: the page cap ends the listing; what it saw may be deleted, but nothing is put back (a
/// blob not seen is not lost) and the pass is not done.
#[tokio::test]
async fn a_listing_cut_by_the_page_cap_deletes_what_it_saw_and_requeues_nothing() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let lost = uploaded_document(&vault, &policy, "d1", b"contract").await;
    vault.relay.lose_blob(&vault_id(&vault), lost);
    let stale = sorted(vec![
        server_blob(&vault, SERVER_NOW - 8 * DAY).await,
        server_blob(&vault, SERVER_NOW - 8 * DAY).await,
        server_blob(&vault, SERVER_NOW - 8 * DAY).await,
    ]);
    vault.relay.set_server_now(Some(SERVER_NOW));
    let limits = Limits {
        page: 1,
        max_pages: 2,
        ..Limits::DEFAULT
    };

    let report = run_with(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &policy,
        &done_pull(),
        NOW,
        limits,
    )
    .await
    .expect("pass");

    assert_eq!(report.deleted, 2);
    assert_eq!(report.requeued, 0);
    assert!(!report.complete);
    assert_eq!(on_server(&vault), vec![stale[2]]);
    assert_eq!(
        entry_state(&vault.a, &lost.to_string()).as_deref(),
        Some("done")
    );
    assert_eq!(passed_at(&vault.a), None);
}

/// Rule 5: a listing that fails part-way deletes nothing and puts nothing back.
#[tokio::test]
async fn a_listing_that_fails_part_way_deletes_and_requeues_nothing() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let lost = uploaded_document(&vault, &policy, "d1", b"contract").await;
    vault.relay.lose_blob(&vault_id(&vault), lost);
    for _ in 0..3 {
        server_blob(&vault, SERVER_NOW - 8 * DAY).await;
    }
    vault.relay.set_server_now(Some(SERVER_NOW));
    let relay = Hooked::new(vault.relay.clone());
    relay.fail_list_call(2, RelayError::Server(503));
    let limits = Limits {
        page: 1,
        ..Limits::DEFAULT
    };

    let report = run_with(
        &relay,
        &vault.a,
        &vault.creds,
        &policy,
        &done_pull(),
        NOW,
        limits,
    )
    .await
    .expect("pass");

    assert_eq!((report.deleted, report.requeued), (0, 0));
    assert!(!report.complete);
    assert_eq!(on_server(&vault).len(), 3);
    assert_eq!(
        entry_state(&vault.a, &lost.to_string()).as_deref(),
        Some("done")
    );
    assert_eq!(passed_at(&vault.a), None);
}

/// Rule 6: a row that names a blob after the listing saves it from the delete.
#[tokio::test]
async fn a_row_added_during_the_pass_saves_its_blob() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let ids = sorted(vec![
        server_blob(&vault, SERVER_NOW - 8 * DAY).await,
        server_blob(&vault, SERVER_NOW - 8 * DAY).await,
    ]);
    vault.relay.set_server_now(Some(SERVER_NOW));
    let relay = Hooked::new(vault.relay.clone());
    let device = vault.a.clone();
    let later = ids[1].to_string();
    // While the first blob is being deleted, the person attaches a document naming the second.
    relay.on_first_delete(Box::new(move || {
        document(&device.lock(), "d-new", &later, 1_767_225_600);
    }));

    let report = run(&relay, &vault, &policy, NOW).await;

    assert_eq!(report.deleted, 1);
    assert_eq!(on_server(&vault), vec![ids[1]]);
}

/// Rule 8: a server without the feature, and one that could not be asked, skip the pass without
/// marking it done.
#[tokio::test]
async fn a_server_without_blobs_or_unknown_skips_the_pass() {
    let vault = device().await;
    let info = vault.relay.server_info().await.expect("info");

    vault.relay.set_failing(
        Route::ServerInfo,
        Some(RelayError::Network("connect".into())),
    );
    let unknown = run(&vault.relay, &vault, &BlobPolicy::default(), NOW).await;
    assert_eq!(unknown.skipped, Some(Skip::ServerUnknown));
    assert_eq!(passed_at(&vault.a), None);

    vault.relay.set_failing(Route::ServerInfo, None);
    vault.relay.set_server_info(Some(ServerInfo {
        blobs: false,
        ..info
    }));
    let policy = BlobPolicy::default();
    let without = run(&vault.relay, &vault, &policy, NOW).await;
    assert_eq!(without.skipped, Some(Skip::NoServerFeature));
    assert_eq!(passed_at(&vault.a), None);
    assert_eq!(lists(&vault.relay), 0);
    assert_eq!(policy.problem(), None, "the pass shows no problem");
}

/// Rule 9: a lapsed or refused account ends the pass quietly, not done.
#[tokio::test]
async fn a_refused_account_ends_the_pass_quietly() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    for refusal in [
        RelayError::PaymentRequired,
        RelayError::Unauthorized,
        RelayError::TokenReadOnly,
    ] {
        vault.relay.fail_next(Route::ListBlobs, refusal.clone());
        let report = run(&vault.relay, &vault, &policy, NOW).await;
        assert!(!report.complete, "{refusal:?}");
        assert_eq!(passed_at(&vault.a), None, "{refusal:?}");
    }
    let stale = server_blob(&vault, 0).await;
    vault
        .relay
        .fail_next(Route::DeleteBlob, RelayError::Unauthorized);
    let report = run(&vault.relay, &vault, &policy, NOW).await;
    assert_eq!(report.deleted, 0);
    assert!(!report.complete);
    assert_eq!(on_server(&vault), vec![stale]);
    assert_eq!(passed_at(&vault.a), None);
}

/// Rule 10: a pass out of time stops, not done.
#[tokio::test]
async fn a_pass_out_of_time_stops_and_is_not_done() {
    let vault = device().await;
    let limits = Limits {
        budget: Duration::ZERO,
        ..Limits::DEFAULT
    };
    let report = run_with(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &BlobPolicy::default(),
        &done_pull(),
        NOW,
        limits,
    )
    .await
    .expect("pass");
    assert!(!report.complete);
    assert_eq!(lists(&vault.relay), 0);
    assert_eq!(passed_at(&vault.a), None);
}

/// Rule 11: a blob deleted from the server leaves no entry claiming the server has it.
#[tokio::test]
async fn a_deleted_blob_leaves_no_done_entry() {
    let vault = device().await;
    let policy = BlobPolicy::default();
    let blob = uploaded_document(&vault, &policy, "d1", b"contract").await;
    vault
        .relay
        .set_blob_created_at(&vault_id(&vault), blob, SERVER_NOW - 8 * DAY);
    vault.relay.set_server_now(Some(SERVER_NOW));
    // The document goes after the last reconcile: its entry is still `done`.
    exec(
        &vault.a.lock(),
        "DELETE FROM insurance_documents WHERE id = 'd1'",
    );
    pushed(&vault.a);
    assert_eq!(
        entry_state(&vault.a, &blob.to_string()).as_deref(),
        Some("done")
    );

    let report = run(&vault.relay, &vault, &policy, NOW).await;

    assert_eq!(report.deleted, 1);
    assert!(on_server(&vault).is_empty());
    assert_eq!(entry_state(&vault.a, &blob.to_string()), None);
}

// ---------------------------------------------------------------------------------------------
// A relay that runs a hook while it deletes, or fails one listing call.
// ---------------------------------------------------------------------------------------------

type Hook = Box<dyn FnOnce() + Send>;

struct Hooked {
    relay: FakeRelay,
    on_delete: Mutex<Option<Hook>>,
    list_calls: AtomicUsize,
    fail_list: Mutex<Option<(usize, RelayError)>>,
}

impl Hooked {
    fn new(relay: FakeRelay) -> Self {
        Self {
            relay,
            on_delete: Mutex::new(None),
            list_calls: AtomicUsize::new(0),
            fail_list: Mutex::new(None),
        }
    }

    fn on_first_delete(&self, hook: Hook) {
        *self.on_delete.lock().expect("hook") = Some(hook);
    }

    /// The `n`-th listing call (1-based) answers `error`.
    fn fail_list_call(&self, n: usize, error: RelayError) {
        *self.fail_list.lock().expect("fail") = Some((n, error));
    }
}

impl Relay for Hooked {
    fn server_info(&self) -> impl Future<Output = RelayResult<ServerInfo>> + Send {
        self.relay.server_info()
    }
    fn create_account(&self) -> impl Future<Output = RelayResult<String>> + Send {
        self.relay.create_account()
    }
    fn account_status(
        &self,
        account: &str,
    ) -> impl Future<Output = RelayResult<AccountStatus>> + Send {
        self.relay.account_status(account)
    }
    fn checkout(
        &self,
        account: &str,
        plan: CheckoutPlan,
    ) -> impl Future<Output = RelayResult<String>> + Send {
        self.relay.checkout(account, plan)
    }
    fn create_vault(
        &self,
        account: &str,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<bool>> + Send {
        self.relay.create_vault(account, vault)
    }
    fn vault_info(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<VaultInfo>> + Send {
        self.relay.vault_info(vault)
    }
    fn push(
        &self,
        vault: &VaultAuth,
        ops: &[OutgoingOp],
    ) -> impl Future<Output = RelayResult<PushReceipt>> + Send {
        self.relay.push(vault, ops)
    }
    fn pull(
        &self,
        vault: &VaultAuth,
        after: u64,
        limit: u32,
    ) -> impl Future<Output = RelayResult<PullPage>> + Send {
        self.relay.pull(vault, after, limit)
    }
    fn put_snapshot(
        &self,
        vault: &VaultAuth,
        seq: u64,
        bytes: Vec<u8>,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.put_snapshot(vault, seq, bytes)
    }
    fn get_snapshot(
        &self,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<SnapshotBlob>> + Send {
        self.relay.get_snapshot(vault)
    }
    fn delete_vault(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.delete_vault(vault)
    }
    fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.put_blob(vault, blob, sealed_len, body)
    }
    fn head_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
    ) -> impl Future<Output = RelayResult<Option<u64>>> + Send {
        self.relay.head_blob(vault, blob)
    }
    fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> impl Future<Output = RelayResult<Delivery>> + Send {
        self.relay.get_blob(vault, blob, sink)
    }
    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<Uuid>,
        limit: u32,
    ) -> RelayResult<BlobPage> {
        let call = self.list_calls.fetch_add(1, Ordering::SeqCst) + 1;
        let failure = {
            let mut fail = self.fail_list.lock().expect("fail");
            match fail.as_ref() {
                Some((n, _)) if *n == call => fail.take().map(|(_, error)| error),
                _ => None,
            }
        };
        if let Some(error) = failure {
            return Err(error);
        }
        self.relay.list_blobs(vault, after, limit).await
    }
    async fn delete_blob(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<()> {
        let hook = self.on_delete.lock().expect("hook").take();
        if let Some(hook) = hook {
            hook();
        }
        self.relay.delete_blob(vault, blob).await
    }
}
