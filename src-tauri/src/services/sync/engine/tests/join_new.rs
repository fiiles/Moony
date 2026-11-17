//! Joining a new device (phase 3): from credentials received by pairing, in either direction,
//! or from a Sync Recovery Kit. A join replaces the newcomer's data, so every refusal comes
//! before the backup and before any relay call, and a join that fails after the backup leaves
//! the profile as it was.

use std::path::Path;

use super::join::{device_of, profile, repair_backups, round, snapshot_of_a, validation_key};
use super::*;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::devices;
use crate::services::sync::engine::RoundMode;
use crate::services::sync::error::{SyncError, SyncResult, Transient};
use crate::services::sync::join::{join_new_device, recovery_credentials};
use crate::services::sync::pairing::session::{PairingSession, Stage};
use crate::services::sync::pairing::{pairing_code_text, Role};
use crate::services::sync::recovery::{
    encode_recovery_code, read_recovery_input, recovery_qr_payload, RecoveryInput,
};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::http::DEFAULT_RELAY_URL;
use crate::services::sync::relay::RelayError;
use crate::services::sync::setup;
use crate::services::sync::test_support::assert_converged;

const RELAY: &str = "https://relay.test";

/// A profile of its own that has never synced, with a row only it holds.
fn newcomer() -> TestConn {
    let conn = TestConn::new(migrated());
    institution(&conn.lock(), "local-only");
    bank_account(&conn.lock(), "local-account");
    conn
}

fn holder_creds(holder: &TestConn) -> Credentials {
    keys::credentials(&holder.lock())
        .expect("credentials")
        .expect("the holder is bound")
}

/// A pairing over the vault's relay between `holder` and a joiner; `joiner_shows` picks the side
/// that shows the code. Returns what the joiner received.
async fn pair(vault: &Vault, holder: &TestConn, joiner_shows: bool) -> Credentials {
    let creds = holder_creds(holder);
    let (mut holder_session, mut joiner_session) = if joiner_shows {
        let joiner = PairingSession::show(Role::Joiner, DEFAULT_RELAY_URL, NOW).expect("show");
        let holder = PairingSession::scan(Role::Holder, &pairing_code_text(&joiner.code()), NOW)
            .expect("scan");
        (holder, joiner)
    } else {
        let holder = PairingSession::show(Role::Holder, &creds.relay_url, NOW).expect("show");
        let joiner = PairingSession::scan(Role::Joiner, &pairing_code_text(&holder.code()), NOW)
            .expect("scan");
        (holder, joiner)
    };
    let (shower, scanner) = if joiner_shows {
        (&mut joiner_session, &mut holder_session)
    } else {
        (&mut holder_session, &mut joiner_session)
    };
    assert_eq!(
        scanner.announce(&vault.relay, NOW).await.expect("announce"),
        Stage::AwaitingAck
    );
    assert_eq!(
        shower.poll(&vault.relay, NOW).await.expect("read the scan"),
        Stage::Compare
    );
    assert_eq!(
        scanner.poll(&vault.relay, NOW).await.expect("read the ack"),
        Stage::Compare
    );
    let digits = joiner_session
        .comparison_code()
        .expect("the joiner's digits");

    assert_eq!(
        joiner_session
            .confirm(&vault.relay, None, None, NOW)
            .await
            .expect("joiner confirms"),
        Stage::WaitingForOther
    );
    assert_eq!(
        holder_session
            .confirm(&vault.relay, Some(&creds), Some(&digits), NOW)
            .await
            .expect("holder confirms"),
        Stage::Sent
    );
    assert_eq!(
        joiner_session
            .poll(&vault.relay, NOW)
            .await
            .expect("read the credentials"),
        Stage::Received
    );
    joiner_session.received().expect("the credentials").clone()
}

async fn join_as(
    vault: &Vault,
    device: &TestConn,
    creds: Credentials,
    name: &str,
    dir: &Path,
) -> SyncResult<std::path::PathBuf> {
    let consent = agreed(device);
    join_new_device(
        &vault.relay,
        device,
        creds,
        name,
        consent,
        dir,
        &NoBinding,
        NOW,
    )
    .await
    .map(|outcome| outcome.backup)
}

