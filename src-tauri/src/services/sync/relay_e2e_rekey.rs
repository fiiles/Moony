//! End to end, phase 5: key changes (removals, new Recovery Kits) and the Kit routes against a
//! real Moony sync server built from the `feat/recovery-kit` branch (Moony-relay-kit, server
//! protocol 2 with the `kit` feature). Every test is ignored; run them one at a time with the
//! other `relay_e2e` suites (see `relay_e2e` for the recipe), or alone:
//!
//! ```text
//! # In Moony-relay-kit (`npm ci && npm run build`), a fresh data folder and a fixed setup code:
//! SETUP_CODE=moony-e2e-setup-code DATA_DIR=$(mktemp -d) PORT=8795 node dist/main.js
//! cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8795 \
//!   cargo test --lib relay_e2e_rekey -- --ignored --test-threads=1
//! ```
//!
//! Failed Kit reads are limited to 10 a minute per client address (a right token is never refused),
//! so the Kit-failure test leaves that quota used up for a minute; no other test here reads a Kit
//! with a wrong token. A device here is
//! a profile like the other suites': `A` set sync up (genesis, its Recovery Kit), `B` and `C`
//! joined by pairing (`pairing::holder::announced_payload` + `join_new_device`, the paired
//! certificates), the key changes are `rotate::remove` and `pickup::pick_up` as the app runs them.

use std::sync::mpsc;
use std::time::Duration;

use zeroize::Zeroizing;

use super::binding::NoBinding;
use super::engine::tests::agreed;
use super::error::SyncResult;
use super::hlc::DeviceId;
use super::identity::{trust, KitSecret};
use super::join::{
    join_new_device, lift_kit_join_hold, recovery_credentials, JoinIdentity, KIT_REPLACED,
};
use super::keys::Credentials;
use super::pairing::holder;
use super::pairing::SlotPayload;
use super::recovery::{encode_kit_code, format_recovery_code, read_kit_input};
use super::rekey::kit;
use super::rekey::rotate::{self, Outcome, Removal};
use super::relay::http::{HttpRelay, DEFAULT_RELAY_URL};
use super::relay::{BlobBody, KitRotation, Relay, RelayError};
use super::relay_e2e::{
    add_finances, converged, first_device_with_kit, now, relay_at, relay_url, sync_until_allowed,
    Device,
};
use super::setup::{leave_best_effort, turn_off_between_rounds, KitDraft, LEAVE_BUDGET};
use super::state;
use crate::error::AppError;

/// This device's ID.
fn me(device: &Device) -> DeviceId {
    state::load(&device.db.lock())
        .expect("state")
        .expect("row")
        .device
}

/// A device `holder` pairs (the holder's snapshot is on the server, its payload carries the
/// ticket), joined by replace as the app does it.
async fn pair(relay: &HttpRelay, holder: &Device, name: &str) -> Device {
    holder.round(relay).await;
    let payload = holder::announced_payload(&holder.db.lock(), now())
        .expect("payload")
        .expect("sync is on");
    let SlotPayload {
        creds,
        heads,
        identity,
    } = payload;
    let joiner = Device::new();
    join_new_device(
        relay,
        &joiner.db,
        creds,
        name,
        agreed(&joiner.db),
        JoinIdentity::Paired(identity.expect("identity"), heads),
        joiner.dir(),
        &NoBinding,
        now(),
    )
    .await
    .expect("join");
    joiner
}

/// Rounds on every device until each has sent and received everything (two passes).
async fn settle(relay: &HttpRelay, devices: &[&Device]) {
    for _ in 0..2 {
        for device in devices {
            device.round(relay).await;
        }
    }
}

/// A three-device vault with a Recovery Kit: `A` set it up, `B` and `C` were paired by `A`, all
/// verified, A's finances everywhere.
async fn three(url: &str) -> (HttpRelay, Device, Device, Device, KitDraft) {
    let relay = relay_at(url);
    let (a, _, kit) = first_device_with_kit(&relay, url, "A").await;
    add_finances(&a);
    a.round(&relay).await;
    let b = pair(&relay, &a, "B").await;
    let c = pair(&relay, &a, "C").await;
    settle(&relay, &[&a, &b, &c]).await;
    for device in [&a, &b, &c] {
        let conn = device.db.lock();
        trust::refresh(&conn, now()).expect("refresh");
        let trusted = trust::entries(&conn)
            .expect("entries")
            .iter()
            .filter(|entry| entry.state == trust::TrustState::Trusted)
            .count();
        assert!(trusted >= 3, "every device verified the others: {trusted}");
    }
    (relay, a, b, c, kit)
}

