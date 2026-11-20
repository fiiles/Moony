//! Joining with the Recovery Kit after any number of key changes (phase 5 revision §2.6): the
//! envelope's keys and heads, the newest epoch never left unchecked, what the server's refusals
//! read as, the store anchored on the Kit and checked against the envelope's signer, the confirmed
//! set, and the push hold ("Data jsou zpět").

use std::path::Path;

use crate::services::sync::devices;

use super::rekey_fixture::*;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::identity::trust::{self, TrustState};
use crate::services::sync::identity::{self, cert, CertKind, KitSecret};
use crate::services::sync::join::{self, join_new_device, recovery_credentials, JoinIdentity};
use crate::services::sync::recovery::{encode_kit_code, format_recovery_code, read_kit_input};
use crate::services::sync::rekey::head;
use crate::services::sync::rekey::kit::{self, KitStorage};
use crate::services::sync::rekey::rotate::Outcome;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::http::DEFAULT_RELAY_URL;
use crate::services::sync::relay::{Relay, RelayError};

const DAY: i64 = 86_400;

/// A profile of its own that has never synced, with a row only it holds.
pub(super) fn newcomer() -> TestConn {
    let conn = TestConn::new(migrated());
    institution(&conn.lock(), "local-only");
    conn
}

pub(super) fn key_of(error: &SyncError) -> Option<String> {
    match error {
        SyncError::Local(AppError::Validation(detail)) => {
            Some(detail.split('|').next().unwrap_or_default().to_string())
        }
        _ => None,
    }
}

pub(super) async fn round_mode(three: &Three, device: &TestConn, mode: RoundMode) -> RoundReport {
    run_round(
        &three.vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        mode,
        NOW,
        0,
    )
    .await
    .expect("round")
}

/// `device` uploads a snapshot of what it holds now.
pub(super) async fn snapshot_by(three: &Three, device: &TestConn) {
    let report = round_mode(three, device, RoundMode::FirstSnapshot).await;
    assert!(report.snapshot_uploaded, "{report:?}");
}

/// What a person types from the Kit `secret`: its v2 code, grouped, and the server's address.
pub(super) fn typed(
    three: &Three,
    secret: &KitSecret,
) -> crate::services::sync::recovery::KitInput {
    let creds = &three.vault.creds;
    let code = encode_kit_code(&creds.vault_id, secret, &creds.account).expect("code");
    read_kit_input(
        &format_recovery_code(&code),
        Some(&creds.relay_url),
        DEFAULT_RELAY_URL,
    )
    .expect("input")
}

/// A new profile joins with the Kit `secret`.
pub(super) async fn kit_join(
    three: &Three,
    device: &TestConn,
    dir: &Path,
    secret: &KitSecret,
) -> SyncResult<()> {
    let (creds, join) =
        recovery_credentials(&three.vault.relay, device, typed(three, secret)).await?;
    join_new_device(
        &three.vault.relay,
        device,
        creds,
        "Laptop",
        agreed(device),
        JoinIdentity::RecoveryKit(Box::new(join)),
        dir,
        &NoBinding,
        NOW,
    )
    .await
    .map(|_| ())
}

pub(super) fn me(conn: &rusqlite::Connection) -> DeviceId {
    state::load(conn).expect("state").expect("row").device
}

pub(super) fn untouched(device: &TestConn) {
    let conn = device.lock();
    assert!(has_institution(&conn, "local-only"), "nothing replaced");
    assert!(!sync_on(&conn));
}

/// The new Kit a key change made.
pub(super) fn new_secret(outcome: Outcome) -> KitSecret {
    match outcome {
        Outcome::Changed { kit: Some(kit), .. } => kit.secret,
        other => panic!("no new Kit: {other:?}"),
    }
}

/// Revision §2.6: a Kit joins after none, one and three key changes, with every key; its
/// certificate is issued by the Kit, which it anchors on.
#[tokio::test]
async fn a_kit_joins_after_zero_one_and_three_key_changes() {
    for changes in [0u32, 1, 3] {
        let three = three().await;
        let mut secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
        if changes >= 1 {
            remove(&three, A, &[C]).await.expect("A removes C");
        }
        for _ in 1..changes {
            secret = new_secret(new_kit(&three, A).await.expect("a new Kit"));
        }
        round_of(&three.vault, &three.vault.a).await;
        snapshot_by(&three, &three.vault.a).await;
        let device = newcomer();
        let dir = tempfile::tempdir().expect("dir");

        kit_join(&three, &device, dir.path(), &secret)
            .await
            .unwrap_or_else(|e| panic!("{changes} key changes: {e:?}"));

        let conn = device.lock();
        assert!(sync_on(&conn));
        assert!(!has_institution(&conn, "local-only"), "replaced");
        assert_eq!(key_epoch(&conn), changes);
        let keys = keys::vault_keys(&conn).expect("keys").expect("keys");
        assert_eq!(keys.epochs().count() as u32, changes + 1, "every key");
        let kit_id = kit_keys(&three, &secret).id();
        let own = trust::entry(&conn, me(&conn))
            .expect("entry")
            .expect("pinned");
        assert_eq!(own.kind, CertKind::RecoveryKit);
        assert_eq!(own.issuer, kit_id);
        let kit = trust::entry(&conn, kit_id)
            .expect("entry")
            .expect("anchored");
        assert_eq!((kit.state, kit.confirmed), (TrustState::Trusted, true));
        // The newest epoch is checked: its head is known (the remover's snapshot names it).
        assert!(state::epochs(&conn).expect("epochs").contains_key(&changes));
        assert!(head::unverified_epochs(&conn)
            .expect("unverified")
            .is_empty());
    }
}

