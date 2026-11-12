//! Back-off and bounds (Task 18, audit L2, L3, L5): a relay that demands a snapshot restore every
//! round gets one per `RESTORE_BACKOFF_SECS`, endless unfinished pulls let the device push after
//! `UNFINISHED_BEFORE_PUSH` rounds, and a `paidUntil` near the maximum has no deletion date.

use super::pull::write_and_seal;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::engine::snapshots::{
    SnapshotPolicy, RESTORE_BACKOFF_SECS, UNFINISHED_BEFORE_PUSH,
};
use crate::services::sync::engine::status::{status, Runtime};
use crate::services::sync::engine::{run_round, PullEnd, RoundMode};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, CheckoutPlan, PullPage, PulledOp, PushReceipt, RelayError, RelayResult,
    SnapshotBlob, VaultInfo,
};

async fn round_at(
    relay: &impl Relay,
    device: &TestConn,
    policy: &mut SnapshotPolicy,
    now: i64,
) -> crate::services::sync::engine::RoundReport {
    run_round(relay, device, policy, RoundMode::Normal, now, 0)
        .await
        .expect("round")
}

fn gets(relay: &FakeRelay) -> usize {
    relay
        .calls()
        .iter()
        .filter(|r| **r == Route::GetSnapshot)
        .count()
}

/// L2: a relay that answers `410` to every pull gets one restore, then none until the back-off
/// has passed.
#[tokio::test]
async fn a_relay_that_answers_410_every_round_gets_one_restore_per_six_hours() {
    let vault = vault().await;
    // A snapshot for the restores to fetch.
    let mut policy_a = SnapshotPolicy::default();
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    push(&vault.relay, &vault.creds, &[a1]).await;
    run_round(
        &vault.relay,
        &vault.a,
        &mut policy_a,
        RoundMode::FirstSnapshot,
        NOW,
        0,
    )
    .await
    .expect("A uploads");
    assert!(vault.relay.snapshot_seq().is_some());
    for _ in 0..12 {
        vault
            .relay
            .fail_next(Route::Pull, RelayError::SnapshotRequired);
    }

    let mut policy = SnapshotPolicy::default();
    let first = round_at(&vault.relay, &vault.b, &mut policy, NOW).await;
    assert_eq!(first.problem, Some(SyncProblem::RelayError));
    assert_eq!(gets(&vault.relay), 1, "one restore in the first round");

    let second = round_at(&vault.relay, &vault.b, &mut policy, NOW + 60).await;
    assert_eq!(second.problem, Some(SyncProblem::RelayError));
    assert!(!second.restored);
    assert_eq!(gets(&vault.relay), 1, "none inside the back-off");

    let later = round_at(
        &vault.relay,
        &vault.b,
        &mut policy,
        NOW + RESTORE_BACKOFF_SECS,
    )
    .await;
    assert_eq!(later.problem, Some(SyncProblem::RelayError));
    assert_eq!(gets(&vault.relay), 2, "another once the back-off is over");
}

/// A relay that serves one more replayed envelope on every page and a head that never ends the
/// pull, so every pull ends at the page limit; everything else goes to the fake relay.
struct Endless {
    inner: FakeRelay,
    ct: Vec<u8>,
}

impl Relay for Endless {
    async fn create_account(&self) -> RelayResult<String> {
        self.inner.create_account().await
    }
    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        self.inner.account_status(account).await
    }
    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        self.inner.checkout(account, plan).await
    }
    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        self.inner.create_vault(account, vault).await
    }
    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        self.inner.vault_info(vault).await
    }
    async fn push(
        &self,
        vault: &VaultAuth,
        ops: &[crate::services::sync::relay::OutgoingOp],
    ) -> RelayResult<PushReceipt> {
        self.inner.push(vault, ops).await
    }
    async fn pull(&self, _: &VaultAuth, after: u64, _: u32) -> RelayResult<PullPage> {
        Ok(PullPage {
            ops: vec![PulledOp {
                seq: after + 1,
                id: op_id(&self.ct),
                ct: self.ct.clone(),
            }],
            head: u64::MAX,
            more: true,
        })
    }
    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        self.inner.put_snapshot(vault, seq, bytes).await
    }
    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        self.inner.get_snapshot(vault).await
    }
    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        self.inner.delete_vault(vault).await
    }
}

