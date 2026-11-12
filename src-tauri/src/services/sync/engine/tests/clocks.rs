//! Audit M6: a changeset whose newest clock is more than ten years past this device's wall clock
//! is held (`PullReport::ahead`, a held gap of kind `Ahead`) instead of being applied, so a forged
//! clock cannot win every later conflict nor jump this device's clock. A clock up to ten years
//! ahead applies as before. Meanwhile this device keeps pulling, pushing and showing `ClockAhead`
//! (phase `waiting`); the author's later envelopes are gaps behind the held one, and skipping it
//! after 14 days releases them.

use super::*;
use crate::models::sync::{HeldGapKind, SyncPhase, SyncProblem};
use crate::services::sync::capture;
use crate::services::sync::chain::gaps::{held_gap_views, held_gaps, skip_missing_change, GapKind};
use crate::services::sync::chain::{too_far_ahead, TEN_YEARS_AHEAD_MS};
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{problem_of_pull, status, Runtime};
use crate::services::sync::engine::{run_round, PullEnd, PullReport, RoundMode, RoundReport};
use crate::services::sync::envelope::envelope_hash;
use crate::services::sync::hlc::{fake_clock, now_ms, pack, unpack, Clock};

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;
const DAY: i64 = 86_400;
/// The physical part of `i64::MAX` as a clock: 2^47 - 1 ms, in year 6429.
const MAX_CLOCK_SECS: i64 = 140_737_488_355;

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
async fn a_clock_more_than_ten_years_ahead_is_held_instead_of_pinning() {
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
        assert_eq!(name_of(&a, "i1"), "Bank i1", "nothing of it applied");
        assert_eq!(
            cursor(&a),
            before,
            "the cursor stays before the held envelope"
        );
        assert_eq!(head(&a, C), None);
        assert!(
            unpack(capture::tick(&a).expect("tick")).0 <= now_ms() + HOUR_MS,
            "this device's clock did not jump"
        );
        let views = held_gap_views(&a).expect("views");
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].kind, HeldGapKind::Ahead);
        assert_eq!(views[0].device_id, C.to_hex());
        assert_eq!(views[0].missing_changes, 1);
        assert_eq!(views[0].dated_at, Some(MAX_CLOCK_SECS));
        assert_eq!(
            (views[0].since, views[0].skippable_at),
            (NOW, NOW + 14 * DAY)
        );
        let mut runtime = Runtime::default();
        runtime.after_round(&first);
        let shown = status(&a, &runtime).expect("status");
        assert_eq!(
            (shown.phase, shown.problem),
            (SyncPhase::Waiting, Some(SyncProblem::ClockAhead)),
            "a held far-future change is no stop"
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

    // C's next change carries an ordinary clock: an ordinary gap behind the held one, under C's
    // one record (the lower, held one), and not a missing change.
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
    assert_eq!((second.ahead, second.gaps, second.applied), (1, 1, 0));
    assert_eq!(second.missing_gaps(), 0);
    assert_eq!(second.end(), PullEnd::Ahead);
    let gaps = held_gaps(&vault.a.lock()).expect("gaps");
    assert_eq!(gaps.len(), 1);
    assert_eq!((gaps[0].kind, gaps[0].held_seq), (GapKind::Ahead, 1));

    // A's own later rename is not beaten by the far-future clock: it reaches B.
    let renamed = {
        let a = vault.a.lock();
        rename(&a, "i1", "Renamed");
        seal_from(&a, &vault.creds)
    };
    push(&vault.relay, &vault.creds, &[renamed]).await;
    let on_b = pull(&vault, &vault.b).await;
    assert_eq!((on_b.applied, on_b.ahead, on_b.gaps), (1, 1, 1));
    assert_eq!(name_of(&vault.b.lock(), "i1"), "Renamed");
    assert_eq!(name_of(&vault.a.lock(), "i1"), "Renamed");
}

#[tokio::test]
async fn a_clock_weeks_ahead_applies_as_before() {
    let vault = vault().await;
    shared_institution(&vault).await;
    let ahead = pack(now_ms() + 30 * DAY_MS, 0);
    let soon = renamed_by_c(&vault, 1, None, "i1", "Soon", ahead);
    push(&vault.relay, &vault.creds, &[soon]).await;

    let report = pull(&vault, &vault.a).await;

    assert_eq!((report.applied, report.ahead), (1, 0));
    assert_eq!(report.end(), PullEnd::Done);
    let a = vault.a.lock();
    assert_eq!(name_of(&a, "i1"), "Soon");
    assert!(
        capture::tick(&a).expect("tick") > ahead,
        "observed, as before"
    );
    assert!(held_gaps(&a).expect("gaps").is_empty());
}