/// Revision §2.6 step 5 (i): after 31 days (no bundle on the server), a snapshot whose `Keys`
/// record names the newest epoch's head gives it, verified.
#[tokio::test]
async fn after_31_days_the_snapshots_keys_record_gives_the_newest_head() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    remove(&three, A, &[C]).await.expect("removed");
    round_of(&three.vault, &three.vault.a).await;
    snapshot_by(&three, &three.vault.a).await;
    let head_on_a = state::epochs(&three.vault.a.lock()).expect("epochs")[&1];
    three.vault.relay.advance_clock(31 * DAY);
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");

    let conn = device.lock();
    assert_eq!(
        state::epochs(&conn).expect("epochs").get(&1),
        Some(&head_on_a)
    );
    assert!(head::unverified_epochs(&conn)
        .expect("unverified")
        .is_empty());
    assert_eq!(state::kit_epoch_floor(&conn).expect("floor"), None);
}

/// Revision §2.6 step 5 (ii): a snapshot sealed under the newest epoch that does not name its
/// head (B uploaded it before it verified the head) stands in for it: an op sealed under an
/// older epoch past the snapshot is refused (`invalidChain`).
#[tokio::test]
async fn a_snapshot_under_the_newest_epoch_without_its_head_bounds_older_ops() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    remove(&three, A, &[C]).await.expect("removed");
    // B picks the key up; A has not sent its signed head, so B's head stays unverified.
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(key_epoch(&three.vault.b.lock()), 1);
    assert!(!head::unverified_epochs(&three.vault.b.lock())
        .expect("unverified")
        .is_empty());
    snapshot_by(&three, &three.vault.b).await;
    // C, removed but holding epoch 0's key, gets an op in past the snapshot (a server that takes
    // a retired token's writes).
    three.vault.relay.set_accept_retired_writes(true);
    institution(&three.c.lock(), "from-removed-c");
    crate::services::sync::engine::push::push_all(&three.vault.relay, &three.c, &three.vault.creds)
        .await
        .expect("C pushes under epoch 0");
    three.vault.relay.advance_clock(31 * DAY);
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");
    let floor = state::kit_epoch_floor(&device.lock()).expect("floor");
    let report = round_mode(&three, &device, RoundMode::Normal).await;

    assert_eq!(floor.map(|f| f.epoch), Some(1));
    assert_eq!(
        report.problem,
        Some(SyncProblem::InvalidChain),
        "{report:?}"
    );
    assert!(!has_institution(&device.lock(), "from-removed-c"));
}

/// Revision §2.6 step 5 (iii): a snapshot older than the newest epoch: ops sealed under an older
/// epoch past it wait until the envelope signer's signed head is found and verified (the server
/// has no bundle after 31 days), then apply.
#[tokio::test]
async fn a_snapshot_older_than_the_newest_epoch_waits_for_the_removers_signed_head() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    // B writes under epoch 0 after the snapshot and before the key change.
    institution(&three.vault.b.lock(), "b-before-the-change");
    round_of(&three.vault, &three.vault.b).await;
    remove(&three, A, &[C]).await.expect("removed");
    three.vault.relay.advance_clock(31 * DAY);
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");
    let waiting = head::unverified_epochs(&device.lock()).expect("unverified");
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        (waiting[0].epoch, waiting[0].head),
        (1, None),
        "no server head"
    );
    assert_eq!(waiting[0].by, A.to_hex(), "the envelope's signer");

    let held = round_mode(&three, &device, RoundMode::Normal).await;
    assert_eq!(
        held.problem,
        Some(SyncProblem::KeyChangeWaiting),
        "{held:?}"
    );
    assert!(!has_institution(&device.lock(), "b-before-the-change"));

    // A sends its signed head; the joiner verifies it and goes on.
    round_of(&three.vault, &three.vault.a).await;
    let report = round_mode(&three, &device, RoundMode::Normal).await;

    assert_eq!(report.problem, None, "{report:?}");
    let conn = device.lock();
    assert!(has_institution(&conn, "b-before-the-change"));
    assert!(head::unverified_epochs(&conn)
        .expect("unverified")
        .is_empty());
    assert_eq!(
        state::epochs(&conn).expect("epochs").get(&1),
        state::epochs(&three.vault.a.lock())
            .expect("epochs")
            .get(&1)
    );
}

