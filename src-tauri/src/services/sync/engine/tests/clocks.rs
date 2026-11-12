//! Audit M6: a changeset whose clock is far in the future waits (`PullReport::ahead`) instead of
//! being applied, so one device with a wrong or malicious clock cannot win every later conflict
//! and cannot jump this device's clock past it. It applies once this device's time (or its newest
//! stored clock) comes within `MAX_CLOCK_AHEAD_MS` of it; meanwhile this device keeps pulling,
//! pushing and showing `ClockAhead` (phase `waiting`), and nothing behind it is offered as a
//! missing change.

use super::*;
use crate::models::sync::{SyncPhase, SyncProblem};
use crate::services::sync::capture;
use crate::services::sync::chain::gaps::held_gaps;
use crate::services::sync::chain::{too_far_ahead, MAX_CLOCK_AHEAD_MS};
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{problem_of_pull, status, Runtime};
use crate::services::sync::engine::{run_round, PullEnd, PullReport, RoundMode, RoundReport};
use crate::services::sync::envelope::envelope_hash;
use crate::services::sync::hlc::{fake_clock, now_ms, pack, unpack, Clock};

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;

async fn round(vault: &Vault, device: &TestConn) -> RoundReport {
    run_round(
        &vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round")
}

async fn pull(vault: &Vault, device: &TestConn) -> PullReport {
    pull_all(&vault.relay, device, &vault.creds, NOW)
        .await
        .expect("pull")
}

fn name_of(conn: &Connection, id: &str) -> String {
    conn.query_row("SELECT name FROM institutions WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .expect("institution name")
}

fn rename(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "UPDATE institutions SET name = ?2 WHERE id = ?1",
        params![id, name],
    )
    .expect("rename");
}

/// `C`'s envelope `seq` renaming institution `id` at clock `hlc`, sealed under the vault's key.
fn renamed_by_c(
    vault: &Vault,
    seq: u64,
    prev: Option<[u8; 32]>,
    id: &str,
    name: &str,
    hlc: u64,
) -> Vec<u8> {
    let at = Clock { hlc, device: C };
    let cs = changeset(
        C,
        seq,
        prev,
        vec![row("institutions", id, at, &[("name", text(name))])],
    );
    forge(&vault.creds, &cs)
}

/// `A` created institution `i1` and `B` has it.
async fn shared_institution(vault: &Vault) {
    let sealed = {
        let a = vault.a.lock();
        institution(&a, "i1");
        seal_from(&a, &vault.creds)
    };
    push(&vault.relay, &vault.creds, &[sealed]).await;
    assert_eq!(pull(vault, &vault.b).await.applied, 1);
    assert_eq!(pull(vault, &vault.a).await.end(), PullEnd::Done, "A's echo");
}

#[tokio::test]
async fn a_clock_far_in_the_future_waits_instead_of_pinning() {
    let vault = vault().await;
    shared_institution(&vault).await;
    let before = cursor(&vault.a.lock());
    let pinned = renamed_by_c(&vault, 1, None, "i1", "Pinned", i64::MAX as u64);
    push(&vault.relay, &vault.creds, std::slice::from_ref(&pinned)).await;

    let first = round(&vault, &vault.a).await;

    let report = first.pull.as_ref().expect("pull");
    assert_eq!(
        (report.ahead, report.applied, report.gaps, report.held),
        (1, 0, 0, 0)
    );
    assert_eq!(report.end(), PullEnd::Ahead);
    assert!(report.may_push() && !report.may_snapshot());
    assert_eq!(first.problem, Some(SyncProblem::ClockAhead));
    {
        let a = vault.a.lock();
        assert_eq!(
            name_of(&a, "i1"),
            "Bank i1",
            "nothing of the far-future change applied"
        );
        assert_eq!(
            cursor(&a),
            before,
            "the cursor stays before the waiting envelope"
        );
        assert_eq!(head(&a, C), None);
        assert!(
            unpack(capture::tick(&a).expect("tick")).0 <= now_ms() + HOUR_MS,
            "this device's clock did not jump"
        );
        let mut runtime = Runtime::default();
        runtime.after_round(&first);
        let shown = status(&a, &runtime).expect("status");
        assert_eq!(
            (shown.phase, shown.problem),
            (SyncPhase::Waiting, Some(SyncProblem::ClockAhead)),
            "a change waiting for its time is no stop"
        );
        assert!(
            state::marks(&a)
                .expect("marks")
                .expect("row")
                .last_success_at
                .is_some(),
            "the round synced everything it could"
        );
    }

    // C's next change carries an ordinary clock: it waits behind the far-future one, as part of
    // the same wait (never a missing change the person is offered to skip).
    let later = renamed_by_c(
        &vault,
        2,
        Some(envelope_hash(&pinned)),
        "i1",
        "Second",
        pack(now_ms(), 0),
    );
    push(&vault.relay, &vault.creds, &[later]).await;
    let second = pull(&vault, &vault.a).await;
    assert_eq!((second.ahead, second.gaps, second.applied), (2, 0, 0));
    assert!(held_gaps(&vault.a.lock()).expect("gaps").is_empty());

    // A's own later rename is not beaten by the far-future clock: it reaches B.
    let renamed = {
        let a = vault.a.lock();
        rename(&a, "i1", "Renamed");
        seal_from(&a, &vault.creds)
    };
    push(&vault.relay, &vault.creds, &[renamed]).await;
    let on_b = pull(&vault, &vault.b).await;
    assert_eq!((on_b.applied, on_b.ahead), (1, 2));
    assert_eq!(name_of(&vault.b.lock(), "i1"), "Renamed");
    assert_eq!(name_of(&vault.a.lock(), "i1"), "Renamed");
}

#[tokio::test]
async fn a_clock_a_little_ahead_applies() {
    let vault = vault().await;
    shared_institution(&vault).await;
    let soon = renamed_by_c(&vault, 1, None, "i1", "Soon", pack(now_ms() + HOUR_MS, 0));
    push(&vault.relay, &vault.creds, &[soon]).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!((report.applied, report.ahead), (1, 0));
    assert_eq!(report.end(), PullEnd::Done);
    assert_eq!(name_of(&vault.a.lock(), "i1"), "Soon");
}

#[tokio::test]
async fn the_wait_ends_when_the_time_comes_and_the_clock_moves_at_most_a_day_ahead() {
    let vault = vault().await;
    shared_institution(&vault).await;
    let start = now_ms();
    fake_clock::start(start);
    let ahead = renamed_by_c(
        &vault,
        1,
        None,
        "i1",
        "Three days",
        pack(start + 3 * DAY_MS, 0),
    );
    push(&vault.relay, &vault.creds, &[ahead]).await;

    let waiting = pull(&vault, &vault.a).await;
    assert_eq!((waiting.ahead, waiting.applied), (1, 0));

    // Two days and a minute later the change is less than a day ahead: it applies.
    fake_clock::advance(2 * DAY_MS + 60_000);
    let applied = pull(&vault, &vault.a).await;
    let a = vault.a.lock();
    let ticked = capture::tick(&a).expect("tick");
    let today = now_ms();
    fake_clock::stop();

    assert_eq!((applied.ahead, applied.applied), (0, 1));
    assert_eq!(applied.end(), PullEnd::Done);
    assert_eq!(name_of(&a, "i1"), "Three days");
    assert!(
        unpack(ticked).0 <= today + MAX_CLOCK_AHEAD_MS,
        "observing the change moved this device's clock at most a day past its time"
    );
}

#[tokio::test]
async fn a_change_behind_a_far_future_one_is_never_offered_as_missing_whatever_the_order() {
    let vault = vault().await;
    shared_institution(&vault).await;
    let pinned = renamed_by_c(&vault, 1, None, "i1", "Pinned", i64::MAX as u64);
    let later = renamed_by_c(
        &vault,
        2,
        Some(envelope_hash(&pinned)),
        "i1",
        "Second",
        pack(now_ms(), 0),
    );
    // The relay serves C's second change before its first.
    push(&vault.relay, &vault.creds, &[later, pinned]).await;

    for _ in 0..2 {
        let report = pull(&vault, &vault.a).await;
        assert_eq!((report.ahead, report.gaps), (2, 0), "{report:?}");
        assert_eq!(report.end(), PullEnd::Ahead);
        assert!(held_gaps(&vault.a.lock()).expect("gaps").is_empty());
    }
}

#[tokio::test]
async fn a_missing_change_of_another_device_outranks_a_far_future_one() {
    let vault = vault().await;
    shared_institution(&vault).await;
    let pinned = renamed_by_c(&vault, 1, None, "i1", "Pinned", i64::MAX as u64);
    let b1 = super::runtime::write_and_seal(&vault, &vault.b, "b1");
    let b2 = super::runtime::write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[pinned, b1, b2]).await;
    // Relay sequences: 1 is A's institution, 2 C's far-future change, 3 and 4 B's.
    vault.relay.drop_op(3);

    let report = round(&vault, &vault.a).await;

    let pull = report.pull.as_ref().expect("pull");
    assert_eq!((pull.ahead, pull.gaps), (1, 1));
    assert_eq!(pull.end(), PullEnd::Waiting);
    assert_eq!(report.problem, Some(SyncProblem::Gap));
    assert_eq!(
        held_gaps(&vault.a.lock()).expect("gaps").len(),
        1,
        "B's gap only"
    );
}

