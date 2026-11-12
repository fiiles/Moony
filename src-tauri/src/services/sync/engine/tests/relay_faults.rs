//! Audit proofs of concept for relay faults, flipped into regression tests by the tasks that fixed
//! them.

use super::pull::{write_and_seal, Mangled};
use super::*;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::push::push_all;
use crate::services::sync::engine::PullEnd;
use crate::services::sync::relay::PullPage;

// Audit M3 (out-of-order author): fixed by Task 15. The envelope that arrives before its author's
// earlier one is held; the earlier one applies, and the next pull applies the held one.
#[tokio::test]
async fn an_out_of_order_author_is_applied_on_the_next_pull() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1, b2]).await;
    let relay = Mangled {
        inner: vault.relay.clone(),
        mangle: |_, mut p: PullPage| {
            if p.ops.len() == 2 {
                let (x, y) = (p.ops[0].ct.clone(), p.ops[1].ct.clone());
                p.ops[0].ct = y;
                p.ops[1].ct = x;
            }
            p
        },
    };

    let first = pull_all(&relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("first pull");

    assert_eq!(first.end(), PullEnd::Waiting);
    assert_eq!((first.gaps, first.applied), (1, 1));
    assert!(first.may_push());
    {
        let a = vault.a.lock();
        assert_eq!(cursor(&a), 0);
        assert!(has_institution(&a, "b1") && !has_institution(&a, "b2"));
    }

    let second = pull_all(&relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("second pull");

    assert!(second.done(), "{second:?}");
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 2);
    assert!(has_institution(&a, "b1") && has_institution(&a, "b2"));
}

// Audit M4 (an envelope the relay acknowledged and then lost): fixed by Task 16. The envelope
// stays in `sync_unsent` until A pulls it back, so A's next push sends it again; B applies both.
#[tokio::test]
async fn an_envelope_the_relay_acknowledged_and_lost_is_sent_again() {
    let vault = vault().await;
    institution(&vault.a.lock(), "a1");
    push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push 1");
    let a1 = vault.relay.ops()[0].id.clone();
    // The relay answered, then lost it (or was restored from an older backup).
    vault.relay.forget_op(1);
    institution(&vault.a.lock(), "a2");

    let second = push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push 2");

    assert_eq!(second.pushed, 2, "a1 again, then a2");
    assert_eq!(
        vault.relay.ops()[0].id,
        a1,
        "a1 is back on the relay, first"
    );
    let b = pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("B");
    assert!(b.done(), "{b:?}");
    {
        let b = vault.b.lock();
        assert!(has_institution(&b, "a1") && has_institution(&b, "a2"));
    }
    assert!(pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("A")
        .done());
    let a = vault.a.lock();
    assert_eq!(
        count(
            &a,
            "SELECT (SELECT COUNT(*) FROM sync_unsent)+(SELECT COUNT(*) FROM sync_outbox)"
        ),
        0,
        "both came back"
    );
}

// Audit M4 with M3: B holds A's second envelope behind the lost first one; A's next round sends
// the first again, and B's pulls apply both (the held one once the first is in, the pull after).
#[tokio::test]
async fn a_lost_envelope_held_behind_on_another_device_arrives_with_the_next_round() {
    let vault = vault().await;
    for id in ["a1", "a2"] {
        institution(&vault.a.lock(), id);
        push_all(&vault.relay, &vault.a, &vault.creds)
            .await
            .expect("push");
    }
    assert_eq!(vault.relay.ops().len(), 2);
    vault.relay.forget_op(1);

    let held = pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("B holds a2");
    assert_eq!(held.end(), PullEnd::Waiting);

    let round = crate::services::sync::engine::run_round(
        &vault.relay,
        &vault.a,
        &mut crate::services::sync::engine::snapshots::SnapshotPolicy::default(),
        crate::services::sync::engine::RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("A's round");
    assert_eq!(round.push.as_ref().map(|p| p.pushed), Some(1), "a1 again");
    assert_eq!(
        count(&vault.a.lock(), "SELECT COUNT(*) FROM sync_unsent"),
        0,
        "a2 came back in the pull, a1 in the round's echo pull"
    );

    let mut pulls = 0;
    loop {
        pulls += 1;
        let report = pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
            .await
            .expect("B");
        if report.done() {
            break;
        }
        assert!(pulls < 3, "{report:?}");
    }
    let b = vault.b.lock();
    assert!(has_institution(&b, "a1") && has_institution(&b, "a2"));
}