/// Revision §2.6 step 5 (iii), within 30 days: the server's head waits for the signer's; a
/// signed head that says otherwise refutes it (`keyChangeUnverified`), and nothing past is
/// applied.
#[tokio::test]
async fn a_server_head_the_signers_record_contradicts_is_refuted() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    institution(&three.vault.b.lock(), "b-before-the-change");
    round_of(&three.vault, &three.vault.b).await;
    remove(&three, A, &[C]).await.expect("removed");
    // The server reports another head with the bundle than the one A signs.
    let vault_id = three.vault.creds.vault_id.to_b64();
    three.vault.relay.set_rekey_head(&vault_id, 1, 1);
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");
    assert_eq!(
        head::unverified_epochs(&device.lock()).expect("unverified")[0].head,
        Some(1)
    );
    round_of(&three.vault, &three.vault.a).await;

    let report = round_mode(&three, &device, RoundMode::Normal).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    assert!(!has_institution(&device.lock(), "b-before-the-change"));
}

/// Revision §2.6 step 3 (review of Task 16): a Kit a newer one replaced says when; a Kit the
/// server does not know (a wrong code, another sync) reads the one text that covers both; never
/// "the key changed".
#[tokio::test]
async fn a_replaced_or_unknown_kit_says_so() {
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let Outcome::Changed { kit: Some(new), .. } = new_kit(&three, A).await.expect("new Kit") else {
        panic!("a new Kit");
    };
    let replaced_at = three
        .vault
        .relay
        .get_kit(
            &three.vault.creds.vault_id.to_b64(),
            kit_keys(&three, &old).token(),
        )
        .await
        .expect_err("replaced");
    let RelayError::KitReplaced { replaced_at } = replaced_at else {
        panic!("{replaced_at:?}");
    };
    let device = newcomer();

    let error = recovery_credentials(&three.vault.relay, &device, typed(&three, &old))
        .await
        .expect_err("replaced");
    let SyncError::Local(app) = &error else {
        panic!("{error:?}");
    };
    let (key, params) = app.keyed().expect("keyed");
    assert_eq!(key, "validation.syncRecoveryKitReplaced");
    assert_eq!(params["at"], replaced_at);

    let stranger = KitSecret::generate().expect("seed");
    let error = recovery_credentials(&three.vault.relay, &device, typed(&three, &stranger))
        .await
        .expect_err("unknown");
    assert_eq!(
        key_of(&error).as_deref(),
        Some("validation.syncRecoveryVaultNotFound")
    );

    three
        .vault
        .relay
        .fail_next(Route::GetKit, RelayError::RateLimited);
    let error = recovery_credentials(&three.vault.relay, &device, typed(&three, &new.secret))
        .await
        .expect_err("rate limited");
    assert!(matches!(error, SyncError::RateLimited), "{error:?}");
    untouched(&device);
}

/// Replaces the Kit's envelope on the server (a retried setup's `PUT /kit` with the same token).
async fn replace_envelope(three: &Three, secret: &KitSecret, envelope: &[u8]) {
    let auth = keys::credentials(&three.vault.a.lock())
        .expect("creds")
        .expect("bound")
        .auth();
    three
        .vault
        .relay
        .put_kit(&auth, kit_keys(three, secret).token(), envelope)
        .await
        .expect("replaced");
}

/// An envelope for `secret`'s Kit with A's keys, signed by `signer` (its seed and certificate).
fn envelope_signed_by(
    three: &Three,
    secret: &KitSecret,
    signer: &identity::SignSeed,
    signer_cert: &identity::Certificate,
) -> Vec<u8> {
    let a = three.vault.a.lock();
    let creds = keys::credentials(&a).expect("creds").expect("bound");
    let kit_cert = trust::entry(&a, kit_keys(three, secret).id())
        .expect("entry")
        .expect("pinned")
        .cert;
    // The Kit itself is named, so the anchor holds and the signer check is what is tested.
    let named = [crate::services::sync::rekey::envelope::Confirmed {
        device: kit_cert.body.device,
        cert_hash: kit_cert.hash(),
    }];
    kit::seal(&a, &creds.keys, &kit_cert, signer, signer_cert, &named, NOW).expect("envelope")
}

