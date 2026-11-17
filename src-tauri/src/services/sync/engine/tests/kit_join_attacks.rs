//! A Kit join made to trust a planted device (final review of phase 5, I-3; As built R-106): a
//! holder of the vault's current key (a backup copy of a device that left, a superseded copy, a
//! replaced Kit's paper) re-seals the Kit's envelope, signs it with a key of its own choosing and
//! names in its confirmed set a genesis it made. The joiner takes the confirmed set (and any
//! genesis it names) only from a signer that speaks on the store the restored rows give: pinned
//! with that certificate, `trusted`, not superseded and not left out for its issuer. Otherwise the
//! Kit opens but nothing of the set is taken, or the signer is not pinned at all and the Kit is
//! unreadable.

use super::kit_join::{key_of, kit_join, new_secret, newcomer, snapshot_by, untouched};
use super::rekey_fixture::*;
use super::*;
use crate::services::sync::identity::trust::{self, TrustState};
use crate::services::sync::identity::{self, cert, CertKind, KitSecret, SignSeed};
use crate::services::sync::rekey::envelope::Confirmed;
use crate::services::sync::rekey::kit::{self, KitStorage};

/// The planted genesis the attacker wants trusted.
const P: DeviceId = DeviceId([0xe0; 16]);
/// The attacker's signer when it is a device of its own.
const X: DeviceId = DeviceId([0xe1; 16]);

fn fields(three: &Three, device: DeviceId, issued_at: i64) -> cert::CertFields {
    cert::CertFields {
        vault_id: three.vault.creds.vault_id.0,
        device,
        box_pub: [device.0[0]; 32],
        issued_at,
    }
}

/// Writes `cert`'s row on the holder, as a vault-key holder's synced write would land there.
fn write_row(three: &Three, cert: &cert::Certificate) {
    three
        .device(three.holder())
        .lock()
        .execute(
            "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
             VALUES (?1, 'k', 'Rogue', 'macos', '1', ?2, ?3)",
            rusqlite::params![cert.body.device.to_hex(), NOW, cert.to_b64()],
        )
        .expect("rogue row");
}

/// A genesis of the attacker's own, in a row of the vault.
fn planted_genesis(three: &Three, device: DeviceId) -> (SignSeed, cert::Certificate) {
    let seed = identity::new_sign_seed().expect("seed");
    let genesis =
        cert::issue_self(CertKind::Genesis, &fields(three, device, NOW), &seed).expect("genesis");
    write_row(three, &genesis);
    (seed, genesis)
}

/// `device`'s signing seed and pinned certificate.
fn keys_of(three: &Three, device: DeviceId) -> (SignSeed, cert::Certificate) {
    let conn = three.device(device).lock();
    (
        identity::own_seed(&conn).expect("seed").expect("seed"),
        trust::entry(&conn, device)
            .expect("entry")
            .expect("pinned")
            .cert,
    )
}

/// The Kit `secret`'s envelope with the holder's keys, signed by `signer`, naming the Kit and
/// `named`.
fn sealed_naming(
    three: &Three,
    secret: &KitSecret,
    signer: &SignSeed,
    signer_cert: &cert::Certificate,
    named: &[&cert::Certificate],
) -> Vec<u8> {
    let a = three.device(three.holder()).lock();
    let creds = keys::credentials(&a).expect("creds").expect("bound");
    let kit_cert = trust::entry(&a, kit_keys(three, secret).id())
        .expect("entry")
        .expect("pinned")
        .cert;
    let set: Vec<Confirmed> = std::iter::once(&kit_cert)
        .chain(named.iter().copied())
        .map(|cert| Confirmed {
            device: cert.body.device,
            cert_hash: cert.hash(),
        })
        .collect();
    kit::seal(&a, &creds.keys, &kit_cert, signer, signer_cert, &set, NOW).expect("envelope")
}

