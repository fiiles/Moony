//! `pull_all` against the fake relay: each verdict and where it leaves the cursor, the re-join,
//! the relay's refusals as sync error classes, and the pages the relay serves.

use std::sync::atomic::Ordering;

use super::*;
use crate::services::sync::capture;
use crate::services::sync::devices;
use crate::services::sync::engine::pull::{pull_all, pull_pages, PULL_PAGE};
use crate::services::sync::engine::{PullEnd, PullReport, PullStop};
use crate::services::sync::error::SyncError;
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, CheckoutPlan, PullPage, PulledOp, PushReceipt, RelayError, RelayResult,
    SnapshotBlob, VaultInfo,
};

async fn pull(vault: &Vault, device: &TestConn) -> PullReport {
    pull_all(&vault.relay, device, &vault.creds, NOW)
        .await
        .expect("pull")
}

/// One institution written on `device` and sealed into its next envelope.
pub(super) fn write_and_seal(vault: &Vault, device: &TestConn, id: &str) -> Vec<u8> {
    let conn = device.lock();
    institution(&conn, id);
    seal_from(&conn, &vault.creds)
}

#[tokio::test]
async fn apply_moves_the_cursor_and_the_author_head() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, std::slice::from_ref(&b1)).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.applied, 1);
    assert_eq!(report.end(), PullEnd::Done);
    assert!(report.done() && !report.behind && !report.rejoined);
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 1);
    assert_eq!(head(&a, B), Some((1, envelope_hash(&b1).to_vec(), false)));
    assert!(has_institution(&a, "b1"));
}

#[tokio::test]
async fn replays_and_own_envelopes_move_the_cursor() {
    let vault = vault().await;
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, &[a1, b1.clone()]).await;
    vault.relay.replay_op(2);

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.applied, 1, "the replay is skipped");
    assert!(report.done());
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 3);
    assert_eq!(head(&a, B), Some((1, envelope_hash(&b1).to_vec(), false)));
}

#[tokio::test]
async fn an_own_envelope_leaves_sync_unsent_when_pulled() {
    let vault = vault().await;
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    let _a2 = write_and_seal(&vault, &vault.a, "a2");
    push(&vault.relay, &vault.creds, &[a1]).await;
    let unsent = |conn: &Connection| count(conn, "SELECT COUNT(*) FROM sync_unsent");
    assert_eq!(unsent(&vault.a.lock()), 2);

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.applied, 0);
    assert!(report.done());
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 1);
    assert_eq!(count(&a, "SELECT MIN(seq) FROM sync_unsent"), 2);
    assert_eq!(
        unsent(&a),
        1,
        "only the envelope the relay does not hold waits"
    );
}

