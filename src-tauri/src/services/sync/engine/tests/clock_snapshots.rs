//! Audit M6, the rest of the clock bound: a snapshot whose newest clock is more than ten years
//! ahead is refused by every restore (a join, a re-sync, the restore after `410`) before this
//! device's clock moves; and a device whose own clock stands more than a day ahead says so in its
//! status (`clockAheadUntil`), as a note that never replaces a problem.

use super::join::{join, snapshot_of_a, validation_key, XorNative};
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::capture;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::snapshots::restore_after_410;
use crate::services::sync::engine::status::{problem_of, status, Runtime};
use crate::services::sync::engine::RoundMode;
use crate::services::sync::error::SyncError;
use crate::services::sync::hlc::{now_ms, pack, unpack};
use crate::services::sync::join::resync_by_replace;
use crate::services::sync::snapshot::CLOCK_AHEAD_KEY;
use crate::services::sync::test_support::migrated;

const DAY_MS: u64 = 86_400_000;

/// `A` uploads a snapshot in which institution `a1` carries the maximum clock; returns its `S`.
async fn max_clock_snapshot(vault: &Vault) -> u64 {
    snapshot_of_a(vault, "a1").await;
    exec(
        &vault.a.lock(),
        "UPDATE sync_clock SET hlc = 9223372036854775807 \
         WHERE tbl = 'institutions' AND pk = 'a1'",
    );
    institution(&vault.a.lock(), "a2");
    let report = super::join::round(vault, &vault.a, RoundMode::FirstSnapshot).await;
    assert!(report.snapshot_uploaded, "{report:?}");
    vault.relay.snapshot_seq().expect("the vault's snapshot")
}

/// This device's clock stands within an hour of its wall clock.
fn clock_unmoved(conn: &Connection) {
    let clock = capture::clock_now(conn).expect("clock");
    assert!(
        unpack(clock).0 <= now_ms() + 3_600_000,
        "the clock moved to {clock}"
    );
}

fn refused(error: &SyncError) {
    assert_eq!(validation_key(error), Some(CLOCK_AHEAD_KEY), "{error:?}");
}

#[tokio::test]
async fn a_join_refuses_a_snapshot_dated_more_than_ten_years_ahead() {
    let vault = vault().await;
    max_clock_snapshot(&vault).await;
    let joiner = TestConn::new(migrated());
    institution(&joiner.lock(), "local-only");
    let dir = tempfile::tempdir().expect("dir");

    let error = join(&vault, &joiner, dir.path(), &XorNative)
        .await
        .expect_err("refused");

    refused(&error);
    let c = joiner.lock();
    assert!(has_institution(&c, "local-only"), "nothing was replaced");
    assert!(!has_institution(&c, "a1"));
    assert!(state::load(&c)
        .expect("state")
        .is_none_or(|sync| !sync.enabled));
    clock_unmoved(&c);
}

#[tokio::test]
async fn the_restore_after_410_refuses_it_and_shows_a_failed_verification() {
    let vault = vault().await;
    let s = max_clock_snapshot(&vault).await;
    vault.relay.prune_to(s);
    assert!(matches!(
        pull_all(&vault.relay, &vault.b, &vault.creds, NOW).await,
        Err(SyncError::SnapshotRequired)
    ));

    let error = restore_after_410(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect_err("refused");

    refused(&error);
    assert_eq!(problem_of(&error), SyncProblem::InvalidChain);
    let b = vault.b.lock();
    assert!(!has_institution(&b, "a1"));
    assert_eq!(cursor(&b), 0);
    clock_unmoved(&b);
}

#[tokio::test]
async fn a_resync_refuses_it_and_keeps_this_devices_data() {
    let vault = vault().await;
    max_clock_snapshot(&vault).await;
    institution(&vault.b.lock(), "b-local");
    let consent = agreed(&vault.b);
    let dir = tempfile::tempdir().expect("dir");

    let error = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &XorNative, NOW)
        .await
        .expect_err("refused");

    refused(&error);
    let b = vault.b.lock();
    assert!(has_institution(&b, "b-local"));
    assert!(!has_institution(&b, "a1"));
    clock_unmoved(&b);
}

#[tokio::test]
async fn the_status_notes_a_clock_more_than_a_day_ahead_of_this_device() {
    let vault = vault().await;
    let a = vault.a.lock();
    let quiet = status(&a, &Runtime::default()).expect("status");
    assert_eq!(quiet.clock_ahead_until, None);

    // Less than a day ahead: no note.
    capture::observe(&a, pack(now_ms() + DAY_MS / 2, 0)).expect("observe");
    assert_eq!(
        status(&a, &Runtime::default())
            .expect("status")
            .clock_ahead_until,
        None
    );

    let ahead_ms = now_ms() + 3 * DAY_MS;
    capture::observe(&a, pack(ahead_ms, 0)).expect("observe");
    let runtime = Runtime {
        problem: Some(SyncProblem::Gap),
        ..Runtime::default()
    };
    let shown = status(&a, &runtime).expect("status");

    assert_eq!(shown.clock_ahead_until, Some((ahead_ms / 1000) as i64));
    assert_eq!(
        shown.problem,
        Some(SyncProblem::Gap),
        "a note, never a problem"
    );
}
