//! `seal_batch`, `next_push_batch_with` and `push_all` against the fake relay: the one-transaction
//! seal into `sync_unsent`, the same op IDs after a crash or a lost answer, the relay's batch
//! limits, its refusals as sync error classes, a refused row, and the barrier marker. A pushed
//! envelope leaves `sync_unsent` only with its echo (audit M4, `echo.rs`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::services::sync::chain::acknowledge_own;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::push::{
    next_push_batch, next_push_batch_with, push_all, seal_batch, PushReport, SealReport,
    PUSH_MAX_OPS, SEAL_BATCH, SEAL_PER_HOLD,
};
use crate::services::sync::error::{SyncError, SyncResult, TooLarge, Transient};
use crate::services::sync::journal::Refused;
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, CheckoutPlan, PullPage, PushReceipt, RelayError, RelayResult, SnapshotBlob,
    VaultInfo,
};
use crate::services::sync::test_support::noise;

async fn push_a(vault: &Vault) -> SyncResult<PushReport> {
    push_all(&vault.relay, &vault.a, &vault.creds).await
}

/// `A` pulls: its pushed envelopes come back and stop waiting.
async fn echo_a(vault: &Vault) {
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
}

/// `(seq, op ID)` of every envelope waiting in `sync_unsent`, in sequence order.
fn unsent(conn: &Connection) -> Vec<(i64, String)> {
    conn.prepare("SELECT seq, envelope FROM sync_unsent ORDER BY seq")
        .expect("unsent")
        .query_map([], |r| Ok((r.get(0)?, op_id(&r.get::<_, Vec<u8>>(1)?))))
        .expect("unsent")
        .collect::<std::result::Result<_, _>>()
        .expect("unsent")
}

fn relay_ids(relay: &FakeRelay) -> Vec<String> {
    relay.ops().into_iter().map(|op| op.id).collect()
}

fn ids(rows: &[(i64, String)]) -> Vec<String> {
    rows.iter().map(|(_, id)| id.clone()).collect()
}

fn marker_barrier(conn: &Connection) -> u32 {
    state::marks(conn)
        .expect("marks")
        .expect("row")
        .marker_barrier
}

fn barrier(conn: &Connection) -> u32 {
    state::load(conn).expect("state").expect("row").barrier
}

/// `seal_batch` on `device`, which the caller must not hold locked.
fn seal(vault: &Vault, device: &TestConn) -> SealReport {
    seal_batch(device, &vault.creds, None).expect("seal")
}

/// A `ConnAccess` that records each lock hold: how long it took and how many envelopes were
/// waiting in `sync_unsent` when it ended.
#[derive(Clone)]
struct Holds {
    inner: TestConn,
    holds: Arc<Mutex<Vec<(Duration, i64)>>>,
}

impl Holds {
    fn new(inner: TestConn) -> Self {
        Self {
            inner,
            holds: Arc::default(),
        }
    }

    fn holds(&self) -> Vec<(Duration, i64)> {
        self.holds.lock().expect("holds").clone()
    }

    /// Envelopes each hold added to `sync_unsent`.
    fn sealed_per_hold(&self) -> Vec<i64> {
        let mut before = 0;
        self.holds()
            .into_iter()
            .map(|(_, waiting)| {
                let added = waiting - before;
                before = waiting;
                added
            })
            .collect()
    }
}

impl ConnAccess for Holds {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.inner.with_conn(|conn| {
            let started = Instant::now();
            let out = f(conn);
            let took = started.elapsed();
            let waiting = count(conn, "SELECT COUNT(*) FROM sync_unsent");
            self.holds.lock().expect("holds").push((took, waiting));
            out
        })
    }

    /// Unlogged: only a cancelled download's cleanup tries the connection.
    fn try_with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Option<Result<T>> {
        self.inner.try_with_conn(f)
    }
}