/// `A` and a copy of it (`VACUUM INTO`, as a restored or copied profile) both write and push
/// their envelope 2. `A` pulls the copy's envelope: an own-ID envelope it never sealed.
#[tokio::test]
async fn clone_rejoins_then_rejudges_the_triggering_envelope() {
    let vault = vault().await;
    let key_before: Vec<u8> = {
        let a = vault.a.lock();
        let public = keys::ensure_device_keypair(&a).expect("key pair");
        devices::record_this_device(&a, "Filip's Mac", &public, NOW).expect("device row");
        state::set_binding_written(&a, true).expect("binding");
        a.query_row("SELECT device_secret_key FROM sync_state", [], |r| r.get(0))
            .expect("key")
    };
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    push(&vault.relay, &vault.creds, &[a1]).await;
    assert!(pull(&vault, &vault.a).await.done());
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("copy.db");
    vault
        .a
        .lock()
        .execute("VACUUM INTO ?1", [path.to_str().expect("utf-8")])
        .expect("copy A");
    let copy = Connection::open(&path).expect("open the copy");
    exec(&copy, "PRAGMA foreign_keys = ON");
    capture::install(&copy, A).expect("capture on the copy");

    let a2 = write_and_seal(&vault, &vault.a, "mine");
    institution(&copy, "theirs");
    let copied = seal_from(&copy, &vault.creds);
    push(&vault.relay, &vault.creds, &[a2, copied.clone()]).await;

    let report = pull(&vault, &vault.a).await;

    assert!(report.rejoined);
    assert_eq!(
        report.applied, 1,
        "the triggering envelope applied after the re-join"
    );
    assert!(report.done());
    let a = vault.a.lock();
    let sync = state::load(&a).expect("state").expect("row");
    assert!(sync.device > A, "a re-join ID sorts above the old one");
    assert_eq!((sync.next_seq, sync.last_hash), (1, None), "a fresh chain");
    assert_eq!(sync.cursor, 3);
    assert_eq!(
        head(&a, A),
        Some((2, envelope_hash(&copied).to_vec(), false)),
        "the old ID is now an author like any other"
    );
    assert!(has_institution(&a, "theirs") && has_institution(&a, "mine"));
    let queued: Vec<String> = a
        .prepare("SELECT tbl || ' ' || pk FROM sync_outbox ORDER BY tbl, pk")
        .expect("outbox")
        .query_map([], |r| r.get(0))
        .expect("outbox")
        .map(|r| r.expect("row"))
        .collect();
    // Re-queued from the watermark on (inclusive): the confirmed envelope 1's newest own row,
    // the own edit the copy's envelope 2 displaced, and the new device row.
    assert_eq!(
        queued,
        vec![
            "institutions a1".to_string(),
            "institutions mine".to_string(),
            format!("sync_devices {}", sync.device.to_hex()),
        ]
    );
    assert_eq!(
        devices::device_row(&a, sync.device).expect("row"),
        Some(("Filip's Mac".to_string(), devices::platform().to_string())),
        "the new ID keeps the old row's name"
    );
    let key_after: Vec<u8> = a
        .query_row("SELECT device_secret_key FROM sync_state", [], |r| r.get(0))
        .expect("key");
    assert_ne!(key_after, key_before, "a new device key pair");
    let marks = state::marks(&a).expect("marks").expect("row");
    assert!(!marks.binding_written, "the binding is written again");
}

/// Since ruling T8-3 the re-join makes the old ID's head `trigger.seq − 1`, so the re-judged
/// trigger can no longer be a gap; what it can still be is a fork of the old ID's head. Here the
/// copy's envelope 1 never reaches `A` (the relay withholds it), so the copy's envelope 2 names a
/// `prev` that is not `A`'s own envelope 1: after the re-join the old ID is quarantined and the
/// trigger held, the cursor before it.
#[tokio::test]
async fn a_clone_whose_trigger_then_forks_is_held() {
    let vault = vault().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("copy.db");
    vault
        .a
        .lock()
        .execute("VACUUM INTO ?1", [path.to_str().expect("utf-8")])
        .expect("copy A");
    let copy = Connection::open(&path).expect("open the copy");
    exec(&copy, "PRAGMA foreign_keys = ON");
    capture::install(&copy, A).expect("capture on the copy");

    let a1 = write_and_seal(&vault, &vault.a, "mine");
    institution(&copy, "theirs-1");
    let copied1 = seal_from(&copy, &vault.creds);
    institution(&copy, "theirs-2");
    let copied2 = seal_from(&copy, &vault.creds);
    push(&vault.relay, &vault.creds, &[a1, copied1, copied2]).await;
    vault.relay.drop_op(2);

    let report = pull(&vault, &vault.a).await;

    assert!(report.rejoined);
    assert_eq!((report.applied, report.held), (0, 1));
    assert_eq!(report.end(), PullEnd::Held);
    let a = vault.a.lock();
    assert!(state::load(&a).expect("state").expect("row").device > A);
    assert_eq!(cursor(&a), 1, "the cursor stays before the held trigger");
    let (seq, _, quarantined) = head(&a, A).expect("old ID head");
    assert_eq!((seq, quarantined), (1, true));
    assert!(!has_institution(&a, "theirs-2"));
}

#[tokio::test]
async fn a_gap_holds_without_moving_the_cursor_past_it() {
    let vault = vault().await;
    let sealed: Vec<Vec<u8>> = ["b1", "b2", "b3"]
        .iter()
        .map(|id| write_and_seal(&vault, &vault.b, id))
        .collect();
    push(&vault.relay, &vault.creds, &sealed).await;
    vault.relay.drop_op(2);

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.end(), PullEnd::Waiting);
    assert_eq!((report.applied, report.gaps), (1, 1));
    assert!(report.may_push() && !report.may_snapshot());
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 1);
    assert_eq!(head(&a, B).map(|h| h.0), Some(1));
    assert!(!has_institution(&a, "b3"));
}

