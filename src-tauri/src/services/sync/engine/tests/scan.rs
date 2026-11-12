//! The pull past a newer-schema stop (Task 16 review, round 2): it scans for this device's own
//! envelopes only while one waits for its echo, the round's echo pull scans only past what the
//! first pull already looked at, and the scan moves no cursor and no other author's head.

use std::sync::{Arc, Mutex};

use super::pull::{write_and_seal, Mangled};
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::engine::pull::{pull_all, pull_echo};
use crate::services::sync::engine::push::push_all;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, PullEnd, PullStop, RoundMode};
use crate::services::sync::registry::keys::schema_number;
use crate::services::sync::relay::PullPage;

/// `C`'s first envelope, written by a newer schema than this build knows.
fn newer_c1(vault: &Vault) -> Vec<u8> {
    let mut newer = changeset(C, 1, None, vec![]);
    newer.schema = schema_number() + 1;
    forge(&vault.creds, &newer)
}

/// The fake relay serving one envelope per page; `afters` records where each page was asked from.
fn one_per_page(
    relay: &FakeRelay,
    afters: Arc<Mutex<Vec<u64>>>,
) -> Mangled<impl Fn(u64, PullPage) -> PullPage + Send + Sync> {
    Mangled {
        inner: relay.clone(),
        mangle: move |after, mut page: PullPage| {
            afters.lock().expect("afters").push(after);
            page.ops.truncate(1);
            page.more = page.ops.last().is_some_and(|op| op.seq < page.head);
            page
        },
    }
}

fn unsent(conn: &Connection) -> i64 {
    count(conn, "SELECT COUNT(*) FROM sync_unsent")
}

#[tokio::test]
async fn a_newer_schema_stop_with_nothing_waiting_stops_at_once() {
    let vault = vault().await;
    let c1 = newer_c1(&vault);
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[c1, b1, b2]).await;
    let afters = Arc::new(Mutex::new(Vec::new()));
    let relay = one_per_page(&vault.relay, afters.clone());

    let report = pull_all(&relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert_eq!(report.stop, Some(PullStop::NewerSchema));
    assert_eq!(
        *afters.lock().expect("afters"),
        vec![0],
        "one page, as before"
    );
    assert_eq!(report.reach, Some(3));
}

#[tokio::test]
async fn the_scan_goes_on_only_while_an_echo_is_missing_and_moves_nothing_else() {
    let vault = vault().await;
    let c1 = newer_c1(&vault);
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[c1, b1, a1, b2]).await;
    assert_eq!(unsent(&vault.a.lock()), 1);
    let before = head(&vault.a.lock(), B);
    let afters = Arc::new(Mutex::new(Vec::new()));
    let relay = one_per_page(&vault.relay, afters.clone());

    let report = pull_all(&relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert_eq!(report.end(), PullEnd::Stopped(PullStop::NewerSchema));
    assert_eq!(
        *afters.lock().expect("afters"),
        vec![0, 1, 2],
        "the scan ends once a1 came back, before b2"
    );
    let a = vault.a.lock();
    assert_eq!(unsent(&a), 0, "a1 acknowledged");
    assert_eq!(cursor(&a), 0, "the cursor stays before the stop");
    assert_eq!(head(&a, B), before, "B's head untouched");
    assert!(head(&a, C).is_none());
    assert!(!has_institution(&a, "b1") && !has_institution(&a, "b2"));
}

#[tokio::test]
async fn the_echo_scan_starts_at_the_first_pulls_reach() {
    let vault = vault().await;
    let c1 = newer_c1(&vault);
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[c1, b1, b2]).await;
    let afters = Arc::new(Mutex::new(Vec::new()));
    let relay = one_per_page(&vault.relay, afters.clone());
    let first = pull_all(&relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("first pull");
    assert_eq!(first.reach, Some(3));
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    push(&vault.relay, &vault.creds, &[a1]).await;
    afters.lock().expect("afters").clear();

    let echo = pull_echo(&relay, &vault.a, &vault.creds, NOW, first.reach)
        .await
        .expect("echo pull");

    assert_eq!(echo.stop, Some(PullStop::NewerSchema));
    assert_eq!(
        *afters.lock().expect("afters"),
        vec![0, 3],
        "the stop again, then straight past the first pull's reach"
    );
    assert_eq!(unsent(&vault.a.lock()), 0);
}

/// The `reached` guard: an echo the relay keeps but never serves, behind a newer-schema stop, is
/// not reported as a relay error: the stop keeps its own problem.
#[tokio::test]
async fn a_withheld_echo_behind_a_newer_schema_stop_stays_newer_schema() {
    let vault = vault().await;
    push(&vault.relay, &vault.creds, &[newer_c1(&vault)]).await;
    institution(&vault.a.lock(), "a1");
    push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push a1");
    vault.relay.drop_op(2);

    for round in ["first", "second"] {
        let report = run_round(
            &vault.relay,
            &vault.a,
            &mut SnapshotPolicy::default(),
            RoundMode::Normal,
            NOW,
            0,
        )
        .await
        .expect(round);

        assert_eq!(report.problem, Some(SyncProblem::NewerSchema), "{round}");
        assert_eq!(unsent(&vault.a.lock()), 1, "{round}");
    }
}
