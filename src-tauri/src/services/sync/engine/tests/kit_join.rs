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
fn newcomer() -> TestConn {
    let conn = TestConn::new(migrated());
    institution(&conn.lock(), "local-only");
    conn
}

fn key_of(error: &SyncError) -> Option<String> {
    match error {
        SyncError::Local(AppError::Validation(detail)) => {
            Some(detail.split('|').next().unwrap_or_default().to_string())
        }
        _ => None,
    }
}

async fn round_mode(three: &Three, device: &TestConn, mode: RoundMode) -> RoundReport {
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
async fn snapshot_by(three: &Three, device: &TestConn) {
    let report = round_mode(three, device, RoundMode::FirstSnapshot).await;
    assert!(report.snapshot_uploaded, "{report:?}");
}

/// What a person types from the Kit `secret`: its v2 code, grouped, and the server's address.
fn typed(three: &Three, secret: &KitSecret) -> crate::services::sync::recovery::KitInput {
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
async fn kit_join(
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

fn me(conn: &rusqlite::Connection) -> DeviceId {
    state::load(conn).expect("state").expect("row").device
}

fn untouched(device: &TestConn) {
    let conn = device.lock();
    assert!(has_institution(&conn, "local-only"), "nothing replaced");
    assert!(!sync_on(&conn));
}

/// The new Kit a key change made.
fn new_secret(outcome: Outcome) -> KitSecret {
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
    kit::seal(&a, &creds.keys, &kit_cert, signer, signer_cert, &[], NOW).expect("envelope")
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