#[tokio::test]
async fn an_invalid_chain_link_stops_without_moving_the_cursor() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    // A later envelope that names no previous one.
    let broken = forge(&vault.creds, &changeset(C, 2, None, vec![]));
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1, broken, b2]).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.stop, Some(PullStop::Invalid));
    assert_eq!(report.applied, 1);
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 1);
    assert_eq!(head(&a, C), None);
    assert!(!has_institution(&a, "b2"));
}

#[tokio::test]
async fn an_undecryptable_envelope_stops_and_is_never_skipped() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let mut tampered = write_and_seal(&vault, &vault.b, "b2");
    *tampered.last_mut().expect("tag") ^= 1;
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    push(&vault.relay, &vault.creds, &[b1, tampered, c1]).await;

    let first = pull(&vault, &vault.a).await;
    let second = pull(&vault, &vault.a).await;

    for report in [&first, &second] {
        assert_eq!(report.stop, Some(PullStop::Undecryptable));
        assert!(!report.may_push());
    }
    assert_eq!((first.applied, second.applied), (1, 0));
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 1);
    assert_eq!(head(&a, C), None, "nothing after the stop is taken");
}

/// The epoch is read before authentication: an unknown one is no sign of a rekey, only of an
/// envelope this device cannot open (obligation a).
#[tokio::test]
async fn an_unknown_epoch_stops_like_an_undecryptable_envelope_and_changes_nothing() {
    let vault = vault().await;
    let json = to_json(&changeset(C, 1, None, vec![])).expect("json");
    let foreign = seal_op(
        vault.creds.keys.current_epoch() + 1,
        &[0x55; 32],
        &vault.creds.vault_id.0,
        &json,
    )
    .expect("seal");
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, &[foreign, b1]).await;
    let before = sync_rows(&vault.a.lock());

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.stop, Some(PullStop::Undecryptable));
    assert_eq!(report.applied, 0);
    assert_eq!(sync_rows(&vault.a.lock()), before);
}

#[tokio::test]
async fn newer_schema_stops_pulling() {
    let vault = vault().await;
    let mut newer = changeset(C, 1, None, vec![]);
    newer.schema = schema_number() + 1;
    let newer = forge(&vault.creds, &newer);
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, &[newer, b1]).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.stop, Some(PullStop::NewerSchema));
    assert_eq!(report.applied, 0);
    assert!(report.may_push(), "an older device keeps pushing (P1)");
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 0);
    assert!(!has_institution(&a, "b1"));
}

#[tokio::test]
async fn quarantine_keeps_the_cursor_before_the_held_envelope_while_others_apply() {
    let vault = vault().await;
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    // C's envelope 2 does not follow its envelope 1: C's chain forked.
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let c3 = forge(
        &vault.creds,
        &changeset(C, 3, Some(envelope_hash(&c2)), vec![]),
    );
    push(&vault.relay, &vault.creds, &[c1.clone(), c2, b1, c3]).await;

    let first = pull(&vault, &vault.a).await;

    assert_eq!((first.applied, first.held), (2, 2));
    assert_eq!(first.end(), PullEnd::Held);
    assert!(!first.done() && first.may_push());
    {
        let a = vault.a.lock();
        assert_eq!(cursor(&a), 1, "the cursor stays before C's forked envelope");
        assert_eq!(head(&a, C), Some((1, envelope_hash(&c1).to_vec(), true)));
        assert!(has_institution(&a, "b1"), "other authors keep syncing");
    }

    let again = pull(&vault, &vault.a).await;

    assert_eq!((again.applied, again.held), (0, 2));
    assert_eq!(cursor(&vault.a.lock()), 1);
}

