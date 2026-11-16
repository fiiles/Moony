//! Picking up another device's key change (phase 5 spec §7.1–§7.5): the three triggers, the walk
//! over several key changes, re-sealing what was not sent, the removed device, and the bundles
//! that only pause sync (`keyNotGiven`, `keyChangeUnverified`).

use zeroize::Zeroizing;

use super::pull::write_and_seal;
use super::rekey_fixture::*;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::envelope::vault_token;
use crate::services::sync::identity::cert::{self, CertFields};
use crate::services::sync::identity::trust;
use crate::services::sync::identity::{self, CertKind};
use crate::services::sync::rekey::bundle::{self, BundleInput, OldKey, Reason, Recipient};
use crate::services::sync::rekey::pickup::{self, Pickup};
use crate::services::sync::rekey::rotate::{Outcome, RemovalMode};
use crate::services::sync::rekey::{self, KeyChangeNotice, RemovedNotice};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;

/// A key change no honest device made: signed by `signer` (with its certificate), sealing a new
/// key to `to` only, stored on the relay with the vault's current token.
async fn rotate_by_hand(
    three: &Three,
    signer: &identity::SignSeed,
    signer_cert: &identity::Certificate,
    to: &[DeviceId],
) {
    let (creds, bytes, key) = {
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
        let key = Zeroizing::new([0x77; 32]);
        let older = [OldKey {
            epoch: 0,
            key: Zeroizing::new(*creds.keys.current()),
            head: Some(0),
        }];
        let bytes = bundle::build(
            &BundleInput {
                vault_id: creds.vault_id.0,
                from_epoch: 0,
                to_epoch: 1,
                new_key: &key,
                recipients: &recipients,
                notices: &[],
                signer,
                signer_cert,
                at: NOW,
                reason: Reason::Rotate,
                removed: &[],
                keys: &older,
            },
            &mut bundle::os_fill,
        )
        .expect("bundle");
        (creds, bytes, key)
    };
    three
        .vault
        .relay
        .rotate_token(&creds.auth(), &vault_token(&key, &creds.vault_id.0), &bytes)
        .await
        .expect("rotated");
}

fn notice(conn: &rusqlite::Connection) -> Option<KeyChangeNotice> {
    rekey::key_change_notice(conn).expect("notice")
}

fn device_removed(conn: &rusqlite::Connection, device: DeviceId) -> bool {
    conn.query_row(
        "SELECT removed = 1 FROM sync_devices WHERE id = ?1",
        [device.to_hex()],
        |r| r.get(0),
    )
    .expect("row")
}

/// A removes C with a key change; B notices it at the op A sealed under the new key, picks the
/// key up and goes on; C opens only its notice and turns sync off, keeping its data.
#[tokio::test]
async fn the_others_pick_the_key_up_and_the_removed_device_turns_off() {
    let three = three().await;
    institution(&three.c.lock(), "c-unsent");
    let outcome = remove(&three, A, &[C], RemovalMode::Rotate)
        .await
        .expect("removed");
    assert!(matches!(outcome, Outcome::Changed { epoch: 1, .. }));
    // A's next round sends the removal record under the new key.
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    {
        let b = three.vault.b.lock();
        assert_eq!(key_epoch(&b), 1);
        assert_eq!(state::rekey_seen(&b).expect("seen"), 1);
        assert!(
            device_removed(&b, C),
            "A's change applied under the new key"
        );
        assert_eq!(
            notice(&b),
            Some(KeyChangeNotice {
                by: "Mac".into(),
                at: NOW,
                removed: vec!["iPad".into()],
            })
        );
        let entry = trust::entry(&b, C).expect("entry").expect("pinned");
        assert_eq!(entry.state, trust::TrustState::Removed);
        assert_eq!(entry.cut_off_epoch, Some(1));
    }

    let report = round_of(&three.vault, &three.c).await;

    assert!(report.removed_here, "{report:?}");
    let c = three.c.lock();
    assert!(!sync_on(&c));
    assert!(has_institution(&c, "c-unsent"), "local data stays");
    assert_eq!(
        rekey::removed_notice(&c).expect("notice"),
        Some(RemovedNotice {
            by: "Mac".into(),
            at: NOW,
            unsent: 1,
        })
    );
    assert_eq!(
        count(&c, "SELECT COUNT(*) FROM sync_devices"),
        0,
        "the device list went with sync; no leave record was signed"
    );
}

