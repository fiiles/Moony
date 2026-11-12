//! Audit proofs of concept for relay faults: each asserts today's faulty behaviour until the task
//! named beside it fixes the finding and flips it.

use super::pull::{write_and_seal, Mangled};
use super::*;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::PullStop;
use crate::services::sync::relay::PullPage;

// Audit M3 (out-of-order author wedges the pull): flipped by the pull-ordering task.
#[tokio::test]
async fn poc_m3_out_of_order_author_wedges_the_pull_for_good() {
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
    for _ in 0..3 {
        let r = pull_all(&relay, &vault.a, &vault.creds, NOW)
            .await
            .expect("pull");
        assert_eq!(r.stop, Some(PullStop::Gap));
        assert!(!r.may_push());
        assert_eq!(cursor(&vault.a.lock()), 0);
    }
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
    assert_eq!(b.stop, Some(PullStop::Gap));
    let a = vault.a.lock();
    assert_eq!(
        count(
            &a,
            "SELECT (SELECT COUNT(*) FROM sync_unsent)+(SELECT COUNT(*) FROM sync_outbox)"
        ),
        0
    );
}
