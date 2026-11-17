//! Fixes from the review of Task 17 (phase 5 revision, As built R-54 on): where the Kit is kept
//! only grows through sync, a new Kit retires every other trusted Kit, a removal with no Kit to
//! seal to makes one when the server holds one, a key change the server took that cannot complete
//! here is given up instead of retried for ever, a bundle's Kit must be its creator's, and what
//! the Kit's envelope leaves out.

use zeroize::Zeroizing;

use super::rekey_fixture::*;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::identity::trust::{self, ExclusionReason, SeenWhat, TrustState};
use crate::services::sync::identity::{self, cert, CertKind, Certificate, SignSeed};
use crate::services::sync::keys;
use crate::services::sync::rekey::envelope;
use crate::services::sync::rekey::kit::{self, KitStorage};
use crate::services::sync::rekey::rotate::{self, Outcome, ReplacesKit};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::Relay;
use crate::services::sync::state;

fn vault_id(three: &Three) -> [u8; 16] {
    three.vault.creds.vault_id.0
}

fn seed_of(conn: &rusqlite::Connection) -> SignSeed {
    identity::own_seed(conn).expect("seed").expect("seed")
}

/// Pins `cert` on `conn` as the trust store would, with `state` and `confirmed` (a store the
/// rules would not have produced on their own: "something is wrong").
pub(super) fn pin_raw(
    conn: &rusqlite::Connection,
    cert: &Certificate,
    state: &str,
    confirmed: bool,
    at: i64,
) {
    conn.execute(
        "INSERT INTO sync_trust (device, cert, kind, issuer, sign_pub, box_pub, ticket_pub, \
           state, pinned_at, confirmed, state_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            cert.body.device.to_hex(),
            cert.to_b64(),
            cert.body.kind.as_u8(),
            cert.body.issuer_device.to_hex(),
            cert.body.sign_pub.to_vec(),
            cert.body.box_pub.to_vec(),
            cert.ticket.as_ref().map(|t| t.ticket_pub.to_vec()),
            state,
            at,
            confirmed,
            (state != "trusted").then_some(at),
        ],
    )
    .expect("pinned");
}

/// A Kit `creator` made (its certificate), not registered anywhere.
pub(super) fn stray_kit(three: &Three, creator: DeviceId) -> Certificate {
    let conn = three.device(creator).lock();
    let (_, _, cert) = kit::create(creator, &seed_of(&conn), &vault_id(three), NOW).expect("Kit");
    cert
}

fn paired(
    vault: [u8; 16],
    holder: DeviceId,
    holder_seed: &SignSeed,
    subject: DeviceId,
) -> Certificate {
    let (secret, ticket) = cert::issue_ticket(holder_seed, vault, holder, NOW).expect("ticket");
    cert::issue_paired(
        &secret,
        &ticket,
        holder,
        &identity::sign_pub(holder_seed),
        &cert::CertFields {
            vault_id: vault,
            device: subject,
            box_pub: [subject.0[0]; 32],
            issued_at: NOW,
        },
        &SignSeed::from_bytes(Zeroizing::new([subject.0[0]; 32])),
    )
    .expect("paired")
}

/// I1: the creator of a copied Kit writes a weaker `kit_storage` with a newer clock before it is
/// removed; the remover still sees `copied` and replaces the Kit.
#[tokio::test]
async fn a_creator_that_lowers_the_kit_storage_still_has_its_kit_replaced() {
    let three = three().await;
    let secret = with_kit(&three, C, Some(KitStorage::Copied)).await;
    let id = kit_keys(&three, &secret).id();
    for weaker in [Some(KitStorage::Paper), None] {
        set_kit_storage(&three.c.lock(), id, weaker);
        settle(&three).await;
        let a = three.vault.a.lock();
        assert_eq!(
            kit::storage(&a, id).expect("storage"),
            Some(KitStorage::Copied)
        );
        let plan = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
        assert_eq!(plan.replaces_kit, ReplacesKit::CreatorRemoved, "{weaker:?}");
    }
    let outcome = remove(&three, A, &[C]).await.expect("removed");
    assert!(
        matches!(outcome, Outcome::Changed { kit: Some(_), .. }),
        "{outcome:?}"
    );
}