/// Revision §2.6 steps 4 and 6: a tampered envelope, a Kit with no row or whose row says it was
/// replaced, and an envelope signed by a device not in the restored store or removed there are
/// all `syncRecoveryKitUnreadable`, with nothing replaced.
#[tokio::test]
async fn what_does_not_hold_up_is_unreadable_and_nothing_is_replaced() {
    type Setup = Box<
        dyn Fn(Three) -> std::pin::Pin<Box<dyn std::future::Future<Output = (Three, KitSecret)>>>,
    >;
    let cases: Vec<(&str, Setup)> = vec![
        (
            "a flipped byte in the envelope",
            Box::new(|three| {
                Box::pin(async move {
                    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
                    let read = three
                        .vault
                        .relay
                        .get_kit(
                            &three.vault.creds.vault_id.to_b64(),
                            kit_keys(&three, &secret).token(),
                        )
                        .await
                        .expect("read");
                    let mut bytes = read.envelope;
                    let last = bytes.len() - 1;
                    bytes[last] ^= 1;
                    replace_envelope(&three, &secret, &bytes).await;
                    (three, secret)
                })
            }),
        ),
        (
            "a Kit with no row",
            Box::new(|three| {
                Box::pin(async move {
                    // A Kit on the server that no device registered.
                    let secret = KitSecret::generate().expect("seed");
                    let envelope = {
                        let a = three.vault.a.lock();
                        let creds = keys::credentials(&a).expect("creds").expect("bound");
                        let seed = identity::own_seed(&a).expect("seed").expect("seed");
                        let own = trust::entry(&a, A).expect("entry").expect("pinned");
                        let cert =
                            cert::issue_kit(&seed, A, &secret.derive(&creds.vault_id.0), NOW)
                                .expect("Kit");
                        kit::seal(&a, &creds.keys, &cert, &seed, &own.cert, &[], NOW)
                            .expect("envelope")
                    };
                    three
                        .vault
                        .relay
                        .put_kit(
                            &three.vault.creds.auth(),
                            kit_keys(&three, &secret).token(),
                            &envelope,
                        )
                        .await
                        .expect("put");
                    (three, secret)
                })
            }),
        ),
        (
            "a Kit whose row carries its removal",
            Box::new(|three| {
                Box::pin(async move {
                    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
                    record_removal(&three.vault.a.lock(), A, kit_keys(&three, &secret).id());
                    settle(&three).await;
                    (three, secret)
                })
            }),
        ),
        (
            "a signer the restored store does not hold",
            Box::new(|three| {
                Box::pin(async move {
                    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
                    let stranger = identity::new_sign_seed().expect("seed");
                    let stranger_cert = cert::issue_self(
                        CertKind::Genesis,
                        &cert::CertFields {
                            vault_id: three.vault.creds.vault_id.0,
                            device: DeviceId([0x5e; 16]),
                            box_pub: [0x5e; 32],
                            issued_at: NOW,
                        },
                        &stranger,
                    )
                    .expect("cert");
                    let envelope = envelope_signed_by(&three, &secret, &stranger, &stranger_cert);
                    replace_envelope(&three, &secret, &envelope).await;
                    (three, secret)
                })
            }),
        ),
        (
            "a signer removed in the restored store",
            Box::new(|three| {
                Box::pin(async move {
                    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
                    let (c_seed, c_cert) = {
                        let c = three.c.lock();
                        (
                            identity::own_seed(&c).expect("seed").expect("seed"),
                            trust::entry(&c, C).expect("entry").expect("pinned").cert,
                        )
                    };
                    remove(&three, A, &[C]).await.expect("removed");
                    round_of(&three.vault, &three.vault.a).await;
                    let envelope = envelope_signed_by(&three, &secret, &c_seed, &c_cert);
                    replace_envelope(&three, &secret, &envelope).await;
                    (three, secret)
                })
            }),
        ),
    ];
    for (what, setup) in cases {
        let (three, secret) = setup(three().await).await;
        snapshot_by(&three, &three.vault.a).await;
        let device = newcomer();
        let dir = tempfile::tempdir().expect("dir");

        let error = kit_join(&three, &device, dir.path(), &secret)
            .await
            .expect_err(what);

        assert_eq!(
            key_of(&error).as_deref(),
            Some("validation.syncRecoveryKitUnreadable"),
            "{what}: {error:?}"
        );
        untouched(&device);
    }
}

/// Revision §2.6 step 6 (security review MEDIUM 3): with the bundles gone, the envelope's
/// confirmed set keeps a genuine device whose issuer was removed among the recipients.
#[tokio::test]
async fn the_confirmed_set_keeps_a_device_whose_issuer_was_removed() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    // B removes A, which paired C: B's key change names C.
    remove(&three, B, &[A]).await.expect("B removes A");
    round_of(&three.vault, &three.vault.b).await;
    snapshot_by(&three, &three.vault.b).await;
    three.vault.relay.advance_clock(31 * DAY);
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");

    let conn = device.lock();
    let recipients = trust::recipients(&conn, &[]).expect("recipients");
    let included: Vec<DeviceId> = recipients.included.iter().map(|e| e.device).collect();
    assert!(
        included.contains(&B) && included.contains(&C),
        "{included:?}"
    );
    assert!(!included.contains(&A));
    assert!(
        trust::entry(&conn, C)
            .expect("entry")
            .expect("pinned")
            .confirmed
    );
}