/// The newcomer joined: its own row is gone, the vault's is there, sync is on under a new device
/// recorded with this platform and a public key; then rounds on it, `A` and it again converge,
/// and `A` lists it.
async fn assert_joined(vault: &Vault, device: &TestConn, zip: &Path, dir: &Path) -> DeviceId {
    assert_eq!(zip, dir.join(format!("sync-repair-{NOW}-kept.zip")));
    assert!(zip.exists(), "the backup comes first");
    let id = {
        let c = device.lock();
        assert!(!has_institution(&c, "local-only"), "replaced, not merged");
        assert!(has_institution(&c, "a1"), "the vault's data");
        assert!(state::load(&c).expect("state").expect("row").enabled);
        let id = device_of(&c);
        let (platform, public_key): (String, Option<String>) = c
            .query_row(
                "SELECT platform, public_key FROM sync_devices WHERE id = ?1",
                [id.to_hex()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("the newcomer's own row");
        assert_eq!(platform, devices::platform());
        assert_eq!(public_key.map(|k| k.len()), Some(43), "a public key");
        id
    };
    for (device, label) in [(device, "newcomer"), (&vault.a, "A"), (device, "newcomer")] {
        let report = round(vault, device, RoundMode::Normal).await;
        assert_eq!(report.problem, None, "{label}: {report:?}");
    }
    assert_converged(&[&*vault.a.lock(), &*device.lock()]);
    assert!(
        devices::device_row(&vault.a.lock(), id)
            .expect("A's rows")
            .is_some(),
        "A lists the newcomer"
    );
    id
}

fn kit_input(account: &str, vault_id: &VaultId, key: &[u8; 32]) -> RecoveryInput {
    let code = encode_recovery_code(vault_id, key);
    read_recovery_input(&code, Some(account), Some(RELAY), DEFAULT_RELAY_URL).expect("input")
}

#[tokio::test]
async fn a_new_device_joins_by_pairing_and_converges() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    let received = pair(&vault, &vault.a, false).await;
    let zip = join_as(&vault, &device, received, "iPhone", dir.path())
        .await
        .expect("join");

    assert_joined(&vault, &device, &zip, dir.path()).await;
    assert_eq!(
        devices::this_device(&device.lock())
            .expect("row")
            .map(|(name, _)| name),
        Some("iPhone".to_string())
    );
}

#[tokio::test]
async fn a_desktop_joins_from_a_phone_that_holds_the_vault() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");

    let received = pair(&vault, &vault.a, true).await;
    let zip = join_as(&vault, &device, received, "MacBook", dir.path())
        .await
        .expect("join");

    assert_joined(&vault, &device, &zip, dir.path()).await;
}

#[tokio::test]
async fn a_recovery_kit_join_finds_the_epoch_and_converges() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let kit = setup::recovery_kit(&vault.a.lock()).expect("kit");
    assert_eq!(kit.relay_url.as_deref(), Some(RELAY));
    let scanned = recovery_qr_payload(&kit.account_number, &kit.recovery_code, Some(RELAY));
    let inputs = [
        read_recovery_input(&scanned, None, None, DEFAULT_RELAY_URL).expect("scanned"),
        read_recovery_input(
            &kit.recovery_code,
            Some(&kit.account_number),
            Some(RELAY),
            DEFAULT_RELAY_URL,
        )
        .expect("typed"),
    ];

    for input in inputs {
        let device = newcomer();
        let dir = tempfile::tempdir().expect("dir");
        let creds = recovery_credentials(&vault.relay, &device, input)
            .await
            .expect("credentials");
        assert_eq!(creds.keys.current_epoch(), 0);
        assert_eq!(creds.relay_url, RELAY);
        assert_eq!(creds.account, vault.creds.account);
        assert_eq!(creds.vault_id, vault.creds.vault_id);
        assert_eq!(*creds.auth().token, *vault.creds.auth().token);

        let zip = join_as(&vault, &device, creds, "Laptop", dir.path())
            .await
            .expect("join");
        assert_joined(&vault, &device, &zip, dir.path()).await;
    }
}