/// I2 (a): a second `trusted` Kit here (not the one sealed to) is retired by the change that
/// makes a new Kit, so the commit pins the new one; the other devices take the change.
#[tokio::test]
async fn a_new_kit_retires_every_other_trusted_kit() {
    let three = three().await;
    let first = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let first = kit_keys(&three, &first).id();
    let stray = stray_kit(&three, B);
    {
        // Its row syncs (every other device has a Kit already: `SecondKit`, never pinned there).
        let a = three.vault.a.lock();
        kit::write_row(&a, &stray, None, NOW).expect("row");
        pin_raw(&a, &stray, "trusted", false, NOW);
    }
    settle(&three).await;
    {
        let a = three.vault.a.lock();
        let plan = rotate::plan(&a, &[], &[], true, NOW).expect("plan");
        assert_eq!(plan.kit.as_ref().map(|k| k.device), Some(first));
        assert_eq!(plan.retired_kits, vec![stray.body.device]);
        // The Kit stays: nothing is retired.
        let staying = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
        assert!(staying.retired_kits.is_empty());
        assert!(!staying.kit_not_updated);
    }

    let outcome = new_kit(&three, A).await.expect("changed");

    let Outcome::Changed { kit: Some(new), .. } = outcome else {
        panic!("{outcome:?}");
    };
    {
        let a = three.vault.a.lock();
        let state_of = |id| trust::entry(&a, id).expect("entry").expect("pinned").state;
        assert_eq!(state_of(first), TrustState::Removed);
        assert_eq!(state_of(stray.body.device), TrustState::Removed);
        assert_eq!(state_of(new.id), TrustState::Trusted);
        assert!(!rotate::stuck(&a).expect("stuck"));
    }
    settle(&three).await;
    for id in [B, C] {
        let conn = three.device(id).lock();
        assert_eq!(key_epoch(&conn), 1, "{id:?}");
        let notice = crate::services::sync::rekey::key_change_notice(&conn)
            .expect("notice")
            .expect("written");
        assert!(notice.kit_replaced);
        assert!(notice.removed.is_empty(), "a Kit is no device: {notice:?}");
        assert_eq!(
            trust::sealing_kit_entry(&conn)
                .expect("kit")
                .map(|k| k.device),
            Some(new.id)
        );
    }
}

/// M1: with no Kit this device can seal to, a removal makes one when the server holds one (else
/// the server refuses it, `kit_required`); with none anywhere it says the Kit is not updated.
#[tokio::test]
async fn a_removal_with_no_usable_kit_makes_one_when_the_server_holds_one() {
    let three = three().await;
    {
        let a = three.vault.a.lock();
        let plan = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
        assert_eq!(plan.replaces_kit, ReplacesKit::No);
        assert!(plan.kit_not_updated);
    }
    // An orphan Kit on the server (a setup whose answer was lost, R-37): no row, no pin here.
    let orphan = identity::KitSecret::generate()
        .expect("seed")
        .derive(&vault_id(&three));
    three
        .vault
        .relay
        .put_kit(&three.vault.creds.auth(), orphan.token(), &[1, 2, 3])
        .await
        .expect("put");
    round_of(&three.vault, &three.vault.a).await;
    {
        let a = three.vault.a.lock();
        let plan = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
        assert_eq!(plan.replaces_kit, ReplacesKit::Missing);
        assert!(!plan.kit_not_updated);
        assert!(
            kit::current(&a).expect("current").is_none(),
            "no usable Kit"
        );
    }

    let outcome = remove(&three, A, &[C]).await.expect("removed");

    let Outcome::Changed { kit: Some(new), .. } = outcome else {
        panic!("{outcome:?}");
    };
    let a = three.vault.a.lock();
    let current = kit::current(&a).expect("current").expect("a Kit now");
    assert_eq!(
        (current.id, current.created_by, current.storage),
        (new.id, A, None)
    );
}

/// M1: a Kit that cannot be sealed to (two `trusted` Kits, both confirmed) counts as none: the
/// removal makes a new Kit and retires both.
#[tokio::test]
async fn two_confirmed_kits_are_both_retired_by_the_next_removal() {
    let three = three().await;
    let first = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let first = kit_keys(&three, &first).id();
    let stray = stray_kit(&three, B);
    pin_raw(&three.vault.a.lock(), &stray, "trusted", true, NOW);
    {
        let a = three.vault.a.lock();
        let plan = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
        assert_eq!(plan.kit, None);
        assert_eq!(plan.replaces_kit, ReplacesKit::Missing);
        let mut both = vec![first, stray.body.device];
        both.sort();
        assert_eq!(plan.retired_kits, both);
    }
    let outcome = remove(&three, A, &[C]).await.expect("removed");
    assert!(
        matches!(outcome, Outcome::Changed { kit: Some(_), .. }),
        "{outcome:?}"
    );
}

