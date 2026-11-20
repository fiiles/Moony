//! A sync server that lies about a key change (phase 5 As built 62–69): the head it reports is
//! only provisional until the remover's signed head confirms it; a lagging device learns of a key
//! change before it pulls; a key the server has no token for, or one signed by a device this one
//! knows was removed, is not taken; the bundle pager ends; and the remover's own change is
//! committed as its own whatever the server answered on the way.

use std::sync::Arc;

use zeroize::Zeroizing;

use super::pull::write_and_seal;
use super::rekey_fixture::*;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::envelope::vault_token;
use crate::services::sync::error::{SyncError, Transient};
use crate::services::sync::identity::{self, trust};
use crate::services::sync::rekey::bundle::{self, BundleInput, OldKey, Reason, Recipient};
use crate::services::sync::rekey::head;
use crate::services::sync::rekey::pickup::{self, Pickup};
use crate::services::sync::rekey::rotate::{self, Outcome};
use crate::services::sync::rekey::{self, RemovalNotDone};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{Rekey, RekeyPage, RelayError};

fn vault_b64(three: &Three) -> String {
    three.vault.creds.vault_id.to_b64()
}

/// Where epoch 1 started, as the remover A recorded it.
fn true_head(three: &Three) -> u64 {
    *state::epochs(&three.vault.a.lock())
        .expect("epochs")
        .get(&1)
        .expect("A's head")
}

fn last_seq(three: &Three) -> u64 {
    three.vault.relay.ops().last().map_or(0, |op| op.seq)
}

/// What `device` holds in `sync_unsent`, by sequence.
fn unsent(conn: &rusqlite::Connection) -> Vec<Vec<u8>> {
    conn.prepare("SELECT envelope FROM sync_unsent ORDER BY seq")
        .expect("unsent")
        .query_map([], |r| r.get(0))
        .expect("rows")
        .map(|row| row.expect("row"))
        .collect()
}

/// Pushes what A holds unsent (its signed head, sealed under the new key) with its new token,
/// without a round (whose pull might stop first).
async fn push_a_unsent(three: &Three) {
    let (creds, envelopes) = {
        let a = three.vault.a.lock();
        (
            keys::credentials(&a).expect("creds").expect("bound"),
            unsent(&a),
        )
    };
    assert!(!envelopes.is_empty(), "A holds its signed head");
    push(&three.vault.relay, &creds, &envelopes).await;
}

/// C, removed but still holding the old key and token, writes `id` and a colluding server takes
/// it.
async fn forge_from_c(three: &Three, id: &str) {
    three.vault.relay.set_accept_retired_writes(true);
    let forged = write_and_seal(&three.vault, &three.c, id);
    push(&three.vault.relay, &three.vault.creds, &[forged]).await;
}

/// A key change from epoch 0 to 1 signed by `signer` (with `signer_cert`), sealing `key` to `to`.
fn hand_bundle(
    three: &Three,
    signer: &identity::SignSeed,
    signer_cert: &identity::Certificate,
    to: &[DeviceId],
    key: &Zeroizing<[u8; 32]>,
) -> Vec<u8> {
    let a = three.vault.a.lock();
    let creds = keys::credentials(&a).expect("creds").expect("bound");
    let recipients: Vec<Recipient> = to
        .iter()
        .map(|device| {
            let entry = trust::entry(&a, *device).expect("entry").expect("pinned");
            Recipient {
                box_pub: entry.box_pub,
                cert: entry.cert,
            }
        })
        .collect();
    let older = [OldKey {
        epoch: 0,
        key: Zeroizing::new(*creds.keys.get(0).expect("epoch 0")),
        head: Some(0),
    }];
    bundle::build(
        &BundleInput {
            vault_id: creds.vault_id.0,
            from_epoch: 0,
            to_epoch: 1,
            new_key: key,
            recipients: &recipients,
            notices: &[],
            signer,
            signer_cert,
            at: NOW,
            reason: Reason::Rotate,
            removed: &[],
            keys: &older,
            kit: None,
        },
        &mut bundle::os_fill,
    )
    .expect("bundle")
}

