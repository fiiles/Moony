//! Tests of `FakeRelay`: the relay semantics the engine relies on, and its fault switches.

use super::*;
use crate::services::sync::envelope::blob::MAX_BLOB_SEALED;
use crate::services::sync::relay::{
    BlobBody, BlobSink, Delivery, Mailbox, SinkFlow, MAX_BLOB_PAGE, MAX_SLOT_BYTES,
};
use zeroize::Zeroizing;

const VAULT_ID: &str = "AAECAwQFBgcICQoLDA0ODw";
const TOKEN: &str = "dG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9";

fn auth() -> VaultAuth {
    VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new(TOKEN.into()),
    }
}

fn op(id: &str, len: usize) -> OutgoingOp {
    OutgoingOp {
        id: id.into(),
        ct: vec![7; len],
    }
}

/// A paid account with one vault.
async fn relay_with_vault() -> (FakeRelay, String) {
    let relay = FakeRelay::new();
    let account = relay.create_account().await.expect("account");
    relay.set_paid(&account, true);
    assert!(relay.create_vault(&account, &auth()).await.expect("vault"));
    (relay, account)
}

#[tokio::test]
async fn duplicate_op_ids_are_stored_once() {
    let (relay, _) = relay_with_vault().await;
    let first = relay
        .push(&auth(), &[op("a", 10), op("b", 10)])
        .await
        .expect("push");
    assert_eq!(
        first,
        PushReceipt {
            head: 2,
            accepted: 2
        }
    );
    let again = relay
        .push(&auth(), &[op("b", 10), op("c", 10), op("a", 10)])
        .await
        .expect("push");
    assert_eq!(
        again,
        PushReceipt {
            head: 3,
            accepted: 1
        }
    );
    let ids: Vec<(u64, String)> = relay.ops().into_iter().map(|o| (o.seq, o.id)).collect();
    assert_eq!(ids, vec![(1, "a".into()), (2, "b".into()), (3, "c".into())]);
    let page = relay.pull(&auth(), 0, 500).await.expect("pull");
    assert_eq!((page.ops.len(), page.head, page.more), (3, 3, false));
}

#[tokio::test]
async fn pulls_below_the_floor_require_a_snapshot() {
    let (relay, _) = relay_with_vault().await;
    let ops: Vec<_> = (0..5).map(|i| op(&format!("op{i}"), 4)).collect();
    relay.push(&auth(), &ops).await.expect("push");
    // Nothing to prune to before a snapshot exists.
    relay.prune_to(3);
    assert_eq!(relay.pull(&auth(), 0, 10).await.expect("pull").ops.len(), 5);

    relay
        .put_snapshot(&auth(), 2, vec![1; 8])
        .await
        .expect("snapshot");
    // The floor never passes the snapshot: 4 becomes 2.
    relay.prune_to(4);
    let info = relay.vault_info(&auth()).await.expect("info");
    assert_eq!(
        (info.head, info.floor, info.snapshot_seq, info.bytes),
        (5, 2, Some(2), 12)
    );
    assert_eq!(
        relay.pull(&auth(), 1, 10).await.expect_err("below floor"),
        RelayError::SnapshotRequired
    );
    let page = relay.pull(&auth(), 2, 10).await.expect("at the floor");
    let seqs: Vec<u64> = page.ops.iter().map(|o| o.seq).collect();
    assert_eq!((seqs, page.head, page.more), (vec![3, 4, 5], 5, false));
    // A pruned op's ID is forgotten, as on the relay.
    let receipt = relay.push(&auth(), &[op("op0", 4)]).await.expect("push");
    assert_eq!((receipt.head, receipt.accepted), (6, 1));
}

#[tokio::test]
async fn writes_need_payment_and_reads_do_not() {
    let (relay, account) = relay_with_vault().await;
    relay.push(&auth(), &[op("a", 3)]).await.expect("push");
    relay.set_paid(&account, false);

    assert_eq!(
        relay.push(&auth(), &[op("b", 3)]).await.expect_err("push"),
        RelayError::PaymentRequired
    );
    assert_eq!(
        relay
            .put_snapshot(&auth(), 1, vec![1])
            .await
            .expect_err("snapshot"),
        RelayError::PaymentRequired
    );
    assert_eq!(relay.pull(&auth(), 0, 10).await.expect("pull").ops.len(), 1);
    assert_eq!(relay.vault_info(&auth()).await.expect("info").head, 1);
    let status = relay.account_status(&account).await.expect("status");
    assert!(!status.active);
    // Past the 30-day read grace even reads stop.
    relay.set_paid_until(&account, now() - 31 * 86_400);
    assert_eq!(
        relay.pull(&auth(), 0, 10).await.expect_err("pull"),
        RelayError::PaymentRequired
    );
    // Deleting the vault never needs a subscription.
    relay.delete_vault(&auth()).await.expect("delete");
    // An unpaid account cannot open a vault.
    let other = relay.create_account().await.expect("account");
    assert_eq!(
        relay
            .create_vault(&other, &auth())
            .await
            .expect_err("vault"),
        RelayError::PaymentRequired
    );
}

#[tokio::test]
async fn a_lost_push_answer_still_stores_the_ops() {
    let (relay, _) = relay_with_vault().await;
    relay.lose_next_push_answer();
    let error = relay
        .push(&auth(), &[op("a", 3), op("b", 3)])
        .await
        .expect_err("lost");
    assert!(matches!(error, RelayError::Network(_)), "{error:?}");
    assert_eq!(relay.ops().len(), 2);
    // The retry is answered from the stored ops: nothing new is accepted.
    let retry = relay
        .push(&auth(), &[op("a", 3), op("b", 3)])
        .await
        .expect("retry");
    assert_eq!(
        retry,
        PushReceipt {
            head: 2,
            accepted: 0
        }
    );
    assert_eq!(relay.push_sizes(), vec![(2, 8), (2, 8)]);
}

#[tokio::test]
async fn snapshots_must_not_go_backwards() {
    let (relay, _) = relay_with_vault().await;
    let ops: Vec<_> = (0..4).map(|i| op(&format!("op{i}"), 2)).collect();
    relay.push(&auth(), &ops).await.expect("push");
    assert_eq!(
        relay.get_snapshot(&auth()).await.expect_err("none"),
        RelayError::NoSnapshot
    );
    assert_eq!(
        relay
            .put_snapshot(&auth(), 5, vec![1])
            .await
            .expect_err("ahead"),
        RelayError::BadRequest("snapshot_ahead_of_head".into())
    );
    relay.put_snapshot(&auth(), 3, vec![3, 3]).await.expect("3");
    assert_eq!(
        relay
            .put_snapshot(&auth(), 2, vec![2])
            .await
            .expect_err("back"),
        RelayError::SnapshotOutdated
    );
    // The same seq replaces; a newer one wins.
    relay
        .put_snapshot(&auth(), 3, vec![9, 9, 9])
        .await
        .expect("3 again");
    relay.put_snapshot(&auth(), 4, vec![4]).await.expect("4");
    let blob = relay.get_snapshot(&auth()).await.expect("blob");
    assert_eq!((blob.seq, blob.bytes), (4, vec![4]));
    assert_eq!(relay.snapshot_seq(), Some(4));
}