/// I2 (b): the server took the change (its answer was lost), and the commit then fails for good
/// (a second Kit appeared meanwhile): the change is given up, not retried for ever; rounds stop
/// with `keyChangeUnverified` until sync is turned off; the other devices take the change.
#[tokio::test]
async fn a_taken_change_whose_commit_fails_for_good_is_given_up() {
    let three = three().await;
    with_kit(&three, A, Some(KitStorage::Paper)).await;
    three.vault.relay.lose_next_answer(Route::RotateToken);
    let outcome = new_kit(&three, A).await.expect("pending");
    assert!(
        matches!(outcome, Outcome::Pending { kit: Some(_) }),
        "{outcome:?}"
    );
    pin_raw(
        &three.vault.a.lock(),
        &stray_kit(&three, B),
        "trusted",
        false,
        NOW,
    );

    let report = round_of(&three.vault, &three.vault.a).await;

    assert_eq!(report.problem, Some(SyncProblem::KeyChangeUnverified));
    {
        let a = three.vault.a.lock();
        assert!(!rotation_pending(&a), "the new key and Kit token are wiped");
        assert!(rotate::stuck(&a).expect("stuck"));
        assert_eq!(key_epoch(&a), 0);
        assert!(rotate::plan(&a, &[C], &[], false, NOW).is_err());
    }
    let calls = three.vault.relay.calls().len();
    let again = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(again.problem, Some(SyncProblem::KeyChangeUnverified));
    assert_eq!(
        three.vault.relay.calls().len(),
        calls,
        "nothing more is asked"
    );
    for id in [B, C] {
        assert_eq!(round_of(&three.vault, three.device(id)).await.problem, None);
        assert_eq!(key_epoch(&three.device(id).lock()), 1, "{id:?}");
    }
    // Turning sync off ends it.
    let a = three.vault.a.lock();
    state::disable(&a).expect("off");
    assert!(!rotate::stuck(&a).expect("stuck"));
}

/// I2 (b): a pending change whose bundle the server no longer lists (bundles go after 30 days)
/// although its token reads the vault is given up.
#[tokio::test]
async fn a_taken_change_whose_bundle_is_gone_is_given_up() {
    let three = three().await;
    three.vault.relay.lose_next_answer(Route::RotateToken);
    let outcome = remove(&three, A, &[C]).await.expect("pending");
    assert_eq!(outcome, Outcome::Pending { kit: None });
    three.vault.relay.advance_clock(31 * 24 * 3600);

    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert_eq!(resolved, Some(Outcome::Stuck));
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert!(rotate::stuck(&a).expect("stuck"));
}

/// M3: a bundle may name a new Kit only as its creator's: one the signer issued under another
/// device's ID (with the signer's own key) is refused, the signer's own is pinned.
#[tokio::test]
async fn a_bundle_kit_must_be_made_by_its_creator() {
    let three = three().await;
    let (signer, forged, genuine) = {
        let a = three.vault.a.lock();
        let seed = seed_of(&a);
        let keys = identity::KitSecret::generate()
            .expect("seed")
            .derive(&vault_id(&three));
        let forged = cert::issue_kit(&seed, C, &keys, NOW).expect("Kit");
        let genuine = cert::issue_kit(&seed, A, &keys, NOW).expect("Kit");
        let signer = trust::entry(&a, A).expect("entry").expect("pinned").cert;
        (signer, forged, genuine)
    };
    let b = three.vault.b.lock();
    let import = |kit: &Certificate| {
        trust::import_bundle(
            &b,
            &signer,
            &[],
            Some(trust::BundleKit {
                cert: kit,
                replaces: None,
            }),
            &[],
            1,
            NOW,
        )
    };
    assert!(import(&forged).is_err(), "a Kit \"made by\" C with A's key");
    assert!(trust::entry(&b, forged.body.device)
        .expect("entry")
        .is_none());
    import(&genuine).expect("A's own Kit");
    assert!(trust::entry(&b, genuine.body.device)
        .expect("entry")
        .is_some());
}