/// The attacker's envelope goes up and the holder uploads a snapshot holding the rows.
async fn publish(three: &Three, secret: &KitSecret, envelope: &[u8]) {
    let holder = three.device(three.holder());
    let auth = keys::credentials(&holder.lock())
        .expect("creds")
        .expect("bound")
        .auth();
    three
        .vault
        .relay
        .put_kit(&auth, kit_keys(three, secret).token(), envelope)
        .await
        .expect("replaced");
    round_of(&three.vault, holder).await;
    snapshot_by(three, holder).await;
}

trait Holder {
    /// The device whose keys, rows and snapshot the attacker uses: A, or C once A is removed.
    fn holder(&self) -> DeviceId;
}

impl Holder for Three {
    fn holder(&self) -> DeviceId {
        let removed = trust::entry(&self.c.lock(), A)
            .expect("entry")
            .is_some_and(|e| e.state == TrustState::Removed);
        if removed {
            C
        } else {
            A
        }
    }
}

fn unreadable(result: crate::services::sync::error::SyncResult<()>, device: &TestConn) {
    let error = result.expect_err("refused");
    assert_eq!(
        key_of(&error).as_deref(),
        Some("validation.syncRecoveryKitUnreadable"),
        "{error:?}"
    );
    untouched(device);
}

/// The Kit opened, and neither the planted genesis nor anything the attacker signed with is
/// pinned trusted-and-confirmed or a recipient of the next key.
fn took_nothing(device: &TestConn, attackers: &[DeviceId]) {
    let conn = device.lock();
    assert!(sync_on(&conn), "the Kit opens");
    let planted = trust::entry(&conn, P).expect("entry");
    assert!(planted.is_none(), "the planted genesis: {planted:?}");
    let recipients = trust::recipients(&conn, &[]).expect("recipients");
    for attacker in std::iter::once(&P).chain(attackers) {
        let entry = trust::entry(&conn, *attacker).expect("entry");
        assert!(
            entry.as_ref().is_none_or(|e| !e.confirmed),
            "{attacker:?} is confirmed: {entry:?}"
        );
        assert!(
            !recipients.included.iter().any(|e| e.device == *attacker),
            "{attacker:?} gets the next key"
        );
    }
}

/// The planted genesis signs the envelope itself and names itself: it is no vouch here (only the
/// Kit's chain is), so the signer is not pinned and the Kit is unreadable.
#[tokio::test]
async fn a_planted_genesis_signer_is_refused() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let (x_seed, x_cert) = planted_genesis(&three, X);
    let envelope = sealed_naming(&three, &secret, &x_seed, &x_cert, &[&x_cert]);
    publish(&three, &secret, &envelope).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    let result = kit_join(&three, &device, dir.path(), &secret).await;

    unreadable(result, &device);
}

/// B re-joined as a successor; a copy of B's older ID still holds its key and signs the envelope,
/// naming a planted genesis. A superseded ID lives on only as a copy: the Kit opens, nothing named
/// is taken.
#[tokio::test]
async fn a_superseded_signer_vouches_for_nothing() {
    const B2: DeviceId = DeviceId([0xb2; 16]);
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let (b_seed, b_cert) = keys_of(&three, B);
    let successor_seed = identity::new_sign_seed().expect("seed");
    let successor =
        cert::issue_rejoin(&b_seed, B, &fields(&three, B2, NOW), &successor_seed).expect("re-join");
    write_row(&three, &successor);
    let (_, planted) = planted_genesis(&three, P);
    let envelope = sealed_naming(&three, &secret, &b_seed, &b_cert, &[&planted, &b_cert]);
    publish(&three, &secret, &envelope).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("the Kit still opens");

    took_nothing(&device, &[]);
    let conn = device.lock();
    let old = trust::entry(&conn, B).expect("entry").expect("pinned");
    assert!(!old.confirmed, "{old:?}");
}