#[tokio::test]
async fn a_missing_parent_rolls_back_and_holds_the_cursor() {
    let vault = vault().await;
    let g = Clock {
        hlc: 1_000 << 16,
        device: C,
    };
    let plan = row(
        "spending_plans",
        "p1",
        g,
        &[
            ("amount", text("1000.00")),
            ("closed_at", SqlValue::Null),
            ("color", text("#336699")),
            ("created_at", SqlValue::Integer(NOW)),
            ("currency", text("CZK")),
            ("icon", text("plane")),
            ("name", text("Plan p1")),
            ("note", text("first")),
            ("updated_at", SqlValue::Integer(NOW)),
            ("year", SqlValue::Integer(2026)),
        ],
    );
    let mut payment = row(
        "bank_transactions",
        "t1",
        g,
        &[
            ("bank_account_id", text("acc-x")),
            ("tx_type", text("debit")),
            ("amount", text("-10.00")),
            ("currency", text("CZK")),
            ("booking_date", SqlValue::Integer(1_767_225_600)),
        ],
    );
    payment.parents.insert("bank_account_id".into(), g);
    let orphan = forge(&vault.creds, &changeset(C, 1, None, vec![plan, payment]));
    push(&vault.relay, &vault.creds, &[orphan]).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.end(), PullEnd::Waiting);
    assert_eq!((report.applied, report.gaps), (0, 1));
    assert!(report.may_push());
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 0);
    assert_eq!(head(&a, C), None);
    assert_eq!(
        count(&a, "SELECT COUNT(*) FROM spending_plans WHERE id = 'p1'"),
        0
    );
}

#[tokio::test]
async fn below_barrier_after_the_marker_moves_head_and_cursor() {
    let vault = vault().await;
    exec(
        &vault.a.lock(),
        "UPDATE sync_state SET barrier = barrier + 1",
    );
    let barrier = state::load(&vault.a.lock())
        .expect("state")
        .expect("row")
        .barrier;
    let mut marker = changeset(B, 1, None, vec![]);
    marker.barrier = barrier;
    let marker = forge(&vault.creds, &marker);
    let mut old = changeset(C, 1, None, vec![]);
    old.barrier = barrier - 1;
    let old = forge(&vault.creds, &old);
    push(&vault.relay, &vault.creds, &[marker, old.clone()]).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.applied, 1, "only the marker is applied");
    assert!(report.done());
    let a = vault.a.lock();
    let sync = state::load(&a).expect("state").expect("row");
    assert_eq!(sync.cursor, 2);
    assert_eq!(sync.barrier_seq, Some(1));
    assert_eq!(sync.missed_below_barrier, Some(2));
    assert_eq!(head(&a, C), Some((1, envelope_hash(&old).to_vec(), false)));
}

#[tokio::test]
async fn a_pruned_cursor_returns_snapshot_required() {
    let vault = vault().await;
    let sealed: Vec<Vec<u8>> = ["b1", "b2", "b3"]
        .iter()
        .map(|id| write_and_seal(&vault, &vault.b, id))
        .collect();
    push(&vault.relay, &vault.creds, &sealed).await;
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 3, vec![1; 64])
        .await
        .expect("snapshot");
    vault.relay.prune_to(3);

    let result = pull_all(&vault.relay, &vault.a, &vault.creds, NOW).await;

    assert!(
        matches!(result, Err(SyncError::SnapshotRequired)),
        "{result:?}"
    );
    assert_eq!(cursor(&vault.a.lock()), 0);
}