#[tokio::test]
async fn fault_switches_and_dishonest_knobs_work() {
    let (relay, _) = relay_with_vault().await;
    relay.fail_next(Route::Pull, RelayError::RateLimited);
    assert_eq!(
        relay.pull(&auth(), 0, 10).await.expect_err("failed"),
        RelayError::RateLimited
    );
    assert!(relay.pull(&auth(), 0, 10).await.is_ok());

    relay
        .push(&auth(), &[op("a", 3), op("b", 3), op("c", 3)])
        .await
        .expect("push");
    relay.drop_op(3);
    let page = relay.pull(&auth(), 0, 10).await.expect("pull");
    let seqs: Vec<u64> = page.ops.iter().map(|o| o.seq).collect();
    assert_eq!((seqs, page.head, page.more), (vec![1, 2], 3, false));
    relay.replay_op(1);
    let page = relay.pull(&auth(), 2, 10).await.expect("pull");
    assert_eq!(
        page.ops
            .iter()
            .map(|o| (o.seq, o.id.as_str()))
            .collect::<Vec<_>>(),
        vec![(4, "a")]
    );
    assert_eq!(page.head, 4);

    relay.wipe_vault(VAULT_ID);
    assert_eq!(
        relay.vault_info(&auth()).await.expect_err("wiped"),
        RelayError::VaultNotFound
    );
    let wrong = VaultAuth {
        vault_id: "short".into(),
        token: Zeroizing::new(TOKEN.into()),
    };
    assert_eq!(
        relay.vault_info(&wrong).await.expect_err("bad id"),
        RelayError::BadRequest("invalid_vault_id".into())
    );
    assert_eq!(relay.calls().first(), Some(&Route::CreateAccount));
}

/// 22 base64url characters, distinct per `n`.
fn vault_id(n: u32) -> String {
    format!("{n:022}")
}

fn vault_with(id: &str, token: &str) -> VaultAuth {
    VaultAuth {
        vault_id: id.into(),
        token: Zeroizing::new(token.into()),
    }
}

async fn push_many(relay: &FakeRelay, count: usize, len: usize) {
    let ops: Vec<_> = (0..count).map(|i| op(&format!("op{i}"), len)).collect();
    relay.push(&auth(), &ops).await.expect("push");
}

#[tokio::test]
async fn pages_are_cut_by_the_limit_and_say_whether_more_follow() {
    let (relay, _) = relay_with_vault().await;
    push_many(&relay, 5, 4).await;
    let mut seen = Vec::new();
    let mut after = 0;
    loop {
        let page = relay.pull(&auth(), after, 2).await.expect("pull");
        let seqs: Vec<u64> = page.ops.iter().map(|o| o.seq).collect();
        seen.push((seqs.clone(), page.more, page.head));
        let Some(last) = seqs.last() else { break };
        after = *last;
        if !page.more {
            break;
        }
    }
    assert_eq!(
        seen,
        vec![
            (vec![1, 2], true, 5),
            (vec![3, 4], true, 5),
            (vec![5], false, 5),
        ]
    );
    // Past the head: an empty page, nothing more.
    let page = relay.pull(&auth(), 5, 2).await.expect("pull");
    assert_eq!((page.ops.len(), page.more, page.head), (0, false, 5));
}

#[tokio::test]
async fn the_limit_is_clamped_to_a_thousand_and_zero_is_refused() {
    let (relay, _) = relay_with_vault().await;
    for chunk in 0..3 {
        let ops: Vec<_> = (0..500).map(|i| op(&format!("c{chunk}-{i}"), 1)).collect();
        relay.push(&auth(), &ops).await.expect("push");
    }
    let page = relay.pull(&auth(), 0, u32::MAX).await.expect("pull");
    assert_eq!((page.ops.len(), page.more, page.head), (1000, true, 1500));
    assert_eq!(page.ops.last().map(|o| o.seq), Some(1000));
    assert_eq!(
        relay.pull(&auth(), 0, 0).await.expect_err("limit 0"),
        RelayError::BadRequest("invalid_limit".into())
    );
}

#[tokio::test]
async fn a_page_is_cut_at_four_mebibytes_of_ciphertext() {
    let (relay, _) = relay_with_vault().await;
    // 22 ops of the largest size (196,608 bytes); 21 of them fit in 4 MiB.
    for batch in 0..2 {
        let ops: Vec<_> = (0..11)
            .map(|i| op(&format!("b{batch}-{i}"), 196_608))
            .collect();
        relay.push(&auth(), &ops).await.expect("push");
    }
    let page = relay.pull(&auth(), 0, 1000).await.expect("pull");
    assert_eq!((page.ops.len(), page.more, page.head), (21, true, 22));
    let rest = relay.pull(&auth(), 21, 1000).await.expect("pull");
    assert_eq!(
        (
            rest.ops.iter().map(|o| o.seq).collect::<Vec<_>>(),
            rest.more
        ),
        (vec![22], false)
    );
}

#[tokio::test]
async fn an_op_over_the_size_limit_is_refused() {
    let (relay, _) = relay_with_vault().await;
    assert_eq!(
        relay
            .push(&auth(), &[op("big", 196_609)])
            .await
            .expect_err("too big"),
        RelayError::BadRequest("invalid_ops".into())
    );
    let ok = relay
        .push(&auth(), &[op("max", 196_608)])
        .await
        .expect("max");
    assert_eq!(ok.accepted, 1);
    assert_eq!(relay.ops().len(), 1);
}

#[tokio::test]
async fn a_wrong_token_is_unauthorized() {
    let (relay, _) = relay_with_vault().await;
    let other = "b3RoZXItdG9rZW4tb3RoZXItdG9rZW4tb3RoZXItdG9";
    assert_eq!(other.len(), 43);
    let intruder = vault_with(VAULT_ID, other);
    assert_eq!(
        relay.vault_info(&intruder).await.expect_err("info"),
        RelayError::Unauthorized
    );
    assert_eq!(
        relay.pull(&intruder, 0, 10).await.expect_err("pull"),
        RelayError::Unauthorized
    );
    assert_eq!(
        relay.delete_vault(&intruder).await.expect_err("delete"),
        RelayError::Unauthorized
    );
}