/// `device` removes `removing` with the plan as it stands (the person confirmed it).
async fn remove(
    relay: &HttpRelay,
    device: &Device,
    removing: &[DeviceId],
    new_kit: bool,
) -> SyncResult<Outcome> {
    let consent = rotate::plan(&device.db.lock(), removing, &[], new_kit, now())
        .expect("plan")
        .consent;
    rotate::remove(
        relay,
        &device.db,
        Removal {
            removing,
            new_kit,
            consent: &consent,
        },
        device.dir(),
        &|| {},
        now(),
    )
    .await
}

fn changed(outcome: &Outcome) -> u32 {
    match outcome {
        Outcome::Changed { epoch, .. } => *epoch,
        other => panic!("the key did not change: {other:?}"),
    }
}

fn new_secret(outcome: Outcome) -> KitSecret {
    match outcome {
        Outcome::Changed { kit: Some(kit), .. } => kit.secret,
        other => panic!("no new Kit: {other:?}"),
    }
}

fn add_account(device: &Device, id: &str) {
    device.exec(&format!(
        "INSERT INTO bank_accounts (id, name, currency, balance) \
           VALUES ('{id}', 'Account {id}', 'EUR', '1.00')"
    ));
}

fn has_account(device: &Device, id: &str) -> bool {
    device.count(&format!(
        "SELECT COUNT(*) FROM bank_accounts WHERE id = '{id}'"
    )) == 1
}

fn removed_row(device: &Device, id: DeviceId) -> bool {
    device.count(&format!(
        "SELECT COUNT(*) FROM sync_devices WHERE id = '{}' AND removed = 1",
        id.to_hex()
    )) == 1
}

/// What a person types from the Kit `secret`: its v2 code, grouped, and the server's address.
fn typed(url: &str, creds: &Credentials, secret: &KitSecret) -> super::recovery::KitInput {
    let code = encode_kit_code(&creds.vault_id, secret, &creds.account).expect("code");
    read_kit_input(&format_recovery_code(&code), Some(url), DEFAULT_RELAY_URL).expect("input")
}

/// A new profile joins with the Kit `secret`, then syncs with `others`.
async fn kit_join(
    relay: &HttpRelay,
    url: &str,
    like: &Credentials,
    secret: &KitSecret,
) -> SyncResult<Device> {
    let device = Device::new();
    let (creds, join) =
        sync_until_allowed(|| recovery_credentials(relay, &device.db, typed(url, like, secret)))
            .await?;
    join_new_device(
        relay,
        &device.db,
        creds,
        "Laptop",
        agreed(&device.db),
        JoinIdentity::RecoveryKit(Box::new(join)),
        device.dir(),
        &NoBinding,
        now(),
    )
    .await?;
    lift_kit_join_hold(&device.db.lock()).expect("decided");
    Ok(device)
}

fn random_key() -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0u8; 32]);
    super::keys::os_random(&mut key[..]).expect("random");
    key
}

/// `creds` one key change on, as a raw rotation would take it: the next epoch's key and token.
fn next_creds(creds: &Credentials) -> Credentials {
    let mut next = creds.clone();
    next.keys = creds.keys.with_next(random_key()).expect("next key");
    next
}

fn kit_error_key(error: crate::services::sync::error::SyncError) -> String {
    match error.to_app_error() {
        AppError::Validation(key) => key.split('|').next().unwrap_or_default().to_string(),
        other => panic!("not a validation key: {other:?}"),
    }
}

const FILLER_ENVELOPE: &[u8] = b"an envelope the server stores as opaque bytes";