#[tokio::test]
async fn a_wiped_vault_returns_vault_not_found() {
    let vault = vault().await;
    vault.relay.wipe_vault(&vault.creds.vault_id.to_b64());

    let result = pull_all(&vault.relay, &vault.a, &vault.creds, NOW).await;

    assert!(
        matches!(result, Err(SyncError::VaultNotFound)),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_refused_pull_returns_its_sync_error_class() {
    let vault = vault().await;
    let cases: Vec<(RelayError, &str)> = vec![
        (RelayError::PaymentRequired, "Lapsed"),
        (RelayError::Unauthorized, "Unauthorized"),
        (RelayError::RateLimited, "RateLimited"),
        (RelayError::Network("timeout".into()), "Transient(Offline)"),
        (RelayError::Server(503), "Transient(Relay)"),
    ];
    for (error, class) in cases {
        vault.relay.fail_next(Route::Pull, error);
        let result = pull_all(&vault.relay, &vault.a, &vault.creds, NOW).await;
        assert_eq!(format!("{:?}", result.expect_err(class)), class);
    }
}

#[tokio::test]
async fn a_locked_profile_ends_the_pull_with_a_local_error() {
    let vault = vault().await;
    vault.a.locked.store(true, Ordering::SeqCst);

    let result = pull_all(&vault.relay, &vault.a, &vault.creds, NOW).await;

    assert!(
        matches!(result, Err(SyncError::Local(AppError::Auth(_)))),
        "{result:?}"
    );
    assert!(
        !vault.relay.calls().contains(&Route::Pull),
        "nothing reaches the relay"
    );
}

#[tokio::test]
async fn pulled_bytes_count_toward_the_snapshot_policy() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1.clone(), b2.clone()]).await;
    let pulled = |conn: &Connection| {
        state::marks(conn)
            .expect("marks")
            .expect("row")
            .pulled_bytes
    };

    pull(&vault, &vault.a).await;

    assert_eq!(pulled(&vault.a.lock()), (b1.len() + b2.len()) as u64);

    // An envelope that stops the pull is not taken, so it does not count.
    let mut tampered = write_and_seal(&vault, &vault.b, "b3");
    *tampered.last_mut().expect("tag") ^= 1;
    push(&vault.relay, &vault.creds, &[tampered]).await;
    assert_eq!(
        pull(&vault, &vault.a).await.stop,
        Some(PullStop::Undecryptable)
    );
    assert_eq!(pulled(&vault.a.lock()), (b1.len() + b2.len()) as u64);
}

#[tokio::test]
async fn a_page_boundary_never_loses_or_repeats_an_envelope() {
    let vault = vault().await;
    let sealed: Vec<Vec<u8>> = (0..1_201)
        .map(|i| write_and_seal(&vault, &vault.b, &format!("b{i:04}")))
        .collect();
    push(&vault.relay, &vault.creds, &sealed).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!(report.applied, 1_201);
    assert!(report.done());
    let pulls = vault
        .relay
        .calls()
        .into_iter()
        .filter(|r| *r == Route::Pull)
        .count();
    assert_eq!(pulls, 3, "pages of 500, 500 and 201");
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 1_201);
    assert_eq!(head(&a, B).map(|h| h.0), Some(1_201));
    assert_eq!(
        count(&a, "SELECT COUNT(*) FROM institutions WHERE id LIKE 'b%'"),
        1_201
    );
}

/// The relay withholds the newest envelope (obligation c): the cursor never moves to `head`, and
/// the device is reported behind, not failed.
#[tokio::test]
async fn a_withheld_trailing_envelope_leaves_the_cursor_and_reports_behind() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1, b2]).await;
    vault.relay.drop_op(2);

    let first = pull(&vault, &vault.a).await;

    assert_eq!(first.applied, 1);
    assert!(first.behind && first.done() && first.may_push());
    assert_eq!(cursor(&vault.a.lock()), 1);

    // Now the page is empty, `more` is false and the head is still 2.
    let second = pull(&vault, &vault.a).await;

    assert_eq!(second.applied, 0);
    assert!(second.behind && second.done());
    assert_eq!(cursor(&vault.a.lock()), 1);
}

/// Rewrites the pull page fetched after the given sequence.
type Mangle = fn(u64, PullPage) -> PullPage;

/// A relay that serves `FakeRelay`'s pull pages through `mangle` (given the `after` of the call).
/// A pull makes no other call.
pub(super) struct Mangled<F> {
    pub(super) inner: FakeRelay,
    pub(super) mangle: F,
}

impl<F: Fn(u64, PullPage) -> PullPage + Send + Sync> Relay for Mangled<F> {
    async fn create_account(&self) -> RelayResult<String> {
        unreachable!("not a pull call")
    }
    async fn account_status(&self, _: &str) -> RelayResult<AccountStatus> {
        unreachable!("not a pull call")
    }
    async fn checkout(&self, _: &str, _: CheckoutPlan) -> RelayResult<String> {
        unreachable!("not a pull call")
    }
    async fn create_vault(&self, _: &str, _: &VaultAuth) -> RelayResult<bool> {
        unreachable!("not a pull call")
    }
    async fn vault_info(&self, _: &VaultAuth) -> RelayResult<VaultInfo> {
        unreachable!("not a pull call")
    }
    async fn push(&self, _: &VaultAuth, _: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        unreachable!("not a pull call")
    }
    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        let page = self.inner.pull(vault, after, limit).await?;
        Ok((self.mangle)(after, page))
    }
    async fn put_snapshot(&self, _: &VaultAuth, _: u64, _: Vec<u8>) -> RelayResult<()> {
        unreachable!("not a pull call")
    }
    async fn get_snapshot(&self, _: &VaultAuth) -> RelayResult<SnapshotBlob> {
        unreachable!("not a pull call")
    }
    async fn delete_vault(&self, _: &VaultAuth) -> RelayResult<()> {
        unreachable!("not a pull call")
    }
}