#[tokio::test]
async fn vault_creation_is_idempotent_for_its_owner_and_refused_otherwise() {
    let (relay, account) = relay_with_vault().await;
    // Same account, same token: already there.
    assert!(!relay.create_vault(&account, &auth()).await.expect("again"));
    // Same account, another token; or another account.
    let other_token = "b3RoZXItdG9rZW4tb3RoZXItdG9rZW4tb3RoZXItdG9";
    assert_eq!(
        relay
            .create_vault(&account, &vault_with(VAULT_ID, other_token))
            .await
            .expect_err("token"),
        RelayError::VaultExists
    );
    let stranger = relay.create_account().await.expect("account");
    relay.set_paid(&stranger, true);
    assert_eq!(
        relay
            .create_vault(&stranger, &auth())
            .await
            .expect_err("account"),
        RelayError::VaultExists
    );
}

#[tokio::test]
async fn an_account_holds_at_most_five_vaults() {
    let relay = FakeRelay::new();
    let account = relay.create_account().await.expect("account");
    relay.set_paid(&account, true);
    for n in 0..5 {
        let vault = vault_with(&vault_id(n), TOKEN);
        assert!(relay.create_vault(&account, &vault).await.expect("vault"));
    }
    assert_eq!(
        relay
            .create_vault(&account, &vault_with(&vault_id(5), TOKEN))
            .await
            .expect_err("sixth"),
        RelayError::VaultLimitReached
    );
    assert_eq!(
        relay.account_status(&account).await.expect("status").vaults,
        5
    );
    // Deleting one frees a slot.
    relay
        .delete_vault(&vault_with(&vault_id(0), TOKEN))
        .await
        .expect("delete");
    assert!(relay
        .create_vault(&account, &vault_with(&vault_id(5), TOKEN))
        .await
        .expect("slot"));
}

#[tokio::test]
async fn pushes_past_the_op_quota_are_refused_whole() {
    let (relay, _) = relay_with_vault().await;
    relay.set_op_quota(10);
    relay
        .push(&auth(), &[op("a", 6)])
        .await
        .expect("within quota");
    assert_eq!(
        relay
            .push(&auth(), &[op("b", 3), op("c", 2)])
            .await
            .expect_err("over quota"),
        RelayError::QuotaExceeded
    );
    assert_eq!(relay.ops().len(), 1);
    // Exactly at the quota is fine.
    relay.push(&auth(), &[op("d", 4)]).await.expect("at quota");
}

#[tokio::test]
async fn reads_stop_exactly_when_the_thirty_day_grace_ends() {
    let (relay, account) = relay_with_vault().await;
    let grace = 30 * 86_400;
    relay.set_paid_until(&account, now() - grace + 5);
    assert!(relay.pull(&auth(), 0, 10).await.is_ok(), "just inside");
    assert!(relay.vault_info(&auth()).await.is_ok(), "just inside");
    relay.set_paid_until(&account, now() - grace - 5);
    assert_eq!(
        relay.pull(&auth(), 0, 10).await.expect_err("just outside"),
        RelayError::PaymentRequired
    );
    assert_eq!(
        relay.get_snapshot(&auth()).await.expect_err("just outside"),
        RelayError::PaymentRequired
    );
}

#[tokio::test]
async fn a_dropped_op_leaves_a_gap_in_the_sequence_numbers() {
    let (relay, _) = relay_with_vault().await;
    push_many(&relay, 4, 3).await;
    relay.drop_op(3);
    let page = relay.pull(&auth(), 0, 10).await.expect("pull");
    let seqs: Vec<u64> = page.ops.iter().map(|o| o.seq).collect();
    assert_eq!((seqs, page.head, page.more), (vec![1, 2, 4], 4, false));
}

#[tokio::test]
async fn a_fully_dropped_page_is_skipped_not_returned_empty() {
    let (relay, _) = relay_with_vault().await;
    push_many(&relay, 5, 3).await;
    relay.drop_op(1);
    relay.drop_op(2);
    // The page of two counts surviving ops: 3 and 4, with 5 still to come.
    let page = relay.pull(&auth(), 0, 2).await.expect("pull");
    let seqs: Vec<u64> = page.ops.iter().map(|o| o.seq).collect();
    assert_eq!((seqs, page.more), (vec![3, 4], true));
    let page = relay.pull(&auth(), 4, 2).await.expect("pull");
    assert_eq!(
        (
            page.ops.iter().map(|o| o.seq).collect::<Vec<_>>(),
            page.more
        ),
        (vec![5], false)
    );
    // Dropping the tail leaves nothing to ask for: never an empty page with `more`.
    relay.drop_op(5);
    let page = relay.pull(&auth(), 4, 2).await.expect("pull");
    assert_eq!((page.ops.len(), page.more, page.head), (0, false, 5));
}

const MAILBOX: &str = "AAECAwQFBgcICQoLDA0ODw";

#[tokio::test]
async fn the_fake_mailbox_is_write_once_and_read_once() {
    let relay = FakeRelay::new();
    relay.put_slot(MAILBOX, 1, b"first").await.expect("put 1");
    assert_eq!(
        relay.put_slot(MAILBOX, 1, b"again").await,
        Err(RelayError::SlotTaken)
    );
    assert_eq!(relay.take_slot(MAILBOX, 2).await, Ok(None));
    assert_eq!(relay.peek_slot(MAILBOX, 1), Some(b"first".to_vec()));
    assert_eq!(
        relay.take_slot(MAILBOX, 1).await,
        Ok(Some(b"first".to_vec()))
    );
    assert_eq!(relay.peek_slot(MAILBOX, 1), None);
    assert_eq!(
        relay.take_slot(MAILBOX, 1).await,
        Err(RelayError::SlotConsumed)
    );
    // A write to a slot that was read is still refused.
    assert_eq!(
        relay.put_slot(MAILBOX, 1, b"late").await,
        Err(RelayError::SlotTaken)
    );
    // The other slot is independent; an attacker's write follows the same rules.
    relay.write_slot_raw(MAILBOX, 2, b"theirs".to_vec());
    assert_eq!(
        relay.put_slot(MAILBOX, 2, b"mine").await,
        Err(RelayError::SlotTaken)
    );
    assert_eq!(
        relay.take_slot(MAILBOX, 2).await,
        Ok(Some(b"theirs".to_vec()))
    );
}

#[tokio::test]
async fn a_burnt_or_expired_mailbox_is_gone_for_every_call() {
    let relay = FakeRelay::new();
    relay.put_slot(MAILBOX, 1, b"x").await.expect("put");
    relay.burn_mailbox(MAILBOX).await.expect("burn");
    assert_eq!(relay.peek_slot(MAILBOX, 1), None);
    for slot in [1, 2] {
        assert_eq!(
            relay.put_slot(MAILBOX, slot, b"y").await,
            Err(RelayError::MailboxGone)
        );
        assert_eq!(
            relay.take_slot(MAILBOX, slot).await,
            Err(RelayError::MailboxGone)
        );
    }
    // Burning again is idempotent.
    relay.burn_mailbox(MAILBOX).await.expect("burn again");

    let other = "BBECAwQFBgcICQoLDA0ODw";
    relay.put_slot(other, 2, b"x").await.expect("put");
    relay.expire_mailbox(other);
    assert_eq!(
        relay.take_slot(other, 2).await,
        Err(RelayError::MailboxGone)
    );
    assert_eq!(
        relay.put_slot(other, 1, b"x").await,
        Err(RelayError::MailboxGone)
    );
}