#[tokio::test]
async fn the_hold_ends_once_the_date_is_within_ten_years_and_the_clock_moves_at_most_that_far() {
    let vault = vault().await;
    shared_institution(&vault).await;
    let start = now_ms();
    fake_clock::start(start);
    let dated = pack(start + TEN_YEARS_AHEAD_MS + 365 * DAY_MS, 0);
    let ahead = renamed_by_c(&vault, 1, None, "i1", "Later", dated);
    push(&vault.relay, &vault.creds, &[ahead]).await;

    let held = pull(&vault, &vault.a).await;
    assert_eq!((held.ahead, held.applied), (1, 0));

    // A year and a day later the change is less than ten years ahead: it applies.
    fake_clock::advance(366 * DAY_MS);
    let applied = pull(&vault, &vault.a).await;
    let a = vault.a.lock();
    let ticked = capture::tick(&a).expect("tick");
    let today = now_ms();
    fake_clock::stop();

    assert_eq!((applied.ahead, applied.applied), (0, 1));
    assert_eq!(applied.end(), PullEnd::Done);
    assert_eq!(name_of(&a, "i1"), "Later");
    assert!(held_gaps(&a).expect("gaps").is_empty(), "the record healed");
    assert!(unpack(ticked).0 <= today + TEN_YEARS_AHEAD_MS);
}

#[tokio::test]
async fn a_held_far_future_change_is_skippable_after_fourteen_days_and_releases_the_rest() {
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
    push(&vault.relay, &vault.creds, &[pinned, later]).await;
    assert_eq!(pull(&vault, &vault.a).await.end(), PullEnd::Ahead);

    {
        let a = vault.a.lock();
        let early = skip_missing_change(&a, C, NOW + 14 * DAY - 1).expect_err("too early");
        assert!(matches!(early, AppError::Validation(_)), "{early:?}");
        assert!(skip_missing_change(&a, C, NOW + 14 * DAY).expect("skip"));
        assert_eq!(
            state::skipped_miss(&a).expect("miss"),
            Some(2),
            "the miss is recorded at the held envelope's position"
        );
    }
    let released = pull(&vault, &vault.a).await;

    assert_eq!((released.applied, released.ahead, released.gaps), (1, 0, 0));
    assert_eq!(released.end(), PullEnd::Done);
    let a = vault.a.lock();
    assert_eq!(
        name_of(&a, "i1"),
        "Second",
        "the later change applied, the held one did not"
    );
    assert_eq!(head(&a, C).map(|h| h.0), Some(2));
    assert!(held_gaps(&a).expect("gaps").is_empty());
}

#[tokio::test]
async fn a_far_future_change_served_after_the_one_behind_it_keeps_one_ahead_record() {
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
        assert_eq!((report.ahead, report.gaps), (1, 1), "{report:?}");
        assert_eq!(report.end(), PullEnd::Ahead);
        let gaps = held_gaps(&vault.a.lock()).expect("gaps");
        assert_eq!(gaps.len(), 1);
        assert_eq!(
            (gaps[0].kind, gaps[0].held_seq, gaps[0].since),
            (GapKind::Ahead, 1, NOW)
        );
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
    assert_eq!((pull.ahead, pull.gaps, pull.missing_gaps()), (1, 1, 1));
    assert_eq!(pull.end(), PullEnd::Waiting);
    assert_eq!(report.problem, Some(SyncProblem::Gap));
    let kinds: Vec<GapKind> = held_gaps(&vault.a.lock())
        .expect("gaps")
        .iter()
        .map(|gap| gap.kind)
        .collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&GapKind::Missing) && kinds.contains(&GapKind::Ahead));
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

#[test]
fn a_clock_is_too_far_ahead_only_past_ten_years_of_this_devices_wall_clock() {
    let now = now_ms();
    let at = |ms: u64| pack(ms, u16::MAX);
    assert!(!too_far_ahead(at(now + DAY_MS), now));
    assert!(!too_far_ahead(at(now + TEN_YEARS_AHEAD_MS), now));
    assert!(too_far_ahead(at(now + TEN_YEARS_AHEAD_MS + 1), now));
    assert!(too_far_ahead(i64::MAX as u64, now));
    assert!(too_far_ahead(u64::MAX, now));
    assert!(!too_far_ahead(0, now));
}

#[tokio::test]
async fn a_sync_server_that_went_back_outranks_a_held_far_future_change() {
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