/// Revision §2.6 step 7: a Kit joiner pulls but sends nothing, not even its own row, until the
/// person decides; then its row goes out and another device pins its Kit-issued certificate.
#[tokio::test]
async fn a_kit_joiner_sends_nothing_until_the_person_decides() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");
    let joiner = me(&device.lock());
    let hold = join::after_kit_join(&device.lock())
        .expect("hold")
        .expect("held");
    let mut listed = hold.devices.clone();
    listed.sort();
    assert_eq!(listed, vec![A.to_hex(), B.to_hex(), C.to_hex()]);
    assert!(hold.newest_change_at.is_some());
    institution(&three.vault.a.lock(), "a-after-the-join");
    round_of(&three.vault, &three.vault.a).await;
    let calls = three.vault.relay.calls().len();

    let report = round_mode(&three, &device, RoundMode::Normal).await;

    assert_eq!(report.problem, None, "{report:?}");
    let since = three.vault.relay.calls()[calls..].to_vec();
    assert!(!since.contains(&Route::Push), "{since:?}");
    assert!(since.contains(&Route::Pull), "{since:?}");
    assert!(
        has_institution(&device.lock(), "a-after-the-join"),
        "pulls go on"
    );
    assert!(devices::device_row(&three.vault.a.lock(), joiner)
        .expect("rows")
        .is_none());

    join::lift_kit_join_hold(&device.lock()).expect("Mám je");
    round_mode(&three, &device, RoundMode::Normal).await;
    round_of(&three.vault, &three.vault.b).await;
    trust::refresh(&three.vault.b.lock(), NOW).expect("refresh");

    let entry = trust::entry(&three.vault.b.lock(), joiner)
        .expect("entry")
        .expect("B pins the joiner");
    assert_eq!(entry.kind, CertKind::RecoveryKit);
    assert_eq!(entry.state, TrustState::Trusted);
    let notices = trust::new_devices(&three.vault.b.lock()).expect("notices");
    assert!(notices
        .iter()
        .any(|n| n.device == joiner.to_hex() && n.via_kit));
}

/// Revision §2.6 step 7, "Odebrat je": a device the restored list still shows (one the person had
/// removed, in a stale copy a dishonest server served) is removed by a key change before anything
/// leaves the joiner; it opens only its notice.
#[tokio::test]
async fn odebrat_je_makes_a_key_change_that_leaves_the_listed_device_out() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");
    assert!(join::after_kit_join(&device.lock())
        .expect("hold")
        .is_some());

    let outcome = remove_in(&three.vault, &device, dir.path(), &[C], false)
        .await
        .expect("removed");
    join::lift_kit_join_hold(&device.lock()).expect("decided");

    assert!(
        matches!(outcome, Outcome::Changed { epoch: 1, .. }),
        "{outcome:?}"
    );
    let stored = three
        .vault
        .relay
        .stored_rekey(&three.vault.creds.vault_id.to_b64(), 1)
        .expect("the bundle");
    let c_secret = keys::device_box_secret(&three.c.lock())
        .expect("secret")
        .expect("box key");
    assert!(matches!(
        crate::services::sync::rekey::bundle::open(
            &stored,
            &three.vault.creds.vault_id.0,
            &c_secret
        )
        .expect("opens"),
        crate::services::sync::rekey::bundle::Opened::Notice { .. }
    ));
    let report = round_mode(&three, &device, RoundMode::Normal).await;
    assert_eq!(report.problem, None, "{report:?}");
    // C never heard of the joiner (its row waited behind the hold): C cannot verify the notice,
    // so it pauses rather than trusting the server's word, and holds no new key either way.
    let report = round_of(&three.vault, &three.c).await;
    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    assert_eq!(key_epoch(&three.c.lock()), 0);
}

/// Review of Tasks 18–19, I1 (the genesis gap): a vault-key holder who also has the Kit's seed
/// writes the Kit's row with a certificate of the Kit's key issued by a device of its own, plus
/// that device's genesis, and uploads a snapshot of it. The envelope (sealed by the honest remover)
/// names the genuine certificate, so the joiner refuses the forged anchor and trusts nothing of it.
#[tokio::test]
async fn a_forged_kit_row_with_a_forged_genesis_is_refused() {
    const D: DeviceId = DeviceId([0xd0; 16]);
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    {
        let b = three.vault.b.lock();
        let vault = three.vault.creds.vault_id.0;
        let d_seed = identity::new_sign_seed().expect("seed");
        let genesis = cert::issue_self(
            CertKind::Genesis,
            &cert::CertFields {
                vault_id: vault,
                device: D,
                box_pub: [0xd0; 32],
                issued_at: NOW,
            },
            &d_seed,
        )
        .expect("genesis");
        let forged = cert::issue_kit(&d_seed, D, &kit_keys(&three, &secret), NOW).expect("Kit");
        b.execute(
            "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
             VALUES (?1, 'k', 'Rogue', 'macos', '1', ?2, ?3)",
            rusqlite::params![D.to_hex(), NOW, genesis.to_b64()],
        )
        .expect("rogue row");
        b.execute(
            "UPDATE sync_devices SET cert = ?1 WHERE id = ?2",
            rusqlite::params![forged.to_b64(), forged.body.device.to_hex()],
        )
        .expect("forged Kit row");
    }
    snapshot_by(&three, &three.vault.b).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    let error = kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect_err("refused");

    assert_eq!(
        key_of(&error).as_deref(),
        Some("validation.syncRecoveryKitUnreadable"),
        "{error:?}"
    );
    untouched(&device);
}