#[tokio::test]
async fn sealing_stores_envelopes_and_clears_the_outbox_in_one_transaction() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        // 400 institutions with ~1 KiB of incompressible notes each: more than one 60 KiB
        // envelope.
        for i in 0..400 {
            a.execute(
                "INSERT INTO institutions (id, name) VALUES (?1, ?2)",
                params![format!("i{i:03}"), noise(1024, i)],
            )
            .expect("institution");
        }
        // Past a barrier not marked yet: any sealed envelope marks it.
        exec(&a, "UPDATE sync_state SET barrier = barrier + 1");
    }

    let report = seal(&vault, &vault.a);

    let a = vault.a.lock();
    let n = report.sealed;
    assert!(n > 1, "the outbox needs more than one envelope: {n}");
    assert!(!report.marker && report.refused.is_empty());
    let rows = unsent(&a);
    assert_eq!(
        rows.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
        (1..=n as i64).collect::<Vec<_>>()
    );
    let own: Vec<(i64, Vec<u8>)> = a
        .prepare("SELECT seq, hash FROM sync_own_log ORDER BY seq")
        .expect("own log")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("own log")
        .collect::<std::result::Result<_, _>>()
        .expect("own log");
    let sealed: Vec<(i64, Vec<u8>)> = a
        .prepare("SELECT seq, envelope FROM sync_unsent ORDER BY seq")
        .expect("unsent")
        .query_map([], |r| {
            Ok((r.get(0)?, envelope_hash(&r.get::<_, Vec<u8>>(1)?).to_vec()))
        })
        .expect("unsent")
        .collect::<std::result::Result<_, _>>()
        .expect("unsent");
    assert_eq!(own, sealed, "the own log holds each sealed envelope's hash");
    let sync = state::load(&a).expect("state").expect("row");
    assert_eq!(sync.next_seq, n as u64 + 1);
    assert_eq!(
        sync.last_hash.map(|h| h.to_vec()),
        Some(sealed[n - 1].1.clone())
    );
    assert_eq!(count(&a, "SELECT COUNT(*) FROM sync_outbox"), 0);
    assert_eq!(
        marker_barrier(&a),
        sync.barrier,
        "every own envelope marks its barrier"
    );
}

#[tokio::test]
async fn a_crash_before_the_push_resends_the_same_op_ids() {
    let vault = vault().await;
    let sealed = {
        let a = vault.a.lock();
        institution(&a, "a1");
        institution(&a, "a2");
        drop(a);
        assert_eq!(seal(&vault, &vault.a).sealed, 1);
        unsent(&vault.a.lock())
    };
    // The process dies before the push. The next round seals nothing new: the envelopes are
    // already waiting, and a later edit waits behind them.
    {
        institution(&vault.a.lock(), "a3");
        let again = seal(&vault, &vault.a);
        assert_eq!((again.sealed, again.marker), (0, false));
        assert_eq!(unsent(&vault.a.lock()), sealed);
    }
    // A push that never reaches the relay leaves them waiting too.
    vault
        .relay
        .fail_next(Route::Push, RelayError::Network("connect".into()));
    assert!(matches!(
        push_a(&vault).await,
        Err(SyncError::Transient(Transient::Offline))
    ));
    assert_eq!(unsent(&vault.a.lock()), sealed);

    let report = push_a(&vault).await.expect("push");

    assert_eq!(
        report.pushed, 2,
        "the waiting envelope, then the later edit"
    );
    let relay = relay_ids(&vault.relay);
    assert_eq!(
        relay[0], sealed[0].1,
        "the same op ID as sealed before the crash"
    );
    assert_eq!(relay.len(), 2);
    assert_eq!(unsent(&vault.a.lock()).len(), 2, "both wait for their echo");
    echo_a(&vault).await;
    assert!(unsent(&vault.a.lock()).is_empty());
}