/// L3: two unfinished pulls hold the push back; the third lets it through and the round reports a
/// relay error instead of recording success.
#[tokio::test]
async fn three_unfinished_pulls_in_a_row_let_the_device_push() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let relay = Endless {
        inner: vault.relay.clone(),
        ct: b1,
    };
    institution(&vault.a.lock(), "a1");
    let mut policy = SnapshotPolicy::default();

    for n in 1..UNFINISHED_BEFORE_PUSH {
        let report = round_at(&relay, &vault.a, &mut policy, NOW + i64::from(n)).await;
        assert_eq!(
            report.pull.as_ref().map(|p| p.end()),
            Some(PullEnd::Unfinished)
        );
        assert!(report.push.is_none(), "round {n} does not push");
        assert!(vault.relay.ops().is_empty());
    }
    let last = round_at(&relay, &vault.a, &mut policy, NOW + 10).await;

    assert!(last.push.is_some_and(|p| p.pushed > 0));
    assert_eq!(vault.relay.ops().len(), 1, "the change reached the relay");
    assert_eq!(last.problem, Some(SyncProblem::RelayError));
    assert_eq!(
        state::marks(&vault.a.lock())
            .expect("marks")
            .expect("row")
            .last_error
            .as_deref(),
        Some("sync stopped: relayError")
    );
}

/// The run restarts after any pull that is not unfinished.
#[tokio::test]
async fn a_pull_that_ends_done_resets_the_unfinished_run() {
    let mut policy = SnapshotPolicy::default();
    let unfinished = crate::services::sync::engine::PullReport {
        unfinished: true,
        ..Default::default()
    };
    let done = crate::services::sync::engine::PullReport::default();
    assert_eq!(policy.note_pull(&unfinished), 1);
    assert_eq!(policy.note_pull(&unfinished), 2);
    assert_eq!(policy.note_pull(&done), 0);
    assert_eq!(policy.note_pull(&unfinished), 1);
}

/// L5: a `paidUntil` near the maximum must not overflow the deletion date.
#[tokio::test]
async fn a_paid_until_near_the_maximum_has_no_deletion_date() {
    let vault = vault().await;
    let runtime = Runtime {
        problem: Some(SyncProblem::Lapsed),
        paid_until: Some(i64::MAX - 1),
        ..Runtime::default()
    };
    let shown = status(&vault.a.lock(), &runtime).expect("status");
    assert_eq!(shown.problem, Some(SyncProblem::Lapsed));
    assert_eq!(shown.deletes_at, None);

    // Beyond year 9999 nothing is shown: no deletion date, no paid-until.
    let hostile = Runtime {
        paid_until: Some(3_000_000_000_000_000),
        ..runtime.clone()
    };
    let shown = status(&vault.a.lock(), &hostile).expect("status");
    assert_eq!((shown.deletes_at, shown.paid_until), (None, None));
    // A self-hosted server's year 9000 stays.
    let self_hosted = Runtime {
        paid_until: Some(221_845_392_000),
        ..runtime.clone()
    };
    let shown = status(&vault.a.lock(), &self_hosted).expect("status");
    assert_eq!(shown.paid_until, Some(221_845_392_000));

    let ordinary = Runtime {
        paid_until: Some(1_000),
        ..runtime
    };
    let shown = status(&vault.a.lock(), &ordinary).expect("status");
    assert_eq!(shown.deletes_at, Some(1_000 + 90 * 86_400));
}

/// Endless pulls whose first envelope waits for a missing one: C's second envelope, replayed.
fn endless_behind_a_gap(vault: &Vault) -> Endless {
    Endless {
        inner: vault.relay.clone(),
        ct: forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![])),
    }
}

fn last_error_of(device: &TestConn) -> Option<String> {
    state::marks(&device.lock())
        .expect("marks")
        .expect("row")
        .last_error
}