/// Every page is checked before any of its envelopes is handled (obligation b).
#[tokio::test]
async fn a_page_outside_the_relay_contract_stops_as_invalid_without_moving_the_cursor() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1, b2]).await;
    let before = sync_rows(&vault.a.lock());
    let mangles: Vec<(&str, Mangle)> = vec![
        ("out of order", |_, mut p| {
            p.ops.reverse();
            p
        }),
        ("repeated", |_, mut p| {
            p.ops[1].seq = p.ops[0].seq;
            p
        }),
        ("not above after", |_, mut p| {
            p.ops[0].seq = 0;
            p
        }),
        ("head below the page", |_, mut p| {
            p.head = 1;
            p
        }),
        ("empty with more", |_, mut p| {
            p.ops.clear();
            p.more = true;
            p
        }),
        ("more than the limit", |_, mut p| {
            let first = p.ops[0].clone();
            p.ops = (1..=PULL_PAGE as u64 + 1)
                .map(|seq| PulledOp {
                    seq,
                    ..first.clone()
                })
                .collect();
            p.head = PULL_PAGE as u64 + 1;
            p
        }),
    ];
    for (what, mangle) in mangles {
        let relay = Mangled {
            inner: vault.relay.clone(),
            mangle,
        };

        let report = pull_all(&relay, &vault.a, &vault.creds, NOW)
            .await
            .expect(what);

        assert_eq!(report.stop, Some(PullStop::Invalid), "{what}");
        assert_eq!(report.applied, 0, "{what}");
        assert_eq!(sync_rows(&vault.a.lock()), before, "{what}");
    }
    // The honest page still applies.
    assert_eq!(pull(&vault, &vault.a).await.applied, 2);
}

#[test]
fn the_report_says_how_the_pull_ended_and_whether_the_round_may_push() {
    let report = |stop: Option<PullStop>, held: usize| PullReport {
        stop,
        held,
        ..PullReport::default()
    };
    let gaps = |stop: Option<PullStop>, held: usize, gaps: usize| PullReport {
        gaps,
        ..report(stop, held)
    };
    let unfinished = |stop: Option<PullStop>, held: usize| PullReport {
        unfinished: true,
        ..report(stop, held)
    };
    let cases = [
        (unfinished(None, 0), PullEnd::Unfinished, false, false),
        (unfinished(None, 3), PullEnd::Unfinished, false, false),
        (
            unfinished(Some(PullStop::NewerSchema), 0),
            PullEnd::Stopped(PullStop::NewerSchema),
            false,
            true,
        ),
        (report(None, 0), PullEnd::Done, true, true),
        (report(None, 2), PullEnd::Held, false, true),
        (
            report(Some(PullStop::NewerSchema), 0),
            PullEnd::Stopped(PullStop::NewerSchema),
            false,
            true,
        ),
        (
            report(Some(PullStop::NewerSchema), 1),
            PullEnd::Stopped(PullStop::NewerSchema),
            false,
            true,
        ),
        (gaps(None, 0, 1), PullEnd::Waiting, false, true),
        // A quarantine outranks a gap; both still push.
        (gaps(None, 2, 1), PullEnd::Held, false, true),
        (
            PullReport {
                unfinished: true,
                ..gaps(None, 0, 1)
            },
            PullEnd::Unfinished,
            false,
            false,
        ),
        (
            gaps(Some(PullStop::NewerSchema), 0, 1),
            PullEnd::Stopped(PullStop::NewerSchema),
            false,
            true,
        ),
        (
            gaps(Some(PullStop::Invalid), 1, 1),
            PullEnd::Stopped(PullStop::Invalid),
            false,
            false,
        ),
        (
            report(Some(PullStop::Undecryptable), 0),
            PullEnd::Stopped(PullStop::Undecryptable),
            false,
            false,
        ),
    ];
    for (report, end, done, may_push) in cases {
        assert_eq!(report.end(), end, "{report:?}");
        assert_eq!(report.done(), done, "{report:?}");
        assert_eq!(report.may_push(), may_push, "{report:?}");
    }
}