#[tokio::test]
async fn a_push_whose_answer_was_lost_is_acknowledged_by_the_next_pull() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        institution(&a, "a1");
        institution(&a, "a2");
    }
    vault.relay.lose_next_push_answer();

    let lost = push_a(&vault).await;

    assert!(
        matches!(lost, Err(SyncError::Transient(Transient::Offline))),
        "{lost:?}"
    );
    let sealed = unsent(&vault.a.lock());
    assert_eq!(sealed.len(), 1, "kept: the answer never came");
    assert_eq!(
        relay_ids(&vault.relay),
        ids(&sealed),
        "but the relay stored it"
    );

    let pulled = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert!(pulled.done());
    assert_eq!(
        pulled.applied, 0,
        "an own envelope is acknowledged, not applied"
    );
    assert!(unsent(&vault.a.lock()).is_empty());
    let again = push_a(&vault).await.expect("push");
    assert_eq!(again.pushed, 0);
    assert_eq!(relay_ids(&vault.relay), ids(&sealed), "each op once");
}

/// Ruling P11: 300 small envelopes reach both limits.
#[tokio::test]
async fn push_batches_respect_the_relay_limits() {
    let vault = vault().await;
    let sealed = {
        let a = vault.a.lock();
        for i in 0..300 {
            institution(&a, &format!("i{i:03}"));
            seal_from(&a, &vault.creds);
        }
        // `seal_from` seals as `seal_batch` does, but leaves the barrier to the caller.
        state::note_sealed_at_barrier(&a, barrier(&a)).expect("barrier");
        unsent(&a)
    };
    assert_eq!(sealed.len(), 300);
    let one = {
        let a = vault.a.lock();
        let lengths: Vec<i64> = a
            .prepare("SELECT length(envelope) FROM sync_unsent")
            .expect("lengths")
            .query_map([], |r| r.get(0))
            .expect("lengths")
            .collect::<std::result::Result<_, _>>()
            .expect("lengths");
        assert!(lengths.iter().all(|l| *l == lengths[0]), "padded alike");
        (lengths[0] as usize).div_ceil(3) * 4
    };

    {
        let a = vault.a.lock();
        let by_count = next_push_batch_with(&a, None, 100, usize::MAX).expect("batch");
        assert_eq!(
            by_count.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            (1..=100).collect::<Vec<u64>>()
        );
        assert_eq!(
            by_count
                .iter()
                .map(|(_, op)| op.id.clone())
                .collect::<Vec<_>>(),
            ids(&sealed[..100])
        );
        let by_size = next_push_batch_with(&a, None, usize::MAX, 3 * one).expect("batch");
        assert_eq!(
            by_size.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            next_push_batch(&a, None).expect("batch").len(),
            PUSH_MAX_OPS
        );
        let above = next_push_batch_with(&a, Some(250), 100, usize::MAX).expect("batch");
        assert_eq!(
            above.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            (251..=300).collect::<Vec<u64>>(),
            "only what lies above the last one sent"
        );
    }

    let report = push_a(&vault).await.expect("push");

    assert_eq!(report.pushed, 300);
    assert_eq!(
        vault.relay.push_sizes(),
        vec![(100, 100 * one), (100, 100 * one), (100, 100 * one)]
    );
    assert_eq!(
        relay_ids(&vault.relay),
        ids(&sealed),
        "relay order = seq order"
    );
    echo_a(&vault).await;
    assert!(unsent(&vault.a.lock()).is_empty());
}

/// One institution written on `A` and sealed into `sync_unsent`; the rows waiting after that.
fn sealed_one(vault: &Vault) -> Vec<(i64, String)> {
    institution(&vault.a.lock(), "a1");
    assert_eq!(seal(vault, &vault.a).sealed, 1);
    unsent(&vault.a.lock())
}

#[tokio::test]
async fn payment_required_keeps_envelopes_and_stops_pushing() {
    let vault = vault().await;
    let sealed = sealed_one(&vault);
    vault.relay.set_paid(&vault.creds.account, false);

    let result = push_a(&vault).await;

    assert!(matches!(result, Err(SyncError::Lapsed)), "{result:?}");
    assert_eq!(unsent(&vault.a.lock()), sealed);
    assert!(vault.relay.ops().is_empty());
}