/// The old token keeps what the rotation promises (pull, rekey fetch, vault read) and loses the
/// rest; the new token takes over. The bundle and the Kit's envelope are opaque to the server.
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn the_old_token_is_read_only_after_a_rotation() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (a, _, _) = first_device_with_kit(&relay, &url, "A").await;
    add_finances(&a);
    a.round(&relay).await;
    let old = a.credentials();
    let new = next_creds(&old);

    let rotated = relay
        .rotate_token_with_kit(
            &old.auth(),
            &new.token(),
            b"a bundle the server never opens",
            KitRotation {
                envelope: FILLER_ENVELOPE,
                token: None,
            },
        )
        .await
        .expect("rotate");
    assert_eq!(rotated.rotation, 1);

    let info = relay.vault_info(&old.auth()).await.expect("vault read");
    assert_eq!(info.rotation, 1);
    assert!(info.kit.is_some(), "the vault still has its Kit");
    relay.pull(&old.auth(), 0, 100).await.expect("pull");
    let page = relay.rekeys(&old.auth(), 0, 8).await.expect("rekey fetch");
    assert_eq!(page.rekeys.len(), 1);
    assert_eq!(page.rekeys[0].rotation, 1);
    assert_eq!(page.rekeys[0].bundle, b"a bundle the server never opens");

    let blob = uuid::Uuid::new_v4();
    let refused = |what: &str, result: Result<(), RelayError>| {
        assert_eq!(result, Err(RelayError::TokenReadOnly), "{what}");
    };
    refused(
        "push",
        relay
            .push(
                &old.auth(),
                &[super::relay::OutgoingOp {
                    id: "op-after".into(),
                    ct: vec![1, 2, 3],
                }],
            )
            .await
            .map(|_| ()),
    );
    refused(
        "blob upload",
        relay
            .put_blob(
                &old.auth(),
                blob,
                3,
                Box::new(std::iter::once(Ok(vec![0; 3]))),
            )
            .await,
    );
    refused("blob delete", relay.delete_blob(&old.auth(), blob).await);
    refused(
        "snapshot upload",
        relay.put_snapshot(&old.auth(), 1, vec![0; 8]).await,
    );
    refused(
        "a second rotation",
        relay
            .rotate_token_with_kit(
                &old.auth(),
                &next_creds(&old).token(),
                b"late",
                KitRotation {
                    envelope: FILLER_ENVELOPE,
                    token: None,
                },
            )
            .await
            .map(|_| ()),
    );
    refused("vault delete", relay.delete_vault(&old.auth()).await);
    // The Kit routes take no vault token on a read, and a new Kit on the old token is refused.
    let late_kit = relay
        .put_kit(&old.auth(), &kit_token_other(), FILLER_ENVELOPE)
        .await;
    assert_eq!(late_kit, Err(RelayError::TokenReadOnly));

    // The new token owns the vault: it pushes, and the old token still reads it.
    let pushed = relay
        .push(
            &new.auth(),
            &[super::relay::OutgoingOp {
                id: "op-new".into(),
                ct: vec![4, 5, 6],
            }],
        )
        .await
        .expect("push on the new token");
    assert_eq!(pushed.accepted, 1);
    let seen = relay.pull(&old.auth(), 0, 100).await.expect("pull again");
    assert!(seen.ops.iter().any(|op| op.id == "op-new"));
    relay.delete_vault(&new.auth()).await.expect("vault delete");
}

/// A Kit token no vault token and no Kit has (43 base64url characters).
fn kit_token_other() -> String {
    let mut bytes = [0u8; 32];
    super::keys::os_random(&mut bytes).expect("random");
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Two rotations with the same current token at once: exactly one is taken, the other hears that
/// the token is already read-only.
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn two_concurrent_rotations_make_exactly_one_win() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (a, _, _) = first_device_with_kit(&relay, &url, "A").await;
    let old = a.credentials();
    let (first, second) = (next_creds(&old), next_creds(&old));
    let rotate = |next: &Credentials, bundle: &'static [u8]| {
        let relay = relay_at(&url);
        let (old, token) = (old.auth(), next.token());
        async move {
            relay
                .rotate_token_with_kit(
                    &old,
                    &token,
                    bundle,
                    KitRotation {
                        envelope: FILLER_ENVELOPE,
                        token: None,
                    },
                )
                .await
        }
    };
    let (one, two) = tokio::join!(
        rotate(&first, b"bundle one"),
        rotate(&second, b"bundle two")
    );
    let (winner, loser, won) = match (one, two) {
        (Ok(rotated), Err(error)) => (rotated, error, &first),
        (Err(error), Ok(rotated)) => (rotated, error, &second),
        other => panic!("exactly one rotation is taken: {other:?}"),
    };
    assert_eq!(winner.rotation, 1);
    assert_eq!(loser, RelayError::TokenReadOnly);
    let info = relay.vault_info(&won.auth()).await.expect("vault read");
    assert_eq!(info.rotation, 1);
    let page = relay.rekeys(&won.auth(), 0, 8).await.expect("bundles");
    assert_eq!(page.rekeys.len(), 1, "one bundle");
}