/// Review of Task 17, missing negatives: the envelope's confirmed set holds no device left out
/// for its issuer (`IssuerUnknown`, `LateVouch`) or as a quiet superseded ID (`SupersededQuiet`),
/// and the old Kit's seed does not open the envelope of the new one.
#[tokio::test]
async fn the_envelope_leaves_out_excluded_devices_and_an_old_kit_cannot_open_a_new_one() {
    const U: DeviceId = DeviceId([0x55; 16]);
    const X1: DeviceId = DeviceId([0x61; 16]);
    const Y: DeviceId = DeviceId([0x62; 16]);
    const X2: DeviceId = DeviceId([0x63; 16]);
    const O: DeviceId = DeviceId([0x64; 16]);
    const R: DeviceId = DeviceId([0x65; 16]);
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let vault = vault_id(&three);
    {
        let a = three.vault.a.lock();
        let a_seed = seed_of(&a);
        // X1: paired by U, which nobody here knows.
        let u_seed = SignSeed::from_bytes(Zeroizing::new([0x55; 32]));
        pin_raw(&a, &paired(vault, U, &u_seed, X1), "trusted", false, NOW);
        // X2: paired by Y after Y's `left` record was seen.
        pin_raw(&a, &paired(vault, A, &a_seed, Y), "left", false, NOW);
        let y_seed = SignSeed::from_bytes(Zeroizing::new([Y.0[0]; 32]));
        pin_raw(&a, &paired(vault, Y, &y_seed, X2), "trusted", false, NOW);
        trust::note_seen(&a, Y, SeenWhat::Left, 5).expect("seen");
        trust::note_seen(&a, X2, SeenWhat::Cert, 9).expect("seen");
        // O: paired by A, superseded by R long after it last wrote (it never did).
        pin_raw(&a, &paired(vault, A, &a_seed, O), "trusted", false, NOW);
        let o_seed = SignSeed::from_bytes(Zeroizing::new([O.0[0]; 32]));
        let r_seed = SignSeed::from_bytes(Zeroizing::new([R.0[0]; 32]));
        let rejoin = cert::issue_rejoin(
            &o_seed,
            O,
            &cert::CertFields {
                vault_id: vault,
                device: R,
                box_pub: [R.0[0]; 32],
                issued_at: 4_000_000_000,
            },
            &r_seed,
        )
        .expect("rejoin");
        pin_raw(&a, &rejoin, "trusted", false, 4_000_000_000);
        let plan = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
        let reason = |d: DeviceId| plan.excluded.iter().find(|(x, _)| *x == d).map(|(_, r)| *r);
        assert_eq!(reason(X1), Some(ExclusionReason::IssuerUnknown));
        assert_eq!(reason(X2), Some(ExclusionReason::LateVouch));
        assert_eq!(reason(O), Some(ExclusionReason::SupersededQuiet));
    }

    let outcome = new_kit(&three, A).await.expect("changed");

    let Outcome::Changed { kit: Some(new), .. } = outcome else {
        panic!("{outcome:?}");
    };
    let read = three
        .vault
        .relay
        .get_kit(
            &three.vault.creds.vault_id.to_b64(),
            kit_keys(&three, &new.secret).token(),
        )
        .await
        .expect("read");
    let opened = envelope::open(&read.envelope, &kit_keys(&three, &new.secret)).expect("opens");
    let devices: Vec<DeviceId> = opened.confirmed.iter().map(|c| c.device).collect();
    for left_out in [X1, X2, O, Y] {
        assert!(!devices.contains(&left_out), "{left_out:?}: {devices:?}");
    }
    assert!(devices.contains(&R), "the successor is a recipient");
    assert!(
        envelope::open(&read.envelope, &kit_keys(&three, &old)).is_err(),
        "the old Kit's seed opens nothing new"
    );
    let a = three.vault.a.lock();
    assert!(keys::credentials(&a).expect("creds").is_some());
    assert_eq!(
        trust::entry(&a, kit_keys(&three, &old).id())
            .expect("entry")
            .map(|e| e.kind),
        Some(CertKind::Kit)
    );
}