fn own_signer(
    conn: &rusqlite::Connection,
    device: DeviceId,
) -> (identity::SignSeed, identity::Certificate) {
    (
        identity::own_seed(conn).expect("seed").expect("own key"),
        trust::entry(conn, device)
            .expect("entry")
            .expect("pinned")
            .cert,
    )
}

/// The attack of the review (finding 1): C, removed, keeps the old key; the server takes C's
/// pushes after the true rotation head and tells B a head above them. B takes the server's head
/// only as provisional, finds A's signed head past it, sees they differ and applies nothing.
#[tokio::test]
async fn a_head_the_server_raised_for_one_device_is_refused_and_nothing_past_it_applies() {
    let three = three().await;
    remove(&three, A, &[C]).await.expect("removed");
    let head = true_head(&three);
    forge_from_c(&three, "c-forged").await;
    push_a_unsent(&three).await;
    three
        .vault
        .relay
        .set_rekey_head(&vault_b64(&three), 1, head + 1);

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    {
        let b = three.vault.b.lock();
        assert!(!has_institution(&b, "c-forged"), "nothing past the head");
        assert!(cursor(&b) <= head);
    }
    // It stays refused in the next rounds (the signed head was already read).
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(report.problem, Some(SyncProblem::KeyChangeUnverified));
    assert!(!has_institution(&three.vault.b.lock(), "c-forged"));

    // A later key change: the refutation stays and still holds the pull.
    new_kit(&three, A).await.expect("second change");
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(report.problem, Some(SyncProblem::KeyChangeUnverified));
    let b = three.vault.b.lock();
    // A's second bundle names the true start of epoch 1 (signed), which contradicts the
    // server's: not taken either.
    assert_eq!(key_epoch(&b), 1);
    let entries = head::unverified_epochs(&b).expect("unverified");
    assert!(
        entries.iter().any(|e| e.epoch == 1 && e.refuted),
        "{entries:?}"
    );
    assert!(head::awaits_head(&b, 0, head + 1).expect("awaits"));
    assert!(!has_institution(&b, "c-forged"));
}

/// A lagging device with old-key changes still to pull past its place waits (visibly, as
/// "picking up the new key") until the remover's signed head is on the server, then goes on.
#[tokio::test]
async fn a_device_behind_waits_for_the_signed_head_and_then_goes_on() {
    let three = three().await;
    institution(&three.vault.a.lock(), "a-before");
    round_of(&three.vault, &three.vault.a).await;
    new_kit(&three, A).await.expect("changed");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeWaiting),
        "{report:?}"
    );
    assert_eq!(key_epoch(&three.vault.b.lock()), 1, "the key was picked up");
    assert!(!has_institution(&three.vault.b.lock(), "a-before"));
    assert_eq!(
        head::unverified_epochs(&three.vault.b.lock())
            .expect("unverified")
            .len(),
        1
    );

    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "A sends its signed head: {report:?}");
    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    let b = three.vault.b.lock();
    assert!(has_institution(&b, "a-before"));
    assert!(head::unverified_epochs(&b).expect("unverified").is_empty());
    assert_eq!(rekey::key_change_overrun(&b).expect("overrun"), None);
}

/// A head lowered below one of B's own envelopes the server took before the key change: B never
/// seals it again (it would be sent twice), and pushes nothing.
#[tokio::test]
async fn a_head_the_server_lowered_seals_nothing_again_and_sends_nothing_twice() {
    let three = three().await;
    let b1 = write_and_seal(&three.vault, &three.vault.b, "b1");
    push(&three.vault.relay, &three.vault.creds, &[b1]).await;
    let b1_at = last_seq(&three);
    write_and_seal(&three.vault, &three.vault.b, "b2");
    new_kit(&three, A).await.expect("changed");
    round_of(&three.vault, &three.vault.a).await;
    three
        .vault
        .relay
        .set_rekey_head(&vault_b64(&three), 1, b1_at - 1);
    pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");
    let ops_before = three.vault.relay.ops().len();

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    assert_eq!(three.vault.relay.ops().len(), ops_before, "nothing pushed");
    let epochs: Vec<u32> = unsent_epochs(&three.vault.b.lock())
        .into_iter()
        .map(|(_, epoch)| epoch)
        .collect();
    assert_eq!(epochs, vec![0, 0], "nothing sealed again");
}