#[tokio::test]
async fn unknown_mailboxes_store_nothing() {
    let relay = FakeRelay::new();
    assert_eq!(relay.take_slot(MAILBOX, 1).await, Ok(None));
    relay.burn_mailbox(MAILBOX).await.expect("burn unknown");
    relay.expire_mailbox(MAILBOX);
    // Neither the read, the burn nor the expiry burnt the ID: a put still opens it.
    relay.put_slot(MAILBOX, 1, b"x").await.expect("put");
    assert_eq!(relay.take_slot(MAILBOX, 1).await, Ok(Some(b"x".to_vec())));
}

#[tokio::test]
async fn a_lost_take_answer_leaves_the_slot_consumed() {
    let relay = FakeRelay::new();
    relay.put_slot(MAILBOX, 1, b"x").await.expect("put");
    relay.lose_next_take_answer();
    assert_eq!(
        relay.take_slot(MAILBOX, 1).await,
        Err(RelayError::Network("timeout".into()))
    );
    assert_eq!(
        relay.take_slot(MAILBOX, 1).await,
        Err(RelayError::SlotConsumed)
    );
}

#[tokio::test]
async fn a_lost_put_answer_leaves_the_slot_written() {
    let relay = FakeRelay::new();
    relay.lose_next_put_answer();
    assert_eq!(
        relay.put_slot(MAILBOX, 1, b"x").await,
        Err(RelayError::Network("timeout".into()))
    );
    assert_eq!(relay.peek_slot(MAILBOX, 1), Some(b"x".to_vec()));
    assert_eq!(
        relay.put_slot(MAILBOX, 1, b"x").await,
        Err(RelayError::SlotTaken)
    );
}

#[tokio::test]
async fn the_fake_checks_mailbox_arguments_like_the_http_client() {
    let relay = FakeRelay::new();
    assert_eq!(
        relay.put_slot("short", 1, b"x").await,
        Err(RelayError::BadRequest("bad_mailbox".into()))
    );
    assert_eq!(
        relay.take_slot(MAILBOX, 4).await,
        Err(RelayError::BadRequest("bad_slot".into()))
    );
    assert_eq!(
        relay.put_slot(MAILBOX, 1, b"").await,
        Err(RelayError::BadRequest("empty_body".into()))
    );
    assert_eq!(
        relay
            .put_slot(MAILBOX, 1, &vec![0; MAX_SLOT_BYTES + 1])
            .await,
        Err(RelayError::BadRequest("body_too_large".into()))
    );
    assert!(relay.calls().is_empty());
    relay.fail_next(Route::MailboxTake, RelayError::RateLimited);
    assert_eq!(
        relay.take_slot(MAILBOX, 1).await,
        Err(RelayError::RateLimited)
    );
    assert_eq!(relay.calls(), vec![Route::MailboxTake]);
}

#[tokio::test]
async fn the_fake_keeps_slot_3_write_once_and_read_once() {
    let relay = FakeRelay::new();
    relay.put_slot(MAILBOX, 3, b"ack").await.expect("put 3");
    assert_eq!(
        relay.put_slot(MAILBOX, 3, b"again").await,
        Err(RelayError::SlotTaken)
    );
    assert_eq!(relay.take_slot(MAILBOX, 1).await, Ok(None));
    assert_eq!(relay.peek_slot(MAILBOX, 3), Some(b"ack".to_vec()));
    assert_eq!(relay.take_slot(MAILBOX, 3).await, Ok(Some(b"ack".to_vec())));
    assert_eq!(relay.peek_slot(MAILBOX, 3), None);
    assert_eq!(
        relay.take_slot(MAILBOX, 3).await,
        Err(RelayError::SlotConsumed)
    );
    relay.write_slot_raw(MAILBOX, 2, b"x".to_vec());
    assert_eq!(
        relay.put_slot(MAILBOX, 4, b"x").await,
        Err(RelayError::BadRequest("bad_slot".into()))
    );
    relay.burn_mailbox(MAILBOX).await.expect("burn");
    assert_eq!(
        relay.put_slot(MAILBOX, 3, b"y").await,
        Err(RelayError::MailboxGone)
    );
}

#[tokio::test]
async fn a_lost_put_answer_is_stored_anyway() {
    let relay = FakeRelay::new();
    relay.lose_next_answer(Route::MailboxPut);
    assert_eq!(
        relay.put_slot(MAILBOX, 3, b"ack").await,
        Err(RelayError::Server(503))
    );
    assert_eq!(relay.peek_slot(MAILBOX, 3), Some(b"ack".to_vec()));
    // The switch fires once.
    relay.put_slot(MAILBOX, 1, b"x").await.expect("put 1");
}

#[tokio::test]
async fn server_info_defaults_to_an_own_server_and_can_look_like_an_old_relay() {
    let relay = FakeRelay::new();
    let info = relay.server_info().await.expect("info");
    assert_eq!(info.protocol, 3);
    assert_eq!(info.mode, ServerMode::SelfHosted);
    relay.set_server_info(None);
    assert_eq!(
        relay.server_info().await,
        Err(RelayError::BadRequest("not_found".into()))
    );
    assert_eq!(relay.calls(), [Route::ServerInfo, Route::ServerInfo]);
}

#[tokio::test]
async fn people_can_be_added_blocked_unblocked_and_removed() {
    let (relay, account) = relay_with_vault().await;
    let person = relay.add_person();
    let status = relay.account_status(&person).await.expect("status");
    assert!(status.active);
    assert_eq!(status.paid_until, Some(253_402_300_799));

    relay.block(&person, 1_700_000_000);
    let status = relay.account_status(&person).await.expect("status");
    assert!(!status.active);
    assert_eq!(status.paid_until, Some(1_700_000_000));
    relay.unblock(&person);
    assert!(relay.account_status(&person).await.expect("status").active);

    // Removing an account takes its vaults along.
    relay.remove_account(&account);
    assert_eq!(
        relay.account_status(&account).await,
        Err(RelayError::AccountNotFound)
    );
    assert_eq!(
        relay.vault_info(&auth()).await,
        Err(RelayError::VaultNotFound)
    );
}