#[tokio::test]
async fn a_code_of_another_vault_is_refused_before_any_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    let before = profile(&device.lock());
    let key = *vault.creds.keys.current();

    // A vault the relay does not know, and this vault's ID with a wrong key.
    for input in [
        kit_input(&vault.creds.account, &VaultId::random().expect("id"), &key),
        kit_input(&vault.creds.account, &vault.creds.vault_id, &[0x5a; 32]),
    ] {
        let error = recovery_credentials(&vault.relay, &device, input)
            .await
            .expect_err("another vault");
        assert_eq!(
            validation_key(&error),
            Some("validation.syncRecoveryVaultNotFound"),
            "{error:?}"
        );
    }
    assert_eq!(profile(&device.lock()), before);
}

#[tokio::test]
async fn a_kit_with_an_unknown_account_is_refused() {
    let vault = vault().await;
    let device = newcomer();
    let before = vault.relay.calls().len();
    let input = kit_input(
        "1111222233334444",
        &vault.creds.vault_id,
        vault.creds.keys.current(),
    );

    let error = recovery_credentials(&vault.relay, &device, input)
        .await
        .expect_err("unknown account");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncRecoveryAccountInvalid"),
        "{error:?}"
    );
    assert_eq!(
        vault.relay.calls()[before..],
        [Route::AccountStatus],
        "the vault is never asked"
    );
}

#[tokio::test]
async fn a_kit_of_a_rotated_vault_is_refused() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    vault.relay.set_rotation(&vault.creds.vault_id.to_b64(), 1);
    let device = newcomer();
    let before = profile(&device.lock());
    let input = kit_input(
        &vault.creds.account,
        &vault.creds.vault_id,
        vault.creds.keys.current(),
    );

    let error = recovery_credentials(&vault.relay, &device, input)
        .await
        .expect_err("rotated");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncRecoveryKitRotated"),
        "{error:?}"
    );
    let c = device.lock();
    assert_eq!(keys::account(&c).expect("account"), None, "nothing stored");
    assert_eq!(profile(&c), before);
}

#[tokio::test]
async fn a_kit_join_with_sync_on_is_refused_before_any_network_call() {
    let vault = vault().await;
    let before = vault.relay.calls();
    let input = kit_input(
        &vault.creds.account,
        &vault.creds.vault_id,
        vault.creds.keys.current(),
    );

    let error = recovery_credentials(&vault.relay, &vault.a, input)
        .await
        .expect_err("sync on");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncAlreadyEnabled"),
        "{error:?}"
    );
    assert_eq!(vault.relay.calls(), before, "no relay call");
}

#[tokio::test]
async fn joining_while_sync_is_on_is_refused() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let before_calls = vault.relay.calls();
    let before = profile(&vault.a.lock());
    let dir = tempfile::tempdir().expect("dir");

    let error = join_as(&vault, &vault.a, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect_err("sync on");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncAlreadyEnabled"),
        "{error:?}"
    );
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
    assert_eq!(vault.relay.calls(), before_calls, "no relay call");
    assert_eq!(profile(&vault.a.lock()), before);
}

#[tokio::test]
async fn joining_during_a_setup_with_another_account_is_refused_before_any_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    keys::save_account(&device.lock(), RELAY, "9999888877776666").expect("a half setup");
    let before_calls = vault.relay.calls();
    let before = profile(&device.lock());
    let dir = tempfile::tempdir().expect("dir");

    let error = join_as(&vault, &device, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect_err("another account");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncJoinSetupPending"),
        "{error:?}"
    );
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
    assert_eq!(vault.relay.calls(), before_calls, "no relay call");
    assert_eq!(profile(&device.lock()), before);

    // A setup that stopped at the same account may join.
    let same = newcomer();
    keys::save_account(&same.lock(), RELAY, &vault.creds.account).expect("same account");
    let zip = join_as(&vault, &same, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect("the same account joins");
    assert_joined(&vault, &same, &zip, dir.path()).await;
}