/// Detect after the fact (review finding 1d): the server hid the key change from B and served C's
/// forged change past the head; when B learns of the key change and verifies the head, it says so
/// (nothing is undone).
#[tokio::test]
async fn changes_applied_past_a_key_change_the_server_hid_are_reported() {
    let three = three().await;
    remove(&three, A, &[C]).await.expect("removed");
    let head = true_head(&three);
    round_of(&three.vault, &three.vault.a).await;
    let a_sent = last_seq(&three);
    assert!(a_sent > head, "A's signed head and its change");
    // Hidden: no rotation in the vault's info, nothing of A's under the new key served.
    for seq in head + 1..=a_sent {
        three.vault.relay.drop_op(seq);
    }
    three.vault.relay.set_rotation(&vault_b64(&three), 0);
    forge_from_c(&three, "c-forged").await;
    round_of(&three.vault, &three.vault.b).await;
    assert!(has_institution(&three.vault.b.lock(), "c-forged"));

    for seq in head + 1..=a_sent {
        three.vault.relay.undrop_op(seq);
    }
    three.vault.relay.set_rotation(&vault_b64(&three), 1);
    round_of(&three.vault, &three.vault.b).await;

    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 1);
    let overrun = rekey::key_change_overrun(&b)
        .expect("overrun")
        .expect("reported");
    assert_eq!((overrun.epoch, overrun.head), (1, head));
    assert!(overrun.through > head, "{overrun:?}");
    assert!(head::unverified_epochs(&b).expect("unverified").is_empty());
}

/// Review finding 1a: a lagging device reads the vault's info before it pulls, so it learns of
/// the key change before the server can feed it old-key changes past the head.
#[tokio::test]
async fn a_lagging_device_learns_of_the_key_change_before_it_pulls() {
    let three = three().await;
    remove(&three, A, &[C]).await.expect("removed");
    forge_from_c(&three, "c-forged").await;
    push_a_unsent(&three).await;

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::InvalidChain),
        "{report:?}"
    );
    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 1);
    assert!(!has_institution(&b, "c-forged"));
    assert!(head::unverified_epochs(&b).expect("unverified").is_empty());
}

/// Review finding 3: a bundle signed by a trusted device whose key the server has no token for
/// (it never rotated to it) is not taken.
#[tokio::test]
async fn a_key_change_whose_key_the_server_does_not_know_is_not_taken() {
    let three = three().await;
    let (seed, cert) = own_signer(&three.vault.a.lock(), A);
    let bytes = hand_bundle(&three, &seed, &cert, &[A, B], &Zeroizing::new([0x77; 32]));
    three.vault.relay.insert_rekey(&vault_b64(&three), &bytes);

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 0);
    assert_eq!(state::rekey_seen(&b).expect("seen"), 0);
}

/// Review finding 3: C, whose removal record A wrote with no key change (a dishonest server
/// holding A's key change back), still holds the token and changes the key itself before B's
/// store has caught up with the removal; B, whose list already carries the verified removal,
/// does not take C's key.
#[tokio::test]
async fn a_key_change_signed_by_a_device_known_to_be_removed_is_not_taken() {
    let three = three().await;
    record_removal(&three.vault.a.lock(), A, C);
    round_of(&three.vault, &three.vault.a).await;
    round_of(&three.vault, &three.vault.b).await;
    assert!(
        three
            .vault
            .b
            .lock()
            .query_row(
                "SELECT removal_record IS NOT NULL FROM sync_devices WHERE id = ?1",
                [C.to_hex()],
                |r| r.get::<_, bool>(0),
            )
            .expect("row"),
        "B's list carries the removal"
    );
    let key = Zeroizing::new([0x66; 32]);
    let (seed, cert) = own_signer(&three.c.lock(), C);
    let bytes = hand_bundle(&three, &seed, &cert, &[B, C], &key);
    three
        .vault
        .relay
        .rotate_token(
            &three.vault.creds.auth(),
            &vault_token(&key, &three.vault.creds.vault_id.0),
            &bytes,
        )
        .await
        .expect("C rotates");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    assert_eq!(key_epoch(&three.vault.b.lock()), 0);
}