/// The second trigger: B's push meets the retired token (`403`), B picks the key up, seals its
/// change again under it and pushes; A applies it.
#[tokio::test]
async fn a_push_refused_by_the_retired_token_picks_the_key_up_and_sends_again() {
    let three = three().await;
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");
    institution(&three.vault.b.lock(), "b-after");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    assert!(three.vault.relay.calls().contains(&Route::Rekeys));
    assert_eq!(key_epoch(&three.vault.b.lock()), 1);
    assert!(unsent_epochs(&three.vault.b.lock()).is_empty(), "echoed");
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert!(has_institution(&three.vault.a.lock(), "b-after"));
}

/// §7.2: of B's two unsent envelopes the relay took one before the key change. The pull through
/// the rotation point acknowledges it; only the other is sealed again, chained to it, and every
/// other device applies both in order.
#[tokio::test]
async fn only_what_never_reached_the_server_is_sealed_again_and_the_chain_holds() {
    let three = three().await;
    let creds = three.vault.creds.clone();
    let first = write_and_seal(&three.vault, &three.vault.b, "b1");
    write_and_seal(&three.vault, &three.vault.b, "b2");
    push(&three.vault.relay, &creds, &[first]).await;
    assert_eq!(unsent_epochs(&three.vault.b.lock()).len(), 2);
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");
    // A sends its signed head, which B waits for before it applies b1's place (As built 62–63).
    round_of(&three.vault, &three.vault.a).await;
    let ops_before = three.vault.relay.ops().len();

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    assert_eq!(
        three.vault.relay.ops().len(),
        ops_before + 1,
        "only b2 was sent again"
    );
    assert!(unsent_epochs(&three.vault.b.lock()).is_empty());
    for device in [A, C] {
        let report = round_of(&three.vault, three.device(device)).await;
        assert_eq!(report.problem, None, "{device:?}: {report:?}");
        let conn = three.device(device).lock();
        assert!(has_institution(&conn, "b1") && has_institution(&conn, "b2"));
        assert_eq!(head(&conn, B).map(|h| h.2), Some(false), "no quarantine");
    }
}

/// A device behind by two key changes walks both bundles and ends with every key.
#[tokio::test]
async fn a_device_behind_by_two_key_changes_walks_both() {
    let three = three().await;
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("first");
    round_of(&three.vault, &three.vault.a).await;
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("second");
    institution(&three.vault.a.lock(), "a-at-2");
    round_of(&three.vault, &three.vault.a).await;

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    let b = three.vault.b.lock();
    let keys = keys::vault_keys(&b).expect("keys").expect("bound");
    assert_eq!(keys.current_epoch(), 2);
    assert!(keys.get(0).is_some() && keys.get(1).is_some());
    assert_eq!(
        state::epochs(&b)
            .expect("epochs")
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(has_institution(&b, "a-at-2"));
}

/// The third trigger: the vault's `rotation` above `rekey_seen` (`pick_up` directly, as the round
/// calls it when its vault info says so).
#[tokio::test]
async fn a_rotation_above_what_was_seen_is_picked_up() {
    let three = three().await;
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");
    let info = three
        .vault
        .relay
        .vault_info(&three.vault.creds.auth())
        .await
        .expect("info");
    assert!(info.rotation > state::rekey_seen(&three.vault.b.lock()).expect("seen"));

    let picked = pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");

    assert_eq!(picked, Pickup::Adopted { epoch: 1, by: A });
    let again = pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");
    assert_eq!(again, Pickup::Nothing, "nothing new");
}

/// §7.2 step 1: unsent changes wait until the pull has reached the rotation point; a pull stopped
/// before it seals nothing again and pushes nothing.
#[tokio::test]
async fn nothing_is_sealed_again_or_pushed_before_the_pull_reaches_the_key_change() {
    let three = three().await;
    // C's envelope that does not open sits before the key change.
    let mut tampered = write_and_seal(&three.vault, &three.c, "c-broken");
    *tampered.last_mut().expect("tag") ^= 1;
    push(&three.vault.relay, &three.vault.creds, &[tampered]).await;
    write_and_seal(&three.vault, &three.vault.b, "b-waits");
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");
    pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");
    let ops_before = three.vault.relay.ops().len();

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::Undecryptable),
        "{report:?}"
    );
    let unsent = unsent_epochs(&three.vault.b.lock());
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].1, 0, "still under the old key");
    assert_eq!(three.vault.relay.ops().len(), ops_before, "nothing pushed");
}