#[tokio::test]
async fn a_closed_registration_refuses_new_accounts() {
    let relay = FakeRelay::new();
    relay.set_registration_closed(true);
    assert_eq!(
        relay.create_account().await,
        Err(RelayError::RegistrationClosed)
    );
    relay.set_registration_closed(false);
    assert!(relay.create_account().await.is_ok());
}

// Blobs (Moony-relay DESIGN.md "Blobs", src/blob-api.ts).

const OTHER_VAULT_ID: &str = "EBESExQVFhcYGRobHB0eHw";

fn other_auth() -> VaultAuth {
    VaultAuth {
        vault_id: OTHER_VAULT_ID.into(),
        token: Zeroizing::new(TOKEN.into()),
    }
}

/// A body of `len` bytes, all `byte` (a sealed blob is at least `MIN_BLOB_SEALED` bytes).
fn sealed(byte: u8, len: usize) -> Vec<u8> {
    vec![byte; len]
}

fn blob_id(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

fn body_of(bytes: &[u8]) -> BlobBody {
    // Two pieces, so the fake joins what it is handed.
    let (a, b) = bytes.split_at(bytes.len() / 2);
    Box::new(vec![Ok(a.to_vec()), Ok(b.to_vec())].into_iter())
}

async fn put(
    relay: &FakeRelay,
    vault: &VaultAuth,
    id: uuid::Uuid,
    bytes: &[u8],
) -> RelayResult<()> {
    relay
        .put_blob(vault, id, bytes.len() as u64, body_of(bytes))
        .await
}

/// Collects what `get_blob` hands over.
#[derive(Default)]
struct Collect {
    begun: Option<u64>,
    bytes: Vec<u8>,
    pieces: usize,
}

impl BlobSink for Collect {
    fn begin(&mut self, sealed_len: u64) -> SinkFlow {
        assert!(self.begun.replace(sealed_len).is_none(), "begun twice");
        SinkFlow::Continue
    }

    fn piece(&mut self, bytes: &[u8]) -> SinkFlow {
        self.pieces += 1;
        self.bytes.extend_from_slice(bytes);
        SinkFlow::Continue
    }
}

async fn fetch(relay: &FakeRelay, vault: &VaultAuth, id: uuid::Uuid) -> RelayResult<Vec<u8>> {
    let mut sink = Collect::default();
    assert_eq!(
        relay.get_blob(vault, id, &mut sink).await?,
        Delivery::Complete
    );
    assert_eq!(sink.begun, Some(sink.bytes.len() as u64));
    Ok(sink.bytes)
}

#[tokio::test]
async fn the_fake_keeps_blobs_per_vault_and_checks_entitlement() {
    let (relay, account) = relay_with_vault().await;
    assert!(relay
        .create_vault(&account, &other_auth())
        .await
        .expect("vault"));
    // Bigger than one piece the fake hands out, so the sink sees several.
    let big: Vec<u8> = (0..150_000u32).map(|i| (i % 253) as u8).collect();
    put(&relay, &auth(), blob_id(2), &big).await.expect("put");
    let one = sealed(1, 100);
    put(&relay, &auth(), blob_id(1), &one).await.expect("put");
    put(&relay, &other_auth(), blob_id(3), b"three")
        .await
        .expect("put");

    assert_eq!(relay.blob_ids(VAULT_ID), vec![blob_id(1), blob_id(2)]);
    assert_eq!(relay.blob_ids(OTHER_VAULT_ID), vec![blob_id(3)]);
    assert_eq!(fetch(&relay, &auth(), blob_id(2)).await, Ok(big.clone()));
    let mut sink = Collect::default();
    relay
        .get_blob(&auth(), blob_id(2), &mut sink)
        .await
        .expect("get");
    assert!(sink.pieces > 1, "{} pieces", sink.pieces);
    assert_eq!(relay.head_blob(&auth(), blob_id(1)).await, Ok(Some(100)));
    // Another vault's blob is not this vault's.
    assert_eq!(relay.head_blob(&auth(), blob_id(3)).await, Ok(None));
    assert_eq!(
        fetch(&relay, &auth(), blob_id(3)).await,
        Err(RelayError::BlobNotFound)
    );

    // The listing: ascending by ID, `after` exclusive, cut at `limit`, the server's clock.
    let page = relay.list_blobs(&auth(), None, 1).await.expect("list");
    assert_eq!(
        page.blobs
            .iter()
            .map(|b| (b.id, b.size))
            .collect::<Vec<_>>(),
        vec![(blob_id(1), 100)]
    );
    assert!(page.more);
    assert!(page.server_now.is_some_and(|at| (at - now()).abs() < 60));
    assert!(page.blobs[0].created_at > 0);
    let page = relay
        .list_blobs(&auth(), Some(blob_id(1)), MAX_BLOB_PAGE)
        .await
        .expect("list");
    assert_eq!(
        page.blobs.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![blob_id(2)]
    );
    assert!(!page.more);
    // The client's own limit check, before anything is asked.
    for limit in [0, MAX_BLOB_PAGE + 1] {
        assert!(matches!(
            relay.list_blobs(&auth(), None, limit).await,
            Err(RelayError::Protocol(_))
        ));
    }
    // So is the cap on a body.
    assert_eq!(
        relay
            .put_blob(&auth(), blob_id(9), MAX_BLOB_SEALED + 1, body_of(b"x"))
            .await,
        Err(RelayError::Protocol("body too large".into()))
    );
    // An empty body and a body that is not its announced length.
    assert_eq!(
        relay.put_blob(&auth(), blob_id(9), 0, body_of(b"")).await,
        Err(RelayError::BadRequest("length_required".into()))
    );
    assert_eq!(
        relay
            .put_blob(&auth(), blob_id(9), 5, body_of(b"abc"))
            .await,
        Err(RelayError::BadRequest("length_required".into()))
    );
    assert_eq!(
        relay
            .put_blob(&auth(), blob_id(9), 2, body_of(b"abc"))
            .await,
        Err(RelayError::BadRequest("length_required".into()))
    );
    assert!(!relay.blob_ids(VAULT_ID).contains(&blob_id(9)));

    // A lapsed account still reads, but uploads nothing; deleting needs no payment.
    relay.set_paid(&account, false);
    assert_eq!(
        put(&relay, &auth(), blob_id(4), b"four").await,
        Err(RelayError::PaymentRequired)
    );
    assert_eq!(fetch(&relay, &auth(), blob_id(1)).await, Ok(one.clone()));
    assert_eq!(relay.head_blob(&auth(), blob_id(1)).await, Ok(Some(100)));
    assert_eq!(
        relay
            .list_blobs(&auth(), None, 10)
            .await
            .map(|p| p.blobs.len()),
        Ok(2)
    );
    relay
        .delete_blob(&auth(), blob_id(1))
        .await
        .expect("delete");
    // Deleting a blob that is not there succeeds too.
    relay
        .delete_blob(&auth(), blob_id(1))
        .await
        .expect("delete again");
    assert_eq!(relay.blob_ids(VAULT_ID), vec![blob_id(2)]);

    // Past the read grace reads stop too.
    relay.set_paid_until(&account, now() - 31 * 86_400);
    assert_eq!(
        relay.head_blob(&auth(), blob_id(2)).await,
        Err(RelayError::PaymentRequired)
    );
    assert_eq!(
        fetch(&relay, &auth(), blob_id(2)).await,
        Err(RelayError::PaymentRequired)
    );
    assert!(matches!(
        relay.list_blobs(&auth(), None, 10).await,
        Err(RelayError::PaymentRequired)
    ));

    // A wrong token, and a vault the server does not know.
    let wrong = VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new("x".repeat(43)),
    };
    assert_eq!(
        relay.head_blob(&wrong, blob_id(2)).await,
        Err(RelayError::Unauthorized)
    );
    assert_eq!(
        relay.delete_blob(&wrong, blob_id(2)).await,
        Err(RelayError::Unauthorized)
    );
    relay.wipe_vault(OTHER_VAULT_ID);
    assert_eq!(
        relay.head_blob(&other_auth(), blob_id(3)).await,
        Err(RelayError::VaultNotFound)
    );
    assert!(relay.blob_ids(OTHER_VAULT_ID).is_empty());
}