/// Review finding 4: a server that answers the same page with "more" for ever, or rotations that
/// do not increase, ends the walk with an error.
#[tokio::test]
async fn a_bundle_pager_that_never_ends_or_goes_back_is_refused() {
    let three = three().await;
    new_kit(&three, A).await.expect("changed");
    let real = three
        .vault
        .relay
        .stored_rekey(&vault_b64(&three), 1)
        .expect("stored");
    let rekey = {
        let real = real.clone();
        move |rotation: u32| Rekey {
            rotation,
            head: 0,
            at: NOW,
            bundle: real.clone(),
        }
    };

    // The same page again and again.
    let page = RekeyPage {
        rekeys: vec![rekey(1)],
        more: true,
    };
    three
        .vault
        .relay
        .set_rekeys_answer(Some(Arc::new(move |_| page.clone())));
    let walked = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        pickup::pick_up(&three.vault.relay, &three.vault.a, None, NOW),
    )
    .await
    .expect("the walk ends");
    assert!(
        matches!(walked, Err(SyncError::Transient(Transient::Relay))),
        "{walked:?}"
    );

    // Rotations that go back.
    let back = RekeyPage {
        rekeys: vec![rekey(3), rekey(2)],
        more: false,
    };
    three
        .vault
        .relay
        .set_rekeys_answer(Some(Arc::new(move |_| back.clone())));
    let walked = pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW).await;
    assert!(
        matches!(walked, Err(SyncError::Transient(Transient::Relay))),
        "{walked:?}"
    );
    assert_eq!(key_epoch(&three.vault.b.lock()), 0, "nothing taken");

    // Ever new rotations, each passed over: the walk stops at its cap.
    let rekey_for = move |after: u32| RekeyPage {
        rekeys: vec![Rekey {
            rotation: after + 1,
            head: 0,
            at: NOW,
            bundle: real.clone(),
        }],
        more: true,
    };
    three
        .vault
        .relay
        .set_rekeys_answer(Some(Arc::new(rekey_for)));
    let walked = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        pickup::pick_up(&three.vault.relay, &three.vault.a, None, NOW),
    )
    .await
    .expect("the walk ends");
    assert!(
        matches!(walked, Err(SyncError::Transient(Transient::Relay))),
        "{walked:?}"
    );
}

/// Review finding 4: a bundle the header passes over (an epoch this device holds) is checked with
/// the key it holds for that epoch; one that does not open does not move `rekey_seen` past the
/// real ones, and is reported.
#[tokio::test]
async fn a_bundle_passed_over_that_does_not_open_does_not_move_what_was_seen() {
    let three = three().await;
    let (seed, cert) = own_signer(&three.vault.a.lock(), A);
    // To epoch 1 under a key nobody holds; B is at epoch 0 then 1 after the real change.
    let junk = hand_bundle(&three, &seed, &cert, &[A, B], &Zeroizing::new([0x55; 32]));
    new_kit(&three, A).await.expect("changed");
    round_of(&three.vault, &three.vault.a).await;
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(key_epoch(&three.vault.b.lock()), 1);
    let seen = state::rekey_seen(&three.vault.b.lock()).expect("seen");
    let page = RekeyPage {
        rekeys: vec![Rekey {
            rotation: 1000,
            head: 0,
            at: NOW,
            bundle: junk,
        }],
        more: false,
    };
    three
        .vault
        .relay
        .set_rekeys_answer(Some(Arc::new(move |_| page.clone())));

    let picked = pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");

    assert_eq!(picked, Pickup::Unverified);
    assert_eq!(
        state::rekey_seen(&three.vault.b.lock()).expect("seen"),
        seen
    );
}