/// A, B and C end to end: A removes C. C's key opens none of what follows, B picks the key up and
/// seals its offline edit under it, A applies it; C, on its next round, is told it was removed.
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn a_removed_device_opens_nothing_and_the_others_carry_on() {
    let url = relay_url();
    let (relay, a, b, c, _kit) = three(&url).await;
    let c_id = me(&c);
    add_account(&b, "acc-b-offline");

    let epoch = changed(&remove(&relay, &a, &[c_id], false).await.expect("removal"));
    assert_eq!(epoch, 1);
    assert!(removed_row(&a, c_id));

    // A writes after the change; B (still offline with its edit) picks the key up and sends both.
    add_account(&a, "acc-a-after");
    a.round(&relay).await;
    let report = b.round(&relay).await;
    assert!(
        report.push.as_ref().is_some_and(|push| push.pushed > 0),
        "{report:?}"
    );
    a.round(&relay).await;
    b.round(&relay).await;
    assert!(has_account(&a, "acc-b-offline"), "B's edit reached A");
    assert!(has_account(&b, "acc-a-after"), "A's edit reached B");
    converged(&a, &b);
    assert_eq!(b.credentials().keys.current_epoch(), 1);
    assert!(removed_row(&b, c_id), "B holds C's removal");

    // C's old token still pulls, but everything after the change is sealed under a key it lacks.
    let held = c.credentials();
    assert_eq!(held.keys.current_epoch(), 0);
    assert!(held.keys.get(1).is_none());
    let page = relay.pull(&held.auth(), 0, 1000).await.expect("C pulls");
    let after: Vec<u32> = page
        .ops
        .iter()
        .filter_map(|op| super::envelope::envelope_epoch(&op.ct))
        .filter(|epoch| *epoch >= 1)
        .collect();
    assert!(!after.is_empty(), "ops sealed under the new key exist");
    assert!(
        relay
            .push(
                &held.auth(),
                &[super::relay::OutgoingOp {
                    id: "c-after".into(),
                    ct: vec![1]
                }]
            )
            .await
            == Err(RelayError::TokenReadOnly)
    );

    // C's next round walks the bundle, finds itself removed and turns sync off.
    add_account(&c, "acc-c-late");
    let report = super::engine::run_round(
        &relay,
        &c.db,
        &mut super::engine::snapshots::SnapshotPolicy::default(),
        super::engine::RoundMode::Normal,
        now(),
        0,
    )
    .await
    .expect("C's round");
    assert!(!c.enabled(), "C is told and turned sync off: {report:?}");
    assert!(!has_account(&a, "acc-c-late"));
    assert!(!has_account(&b, "acc-c-late"));
}

/// Revision §2.6 and R-74: a removal with a new Kit. The old Kit's token hears `kit_replaced`
/// (never counted against anyone), the new one reads; B, which only picked the change up (A has
/// not synced since), holds the removal record and the new Kit's row; and a device joins with
/// the new Kit's code and converges with A and B.
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn a_new_kit_replaces_the_old_one_and_a_device_joins_with_it() {
    let url = relay_url();
    let (relay, a, b, c, kit) = three(&url).await;
    let c_id = me(&c);
    let creds = a.credentials();
    let old_kit_id = kit.secret().derive(&creds.vault_id.0);

    let outcome = remove(&relay, &a, &[c_id], true).await.expect("removal");
    let (new_id, new_secret) = match outcome {
        Outcome::Changed {
            epoch: 1,
            kit: Some(kit),
            ..
        } => (kit.id, kit.secret),
        other => panic!("a new Kit: {other:?}"),
    };
    let vault = creds.vault_id.to_b64();
    let replaced = relay.get_kit(&vault, old_kit_id.token()).await;
    assert!(
        matches!(replaced, Err(RelayError::KitReplaced { .. })),
        "{replaced:?}"
    );
    let new_kit_keys = new_secret.derive(&creds.vault_id.0);
    let read = relay
        .get_kit(&vault, new_kit_keys.token())
        .await
        .expect("the new Kit reads");
    assert!(!read.envelope.is_empty());

    // B picks the change up before A has pushed anything since: the rows it holds are the ones
    // its pickup wrote (R-74).
    b.round(&relay).await;
    assert_eq!(b.credentials().keys.current_epoch(), 1);
    assert!(removed_row(&b, c_id), "B holds C's removal");
    let current = kit::current(&b.db.lock())
        .expect("current Kit")
        .expect("a Kit row");
    assert_eq!(current.id, new_id, "B holds the new Kit's row");
    settle(&relay, &[&a, &b]).await;

    // The old Kit's code is refused with the replacement's own message; the new one joins.
    let stale = kit_join(&relay, &url, &creds, kit.secret())
        .await
        .err()
        .expect("the replaced Kit");
    assert_eq!(kit_error_key(stale), KIT_REPLACED);
    add_account(&a, "acc-a-newer");
    a.round(&relay).await;
    let d = kit_join(&relay, &url, &creds, &new_secret)
        .await
        .expect("joined with the new Kit");
    d.round(&relay).await;
    a.round(&relay).await;
    b.round(&relay).await;
    d.round(&relay).await;
    assert_eq!(d.credentials().keys.current_epoch(), 1);
    assert!(has_account(&d, "acc-a-newer"));
    assert_eq!(d.count("SELECT COUNT(*) FROM bank_transactions"), 20);
    converged(&a, &d);
    converged(&b, &d);
    assert!(removed_row(&d, c_id), "the new device sees C as removed");
}