#[tokio::test]
async fn the_fake_answers_the_quota_and_no_room() {
    let (relay, _) = relay_with_vault().await;
    relay.set_blob_quota(10);
    put(&relay, &auth(), blob_id(1), b"123456")
        .await
        .expect("put");
    assert_eq!(
        put(&relay, &auth(), blob_id(2), b"12345").await,
        Err(RelayError::QuotaExceeded)
    );
    put(&relay, &auth(), blob_id(2), b"1234")
        .await
        .expect("fits exactly");
    // Deleting frees the quota.
    relay
        .delete_blob(&auth(), blob_id(1))
        .await
        .expect("delete");
    put(&relay, &auth(), blob_id(3), b"123456")
        .await
        .expect("put");

    relay.set_blob_quota(100);
    relay.set_no_room(true);
    assert_eq!(
        put(&relay, &auth(), blob_id(4), b"1").await,
        Err(RelayError::InsufficientStorage)
    );
    // A snapshot upload answers it too.
    assert_eq!(
        relay.put_snapshot(&auth(), 0, vec![1]).await,
        Err(RelayError::InsufficientStorage)
    );
    // Reads and an existing blob's upload are not refused for room.
    assert_eq!(
        fetch(&relay, &auth(), blob_id(2)).await,
        Ok(b"1234".to_vec())
    );
    put(&relay, &auth(), blob_id(2), b"1234")
        .await
        .expect("already there");
    relay.set_no_room(false);
    put(&relay, &auth(), blob_id(4), b"1")
        .await
        .expect("room again");

    // A sticky failure holds until it is cleared; a one-off failure comes first.
    relay.set_failing(Route::GetBlob, Some(RelayError::Server(503)));
    relay.fail_next(Route::GetBlob, RelayError::RateLimited);
    assert_eq!(
        fetch(&relay, &auth(), blob_id(4)).await,
        Err(RelayError::RateLimited)
    );
    for _ in 0..3 {
        assert_eq!(
            fetch(&relay, &auth(), blob_id(4)).await,
            Err(RelayError::Server(503))
        );
    }
    relay.set_failing(Route::GetBlob, None);
    assert_eq!(fetch(&relay, &auth(), blob_id(4)).await, Ok(b"1".to_vec()));
}

#[tokio::test]
async fn a_put_of_an_existing_blob_succeeds_without_storing_again() {
    let (relay, _) = relay_with_vault().await;
    put(&relay, &auth(), blob_id(1), b"first")
        .await
        .expect("put");
    put(&relay, &auth(), blob_id(1), b"other")
        .await
        .expect("again");
    assert_eq!(relay.blob_puts(), 2);
    assert_eq!(
        fetch(&relay, &auth(), blob_id(1)).await,
        Ok(b"first".to_vec())
    );
    assert_eq!(relay.blob_ids(VAULT_ID), vec![blob_id(1)]);
}

#[tokio::test]
async fn a_tampered_or_truncated_blob_is_served_as_changed() {
    let (relay, _) = relay_with_vault().await;
    let original: Vec<u8> = (0..100u8).collect();
    for n in 1..=3 {
        put(&relay, &auth(), blob_id(n), &original)
            .await
            .expect("put");
    }

    relay.tamper_blob(VAULT_ID, blob_id(1));
    let tampered = fetch(&relay, &auth(), blob_id(1)).await.expect("get");
    assert_eq!(tampered.len(), original.len());
    assert_eq!(
        tampered
            .iter()
            .zip(&original)
            .filter(|(a, b)| a != b)
            .count(),
        1
    );

    relay.truncate_blob(VAULT_ID, blob_id(2), 80);
    assert_eq!(
        fetch(&relay, &auth(), blob_id(2)).await,
        Ok(original[..80].to_vec())
    );
    assert_eq!(relay.head_blob(&auth(), blob_id(2)).await, Ok(Some(80)));
    // Cut below any sealed blob's size: the client refuses the size, as `HttpRelay` does.
    relay.truncate_blob(VAULT_ID, blob_id(2), 4);
    assert_eq!(
        relay.head_blob(&auth(), blob_id(2)).await,
        Err(RelayError::Protocol("unexpected answer".into()))
    );
    assert_eq!(
        relay.list_blobs(&auth(), None, 10).await,
        Err(RelayError::Protocol("unexpected answer".into()))
    );

    relay.lose_blob(VAULT_ID, blob_id(3));
    assert_eq!(
        fetch(&relay, &auth(), blob_id(3)).await,
        Err(RelayError::BlobNotFound)
    );
    assert_eq!(relay.head_blob(&auth(), blob_id(3)).await, Ok(None));
    assert_eq!(relay.blob_ids(VAULT_ID), vec![blob_id(1), blob_id(2)]);
}

#[tokio::test]
async fn the_fake_reports_the_ages_it_is_told() {
    let (relay, _) = relay_with_vault().await;
    put(&relay, &auth(), blob_id(1), &sealed(1, 100))
        .await
        .expect("put");
    relay.set_blob_created_at(VAULT_ID, blob_id(1), 1_000);
    relay.set_server_now(Some(2_000));
    let page = relay.list_blobs(&auth(), None, 10).await.expect("list");
    assert_eq!(page.blobs[0].created_at, 1_000);
    assert_eq!(page.server_now, Some(2_000));
    // A server that sends no clock.
    relay.set_server_now(None);
    let page = relay.list_blobs(&auth(), None, 10).await.expect("list");
    assert_eq!(page.server_now, None);
    // A clock or a creation time before 1970: refused, as `HttpRelay` refuses it.
    relay.set_server_now(Some(-1));
    assert_eq!(
        relay.list_blobs(&auth(), None, 10).await,
        Err(RelayError::Protocol("unexpected answer".into()))
    );
    relay.set_server_now(Some(2_000));
    relay.set_blob_created_at(VAULT_ID, blob_id(1), -1);
    assert_eq!(
        relay.list_blobs(&auth(), None, 10).await,
        Err(RelayError::Protocol("unexpected answer".into()))
    );
}