/// Review of Tasks 18–19, I3, narrowed by As built R-75: an envelope signed by a device that has
/// `left` in the restored store is unreadable when it holds a key of an epoch after the one the
/// device left at (only a copy that rotated after it left could have sealed it).
#[tokio::test]
async fn an_envelope_signed_by_a_device_that_left_is_unreadable() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let (b_seed, b_cert) = {
        let b = three.vault.b.lock();
        (
            identity::own_seed(&b).expect("seed").expect("seed"),
            trust::entry(&b, B).expect("entry").expect("pinned").cert,
        )
    };
    assert!(
        crate::services::sync::setup::write_left_record(&three.vault.b.lock(), NOW).expect("left")
    );
    round_of(&three.vault, &three.vault.b).await;
    round_of(&three.vault, &three.vault.a).await;
    remove(&three, A, &[C]).await.expect("removed");
    round_of(&three.vault, &three.vault.a).await;
    // Epoch 1's keys, signed with the key of B, which left at epoch 0.
    let envelope = envelope_signed_by(&three, &secret, &b_seed, &b_cert);
    replace_envelope(&three, &secret, &envelope).await;
    snapshot_by(&three, &three.vault.a).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    let error = kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect_err("refused");

    assert_eq!(
        key_of(&error).as_deref(),
        Some("validation.syncRecoveryKitUnreadable"),
        "{error:?}"
    );
    untouched(&device);
}

/// A copy of a device that left (a backup holds its signing key and, since leaving does not rotate
/// the key, the vault's current key) re-seals the Kit's envelope with a confirmed set that names a
/// genesis it made itself. The epoch comparison of As built R-75 is self-asserted, so what protects
/// the joiner is that a left signer vouches for nothing: the Kit still opens (R-75's liveness), but
/// the planted device is neither trusted nor confirmed on the joiner.
#[tokio::test]
async fn a_left_signers_confirmed_set_vouches_for_nothing() {
    const D: DeviceId = DeviceId([0xd0; 16]);
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let (b_seed, b_cert) = {
        let b = three.vault.b.lock();
        (
            identity::own_seed(&b).expect("seed").expect("seed"),
            trust::entry(&b, B).expect("entry").expect("pinned").cert,
        )
    };
    assert!(
        crate::services::sync::setup::write_left_record(&three.vault.b.lock(), NOW).expect("left")
    );
    round_of(&three.vault, &three.vault.b).await;
    round_of(&three.vault, &three.vault.a).await;
    // The planted device: a genesis of its own in a row of the restored data.
    let planted = {
        let vault = three.vault.creds.vault_id.0;
        let d_seed = identity::new_sign_seed().expect("seed");
        let genesis = cert::issue_self(
            CertKind::Genesis,
            &cert::CertFields {
                vault_id: vault,
                device: D,
                box_pub: [0xd0; 32],
                issued_at: NOW,
            },
            &d_seed,
        )
        .expect("genesis");
        three
            .vault
            .a
            .lock()
            .execute(
                "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert)                  VALUES (?1, 'k', 'Rogue', 'macos', '1', ?2, ?3)",
                rusqlite::params![D.to_hex(), NOW, genesis.to_b64()],
            )
            .expect("rogue row");
        genesis
    };
    // Epoch 0's keys, signed by the copy of B, naming the Kit and the planted genesis.
    let envelope = {
        let a = three.vault.a.lock();
        let creds = keys::credentials(&a).expect("creds").expect("bound");
        let kit_cert = trust::entry(&a, kit_keys(&three, &secret).id())
            .expect("entry")
            .expect("pinned")
            .cert;
        let named = [
            crate::services::sync::rekey::envelope::Confirmed {
                device: kit_cert.body.device,
                cert_hash: kit_cert.hash(),
            },
            crate::services::sync::rekey::envelope::Confirmed {
                device: D,
                cert_hash: planted.hash(),
            },
        ];
        kit::seal(&a, &creds.keys, &kit_cert, &b_seed, &b_cert, &named, NOW).expect("envelope")
    };
    replace_envelope(&three, &secret, &envelope).await;
    round_of(&three.vault, &three.vault.a).await;
    snapshot_by(&three, &three.vault.a).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("the Kit still opens");

    let conn = device.lock();
    let planted_entry = trust::entry(&conn, D).expect("entry");
    assert!(
        planted_entry
            .as_ref()
            .is_none_or(|e| e.state != TrustState::Trusted && !e.confirmed),
        "the planted device is not trusted: {planted_entry:?}"
    );
}