/// §7.2: a gap held before the rotation point does not stop the pull, which still acknowledges
/// this device's own envelopes past it: B seals again and pushes.
#[tokio::test]
async fn a_gap_before_the_key_change_does_not_hold_the_reseal() {
    let three = three().await;
    let _c1 = write_and_seal(&three.vault, &three.c, "c1");
    let c2 = write_and_seal(&three.vault, &three.c, "c2");
    // C's second envelope arrives without its first: a gap on every other device.
    push(&three.vault.relay, &three.vault.creds, &[c2]).await;
    institution(&three.vault.b.lock(), "b-later");
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");
    // A sends its signed head, which B waits for before it handles c2's place (As built 62–63).
    round_of(&three.vault, &three.vault.a).await;

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, Some(SyncProblem::Gap), "{report:?}");
    assert_eq!(key_epoch(&three.vault.b.lock()), 1);
    assert!(
        unsent_epochs(&three.vault.b.lock()).is_empty(),
        "sent and echoed"
    );
    round_of(&three.vault, &three.vault.a).await;
    assert!(has_institution(&three.vault.a.lock(), "b-later"));
}

/// A key change that seals nothing to this device and carries no notice for it: paused, never
/// turned off, nothing deleted.
#[tokio::test]
async fn a_key_change_that_gave_this_device_nothing_pauses_sync() {
    let three = three().await;
    let seed = identity::own_seed(&three.vault.a.lock())
        .expect("seed")
        .expect("A's key");
    let a_cert = trust::entry(&three.vault.a.lock(), A)
        .expect("entry")
        .expect("pinned")
        .cert;
    rotate_by_hand(&three, &seed, &a_cert, &[A]).await;
    institution(&three.vault.b.lock(), "b-kept");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, Some(SyncProblem::KeyNotGiven), "{report:?}");
    let b = three.vault.b.lock();
    assert!(sync_on(&b));
    assert_eq!(key_epoch(&b), 0);
    assert!(has_institution(&b, "b-kept"));
    assert_eq!(rekey::removed_notice(&b).expect("notice"), None);
}

/// A bundle signed by a device this one never verified: paused (`keyChangeUnverified`).
#[tokio::test]
async fn a_key_change_signed_by_an_unknown_device_is_not_taken() {
    let three = three().await;
    let stranger = identity::new_sign_seed().expect("seed");
    let stranger_cert = cert::issue_self(
        CertKind::Genesis,
        &CertFields {
            vault_id: three.vault.creds.vault_id.0,
            device: DeviceId([9; 16]),
            box_pub: [0x42; 32],
            issued_at: NOW,
        },
        &stranger,
    )
    .expect("cert");
    rotate_by_hand(&three, &stranger, &stranger_cert, &[A, B]).await;
    institution(&three.vault.b.lock(), "b-pushes");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    let b = three.vault.b.lock();
    assert!(sync_on(&b));
    assert_eq!(key_epoch(&b), 0);
    assert_eq!(state::rekey_seen(&b).expect("seen"), 0);
}