/// B left; a copy of it (its key survives in a backup) issues a re-join successor dated before it
/// left, which the joiner pins but leaves out for a late vouch (one restored snapshot gives equal
/// positions, R-23). That successor signs the envelope and names a planted genesis: it speaks for
/// nothing, so the Kit opens and nothing named is taken.
#[tokio::test]
async fn a_late_vouch_signer_vouches_for_nothing() {
    const X2: DeviceId = DeviceId([0xe2; 16]);
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let (b_seed, _) = keys_of(&three, B);
    assert!(
        crate::services::sync::setup::write_left_record(&three.vault.b.lock(), NOW).expect("left")
    );
    round_of(&three.vault, &three.vault.b).await;
    round_of(&three.vault, &three.vault.a).await;
    let x_seed = identity::new_sign_seed().expect("seed");
    let x_cert =
        cert::issue_rejoin(&b_seed, B, &fields(&three, X2, NOW), &x_seed).expect("re-join");
    write_row(&three, &x_cert);
    let (_, planted) = planted_genesis(&three, P);
    let envelope = sealed_naming(&three, &secret, &x_seed, &x_cert, &[&planted, &x_cert]);
    publish(&three, &secret, &envelope).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("the Kit still opens");

    took_nothing(&device, &[X2]);
    let conn = device.lock();
    let signer = trust::entry(&conn, X2).expect("entry").expect("pinned");
    assert_eq!(signer.state, TrustState::Trusted);
}

/// A replaced Kit's paper still derives its key. A device it "issues" after its removal record is
/// no vouch of it (the replaced Kit speaks only for what it issued before it was replaced): the
/// signer is not pinned and the replacing Kit is unreadable.
#[tokio::test]
async fn a_replaced_kits_device_issued_after_its_removal_is_refused() {
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let next = new_secret(new_kit(&three, A).await.expect("a new Kit"));
    round_of(&three.vault, &three.vault.a).await;
    let y_seed = identity::new_sign_seed().expect("seed");
    let y_cert = cert::issue_kit_join(
        &kit_keys(&three, &old),
        &fields(&three, X, NOW + 60),
        &y_seed,
    )
    .expect("Kit-joined");
    write_row(&three, &y_cert);
    let (_, planted) = planted_genesis(&three, P);
    let envelope = sealed_naming(&three, &next, &y_seed, &y_cert, &[&planted, &y_cert]);
    publish(&three, &next, &envelope).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    let result = kit_join(&three, &device, dir.path(), &next).await;

    unreadable(result, &device);
}

/// The same with the device dated before the replaced Kit's removal: the joiner pins it (the
/// replaced Kit vouches for what it issued until then) but leaves it out, its issuer removed, so
/// it speaks for nothing: the Kit opens and nothing named is taken.
#[tokio::test]
async fn a_replaced_kits_backdated_device_vouches_for_nothing() {
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let next = new_secret(new_kit(&three, A).await.expect("a new Kit"));
    round_of(&three.vault, &three.vault.a).await;
    let y_seed = identity::new_sign_seed().expect("seed");
    let y_cert = cert::issue_kit_join(
        &kit_keys(&three, &old),
        &fields(&three, X, NOW - 60),
        &y_seed,
    )
    .expect("Kit-joined");
    write_row(&three, &y_cert);
    let (_, planted) = planted_genesis(&three, P);
    let envelope = sealed_naming(&three, &next, &y_seed, &y_cert, &[&planted, &y_cert]);
    publish(&three, &next, &envelope).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &next)
        .await
        .expect("the Kit still opens");

    took_nothing(&device, &[X]);
}

/// C removes A, which paired B; B (left out on the joiner for its removed issuer, and not the one
/// that removed it) signs the envelope and names a planted genesis. Only a remover of its own
/// issuer or the Kit's creator speaks while left out for its issuer: the Kit opens, nothing named
/// is taken, and B shows as one to check (the cost As built R-106 accepts).
#[tokio::test]
async fn a_signer_whose_issuer_another_device_removed_vouches_for_nothing() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    remove(&three, C, &[A]).await.expect("C removes A");
    round_of(&three.vault, &three.c).await;
    let (b_seed, b_cert) = keys_of(&three, B);
    let (_, planted) = planted_genesis(&three, P);
    let envelope = sealed_naming(&three, &secret, &b_seed, &b_cert, &[&planted, &b_cert]);
    publish(&three, &secret, &envelope).await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    kit_join(&three, &device, dir.path(), &secret)
        .await
        .expect("the Kit still opens");

    took_nothing(&device, &[B]);
}