/// A removal that keeps the Kit (the plan says the Kit survives): the same code still joins, now
/// with both keys, after a key change.
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn the_kit_still_joins_after_a_removal_that_keeps_it() {
    let url = relay_url();
    let (relay, a, b, c, kit) = three(&url).await;
    let creds = a.credentials();
    let c_id = me(&c);
    changed(&remove(&relay, &a, &[c_id], false).await.expect("removal"));
    settle(&relay, &[&a, &b]).await;

    let d = kit_join(&relay, &url, &creds, kit.secret())
        .await
        .expect("the Kit joins after a key change");
    d.round(&relay).await;
    a.round(&relay).await;
    b.round(&relay).await;
    d.round(&relay).await;
    let joined = d.credentials();
    assert_eq!(joined.keys.current_epoch(), 1);
    assert!(joined.keys.get(0).is_some(), "the old key came with it");
    assert_eq!(d.count("SELECT COUNT(*) FROM bank_transactions"), 20);
    converged(&a, &d);

    // And it is a full member: its edit reaches A.
    add_account(&d, "acc-d");
    d.round(&relay).await;
    a.round(&relay).await;
    assert!(has_account(&a, "acc-d"));
}

/// R-75: a Kit's envelope sealed by a device that has since left still opens a Kit join.
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn a_kit_sealed_by_a_device_that_left_still_joins() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (a, _, kit) = first_device_with_kit(&relay, &url, "A").await;
    add_finances(&a);
    a.round(&relay).await;
    let creds = a.credentials();
    let b = pair(&relay, &a, "B").await;
    settle(&relay, &[&a, &b]).await;

    // A, the Kit's creator and its envelope's signer, leaves.
    leave_best_effort(&relay, &a.db, a.dir(), &NoBinding, LEAVE_BUDGET).await;
    turn_off_between_rounds(&a.db, a.dir()).await.expect("off");
    assert!(!a.enabled());
    b.round(&relay).await;

    let d = kit_join(&relay, &url, &creds, kit.secret())
        .await
        .expect("the Kit signed by a device that left still joins");
    d.round(&relay).await;
    b.round(&relay).await;
    d.round(&relay).await;
    assert_eq!(d.count("SELECT COUNT(*) FROM bank_transactions"), 20);
    converged(&b, &d);
}

/// Revision §2.9: the Kit's tight limit counts failures only. Right-token reads are never refused
/// (30 in a minute, far past the failure quota); a replaced Kit's 410 is never counted; the
/// tenth wrong token in a minute is still answered `401`, the eleventh is limited, and a right
/// token still reads (the quota counts failures only).
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn failed_kit_reads_are_limited_and_right_ones_are_not() {
    let url = relay_url();
    let (relay, a, b, c, kit) = three(&url).await;
    let creds = a.credentials();
    let vault = creds.vault_id.to_b64();
    let first = kit.secret().derive(&creds.vault_id.0);
    let _ = (&b, &c);

    for _ in 0..30 {
        relay
            .get_kit(&vault, first.token())
            .await
            .expect("a right token is never refused");
    }

    // A new Kit replaces it: the old token hears 410 over and over, uncounted.
    let c_id = me(&c);
    let second = new_secret(remove(&relay, &a, &[c_id], true).await.expect("new Kit"))
        .derive(&creds.vault_id.0);
    for _ in 0..15 {
        let gone = relay.get_kit(&vault, first.token()).await;
        assert!(
            matches!(gone, Err(RelayError::KitReplaced { .. })),
            "{gone:?}"
        );
    }
    relay
        .get_kit(&vault, second.token())
        .await
        .expect("the new Kit still reads after the 410s");

    // Wrong tokens: ten failures answered, then the limit.
    for n in 1..=10 {
        let wrong = relay.get_kit(&vault, &kit_token_other()).await;
        assert!(
            matches!(wrong, Err(RelayError::Unauthorized)),
            "failure {n}: {wrong:?}"
        );
    }
    let limited = relay.get_kit(&vault, &kit_token_other()).await;
    assert!(
        matches!(limited, Err(RelayError::RateLimited)),
        "{limited:?}"
    );
    // A right token is never refused by the failure limits, even while this client's are used up.
    relay
        .get_kit(&vault, second.token())
        .await
        .expect("a right token still reads while the failure quota is gone");
}