const NEW_TOKEN: &str = "bmV3LXRva2VuLW5ldy10b2tlbi1uZXctdG9rZW4tbmV";
const THIRD_TOKEN: &str = "dGhpcmQtdG9rZW4tdGhpcmQtdG9rZW4tdGhpcmQtdG9";
const DAY: i64 = 86_400;

fn new_auth() -> VaultAuth {
    VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new(NEW_TOKEN.into()),
    }
}

/// A vault with two ops, a snapshot and a blob, rotated once to `NEW_TOKEN` with bundle `[1; 9]`.
async fn rotated_vault() -> (FakeRelay, String, Uuid) {
    let (relay, account) = relay_with_vault().await;
    relay
        .push(&auth(), &[op("a", 4), op("b", 4)])
        .await
        .expect("push");
    relay
        .put_snapshot(&auth(), 1, vec![1; 8])
        .await
        .expect("snapshot");
    let blob = Uuid::new_v4();
    let sealed = crate::services::sync::relay::MIN_BLOB_SEALED;
    relay
        .put_blob(
            &auth(),
            blob,
            sealed,
            Box::new(std::iter::once(Ok(vec![5u8; sealed as usize]))),
        )
        .await
        .expect("blob");
    let rotated = relay
        .rotate_token(&auth(), NEW_TOKEN, &[1; 9])
        .await
        .expect("rotate");
    assert_eq!(
        rotated,
        Rotated {
            rotation: 1,
            head: 2
        }
    );
    (relay, account, blob)
}

#[tokio::test]
async fn a_rotation_answers_its_number_and_the_head_and_counts_in_vault_info() {
    let (relay, _) = relay_with_vault().await;
    relay.push(&auth(), &[op("a", 4)]).await.expect("push");
    assert_eq!(relay.vault_info(&auth()).await.expect("info").rotation, 0);
    relay.push(&auth(), &[op("b", 4)]).await.expect("push");
    let first = relay
        .rotate_token(&auth(), NEW_TOKEN, &[1])
        .await
        .expect("first");
    // The head recorded per bundle is the head at the rotation.
    assert_eq!(
        first,
        Rotated {
            rotation: 1,
            head: 2
        }
    );
    relay
        .push(&new_auth(), &[op("c", 4)])
        .await
        .expect("push with the new token");
    let second = relay
        .rotate_token(&new_auth(), THIRD_TOKEN, &[2])
        .await
        .expect("second");
    assert_eq!(
        second,
        Rotated {
            rotation: 2,
            head: 3
        }
    );
    let third = VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new(THIRD_TOKEN.into()),
    };
    assert_eq!(relay.vault_info(&third).await.expect("info").rotation, 2);
    let page = relay.rekeys(&third, 0, 8).await.expect("rekeys");
    let heads: Vec<(u32, u64)> = page.rekeys.iter().map(|r| (r.rotation, r.head)).collect();
    assert_eq!(heads, vec![(1, 2), (2, 3)]);
    assert_eq!(page.rekeys[0].bundle, vec![1]);
}

#[tokio::test]
async fn a_retired_token_reads_only_the_vault_info_and_the_key_changes() {
    let (relay, _, blob) = rotated_vault().await;
    // What it may look up (protocol 3): the vault's info and the key changes.
    assert_eq!(relay.vault_info(&auth()).await.expect("info").rotation, 1);
    let page = relay.rekeys(&auth(), 0, 8).await.expect("rekeys");
    assert_eq!(page.rekeys.len(), 1);
    // Every other read is refused: pull, snapshot, blob get/head/list.
    let read_only = Err(RelayError::TokenReadOnly);
    assert_eq!(relay.pull(&auth(), 0, 10).await.map(|_| ()), read_only);
    assert_eq!(relay.get_snapshot(&auth()).await.map(|_| ()), read_only);
    assert_eq!(relay.head_blob(&auth(), blob).await.map(|_| ()), read_only);
    assert_eq!(
        relay.list_blobs(&auth(), None, 10).await.map(|_| ()),
        read_only
    );
    struct Count(u64);
    impl BlobSink for Count {
        fn begin(&mut self, _: u64) -> SinkFlow {
            SinkFlow::Continue
        }
        fn piece(&mut self, bytes: &[u8]) -> SinkFlow {
            self.0 += bytes.len() as u64;
            SinkFlow::Continue
        }
    }
    let mut sink = Count(0);
    assert_eq!(
        relay.get_blob(&auth(), blob, &mut sink).await.map(|_| ()),
        read_only
    );
    assert_eq!(sink.0, 0);
    // The current token still reads all of it.
    assert_eq!(
        relay
            .pull(&new_auth(), 0, 10)
            .await
            .expect("pull")
            .ops
            .len(),
        2
    );
    assert_eq!(
        relay.get_snapshot(&new_auth()).await.expect("snapshot").seq,
        1
    );
    // A relay that hides a key change serves a retired token's reads too (its own knob).
    relay.set_accept_retired_reads(true);
    assert_eq!(relay.pull(&auth(), 0, 10).await.expect("pull").ops.len(), 2);
    relay.set_accept_retired_reads(false);

    // Writes: push, snapshot put, blob put and delete, vault delete, a second rotation.
    let read_only = Err(RelayError::TokenReadOnly);
    assert_eq!(
        relay.push(&auth(), &[op("c", 4)]).await.map(|_| ()),
        read_only
    );
    assert_eq!(relay.put_snapshot(&auth(), 2, vec![2; 8]).await, read_only);
    let sealed = crate::services::sync::relay::MIN_BLOB_SEALED;
    assert_eq!(
        relay
            .put_blob(
                &auth(),
                Uuid::new_v4(),
                sealed,
                Box::new(std::iter::once(Ok(vec![5u8; sealed as usize]))),
            )
            .await,
        read_only
    );
    assert_eq!(relay.delete_blob(&auth(), blob).await, read_only);
    assert_eq!(relay.delete_vault(&auth()).await, read_only);
    assert_eq!(
        relay
            .rotate_token(&auth(), THIRD_TOKEN, &[2])
            .await
            .map(|_| ()),
        read_only
    );
    // Nothing of that landed.
    assert_eq!(
        relay.vault_info(&new_auth()).await.expect("info").rotation,
        1
    );
    assert!(relay
        .head_blob(&new_auth(), blob)
        .await
        .expect("head")
        .is_some());
}