/// A key change signed by a device this one trusts but leaves out for its issuer (revision §3.1;
/// review of Tasks 13–15, finding 1): C was paired by A; here A left before C's certificate was
/// seen (a late vouch), or A was removed (its issuer removed). C's key change is not taken
/// (`keyChangeUnverified`) until the person keeps C ("Je moje").
#[tokio::test]
async fn a_key_change_by_a_device_left_out_for_its_issuer_is_not_taken_until_it_is_kept() {
    for removed in [false, true] {
        let three = three().await;
        let vault = three.vault.creds.vault_id.0;
        {
            let b = three.vault.b.lock();
            if removed {
                let b_seed = identity::own_seed(&b).expect("seed").expect("B's key");
                let record = identity::Record::sign(
                    identity::RecordType::Removed,
                    vault,
                    A,
                    B,
                    NOW,
                    0,
                    &b_seed,
                )
                .expect("removal");
                trust::mark_removed(&b, &record, None).expect("A removed here");
            } else {
                let a_seed = identity::own_seed(&three.vault.a.lock())
                    .expect("seed")
                    .expect("A's key");
                let record = identity::Record::sign(
                    identity::RecordType::Left,
                    vault,
                    A,
                    A,
                    NOW,
                    0,
                    &a_seed,
                )
                .expect("left");
                trust::mark_left(&b, &record).expect("A left here");
                // A's `left` record was seen before C's certificate.
                trust::note_seen(&b, A, trust::SeenWhat::Left, 0).expect("seen");
            }
            assert!(trust::recipients(&b, &[])
                .expect("recipients")
                .excluded
                .iter()
                .any(|(device, _)| *device == C));
        }
        let (c_seed, c_cert) = {
            let c = three.c.lock();
            let seed = identity::own_seed(&c).expect("seed").expect("C's key");
            let cert = trust::entry(&c, C).expect("entry").expect("pinned").cert;
            (seed, cert)
        };
        rotate_by_hand(&three, &c_seed, &c_cert, &[A, B, C]).await;

        let report = round_of(&three.vault, &three.vault.b).await;

        assert_eq!(
            report.problem,
            Some(SyncProblem::KeyChangeUnverified),
            "removed: {removed}: {report:?}"
        );
        assert_eq!(key_epoch(&three.vault.b.lock()), 0);
        assert!(
            !trust::entry(&three.vault.b.lock(), C)
                .expect("entry")
                .expect("pinned")
                .confirmed
        );

        assert!(trust::keep(&three.vault.b.lock(), C).expect("kept"));
        let report = round_of(&three.vault, &three.vault.b).await;
        assert_eq!(report.problem, None, "removed: {removed}: {report:?}");
        assert_eq!(key_epoch(&three.vault.b.lock()), 1);
    }
}

/// Junk the server stores as a bundle: nothing opens (`keyNotGiven`).
#[tokio::test]
async fn a_junk_bundle_pauses_sync() {
    let three = three().await;
    three
        .vault
        .relay
        .rotate_token(
            &three.vault.creds.auth(),
            &vault_token(&[0x31; 32], &three.vault.creds.vault_id.0),
            b"not a bundle at all",
        )
        .await
        .expect("rotated");
    institution(&three.vault.b.lock(), "b-pushes");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, Some(SyncProblem::KeyNotGiven), "{report:?}");
    assert!(sync_on(&three.vault.b.lock()));
}