#[tokio::test]
async fn quota_and_rotated_tokens_stop_pushing() {
    let vault = vault().await;
    let sealed = sealed_one(&vault);

    vault.relay.set_op_quota(1);
    let quota = push_a(&vault).await;
    assert!(
        matches!(quota, Err(SyncError::TooLarge(TooLarge::Quota))),
        "{quota:?}"
    );
    assert_eq!(unsent(&vault.a.lock()), sealed);

    vault.relay.set_op_quota(u64::MAX);
    vault
        .relay
        .fail_next(Route::Push, RelayError::TokenReadOnly);
    let read_only = push_a(&vault).await;
    assert!(
        matches!(read_only, Err(SyncError::ReadOnlyToken)),
        "{read_only:?}"
    );
    assert_eq!(unsent(&vault.a.lock()), sealed);

    vault.relay.fail_next(Route::Push, RelayError::Unauthorized);
    let rotated = push_a(&vault).await;
    assert!(
        matches!(rotated, Err(SyncError::Unauthorized)),
        "{rotated:?}"
    );
    assert_eq!(unsent(&vault.a.lock()), sealed);

    // 413 body_too_large: the push body was over the relay's limit.
    vault
        .relay
        .fail_next(Route::Push, RelayError::BadRequest("body_too_large".into()));
    let too_large = push_a(&vault).await;
    assert!(
        matches!(
            too_large,
            Err(SyncError::Refused(RelayError::BadRequest(ref code))) if code == "body_too_large"
        ),
        "{too_large:?}"
    );
    assert_eq!(unsent(&vault.a.lock()), sealed);
    assert!(vault.relay.ops().is_empty());
}

#[tokio::test]
async fn a_vault_gone_answer_is_reported() {
    let vault = vault().await;
    let sealed = sealed_one(&vault);
    vault.relay.wipe_vault(&vault.creds.auth().vault_id);

    let result = push_a(&vault).await;

    assert!(
        matches!(result, Err(SyncError::VaultNotFound)),
        "{result:?}"
    );
    assert_eq!(unsent(&vault.a.lock()), sealed);
}

#[tokio::test]
async fn an_oversized_row_is_refused_and_the_loop_still_ends() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        exec(
            &a,
            "INSERT INTO watched_stocks (id, ticker) VALUES ('w1', 'AAPL')",
        );
        a.execute(
            "UPDATE watched_stocks SET notes = ?1 WHERE id = 'w1'",
            [noise(300 * 1024, 29)],
        )
        .expect("notes");
        institution(&a, "a1");
    }

    let report = push_a(&vault).await.expect("push");

    assert_eq!(
        report.refused, 1,
        "counted once, though every seal meets it"
    );
    assert_eq!(report.pushed, 1, "the institution's envelope");
    echo_a(&vault).await;
    let a = vault.a.lock();
    assert_eq!(
        count(
            &a,
            "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'watched_stocks' AND pk = 'w1'"
        ),
        1,
        "the refused row stays queued"
    );
    assert!(unsent(&a).is_empty());
    assert_eq!(vault.relay.ops().len(), 1);
}

/// A device that turned sync on or joined starts at its barrier: it has no barrier to mark, so an
/// idle first push seals nothing, although it has not met a marker (`barrier_seq` unknown).
#[tokio::test]
async fn a_fresh_device_with_nothing_queued_seals_no_marker() {
    let vault = vault().await;
    for (name, device) in [("A", &vault.a), ("B", &vault.b)] {
        let conn = device.lock();
        let sync = state::load(&conn).expect("state").expect("row");
        assert!(sync.barrier > 0, "{name}: the build has a barrier");
        assert_eq!(sync.barrier_seq, None, "{name}: no marker met");
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM sync_outbox"),
            0,
            "{name}"
        );
        assert_eq!(marker_barrier(&conn), sync.barrier, "{name}");
        drop(conn);

        let report = seal(&vault, device);

        assert_eq!((report.sealed, report.marker), (0, false), "{name}");
        assert!(unsent(&device.lock()).is_empty(), "{name}");
    }
}