/// `count` copies of `ct` at the positions after `after`, with `more` set, the head where `head`
/// puts it: a relay that re-serves one valid envelope forever.
fn replays(ct: Vec<u8>, count: u64, head: fn(u64) -> u64) -> impl Fn(u64, PullPage) -> PullPage {
    move |after, _| PullPage {
        ops: (after + 1..=after + count)
            .map(|seq| PulledOp {
                seq,
                id: op_id(&ct),
                ct: ct.clone(),
            })
            .collect(),
        head: head(after + count),
        more: true,
    }
}

fn pulls(relay: &FakeRelay) -> usize {
    relay
        .calls()
        .into_iter()
        .filter(|r| *r == Route::Pull)
        .count()
}

/// Relay positions are not authenticated: a relay can re-serve one envelope at every position
/// with `more` set (item 1). The pull ends at the head its first page reported.
#[tokio::test]
async fn endless_replays_end_the_pull_at_the_first_pages_head() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let relay = Mangled {
        inner: vault.relay.clone(),
        mangle: replays(b1, PULL_PAGE as u64, |last| last),
    };

    let report = pull_all(&relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert_eq!(report.applied, 1, "every copy after the first is a replay");
    assert_eq!(report.end(), PullEnd::Done);
    assert_eq!(pulls(&vault.relay), 1);
    assert_eq!(cursor(&vault.a.lock()), PULL_PAGE as u64);
}

/// A relay that also keeps raising its head is cut off by the page limit; the pull is unfinished
/// and the round does not push.
#[tokio::test]
async fn a_relay_that_keeps_raising_its_head_leaves_the_pull_unfinished() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let relay = Mangled {
        inner: vault.relay.clone(),
        mangle: replays(b1, PULL_PAGE as u64, |_| u64::MAX),
    };

    let report = pull_pages(&relay, &vault.a, &vault.creds, NOW, 3)
        .await
        .expect("pull");

    assert!(report.unfinished);
    assert_eq!(report.end(), PullEnd::Unfinished);
    assert!(!report.done() && !report.may_push());
    assert_eq!((report.applied, report.stop), (1, None));
    assert_eq!(pulls(&vault.relay), 3);
    assert_eq!(cursor(&vault.a.lock()), 3 * PULL_PAGE as u64);
}

/// The relay's head never goes back (item 2): after a pull to sequence 2, an empty page with head
/// 0 is outside the contract.
#[tokio::test]
async fn a_head_below_the_cursor_is_invalid() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1, b2]).await;
    assert!(pull(&vault, &vault.a).await.done());
    assert_eq!(cursor(&vault.a.lock()), 2);
    let before = sync_rows(&vault.a.lock());
    let relay = Mangled {
        inner: vault.relay.clone(),
        mangle: |_, mut p: PullPage| {
            assert!(p.ops.is_empty() && !p.more);
            p.head = 0;
            p
        },
    };

    let report = pull_all(&relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert_eq!(report.stop, Some(PullStop::Invalid));
    assert_eq!(sync_rows(&vault.a.lock()), before);
}

/// An envelope fetched again counts its bytes once (item 3): while C is quarantined, every pull
/// fetches C's held envelopes and B's envelope behind them again.
#[tokio::test]
async fn an_envelope_fetched_again_counts_its_bytes_once() {
    let vault = vault().await;
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, &[c1.clone(), c2, b1]).await;
    let pulled = |conn: &Connection| {
        state::marks(conn)
            .expect("marks")
            .expect("row")
            .pulled_bytes
    };

    for _ in 0..3 {
        let report = pull(&vault, &vault.a).await;
        assert_eq!(report.end(), PullEnd::Held);
        assert_eq!(
            pulled(&vault.a.lock()),
            c1.len() as u64,
            "only what the cursor moved past counts"
        );
    }
}