/// Review finding 5: the change landed, the new token was answered `401` all the same, the second
/// call met the retired token: the bundle on the server is this device's own, so it commits as
/// its own (removal records, the signed head), not as another device's change.
#[tokio::test]
async fn a_change_that_landed_although_its_token_was_refused_commits_as_this_devices_own() {
    let three = three().await;
    three.vault.relay.lose_next_answer(Route::RotateToken);
    let outcome = remove(&three, A, &[C]).await.expect("pending");
    assert_eq!(outcome, Outcome::Pending { kit: None });
    three
        .vault
        .relay
        .fail_next(Route::VaultInfo, RelayError::Unauthorized);

    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert_eq!(
        resolved,
        Some(Outcome::Changed {
            epoch: 1,
            removed: vec![C],
            kit: None
        })
    );
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(key_epoch(&a), 1);
    let (record, removed): (Option<String>, i64) = a
        .query_row(
            "SELECT removal_record, removed FROM sync_devices WHERE id = ?1",
            [C.to_hex()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row");
    assert!(record.is_some() && removed == 1);
    assert_eq!(
        trust::entry(&a, C).expect("entry").map(|e| e.state),
        Some(trust::TrustState::Removed)
    );
    assert_eq!(
        unsent_epochs(&a),
        vec![(unsent_first(&a), 1)],
        "the signed head"
    );
    assert_eq!(rekey::removal_not_done(&a).expect("notice"), None);
}

fn unsent_first(conn: &rusqlite::Connection) -> i64 {
    count(conn, "SELECT MIN(seq) FROM sync_unsent")
}

/// Review finding 5 (crash): the pickup after a lost race fails; the change stays pending, and
/// the next resolution finishes it as a lost race, with the notice.
#[tokio::test]
async fn a_pickup_that_fails_after_a_lost_race_keeps_the_change_until_the_next_resolution() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));
    remove(&three, A, &[C]).await.expect("pending");
    new_kit(&three, B).await.expect("B changes the key");
    three
        .vault
        .relay
        .fail_next(Route::Rekeys, RelayError::Network("offline".into()));

    let failed = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW).await;

    assert!(failed.is_err(), "{failed:?}");
    assert!(rotation_pending(&three.vault.a.lock()), "kept");
    assert_eq!(key_epoch(&three.vault.a.lock()), 0);

    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert_eq!(
        resolved,
        Some(Outcome::Raced {
            by: Some("iPhone".into())
        })
    );
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(key_epoch(&a), 1, "B's key");
    assert_eq!(
        rekey::removal_not_done(&a).expect("notice"),
        Some(RemovalNotDone {
            by: Some("iPhone".into()),
            devices: vec!["iPad".into()],
            already_removed: false,
            kit_replaced: false,
        })
    );
}

/// Review finding 2: a removal confirmed by the person that a background round finds lost to
/// another device's key change leaves a notice; one the other device had already done says so.
#[tokio::test]
async fn a_removal_a_background_round_finds_not_done_leaves_a_notice() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));
    remove(&three, A, &[C]).await.expect("pending");
    remove(&three, B, &[C]).await.expect("B removes C first");

    round_of(&three.vault, &three.vault.a).await;

    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(
        rekey::removal_not_done(&a).expect("notice"),
        Some(RemovalNotDone {
            by: Some("iPhone".into()),
            devices: vec!["iPad".into()],
            already_removed: true,
            kit_replaced: false,
        })
    );
}

/// The entry for `epoch` of `conn`'s provisional heads.
fn entry(conn: &rusqlite::Connection, epoch: u32) -> Option<head::UnverifiedEpoch> {
    head::unverified_epochs(conn)
        .expect("unverified")
        .into_iter()
        .find(|e| e.epoch == epoch)
}

/// A's credentials (after its key change).
fn a_creds(three: &Three) -> Credentials {
    keys::credentials(&three.vault.a.lock())
        .expect("creds")
        .expect("bound")
}

