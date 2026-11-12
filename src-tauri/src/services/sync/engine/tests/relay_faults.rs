//! Audit proofs of concept for relay faults: each asserts today's faulty behaviour until the task
//! named beside it fixes the finding and flips it.

use super::pull::{write_and_seal, Mangled};
use super::*;
use crate::services::sync::engine::pull::pull_all;
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

// Audit M4 (acknowledged envelope the relay drops): flipped by the push-verification task.
#[tokio::test]
async fn poc_m4_acknowledged_envelope_the_relay_drops_is_never_sent_again() {
    use crate::services::sync::engine::push::push_all;
    let vault = vault().await;
    institution(&vault.a.lock(), "a1");
    push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push 1");
    vault.relay.drop_op(1);
    institution(&vault.a.lock(), "a2");
    push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push 2");
    assert!(pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("A")
        .done());
    let b = pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("B");
    // B holds A's envelope 2 behind the missing 1 (Task 15) and waits for it.
    assert_eq!(b.end(), PullEnd::Waiting);
    let a = vault.a.lock();
    assert_eq!(
        count(
            &a,
            "SELECT (SELECT COUNT(*) FROM sync_unsent)+(SELECT COUNT(*) FROM sync_outbox)"
        ),
        0
    );
}