/// Review of Tasks 18–19, I3: a server that replays an older envelope (from before a key change)
/// beside the current snapshot gives the joiner no key that opens it: refused, nothing replaced.
#[tokio::test]
async fn a_stale_envelope_with_a_current_snapshot_is_refused() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let stale = three
        .vault
        .relay
        .get_kit(
            &three.vault.creds.vault_id.to_b64(),
            kit_keys(&three, &secret).token(),
        )
        .await
        .expect("read")
        .envelope;
    remove(&three, A, &[C]).await.expect("removed");
    round_of(&three.vault, &three.vault.a).await;
    snapshot_by(&three, &three.vault.a).await;
    replace_envelope(&three, &secret, &stale).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    assert!(kit_join(&three, &device, dir.path(), &secret)
        .await
        .is_err());

    untouched(&device);
}

/// Review of Tasks 18–19, I3, and owner residual R1: the current envelope beside a stale snapshot
/// (from before a removal whose record never reached it): the removed device C looks active in
/// what was restored, and nothing in the join can tell (the envelope's confirmed set cannot
/// distinguish "removed" from "paired after the envelope"). What protects the person is the
/// hold: C is listed in "Data jsou zpět", nothing is sent while it stands, and "Odebrat je"
/// leaves C out before the first change goes anywhere.
#[tokio::test]
async fn a_stale_snapshot_lists_the_removed_device_under_the_hold() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    // A removes C, and its rows (the removal record, its next snapshot) never reach the server.
    three.vault.relay.set_swallow_pushes(true);
    remove(&three, A, &[C]).await.expect("removed");
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");

    let conn = device.lock();
    let hold = join::after_kit_join(&conn).expect("hold").expect("held");
    assert!(hold.devices.contains(&C.to_hex()), "{hold:?}");
    let confirmed = trust::entry(&conn, C)
        .expect("entry")
        .is_some_and(|e| e.confirmed);
    assert!(!confirmed, "the envelope never confirms C");
    assert!(crate::services::sync::engine::runtime::uploads_held(&conn).expect("held"));
}

/// Review of Tasks 18–19, I3: while the hold stands no snapshot, no document and no daily pass
/// goes out, even in a round that would force a snapshot; once lifted, uploads may run.
#[tokio::test]
async fn the_hold_stops_snapshots_uploads_and_the_daily_pass() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("join");
    assert!(crate::services::sync::engine::runtime::uploads_held(&device.lock()).expect("held"));
    let calls = three.vault.relay.calls().len();

    let report = round_mode(&three, &device, RoundMode::FirstSnapshot).await;

    assert!(!report.snapshot_uploaded, "{report:?}");
    let since = three.vault.relay.calls()[calls..].to_vec();
    for route in [
        Route::Push,
        Route::PutSnapshot,
        Route::PutBlob,
        Route::ListBlobs,
        Route::DeleteBlob,
    ] {
        assert!(!since.contains(&route), "{route:?}: {since:?}");
    }
    join::lift_kit_join_hold(&device.lock()).expect("Mám je");
    assert!(!crate::services::sync::engine::runtime::uploads_held(&device.lock()).expect("held"));
}

/// Task 33 live check: a Kit joiner removes the devices the restored list showed ("Odebrat je")
/// and so makes the next Kit; a new profile then joins with that Kit. The envelope's signer is the
/// first joiner, whose certificate the replaced Kit issued: the second joiner must pin it (and the
/// replaced Kit that vouches for it) from the restored rows, or the Kit is unreadable.
#[tokio::test]
async fn a_kit_made_by_a_kit_joiner_lets_the_next_device_join() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    let first = newcomer();
    let first_dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &first, first_dir.path(), &secret)
        .await
        .expect("the first Kit join");
    let first_id = me(&first.lock());
    let outcome = remove_in(&three.vault, &first, first_dir.path(), &[A, B, C], true)
        .await
        .expect("removed");
    join::lift_kit_join_hold(&first.lock()).expect("decided");
    let next = new_secret(outcome);
    snapshot_by(&three, &first).await;
    let second = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &second, dir.path(), &next)
        .await
        .expect("the second Kit join");

    let conn = second.lock();
    assert!(sync_on(&conn));
    let signer = trust::entry(&conn, first_id)
        .expect("entry")
        .expect("the signer is pinned");
    assert_eq!(signer.state, TrustState::Trusted);
    // The signer made the Kit, so it speaks though its issuer was replaced (As built R-106).
    assert!(signer.confirmed, "{signer:?}");
    let kit = trust::entry(&conn, kit_keys(&three, &next).id())
        .expect("entry")
        .expect("anchored");
    assert_eq!(kit.state, TrustState::Trusted);
    // The replaced Kit vouches for the first joiner only through its past: it is pinned removed.
    let replaced = trust::entry(&conn, kit_keys(&three, &secret).id())
        .expect("entry")
        .expect("the replaced Kit is pinned");
    assert_eq!(replaced.state, TrustState::Removed);
    assert_eq!(replaced.state_by, Some(first_id));
}

