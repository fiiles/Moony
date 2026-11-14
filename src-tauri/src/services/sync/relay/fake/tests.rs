//! Tests of `FakeRelay`: the relay semantics the engine relies on, and its fault switches.

use super::*;
use crate::services::sync::relay::{Mailbox, MAX_SLOT_BYTES};
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
    assert_eq!(info.protocol, 2);
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