/// The case the marker is for: a device that crossed a barrier after it started.
#[tokio::test]
async fn a_device_past_a_new_barrier_with_nothing_to_send_pushes_one_marker() {
    let vault = vault().await;
    {
        // A device that turned sync on at its barrier starts marked: nothing to send is nothing
        // pushed.
        {
            let a = vault.a.lock();
            assert_eq!(marker_barrier(&a), barrier(&a));
        }
        let idle = seal(&vault, &vault.a);
        assert_eq!((idle.sealed, idle.marker), (0, false));
        // Past a new barrier whose marker this device already met: no marker of its own.
        exec(
            &vault.a.lock(),
            "UPDATE sync_state SET barrier = barrier + 1, barrier_seq = 7",
        );
        let met = seal(&vault, &vault.a);
        assert_eq!((met.sealed, met.marker), (0, false));
        let a = vault.a.lock();
        assert!(unsent(&a).is_empty());
        exec(&a, "UPDATE sync_state SET barrier_seq = NULL");
    }

    let first = push_a(&vault).await.expect("push");

    assert_eq!(first.pushed, 1);
    let ops = vault.relay.ops();
    assert_eq!(ops.len(), 1);
    {
        let a = vault.a.lock();
        let raised = barrier(&a);
        assert_eq!(marker_barrier(&a), raised);
        let opened = crate::services::sync::envelope::open_op(
            |epoch| vault.creds.keys.get(epoch),
            &vault.creds.vault_id.0,
            &ops[0].ct,
        )
        .expect("open");
        let cs = crate::services::sync::changeset::from_json(&opened).expect("changeset");
        assert_eq!((cs.device, cs.seq, cs.barrier), (A, 1, raised));
        assert!(cs.rows.is_empty(), "a marker carries no rows");
        assert_eq!(
            state::load(&a).expect("state").expect("row").barrier_seq,
            None
        );
    }

    echo_a(&vault).await;
    let second = push_a(&vault).await.expect("push again");

    assert_eq!(second.pushed, 0, "at most one marker per barrier");
    assert_eq!(vault.relay.ops().len(), 1);
}

/// A relay that answers every push with more accepted ops than it was sent.
struct Inflated {
    inner: FakeRelay,
}

impl Relay for Inflated {
    async fn server_info(&self) -> RelayResult<crate::services::sync::relay::ServerInfo> {
        unreachable!("not a push call")
    }
    async fn create_account(&self) -> RelayResult<String> {
        unreachable!("not a push call")
    }
    async fn account_status(&self, _: &str) -> RelayResult<AccountStatus> {
        unreachable!("not a push call")
    }
    async fn checkout(&self, _: &str, _: CheckoutPlan) -> RelayResult<String> {
        unreachable!("not a push call")
    }
    async fn create_vault(&self, _: &str, _: &VaultAuth) -> RelayResult<bool> {
        unreachable!("not a push call")
    }
    async fn vault_info(&self, _: &VaultAuth) -> RelayResult<VaultInfo> {
        unreachable!("not a push call")
    }
    async fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        let receipt = self.inner.push(vault, ops).await?;
        Ok(PushReceipt {
            head: receipt.head,
            accepted: ops.len() as u32 + 1,
        })
    }
    async fn pull(&self, _: &VaultAuth, _: u64, _: u32) -> RelayResult<PullPage> {
        unreachable!("not a push call")
    }
    async fn put_snapshot(&self, _: &VaultAuth, _: u64, _: Vec<u8>) -> RelayResult<()> {
        unreachable!("not a push call")
    }
    async fn get_snapshot(&self, _: &VaultAuth) -> RelayResult<SnapshotBlob> {
        unreachable!("not a push call")
    }
    async fn delete_vault(&self, _: &VaultAuth) -> RelayResult<()> {
        unreachable!("not a push call")
    }
    async fn rotate_token(
        &self,
        _: &VaultAuth,
        _: &str,
        _: &[u8],
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        unreachable!("not a push call")
    }
    async fn rotate_token_with_kit(
        &self,
        _: &VaultAuth,
        _: &str,
        _: &[u8],
        _: crate::services::sync::relay::KitRotation<'_>,
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        unreachable!("not a push call")
    }
    async fn put_kit(&self, _: &VaultAuth, _: &str, _: &[u8]) -> RelayResult<bool> {
        unreachable!("not a push call")
    }
    async fn get_kit(
        &self,
        _: &str,
        _: &str,
    ) -> RelayResult<crate::services::sync::relay::KitRead> {
        unreachable!("not a push call")
    }
    async fn rekeys(
        &self,
        _: &VaultAuth,
        _: u32,
        _: u32,
    ) -> RelayResult<crate::services::sync::relay::RekeyPage> {
        unreachable!("not a push call")
    }
    async fn put_blob(
        &self,
        _: &VaultAuth,
        _: uuid::Uuid,
        _: u64,
        _: crate::services::sync::relay::BlobBody,
    ) -> RelayResult<()> {
        unreachable!("not a push call")
    }
    async fn head_blob(&self, _: &VaultAuth, _: uuid::Uuid) -> RelayResult<Option<u64>> {
        unreachable!("not a push call")
    }
    async fn get_blob(
        &self,
        _: &VaultAuth,
        _: uuid::Uuid,
        _: &mut (dyn crate::services::sync::relay::BlobSink + Send),
    ) -> RelayResult<crate::services::sync::relay::Delivery> {
        unreachable!("not a push call")
    }
    async fn list_blobs(
        &self,
        _: &VaultAuth,
        _: Option<uuid::Uuid>,
        _: u32,
    ) -> RelayResult<crate::services::sync::relay::BlobPage> {
        unreachable!("not a push call")
    }
    async fn delete_blob(&self, _: &VaultAuth, _: uuid::Uuid) -> RelayResult<()> {
        unreachable!("not a push call")
    }
}