/// Task 33, the same gap off the chain: a device that joined with a Kit since replaced still syncs;
/// a new profile joining with the replacing Kit pins it (its issuer, the replaced Kit, vouches for
/// what it issued before it was replaced) and seals the next key to it.
#[tokio::test]
async fn a_device_an_older_kit_issued_is_kept_by_the_next_kit_joiner() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    let first = newcomer();
    let first_dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &first, first_dir.path(), &secret)
        .await
        .expect("the first Kit join");
    let first_id = me(&first.lock());
    join::lift_kit_join_hold(&first.lock()).expect("Mám je");
    round_mode(&three, &first, RoundMode::Normal).await;
    round_of(&three.vault, &three.vault.a).await;
    trust::refresh(&three.vault.a.lock(), NOW).expect("refresh");
    let next = new_secret(new_kit(&three, A).await.expect("a new Kit"));
    round_of(&three.vault, &three.vault.a).await;
    snapshot_by(&three, &three.vault.a).await;
    let second = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &second, dir.path(), &next)
        .await
        .expect("the second Kit join");

    let conn = second.lock();
    let entry = trust::entry(&conn, first_id)
        .expect("entry")
        .expect("the first joiner is pinned");
    assert_eq!(entry.state, TrustState::Trusted);
    let recipients = trust::recipients(&conn, &[]).expect("recipients");
    assert!(
        recipients.included.iter().any(|e| e.device == first_id),
        "{:?}",
        recipients.excluded
    );
}

/// Task 33's live run: B removes A (the first device, the first Kit's creator) with a new Kit; a
/// new profile joins with that Kit and removes B and C with the next one; another profile joins
/// with it. B, the replaced Kit's creator, is left out for its removed issuer while the store is
/// built (its confirmation comes with the envelope), which must not keep the replaced Kit out.
#[tokio::test]
async fn a_kit_chain_through_a_removed_first_device_still_joins() {
    let three = three().await;
    with_kit(&three, A, Some(KitStorage::Paper)).await;
    let outcome = remove_in(&three.vault, &three.vault.b, three.dir.path(), &[A], true)
        .await
        .expect("B removes A");
    let second_kit = new_secret(outcome);
    settle_remaining(&three).await;
    snapshot_by(&three, &three.vault.b).await;
    let first = newcomer();
    let first_dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &first, first_dir.path(), &second_kit)
        .await
        .expect("the first Kit join");
    let first_id = me(&first.lock());
    let outcome = remove_in(&three.vault, &first, first_dir.path(), &[B, C], true)
        .await
        .expect("removed");
    join::lift_kit_join_hold(&first.lock()).expect("decided");
    let third_kit = new_secret(outcome);
    snapshot_by(&three, &first).await;
    let second = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &second, dir.path(), &third_kit)
        .await
        .expect("the second Kit join");

    let conn = second.lock();
    let signer = trust::entry(&conn, first_id)
        .expect("entry")
        .expect("the signer is pinned");
    assert_eq!(signer.state, TrustState::Trusted);
}

/// B and C sync after A was removed.
async fn settle_remaining(three: &Three) {
    for _ in 0..2 {
        for device in [&three.vault.b, &three.c] {
            round_of(&three.vault, device).await;
        }
    }
}

/// Protocol 3: a Kit joiner that read its keys just before a key change reads the snapshot with a
/// token that was retired meanwhile. The server refuses it (`403`), which ends the join the way a
/// snapshot sealed under a key the joiner lacks does (As built R-73): nothing is replaced, and
/// the person reads the Kit again.
#[tokio::test]
async fn a_join_overtaken_by_a_key_change_says_try_again() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    snapshot_by(&three, &three.vault.a).await;
    let (creds, join) =
        recovery_credentials(&three.vault.relay, &newcomer(), typed(&three, &secret))
            .await
            .expect("the keys are read");
    remove(&three, A, &[C]).await.expect("removed");
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    let error = join_new_device(
        &three.vault.relay,
        &device,
        creds,
        "Laptop",
        agreed(&device),
        JoinIdentity::RecoveryKit(Box::new(join)),
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("refused");

    assert_eq!(
        key_of(&error).as_deref(),
        Some("validation.syncRecoveryKitUnreadable"),
        "{error:?}"
    );
    untouched(&device);
}