/// Review MEDIUM 2: a device that was caught up at the key change seals what it had not sent
/// again and pushes it without waiting for the remover's signed head (its pull reached the
/// server's head without a stop), and the head stays provisional meanwhile.
#[tokio::test]
async fn a_caught_up_device_sends_its_unsent_changes_without_the_signed_head() {
    let three = three().await;
    write_and_seal(&three.vault, &three.vault.b, "b-unsent");
    new_kit(&three, A).await.expect("changed");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    assert!(
        unsent_epochs(&three.vault.b.lock()).is_empty(),
        "sent, echoed"
    );
    assert!(
        entry(&three.vault.b.lock(), 1).is_some(),
        "still provisional"
    );
    round_of(&three.vault, &three.vault.a).await;
    assert!(has_institution(&three.vault.a.lock(), "b-unsent"));
}

/// Review MEDIUM 2, the constraint: with the head lowered below an own envelope the server took
/// and no signed head yet, the pull stops at that envelope, so nothing is sealed again or sent
/// twice.
#[tokio::test]
async fn a_lowered_head_without_the_signed_head_stops_the_pull_and_sends_nothing() {
    let three = three().await;
    let b1 = write_and_seal(&three.vault, &three.vault.b, "b1");
    push(&three.vault.relay, &three.vault.creds, &[b1]).await;
    let b1_at = last_seq(&three);
    write_and_seal(&three.vault, &three.vault.b, "b2");
    new_kit(&three, A).await.expect("changed");
    three
        .vault
        .relay
        .set_rekey_head(&vault_b64(&three), 1, b1_at - 1);
    let ops_before = three.vault.relay.ops().len();

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::InvalidChain),
        "{report:?}"
    );
    assert_eq!(three.vault.relay.ops().len(), ops_before, "nothing pushed");
    let epochs: Vec<u32> = unsent_epochs(&three.vault.b.lock())
        .into_iter()
        .map(|(_, epoch)| epoch)
        .collect();
    assert_eq!(epochs, vec![0, 0], "nothing sealed again");
}

/// Review MEDIUM 1: the signed head was pruned below a snapshot; the snapshot B restores names
/// the same head (its dumper verified it), which settles B's provisional one.
#[tokio::test]
async fn a_restored_snapshot_naming_the_head_verifies_it() {
    let three = three().await;
    new_kit(&three, A).await.expect("changed");
    let head = true_head(&three);
    // A's next round sends its signed head and the snapshot owed under the new key.
    round_of(&three.vault, &three.vault.a).await;
    let seq = three.vault.relay.snapshot_seq().expect("A's snapshot");
    assert!(seq > head, "{seq} {head}");
    three.vault.relay.prune_to(seq);

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    assert!(report.restored, "{report:?}");
    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 1);
    assert!(head::unverified_epochs(&b).expect("unverified").is_empty());
}

/// Review MEDIUM 1: a restored snapshot that does not name the head (its dumper held it only
/// provisionally) moves the look-ahead to the snapshot, past the pruned ops, and the signed head
/// above it is found the next round.
#[tokio::test]
async fn a_restored_snapshot_without_the_head_moves_the_look_ahead_past_the_prune() {
    let three = three().await;
    institution(&three.vault.a.lock(), "a-before");
    round_of(&three.vault, &three.vault.a).await;
    round_of(&three.vault, &three.c).await;
    new_kit(&three, A).await.expect("changed");
    let head = true_head(&three);
    // C takes the key provisionally, writes under it and uploads a snapshot naming no head.
    institution(&three.c.lock(), "c-after");
    let report = round_of(&three.vault, &three.c).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert!(entry(&three.c.lock(), 1).is_some());
    run_round(
        &three.vault.relay,
        &three.c,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        NOW,
        0,
    )
    .await
    .expect("C's snapshot");
    let seq = three.vault.relay.snapshot_seq().expect("snapshot");
    assert!(seq > head, "{seq} {head}");
    // A sends its signed head above C's snapshot, and owes no snapshot of its own here (it would
    // name the head and settle it as in the test above).
    exec(
        &three.vault.a.lock(),
        "UPDATE sync_state SET snapshot_epoch_due = 0",
    );
    round_of(&three.vault, &three.vault.a).await;
    assert_eq!(
        three.vault.relay.snapshot_seq(),
        Some(seq),
        "C's snapshot stays the newest"
    );
    three.vault.relay.prune_to(seq);

    let report = round_of(&three.vault, &three.vault.b).await;

    assert!(report.restored, "{report:?}");
    let waiting =
        entry(&three.vault.b.lock(), 1).unwrap_or_else(|| panic!("still provisional: {report:?}"));
    assert!(waiting.scanned >= seq && !waiting.refuted, "{waiting:?}");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    let b = three.vault.b.lock();
    assert!(head::unverified_epochs(&b).expect("unverified").is_empty());
    assert!(has_institution(&b, "a-before") && has_institution(&b, "c-after"));
}