/// Task 6 review obligation: a receipt claiming more ops than were sent is outside the relay
/// contract, and nothing is acknowledged on its word.
#[tokio::test]
async fn a_receipt_accepting_more_than_was_sent_acknowledges_nothing() {
    let vault = vault().await;
    let sealed = sealed_one(&vault);
    let relay = Inflated {
        inner: vault.relay.clone(),
    };

    let result = push_all(&relay, &vault.a, &vault.creds).await;

    assert!(
        matches!(result, Err(SyncError::Transient(Transient::Relay))),
        "{result:?}"
    );
    assert_eq!(unsent(&vault.a.lock()), sealed);
}

#[tokio::test]
async fn an_echo_removes_only_its_own_envelope() {
    let vault = vault().await;
    let a = vault.a.lock();
    for id in ["a1", "a2", "a3"] {
        institution(&a, id);
        seal_from(&a, &vault.creds);
    }
    let before = unsent(&a);

    acknowledge_own(&a, 1).expect("echo 1");
    acknowledge_own(&a, 3).expect("echo 3");

    assert_eq!(unsent(&a), vec![before[1].clone()]);
}

/// A large drain releases the profile between holds of at most `SEAL_PER_HOLD` envelopes, each
/// hold committed on its own, and stops at `SEAL_BATCH` envelopes per batch.
#[tokio::test]
async fn a_large_drain_takes_several_lock_holds() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        // 70 rows of 40 KiB incompressible notes: one row per 60 KiB envelope.
        for i in 0..70u64 {
            a.execute(
                "INSERT INTO institutions (id, name) VALUES (?1, ?2)",
                params![format!("i{i:02}"), noise(40 * 1024, i)],
            )
            .expect("institution");
        }
    }
    let db = Holds::new(vault.a.clone());

    let report = seal_batch(&db, &vault.creds, None).expect("seal");

    assert_eq!((SEAL_BATCH, SEAL_PER_HOLD), (64, 1));
    assert_eq!(report.sealed, 64);
    assert_eq!(
        db.sealed_per_hold(),
        vec![1; 64],
        "64 envelopes in 64 holds of one"
    );
    let a = vault.a.lock();
    assert_eq!(count(&a, "SELECT COUNT(*) FROM sync_outbox"), 6);
    assert_eq!(
        unsent(&a).iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
        (1..=SEAL_BATCH as i64).collect::<Vec<_>>()
    );
    drop(a);

    // The rest waits for the next batch, which starts only once these were sent (`after` past
    // them) or came back.
    let early = seal_batch(&db, &vault.creds, Some(SEAL_BATCH as u64 - 1)).expect("seal");
    assert_eq!(early.sealed, 0, "envelope 64 was not sent yet");
    for seq in 1..=SEAL_BATCH as u64 {
        acknowledge_own(&vault.a.lock(), seq).expect("echo");
    }
    let before = db.holds().len();
    let rest = seal_batch(&db, &vault.creds, None).expect("seal the rest");
    assert_eq!(rest.sealed, 6);
    let waiting: Vec<i64> = db.holds()[before..].iter().map(|(_, w)| *w).collect();
    assert_eq!(
        waiting,
        vec![1, 2, 3, 4, 5, 6, 6],
        "one envelope per hold, then a hold that finds the outbox spent"
    );
}