#[tokio::test]
async fn a_rotation_needs_no_subscription_and_refuses_a_remembered_token() {
    let (relay, account) = relay_with_vault().await;
    relay.set_paid(&account, false);
    relay
        .rotate_token(&auth(), NEW_TOKEN, &[1])
        .await
        .expect("an unpaid vault still rotates");
    // The retired token can never come back while it is remembered; neither can the current one.
    for token in [TOKEN, NEW_TOKEN] {
        let error = relay
            .rotate_token(&new_auth(), token, &[1])
            .await
            .expect_err("known token");
        assert_eq!(error, RelayError::BadRequest("invalid_token".into()));
    }
    // A stranger's token is unauthorized, a missing vault is not found.
    let stranger = vault_with(VAULT_ID, THIRD_TOKEN);
    assert_eq!(
        relay.rotate_token(&stranger, NEW_TOKEN, &[1]).await,
        Err(RelayError::Unauthorized)
    );
    assert_eq!(
        relay
            .rotate_token(&vault_with(&vault_id(7), TOKEN), THIRD_TOKEN, &[1])
            .await,
        Err(RelayError::VaultNotFound)
    );
}

#[tokio::test]
async fn bad_arguments_are_refused_like_the_http_client_does() {
    let (relay, _) = relay_with_vault().await;
    assert_eq!(
        relay.rotate_token(&auth(), "short", &[1]).await,
        Err(RelayError::BadRequest("invalid_token".into()))
    );
    assert_eq!(
        relay.rotate_token(&auth(), NEW_TOKEN, &[]).await,
        Err(RelayError::BadRequest("invalid_rekey".into()))
    );
    assert_eq!(
        relay
            .rotate_token(&auth(), NEW_TOKEN, &vec![0; MAX_REKEY_BUNDLE + 1])
            .await,
        Err(RelayError::BadRequest("rekey_too_large".into()))
    );
    assert_eq!(
        relay.rekeys(&auth(), 0, 0).await,
        Err(RelayError::BadRequest("invalid_limit".into()))
    );
}

#[tokio::test]
async fn the_old_token_and_its_bundle_expire_after_thirty_days() {
    let (relay, _, _) = rotated_vault().await;
    relay.advance_clock(30 * DAY - 60);
    assert!(relay.vault_info(&auth()).await.is_ok());
    assert_eq!(
        relay
            .rekeys(&auth(), 0, 8)
            .await
            .expect("rekeys")
            .rekeys
            .len(),
        1
    );
    relay.advance_clock(120);
    assert_eq!(
        relay.vault_info(&auth()).await,
        Err(RelayError::Unauthorized)
    );
    assert_eq!(
        relay.rekeys(&auth(), 0, 8).await,
        Err(RelayError::Unauthorized)
    );
    // The bundle is gone for the current token too.
    assert!(relay
        .rekeys(&new_auth(), 0, 8)
        .await
        .expect("rekeys")
        .rekeys
        .is_empty());
    assert_eq!(relay.stored_rekey(VAULT_ID, 1), None);
}

#[tokio::test]
async fn the_thirty_third_rotation_within_thirty_days_is_refused() {
    let (relay, _) = relay_with_vault().await;
    let token_n = |n: u32| format!("{:0>43}", format!("t{n}"));
    let mut current = auth();
    for n in 1..=32u32 {
        let next = token_n(n);
        relay
            .rotate_token(&current, &next, &[n as u8])
            .await
            .unwrap_or_else(|e| panic!("rotation {n}: {e:?}"));
        current = vault_with(VAULT_ID, &next);
    }
    assert_eq!(
        relay
            .rotate_token(&current, &token_n(33), &[33])
            .await
            .map(|_| ()),
        Err(RelayError::TooManyRotations)
    );
    // A bundle older than 30 days frees its slot.
    relay.advance_clock(30 * DAY + 60);
    relay
        .rotate_token(&current, &token_n(33), &[33])
        .await
        .expect("a slot is free again");
}

#[tokio::test]
async fn rekey_pages_are_cut_by_limit_and_bytes_and_start_after_a_rotation() {
    let (relay, _) = relay_with_vault().await;
    let mut current = auth();
    for n in 1..=5u32 {
        let next = format!("{:0>43}", format!("p{n}"));
        relay
            .rotate_token(&current, &next, &[n as u8; 100])
            .await
            .expect("rotate");
        current = vault_with(VAULT_ID, &next);
    }
    let page = relay.rekeys(&current, 0, 2).await.expect("page");
    assert_eq!(
        (
            page.rekeys.iter().map(|r| r.rotation).collect::<Vec<_>>(),
            page.more
        ),
        (vec![1, 2], true)
    );
    let page = relay.rekeys(&current, 2, 8).await.expect("page");
    assert_eq!(
        (
            page.rekeys.iter().map(|r| r.rotation).collect::<Vec<_>>(),
            page.more
        ),
        (vec![3, 4, 5], false)
    );
    assert!(relay
        .rekeys(&current, 5, 8)
        .await
        .expect("page")
        .rekeys
        .is_empty());
    // The limit is clamped to 32.
    assert_eq!(
        relay
            .rekeys(&current, 0, 1000)
            .await
            .expect("page")
            .rekeys
            .len(),
        5
    );

    // Big bundles: a page stops at 4 MiB but always carries at least one.
    let (relay, _) = relay_with_vault().await;
    let mut current = auth();
    for n in 1..=17u32 {
        let next = format!("{:0>43}", format!("q{n}"));
        relay
            .rotate_token(&current, &next, &vec![0u8; MAX_REKEY_BUNDLE])
            .await
            .expect("rotate");
        current = vault_with(VAULT_ID, &next);
    }
    let page = relay.rekeys(&current, 0, 32).await.expect("page");
    assert_eq!(page.rekeys.len(), 16);
    assert!(page.more);
    crate::services::sync::relay::check_rekey_page(&page, 0, 32).expect("a valid page");
    let rest = relay.rekeys(&current, 16, 32).await.expect("page");
    assert_eq!((rest.rekeys.len(), rest.more), (1, false));
}

#[tokio::test]
async fn a_lost_rotation_answer_keeps_the_rotation() {
    let (relay, _) = relay_with_vault().await;
    relay.lose_next_answer(Route::RotateToken);
    assert_eq!(
        relay.rotate_token(&auth(), NEW_TOKEN, &[1]).await,
        Err(RelayError::Server(503))
    );
    // It landed: the new token works, the old one is read-only.
    assert_eq!(
        relay.vault_info(&new_auth()).await.expect("info").rotation,
        1
    );
    assert_eq!(
        relay.push(&auth(), &[op("a", 4)]).await.map(|_| ()),
        Err(RelayError::TokenReadOnly)
    );
}