/// A blob upload whose token rotates while the body streams leaves no blob (spec: "no file and no
/// record"); the new token's vault has none and uploads fine.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn a_blob_upload_whose_token_rotates_mid_stream_leaves_nothing() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (a, _, _) = first_device_with_kit(&relay, &url, "A").await;
    let old = a.credentials();
    let new = next_creds(&old);
    let blob = uuid::Uuid::new_v4();
    const HALF: usize = 64 * 1024;

    // The body hands over its first half, tells the test, and waits until the rotation is done.
    let (half_sent, half_seen) = mpsc::channel::<()>();
    let (rotated, rotated_seen) = mpsc::channel::<()>();
    let mut step = 0;
    let body: BlobBody = Box::new(std::iter::from_fn(move || {
        step += 1;
        match step {
            1 => Some(Ok(vec![7u8; HALF])),
            2 => {
                half_sent.send(()).ok();
                rotated_seen.recv_timeout(Duration::from_secs(30)).ok();
                Some(Ok(vec![8u8; HALF]))
            }
            _ => None,
        }
    }));
    let upload = {
        let relay = relay_at(&url);
        let auth = old.auth();
        tokio::spawn(async move { relay.put_blob(&auth, blob, 2 * HALF as u64, body).await })
    };
    tokio::task::spawn_blocking(move || half_seen.recv_timeout(Duration::from_secs(30)))
        .await
        .expect("join")
        .expect("the upload is streaming");
    relay
        .rotate_token_with_kit(
            &old.auth(),
            &new.token(),
            b"bundle",
            KitRotation {
                envelope: FILLER_ENVELOPE,
                token: None,
            },
        )
        .await
        .expect("rotate while the body streams");
    rotated.send(()).ok();
    let result = upload.await.expect("upload task");
    assert!(result.is_err(), "the upload is refused: {result:?}");

    assert_eq!(
        relay.head_blob(&new.auth(), blob).await.expect("head"),
        None,
        "no record of the blob"
    );
    relay
        .put_blob(
            &new.auth(),
            blob,
            100,
            Box::new(std::iter::once(Ok(vec![1; 100]))),
        )
        .await
        .expect("the new token uploads");
    assert_eq!(
        relay.head_blob(&new.auth(), blob).await.expect("head"),
        Some(100)
    );
}

/// A and B remove each other at once through the real server: exactly one key change is taken,
/// the other device hears it lost the race or was removed.
#[tokio::test]
#[ignore = "needs a Moony sync server (Moony-relay-kit) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8795"]
async fn two_devices_removing_each_other_at_once_make_one_key_change() {
    let url = relay_url();
    let (relay, a, b, c, _kit) = three(&url).await;
    let (a_id, b_id) = (me(&a), me(&b));
    let _ = &c;
    let relay_b = relay_at(&url);
    let (b_ids, a_ids) = ([b_id], [a_id]);
    let (from_a, from_b) = tokio::join!(
        remove(&relay, &a, &b_ids, false),
        remove(&relay_b, &b, &a_ids, false)
    );
    let outcomes = [&from_a, &from_b];
    let wins = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(Outcome::Changed { .. })))
        .count();
    assert_eq!(wins, 1, "exactly one key change: {from_a:?} {from_b:?}");
    let info = relay
        .vault_info(
            &if wins == 1 && matches!(from_a, Ok(Outcome::Changed { .. })) {
                a.credentials()
            } else {
                b.credentials()
            }
            .auth(),
        )
        .await
        .expect("vault info");
    assert_eq!(info.rotation, 1);
}