/// P1 allows a push after a pull that did not acknowledge anything (`NewerSchema`, `Held`): the
/// lost push is simply sent again, and the relay keeps each op once.
#[tokio::test]
async fn a_lost_answer_pushed_again_without_a_pull_stores_each_op_once() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        institution(&a, "a1");
        institution(&a, "a2");
    }
    vault.relay.lose_next_push_answer();
    assert!(matches!(
        push_a(&vault).await,
        Err(SyncError::Transient(Transient::Offline))
    ));
    let sealed = unsent(&vault.a.lock());
    assert_eq!(sealed.len(), 1);

    let again = push_a(&vault).await.expect("push again");

    assert_eq!(again.pushed, 1, "the same envelope, acknowledged this time");
    assert_eq!(relay_ids(&vault.relay), ids(&sealed), "each op once");
    assert_eq!(unsent(&vault.a.lock()), sealed, "it waits for its echo");
    echo_a(&vault).await;
    assert!(unsent(&vault.a.lock()).is_empty());
}

/// Row keys never reach a `{:?}` log: a refused row shows its table and size only.
#[test]
fn a_seal_report_debug_leaves_row_keys_out() {
    let report = SealReport {
        sealed: 2,
        marker: false,
        unechoed_cap: false,
        refused: vec![Refused {
            table: "watched_stocks".into(),
            pk: "secret-key-w1".into(),
            bytes: 300_000,
        }],
    };

    let shown = format!("{report:?}");

    assert!(!shown.contains("secret-key-w1"), "{shown}");
    assert!(
        shown.contains("watched_stocks") && shown.contains("300000"),
        "{shown}"
    );
}

#[test]
#[ignore = "a measurement: run in release with --ignored --nocapture"]
fn draining_a_50000_row_outbox() {
    let conn = migrated();
    state::enable(&conn, A).expect("enable");
    let creds = Credentials {
        relay_url: "https://relay.test".into(),
        account: "4000000000000001".into(),
        vault_id: VaultId::random().expect("vault id"),
        keys: VaultKeys::new_random().expect("vault keys"),
    };
    {
        let tx = conn.unchecked_transaction().expect("transaction");
        for i in 0..50_000 {
            tx.execute(
                "INSERT INTO institutions (id, name) VALUES (?1, ?2)",
                params![format!("inst-{i:05}"), format!("Bank {i}")],
            )
            .expect("institution");
        }
        tx.commit().expect("commit");
    }

    let db = Holds::new(TestConn::new(conn));
    let started = Instant::now();
    let mut envelopes = 0;
    loop {
        let report = seal_batch(&db, &creds, None).expect("seal");
        if report.sealed == 0 {
            break;
        }
        envelopes += report.sealed;
        // As if pushed and pulled back.
        exec(&db.inner.lock(), "DELETE FROM sync_unsent");
    }
    let total = started.elapsed();
    let holds = db.holds();
    let longest = holds
        .iter()
        .map(|(took, _)| *took)
        .max()
        .unwrap_or_default();
    println!(
        "draining 50,000 queued rows: {envelopes} envelopes in {:.2} s, {} lock holds, \
         longest hold {:.3} s",
        total.as_secs_f64(),
        holds.len(),
        longest.as_secs_f64()
    );
}