/// Coordinator's ruling 2 on Tasks 7–8: a sync server restored from a backup made after a key
/// change, below this device's place, reissues positions under the new epoch. The head below the
/// cursor is caught before any op is judged (`state::note_vault_went_back`); once the head passes
/// the cursor again the pull goes on, and the epoch rule never stops it for good.
#[tokio::test]
async fn a_server_that_went_back_after_a_key_change_does_not_stop_the_pull_for_good() {
    let three = three().await;
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");
    let rotation_head = *state::epochs(&three.vault.a.lock())
        .expect("epochs")
        .get(&1)
        .expect("head");
    for id in ["a1", "a2", "a3"] {
        institution(&three.vault.a.lock(), id);
        round_of(&three.vault, &three.vault.a).await;
    }
    round_of(&three.vault, &three.vault.b).await;
    let cursor_b = cursor(&three.vault.b.lock());
    assert!(cursor_b > rotation_head + 1, "{cursor_b} {rotation_head}");
    // Restored from a backup made after the key change, below B's place.
    three
        .vault
        .relay
        .roll_back_head(&three.vault.creds.vault_id.to_b64(), rotation_head + 1);

    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        report.problem,
        Some(SyncProblem::ServerWentBack),
        "{report:?}"
    );

    // C, whose place is below the restored head (it last synced before the key change), writes
    // on until the head passes B's place: it picks the new key up, and the reissued positions
    // are under the new epoch, past the rotation head. (A, past the restored head like B, waits
    // like B for the head to come back.)
    for n in 0..20 {
        institution(&three.c.lock(), &format!("again-{n}"));
        round_of(&three.vault, &three.c).await;
        if three.vault.relay.ops().last().map_or(0, |op| op.seq) > cursor_b {
            break;
        }
    }
    assert!(three.vault.relay.ops().last().map_or(0, |op| op.seq) > cursor_b);
    let report = round_of(&three.vault, &three.vault.b).await;
    let pull = report.pull.as_ref().expect("pulled");
    // Nothing stops the pull: C's envelopes the restore renumbered below B's place are a gap
    // (the server going back lost them for B, as before key changes), and the mark stays.
    assert_eq!(pull.stop, None, "{report:?}");
    assert!(pull.through > cursor_b, "{report:?}");
    assert_eq!(
        report.problem,
        Some(SyncProblem::ServerWentBack),
        "{report:?}"
    );
}

/// `KeyRotated` stays while a pickup cannot finish: a retired token with no key change to pick
/// up yet.
#[tokio::test]
async fn a_retired_token_without_a_key_change_stays_key_rotated() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::Push, RelayError::TokenReadOnly);
    institution(&three.vault.b.lock(), "b-refused");

    let report = run_round(
        &three.vault.relay,
        &three.vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round");

    assert_eq!(report.problem, Some(SyncProblem::KeyRotated), "{report:?}");
    assert!(three.vault.relay.calls().contains(&Route::Rekeys));
}

/// The third trigger in the engine itself: a new engine's first round reads the vault's info, and
/// a `rotation` above `rekey_seen` is picked up although no op and no push met the new key; a
/// removed device turns sync off there and its loop ends.
#[tokio::test]
async fn the_engine_picks_a_key_change_up_in_its_first_round() {
    use super::runtime_loop::{start, until};
    use crate::services::sync::binding::NoBinding;
    let three = three().await;
    remove(&three, A, &[C], RemovalMode::Rotate)
        .await
        .expect("removed");

    let b = start(
        &three.vault,
        &three.vault.b,
        tempfile::tempdir().expect("dir"),
        Box::new(NoBinding),
    );
    let c = start(
        &three.vault,
        &three.c,
        tempfile::tempdir().expect("dir"),
        Box::new(NoBinding),
    );

    until("B holds the new key", || {
        key_epoch(&three.vault.b.lock()) == 1
    })
    .await;
    until("C turned sync off", || !sync_on(&three.c.lock())).await;
    until("C's loop ended", || !c.handle.is_running()).await;
    assert!(rekey::removed_notice(&three.c.lock())
        .expect("notice")
        .is_some());
    b.handle.stop();
}

/// Re-sealing twice changes nothing the second time.
#[tokio::test]
async fn sealing_again_is_idempotent() {
    let three = three().await;
    write_and_seal(&three.vault, &three.vault.b, "b1");
    write_and_seal(&three.vault, &three.vault.b, "b2");
    remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");
    pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");
    let b = three.vault.b.lock();
    let creds = keys::credentials(&b).expect("creds").expect("bound");

    assert_eq!(
        crate::services::sync::rekey::reseal::reseal_unsent(&b, &creds).expect("reseal"),
        2
    );
    let first = sync_rows(&b);
    assert_eq!(
        crate::services::sync::rekey::reseal::reseal_unsent(&b, &creds).expect("reseal"),
        0
    );
    assert_eq!(sync_rows(&b), first);
    assert_eq!(
        unsent_epochs(&b),
        vec![(first_seq(&b), 1), (first_seq(&b) + 1, 1)]
    );
}

fn first_seq(conn: &rusqlite::Connection) -> i64 {
    count(conn, "SELECT MIN(seq) FROM sync_unsent")
}