#[test]
fn a_pull_ends_held_then_waiting_then_ahead() {
    let report = |held: usize, gaps: usize, ahead: usize| PullReport {
        held,
        gaps,
        ahead,
        ..PullReport::default()
    };
    assert_eq!(report(1, 1, 1).end(), PullEnd::Held);
    assert_eq!(report(0, 1, 1).end(), PullEnd::Waiting);
    assert_eq!(report(0, 0, 1).end(), PullEnd::Ahead);
    assert_eq!(
        problem_of_pull(&report(0, 0, 1)),
        Some(SyncProblem::ClockAhead)
    );
    let unfinished = PullReport {
        ahead: 1,
        unfinished: true,
        ..PullReport::default()
    };
    assert_eq!(unfinished.end(), PullEnd::Unfinished);
    assert!(!unfinished.may_push());
    assert_eq!(problem_of_pull(&unfinished), Some(SyncProblem::ClockAhead));
}

#[tokio::test]
async fn a_clock_is_too_far_ahead_only_past_both_this_devices_time_and_its_newest_clock() {
    let vault = vault().await;
    let a = vault.a.lock();
    let now = now_ms();
    let at = |ms: u64| pack(ms, 0);
    assert!(!too_far_ahead(&a, at(now + MAX_CLOCK_AHEAD_MS), now).expect("check"));
    assert!(too_far_ahead(&a, at(now + MAX_CLOCK_AHEAD_MS + 1), now).expect("check"));
    assert!(too_far_ahead(&a, i64::MAX as u64, now).expect("check"));
    assert!(!too_far_ahead(&a, 0, now).expect("check"));

    // This device already holds a clock two days ahead (it applied one, or its own time was
    // ahead): a change within a day of that clock moves nothing further and applies.
    exec(
        &a,
        &format!(
            "INSERT INTO sync_clock (tbl, pk, col, hlc, device) \
             VALUES ('institutions', 'x', 'name', {}, X'{}')",
            at(now + 2 * DAY_MS),
            B.to_hex()
        ),
    );
    assert!(!too_far_ahead(&a, at(now + 2 * DAY_MS + 23 * HOUR_MS), now).expect("check"));
    assert!(too_far_ahead(&a, at(now + 3 * DAY_MS + 1), now).expect("check"));
}

#[tokio::test]
async fn a_sync_server_that_went_back_outranks_a_change_waiting_for_its_time() {
    let vault = vault().await;
    let a = vault.a.lock();
    state::note_vault_went_back(&a, NOW).expect("went back");
    let runtime = Runtime {
        problem: Some(SyncProblem::ClockAhead),
        ..Runtime::default()
    };

    let shown = status(&a, &runtime).expect("status");

    assert_eq!(
        (shown.phase, shown.problem),
        (SyncPhase::Error, Some(SyncProblem::ServerWentBack))
    );
}