/// Back-to-back key changes by one remover that sent nothing in between: its first signed head
/// is sealed again under the second key, and B, behind by both, verifies both heads in one round.
#[tokio::test]
async fn two_signed_heads_of_back_to_back_key_changes_verify_in_one_round() {
    let three = three().await;
    new_kit(&three, A).await.expect("first");
    new_kit(&three, A).await.expect("second");
    let unsent: Vec<u32> = unsent_epochs(&three.vault.a.lock())
        .into_iter()
        .map(|(_, epoch)| epoch)
        .collect();
    assert_eq!(unsent, vec![1, 2], "a signed head per change");

    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");
    let sent: Vec<Option<u32>> = three
        .vault
        .relay
        .ops()
        .iter()
        .rev()
        .take(2)
        .map(|op| crate::services::sync::envelope::envelope_epoch(&op.ct))
        .collect();
    assert_eq!(sent, vec![Some(2), Some(2)], "the first sealed again");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 2);
    assert!(head::unverified_epochs(&b).expect("unverified").is_empty());
}

/// A head record signed by the bundle's signer but naming another epoch, another vault, or
/// another signer is no confirmation and no refutation: B keeps waiting until the real one.
#[tokio::test]
async fn a_signed_head_for_something_else_neither_confirms_nor_refutes() {
    let three = three().await;
    new_kit(&three, A).await.expect("changed");
    let head = true_head(&three);
    let creds = a_creds(&three);
    let seed = identity::own_seed(&three.vault.a.lock())
        .expect("seed")
        .expect("A's key");
    let vault = creds.vault_id.0;
    let records = [
        head::HeadRecord::sign(vault, A, 0, 5, 1, head, &seed),
        head::HeadRecord::sign([0x99; 16], A, 0, 1, 1, head, &seed),
        head::HeadRecord::sign(vault, C, 0, 1, 1, head + 7, &seed),
    ];
    let forged: Vec<Vec<u8>> = records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            let mut cs = changeset(DeviceId([0x90 + i as u8; 16]), 1, None, Vec::new());
            cs.rekey_head = Some(record.encode());
            forge(&creds, &cs)
        })
        .collect();
    push(&three.vault.relay, &creds, &forged).await;

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    let waiting = entry(&three.vault.b.lock(), 1).expect("still provisional");
    assert!(!waiting.refuted, "{waiting:?}");

    push_a_unsent(&three).await;
    round_of(&three.vault, &three.vault.b).await;
    assert!(entry(&three.vault.b.lock(), 1).is_none(), "verified");
}