#[tokio::test]
async fn joining_during_a_setup_on_another_server_is_refused_before_any_backup() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    // The same account digits, but a setup that was started on another sync server.
    keys::save_account(&device.lock(), "https://other.test", &vault.creds.account)
        .expect("a half setup");
    let before_calls = vault.relay.calls();
    let before = profile(&device.lock());
    let dir = tempfile::tempdir().expect("dir");

    let error = join_as(&vault, &device, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect_err("another server");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncJoinSetupPending"),
        "{error:?}"
    );
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
    assert_eq!(vault.relay.calls(), before_calls, "no relay call");
    assert_eq!(profile(&device.lock()), before);

    // The same server however it is spelled, with the same account, may join.
    let same = newcomer();
    keys::save_account(&same.lock(), "https://RELAY.test/", &vault.creds.account)
        .expect("same server");
    let zip = join_as(&vault, &same, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect("the same server joins");
    assert_joined(&vault, &same, &zip, dir.path()).await;
}

#[tokio::test]
async fn a_hostile_sync_server_in_the_credentials_is_refused_before_anything() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    let before_calls = vault.relay.calls();
    let before = profile(&device.lock());
    let dir = tempfile::tempdir().expect("dir");
    let mut creds = vault.creds.clone();
    creds.relay_url = "ftp://evil.example".into();

    let error = join_as(&vault, &device, creds, "Laptop", dir.path())
        .await
        .expect_err("hostile server");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncRelayUrlInvalid"),
        "{error:?}"
    );
    assert!(repair_backups(dir.path()).is_empty(), "no backup");
    assert_eq!(vault.relay.calls(), before_calls, "no relay call");
    assert_eq!(profile(&device.lock()), before);
}

#[tokio::test]
async fn a_join_whose_snapshot_download_fails_changes_nothing() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    let before = profile(&device.lock());
    let dir = tempfile::tempdir().expect("dir");
    vault
        .relay
        .fail_next(Route::GetSnapshot, RelayError::Network("down".into()));

    let error = join_as(&vault, &device, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect_err("offline");

    assert!(
        matches!(error, SyncError::Transient(Transient::Offline)),
        "{error:?}"
    );
    {
        let c = device.lock();
        assert_eq!(profile(&c), before, "the profile is as it was");
        assert!(state::load(&c).expect("state").is_none(), "sync stays off");
    }
    assert!(
        repair_backups(dir.path()).is_empty(),
        "the backup comes in the hold, after the snapshot is open"
    );

    let zip = join_as(&vault, &device, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect("the second join");
    assert_eq!(zip, dir.path().join(format!("sync-repair-{NOW}-kept.zip")));
    assert!(!has_institution(&device.lock(), "local-only"));
    for device in [&device, &vault.a, &device] {
        round(&vault, device, RoundMode::Normal).await;
    }
    assert_converged(&[&*vault.a.lock(), &*device.lock()]);
}

#[tokio::test]
async fn a_device_that_turned_sync_off_joins_again() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let device = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    let zip = join_as(&vault, &device, vault.creds.clone(), "Laptop", dir.path())
        .await
        .expect("join");
    let first = assert_joined(&vault, &device, &zip, dir.path()).await;

    setup::turn_off(&mut device.lock(), dir.path()).expect("turn off");
    assert!(state::load(&device.lock()).expect("state").is_none());
    let again = tempfile::tempdir().expect("dir");
    institution(&device.lock(), "local-only");
    bank_account(&device.lock(), "local-account");
    let zip = join_as(&vault, &device, vault.creds.clone(), "Laptop", again.path())
        .await
        .expect("join again");

    let second = assert_joined(&vault, &device, &zip, again.path()).await;
    assert_ne!(second, first, "a new device ID");
}