/// Task 17 ruling: a run of unfinished pulls behind a held gap still pushes after
/// `UNFINISHED_BEFORE_PUSH`, but the problem stays `Gap` (the skip is offered for it only), and
/// only the rounds that pushed count for "Last synced".
#[tokio::test]
async fn endless_pulls_behind_a_held_gap_keep_the_gap_and_count_only_when_they_push() {
    let vault = vault().await;
    let relay = endless_behind_a_gap(&vault);
    institution(&vault.a.lock(), "a1");
    let mut policy = SnapshotPolicy::default();

    for n in 1..UNFINISHED_BEFORE_PUSH {
        let report = round_at(&relay, &vault.a, &mut policy, NOW + i64::from(n)).await;
        assert_eq!(report.problem, Some(SyncProblem::Gap), "round {n}");
        assert!(report.push.is_none());
        assert_eq!(
            last_error_of(&vault.a).as_deref(),
            Some("sync waiting: gap"),
            "an unfinished round that pushed nothing is no success"
        );
        assert_eq!(
            state::marks(&vault.a.lock())
                .unwrap()
                .unwrap()
                .last_success_at,
            None
        );
    }
    let last = round_at(&relay, &vault.a, &mut policy, NOW + 10).await;

    assert!(last.push.is_some_and(|p| p.pushed > 0));
    assert_eq!(vault.relay.ops().len(), 1);
    assert_eq!(last.problem, Some(SyncProblem::Gap), "not relayError");
    assert_eq!(
        state::marks(&vault.a.lock())
            .unwrap()
            .unwrap()
            .last_success_at,
        Some(NOW + 10),
        "the round that pushed counts"
    );
}

/// Review fix 1: a restore that failed on the network does not use up the back-off window.
#[tokio::test]
async fn a_transient_restore_failure_does_not_consume_the_back_off() {
    let vault = vault().await;
    let mut policy_a = SnapshotPolicy::default();
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    push(&vault.relay, &vault.creds, &[a1]).await;
    run_round(
        &vault.relay,
        &vault.a,
        &mut policy_a,
        RoundMode::FirstSnapshot,
        NOW,
        0,
    )
    .await
    .expect("A uploads");
    vault
        .relay
        .fail_next(Route::Pull, RelayError::SnapshotRequired);
    vault
        .relay
        .fail_next(Route::GetSnapshot, RelayError::Network("down".into()));
    let mut policy = SnapshotPolicy::default();

    let first = run_round(
        &vault.relay,
        &vault.b,
        &mut policy,
        RoundMode::Normal,
        NOW,
        0,
    )
    .await;
    assert!(first.is_err(), "the round fails on the transient error");
    assert!(policy.restore_allowed(NOW + 60));

    vault
        .relay
        .fail_next(Route::Pull, RelayError::SnapshotRequired);
    let second = round_at(&vault.relay, &vault.b, &mut policy, NOW + 60).await;
    assert!(second.restored, "the next round restores");
    assert_eq!(gets(&vault.relay), 2);
    assert!(!policy.restore_allowed(NOW + 120));
}

/// Review fix 2: a sync server that went back outranks the relay errors of the endless pull.
#[tokio::test]
async fn server_went_back_outranks_the_endless_pull_relay_error() {
    let vault = vault().await;
    let relay = Endless {
        inner: vault.relay.clone(),
        ct: write_and_seal(&vault, &vault.b, "b1"),
    };
    state::note_vault_went_back(&vault.a.lock(), NOW).expect("mark");
    let mut policy = SnapshotPolicy::default();
    let mut last = None;
    for n in 0..UNFINISHED_BEFORE_PUSH {
        last = Some(round_at(&relay, &vault.a, &mut policy, NOW + i64::from(n)).await);
    }
    assert_eq!(
        last.expect("round").problem,
        Some(SyncProblem::ServerWentBack)
    );
}

/// Review fix 5: a quarantine behind an endless pull stays `Quarantined` and still pushes after
/// `UNFINISHED_BEFORE_PUSH`.
#[tokio::test]
async fn a_quarantine_behind_an_endless_pull_keeps_its_problem_and_pushes() {
    let vault = vault().await;
    // C forks: two different envelopes at seq 1 make C quarantined; replay the fork's second.
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    push(&vault.relay, &vault.creds, &[c1, c2.clone()]).await;
    // A meets the fork first, so C is quarantined; the endless relay then replays C's second.
    let first =
        crate::services::sync::engine::pull::pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
            .await
            .expect("pull");
    assert!(first.held > 0, "{first:?}");
    let relay = Endless {
        inner: vault.relay.clone(),
        ct: c2,
    };
    institution(&vault.a.lock(), "a1");
    let mut policy = SnapshotPolicy::default();
    for n in 1..UNFINISHED_BEFORE_PUSH {
        let report = round_at(&relay, &vault.a, &mut policy, NOW + i64::from(n)).await;
        assert!(report.push.is_none());
        assert_eq!(report.problem, Some(SyncProblem::Quarantined), "round {n}");
    }
    let last = round_at(&relay, &vault.a, &mut policy, NOW + 10).await;
    assert!(last.push.is_some_and(|p| p.pushed > 0));
    assert_eq!(last.problem, Some(SyncProblem::Quarantined));
}