/// The look-ahead reads at most `MAX_LOOKAHEAD_PAGES` pages a round (a few ops each in tests): a
/// signed head past them is found in a later round, the scan going on from where it stopped.
#[tokio::test]
async fn a_signed_head_past_the_look_ahead_cap_is_found_in_a_later_round() {
    let three = three().await;
    new_kit(&three, A).await.expect("changed");
    let head = true_head(&three);
    let creds = a_creds(&three);
    let author = DeviceId([0x70; 16]);
    let mut prev = None;
    let filler: Vec<Vec<u8>> = (1..=20u64)
        .map(|seq| {
            let ct = forge(&creds, &changeset(author, seq, prev, Vec::new()));
            prev = Some(crate::services::sync::envelope::envelope_hash(&ct));
            ct
        })
        .collect();
    push(&three.vault.relay, &creds, &filler).await;
    push_a_unsent(&three).await;

    round_of(&three.vault, &three.vault.b).await;

    let waiting = entry(&three.vault.b.lock(), 1).expect("not found yet");
    assert_eq!(waiting.scanned, head + 16, "{waiting:?}");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    assert!(entry(&three.vault.b.lock(), 1).is_none(), "found on");
}

/// Review: a crash after the lost race's pickup adopted the other change and before the pending
/// change was wiped; the next resolution finds the key moved on, wipes it and leaves the notice.
#[tokio::test]
async fn a_crash_between_the_races_adoption_and_the_wipe_resolves_with_the_notice() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));
    remove(&three, A, &[C]).await.expect("pending");
    new_kit(&three, B).await.expect("B changes the key");
    let picked = pickup::pick_up(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("pickup");
    assert_eq!(picked, Pickup::Adopted { epoch: 1, by: B });
    assert!(rotation_pending(&three.vault.a.lock()), "the crash");

    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert_eq!(resolved, None);
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(
        rekey::removal_not_done(&a).expect("notice"),
        Some(RemovalNotDone {
            by: None,
            devices: vec!["iPad".into()],
            already_removed: false,
            kit_replaced: false,
        })
    );
}

/// Review LOW: a rotation counter above `rekey_seen` with nothing to pick up (bundles gone) is
/// acted on once a round, not again after the pass for the same info.
#[tokio::test]
async fn a_rotation_with_nothing_to_pick_up_is_looked_at_once_a_round() {
    let three = three().await;
    three.vault.relay.set_rotation(&vault_b64(&three), 5);
    let from = three.vault.relay.calls().len();

    round_of(&three.vault, &three.vault.b).await;

    let walks = three.vault.relay.calls()[from..]
        .iter()
        .filter(|route| **route == Route::Rekeys)
        .count();
    assert_eq!(walks, 1);
}

/// Protocol 3: a retired token reads no ops, so a device that waits for a signed head and is
/// overtaken by a second key change meets the `403` in its look-ahead, before its pull and
/// without a read of the vault's info. That is a key change to pick up, not the end of the round
/// as `keyRotated`.
#[tokio::test]
async fn a_device_with_an_unverified_head_picks_up_a_second_key_change_from_its_look_ahead() {
    let three = three().await;
    institution(&three.vault.a.lock(), "a-before");
    round_of(&three.vault, &three.vault.a).await;
    new_kit(&three, A).await.expect("first");
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeWaiting),
        "{report:?}"
    );
    assert_eq!(key_epoch(&three.vault.b.lock()), 1);
    new_kit(&three, A).await.expect("second");
    let from = three.vault.relay.calls().len();

    // A `Drain` round reads no vault info: only the look-ahead can tell B of the second change.
    let report = run_round(
        &three.vault.relay,
        &three.vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::Drain,
        NOW,
        0,
    )
    .await
    .expect("round");

    let calls = &three.vault.relay.calls()[from..];
    assert_eq!(
        calls.first(),
        Some(&Route::Pull),
        "the round starts at the look-ahead, with no read of the vault's info: {calls:?}"
    );
    let walk = calls.iter().position(|r| *r == Route::Rekeys);
    let info = calls.iter().position(|r| *r == Route::VaultInfo);
    assert!(
        walk.is_some_and(|walk| info.is_none_or(|info| walk < info)),
        "the pickup follows the look-ahead's refusal: {calls:?}"
    );
    assert_ne!(report.problem, Some(SyncProblem::KeyRotated), "{report:?}");
    assert_eq!(key_epoch(&three.vault.b.lock()), 2, "{report:?}");
}
