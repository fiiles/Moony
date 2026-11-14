//! First-device setup (spec §11.5, ruling P4), turning sync off and deleting synced data (§11.6),
//! and the self-hosted relay's address and checkout (rulings SH-1, SH-2), on a fresh profile
//! against a `FakeRelay`.

use std::collections::BTreeSet;
use std::path::Path;

use super::join::{validation_key, XorNative};
use super::*;
use crate::models::sync::{CheckoutTarget, SyncAccount, SyncPlan};
use crate::services::sync::binding::{
    startup_check, write_rejoin_marker, StartupCheck, BINDING_FILE, REJOIN_MARKER,
};
use crate::services::sync::capture;
use crate::services::sync::changeset::from_json;
use crate::services::sync::connection_code::{encode_connection_code, parse_connection_code};
use crate::services::sync::engine::snapshots::{open_sealed, SnapshotPolicy};
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::envelope::open_op;
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::http::DEFAULT_RELAY_URL;
use crate::services::sync::relay::RelayError;
use crate::services::sync::setup::{
    account_status, checkout_url, connect_own_server, create_account, delete_synced_data,
    delete_vault_best_effort, finish_setup, recovery_kit, setup_recovery_kit,
    turn_off_between_rounds,
};
use crate::services::sync::snapshot::{read_record, Record};

pub(super) const RELAY: &str = "https://relay.example.org";

pub(super) fn fresh() -> TestConn {
    TestConn::new(migrated())
}

/// The validation key of a refusal made on this device (`SyncError::Local`).
fn key_of(result: SyncResult<impl std::fmt::Debug>) -> String {
    let error = result.expect_err("refused");
    validation_key(&error)
        .unwrap_or_else(|| panic!("not a local validation error: {error:?}"))
        .to_string()
}

/// The `validation.sync*` key a command would return for `result`'s error.
fn command_key(result: SyncResult<impl std::fmt::Debug>) -> String {
    match result.expect_err("refused").to_app_error() {
        AppError::Validation(key) => key,
        other => panic!("not a validation key: {other:?}"),
    }
}

async fn finish(relay: &FakeRelay, device: &TestConn, dir: &Path) -> SyncResult<()> {
    finish_setup(relay, device, "Laptop", true, dir, &XorNative, NOW).await
}

/// A profile with sync set up on `relay`: account, payment, kit, finish.
pub(super) async fn set_up(relay: &FakeRelay, device: &TestConn, dir: &Path) -> Credentials {
    let account = create_account(relay, device, RELAY).await.expect("account");
    relay.set_paid(&account.account_number, true);
    recovery_kit(&device.lock()).expect("kit");
    finish(relay, device, dir).await.expect("setup");
    keys::credentials(&device.lock())
        .expect("credentials")
        .expect("stored")
}

pub(super) fn enabled(conn: &Connection) -> bool {
    state::load(conn).expect("state").is_some_and(|s| s.enabled)
}

/// Every institution row, as text, in id order.
fn institutions(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT * FROM institutions ORDER BY id")
        .expect("institutions");
    let columns = stmt.column_count();
    let rows = stmt
        .query_map([], |r| {
            (0..columns)
                .map(|i| r.get::<_, Value>(i).map(|v| format!("{v:?}")))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .expect("rows");
    rows.map(|row| row.expect("row").join("|")).collect()
}

/// The `sync_devices` keys among `rows`.
fn device_keys(rows: &[RowChange]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.table == "sync_devices")
        .map(|row| format!("{:?}", row.key))
        .collect()
}

/// A valid `https` relay address of exactly `len` characters (labels of at most 50).
fn relay_of_len(len: usize) -> String {
    let mut host = String::new();
    let wanted = len - "https://".len();
    while host.len() < wanted {
        if !host.is_empty() {
            host.push('.');
        }
        let label = (wanted - host.len()).min(50);
        host.push_str(&"r".repeat(label));
    }
    // A label of one character left after a dot is fine; never end on a dot.
    assert!(!host.ends_with('.'));
    let url = format!("https://{host}");
    assert_eq!(url.len(), len);
    url
}

fn device_of(conn: &Connection) -> DeviceId {
    state::load(conn).expect("state").expect("row").device
}

#[tokio::test]
async fn first_device_setup_creates_the_vault_pushes_and_uploads_a_snapshot() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");

    let account = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    assert_eq!(account.account_number.len(), 16);
    assert!(account.account_number.bytes().all(|b| b.is_ascii_digit()));
    assert!(!account.active);
    assert_eq!(account.paid_until, None);
    assert!(
        !enabled(&device.lock()),
        "an account alone does not turn sync on"
    );
    let saved_device = device_of(&device.lock());

    relay.set_paid(&account.account_number, true);
    let paid = account_status(&relay, &device).await.expect("status");
    assert_eq!(paid.account_number, account.account_number);
    assert!(paid.active);
    assert!(paid.paid_until.is_some_and(|until| until > NOW));

    let kit = recovery_kit(&device.lock()).expect("kit");
    assert_eq!(kit.account_number, account.account_number);
    assert_eq!(kit.relay_url.as_deref(), Some(RELAY));

    finish(&relay, &device, dir.path()).await.expect("setup");

    let creds = keys::credentials(&device.lock())
        .expect("credentials")
        .expect("stored");
    assert_eq!(creds.relay_url, RELAY);
    let info = relay.vault_info(&creds.auth()).await.expect("the vault");
    assert_eq!(info.head, 1);

    // One envelope: the rule-pack keys and this device's row.
    let this = device_of(&device.lock());
    assert_eq!(
        this, saved_device,
        "setup turns sync on for the device the row holds"
    );
    let ops = relay.ops();
    assert_eq!(ops.len(), 1);
    let json = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &ops[0].ct).expect("open");
    let cs = from_json(&json).expect("changeset");
    assert_eq!((cs.device, cs.seq, cs.prev), (this, 1, None));
    let rows: BTreeSet<(String, String)> = cs
        .rows
        .iter()
        .map(|row| (row.table.clone(), format!("{:?}", row.key)))
        .collect();
    let expected: BTreeSet<(String, String)> = [
        (
            "app_config",
            "Single(\"rule_packs_disabled_rules\")".to_string(),
        ),
        ("app_config", "Single(\"rule_packs_enabled\")".to_string()),
        ("sync_devices", format!("Single({:?})", this.to_hex())),
    ]
    .into_iter()
    .map(|(t, k)| (t.to_string(), k))
    .collect();
    assert_eq!(rows, expected);

    // The snapshot at S = the relay head after the second pull (ruling P4).
    assert_eq!(relay.snapshot_seq(), Some(1));
    let blob = relay.get_snapshot(&creds.auth()).await.expect("snapshot");
    let plain = open_sealed(&creds, &blob).expect("opens");
    let mut stream = &plain[..];
    let mut devs = Vec::new();
    let (seq, heads) = loop {
        match read_record(&mut stream, &devs)
            .expect("record")
            .expect("a header")
        {
            Record::Devices(list) => devs = list,
            Record::Header { seq, heads, .. } => break (seq, heads),
            other => panic!("a record before the header: {other:?}"),
        }
    };
    assert_eq!(seq, 1);
    assert_eq!(
        heads
            .iter()
            .map(|h| (h.device, h.seq, h.quarantined))
            .collect::<Vec<_>>(),
        vec![(this, 1, false)]
    );

    let conn = device.lock();
    assert!(enabled(&conn));
    assert!(capture::is_installed(&conn).expect("capture"));
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_unsent"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);
    let sync = state::load(&conn).expect("state").expect("row");
    assert_eq!(sync.cursor, 1, "the own envelope came back acknowledged");
    assert_eq!(count(&conn, "SELECT snapshot_seq FROM sync_state"), 1);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sync_devices WHERE name = 'Laptop' AND public_key IS NOT NULL"
        ),
        1
    );
    let marks = state::marks(&conn).expect("marks").expect("row");
    assert!(
        marks.binding_written,
        "the binding is sealed for this device"
    );
    assert!(dir.path().join(BINDING_FILE).exists());
    assert_eq!(
        startup_check(&conn, dir.path(), &XorNative).expect("check"),
        StartupCheck::Fine,
        "the binding matches the device ID sync runs under"
    );
}

#[tokio::test]
async fn the_setup_kit_is_refused_once_sync_is_on() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let account = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    relay.set_paid(&account.account_number, true);

    // While setup runs, the same kit as `recovery_kit`, as often as asked.
    let during = setup_recovery_kit(&device.lock()).expect("kit during setup");
    let kit = recovery_kit(&device.lock()).expect("kit");
    assert_eq!(during.recovery_code, kit.recovery_code);
    assert_eq!(during.account_number, account.account_number);

    // Once sync is on, only the password-checked path shows it (`sync_show_recovery_kit`).
    finish(&relay, &device, dir.path()).await.expect("setup");
    assert_eq!(
        key_of(setup_recovery_kit(&device.lock()).map_err(SyncError::from)),
        "validation.syncAlreadyEnabled"
    );
    assert!(recovery_kit(&device.lock()).is_ok(), "the kit itself stays");
}

#[tokio::test]
async fn setup_resumes_with_the_stored_account_and_vault_after_a_restart() {
    let relay = FakeRelay::new();
    let device = fresh();
    let first = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    let kit = recovery_kit(&device.lock()).expect("kit");

    // The app restarts: a new relay handle on the same relay, the same profile.
    let again = relay.clone();
    let second = create_account(&again, &device, &format!("{RELAY}/"))
        .await
        .expect("account again");
    let kit_again = recovery_kit(&device.lock()).expect("kit again");

    assert_eq!(second.account_number, first.account_number);
    assert_eq!(kit_again.recovery_code, kit.recovery_code);
    assert_eq!(kit_again.qr_svg, kit.qr_svg);
    assert_eq!(
        relay
            .calls()
            .iter()
            .filter(|r| **r == Route::CreateAccount)
            .count(),
        1,
        "no second account"
    );
    assert_eq!(
        keys::account(&device.lock()).expect("account"),
        Some((RELAY.to_string(), first.account_number.clone()))
    );

    // A setup that stopped after the vault was created finishes on the next try.
    relay.set_paid(&first.account_number, true);
    let dir = tempfile::tempdir().expect("dir");
    relay.fail_next(Route::Pull, RelayError::Network("timeout".into()));
    let stopped = finish(&relay, &device, dir.path()).await;
    assert!(
        matches!(stopped, Err(SyncError::Transient(_))),
        "{stopped:?}"
    );
    assert!(
        enabled(&device.lock()),
        "sync is on, the first round is owed"
    );
    finish(&relay, &device, dir.path())
        .await
        .expect("resumed setup");
    assert_eq!(relay.snapshot_seq(), Some(1));
    assert_eq!(relay.ops().len(), 1);
}

#[tokio::test]
async fn finishing_without_a_saved_kit_or_payment_is_refused() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");

    let unconfirmed = finish_setup(
        &relay,
        &device,
        "Laptop",
        false,
        dir.path(),
        &XorNative,
        NOW,
    );
    assert_eq!(
        key_of(unconfirmed.await),
        "validation.syncRecoveryKitNotConfirmed"
    );
    assert_eq!(
        key_of(finish(&relay, &device, dir.path()).await),
        "validation.syncSetupIncomplete",
        "no account"
    );
    let account = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    assert_eq!(
        key_of(finish(&relay, &device, dir.path()).await),
        "validation.syncSetupIncomplete",
        "no Recovery Kit yet"
    );
    recovery_kit(&device.lock()).expect("kit");
    let unnamed = finish_setup(&relay, &device, "  ", true, dir.path(), &XorNative, NOW);
    assert_eq!(key_of(unnamed.await), "validation.syncDeviceNameInvalid");

    let unpaid = finish(&relay, &device, dir.path()).await;
    assert!(matches!(unpaid, Err(SyncError::Lapsed)), "{unpaid:?}");
    assert_eq!(
        command_key(finish(&relay, &device, dir.path()).await),
        "validation.syncPaymentRequired"
    );
    relay.set_paid(&account.account_number, true);
    relay.fail_next(Route::CreateVault, RelayError::VaultLimitReached);
    assert_eq!(
        command_key(finish(&relay, &device, dir.path()).await),
        "validation.syncVaultLimitReached"
    );

    {
        let conn = device.lock();
        assert!(!enabled(&conn), "nothing was turned on");
        assert!(!capture::is_installed(&conn).expect("capture"));
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_devices"), 0);
        assert!(relay.ops().is_empty());
    }

    finish(&relay, &device, dir.path()).await.expect("setup");
    assert_eq!(
        key_of(finish(&relay, &device, dir.path()).await),
        "validation.syncAlreadyEnabled"
    );
    assert_eq!(
        key_of(create_account(&relay, &device, "https://other.example.org").await),
        "validation.syncAlreadyEnabled",
        "an enabled profile does not move to another relay"
    );
}

#[tokio::test]
async fn a_failed_first_upload_is_retried_by_the_policy() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    relay.fail_next(Route::PutSnapshot, RelayError::Server(503));

    let creds = set_up(&relay, &device, dir.path()).await;

    assert_eq!(relay.snapshot_seq(), None, "the upload failed");
    assert_eq!(relay.ops().len(), 1, "the push went through");
    assert!(enabled(&device.lock()));
    assert_eq!(
        count(
            &device.lock(),
            "SELECT COUNT(*) FROM sync_state WHERE snapshot_seq IS NULL"
        ),
        1
    );
    assert_eq!(
        relay
            .vault_info(&creds.auth())
            .await
            .expect("info")
            .snapshot_seq,
        None
    );

    let report = run_round(
        &relay,
        &device,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW + 60,
        0,
    )
    .await
    .expect("round");
    assert!(report.snapshot_uploaded, "{report:?}");
    assert_eq!(relay.snapshot_seq(), Some(1));
}

#[tokio::test]
async fn the_checkout_url_must_be_polar_https() {
    let relay = FakeRelay::new();
    let device = fresh();
    assert_eq!(
        key_of(checkout_url(&relay, &device, SyncPlan::Monthly).await),
        "validation.syncSetupIncomplete"
    );
    create_account(&relay, &device, RELAY)
        .await
        .expect("account");

    for (base, plan, url) in [
        (
            "https://buy.polar.sh/checkout/",
            SyncPlan::Yearly,
            "https://buy.polar.sh/checkout/yearly",
        ),
        (
            "https://polar.sh/checkout/",
            SyncPlan::Monthly,
            "https://polar.sh/checkout/monthly",
        ),
        (
            "https://sandbox.api.polar.sh/c/",
            SyncPlan::Monthly,
            "https://sandbox.api.polar.sh/c/monthly",
        ),
    ] {
        relay.set_checkout_base(base);
        assert_eq!(
            checkout_url(&relay, &device, plan).await.expect(base),
            CheckoutTarget::Open { url: url.into() }
        );
    }
    for base in [
        "https://checkout.example.test/",
        "http://buy.polar.sh/",
        "https://polar.sh.evil.example/",
        "https://evilpolar.sh/",
        "https://user@buy.polar.sh/",
        "https://buy.polar.sh:8443/",
        "moony://polar.sh/",
        "not a url ",
    ] {
        relay.set_checkout_base(base);
        let refused = checkout_url(&relay, &device, SyncPlan::Monthly).await;
        assert_eq!(key_of(refused), "validation.syncRelayError", "{base}");
    }
}

#[tokio::test]
async fn a_self_hosted_relay_answers_checkout_without_a_url() {
    let relay = FakeRelay::new();
    relay.set_self_hosted(true);
    let device = fresh();
    let account = create_account(&relay, &device, "https://sync.example.org")
        .await
        .expect("account");

    assert_eq!(
        checkout_url(&relay, &device, SyncPlan::Yearly)
            .await
            .expect("checkout"),
        CheckoutTarget::SelfHosted
    );
    assert!(
        !account_status(&relay, &device)
            .await
            .expect("status")
            .active
    );
    // The operator unblocks the person (the fake's `set_paid`); the next poll sees it active.
    relay.set_paid(&account.account_number, true);
    assert!(
        account_status(&relay, &device)
            .await
            .expect("status")
            .active
    );

    let kit = recovery_kit(&device.lock()).expect("kit");
    assert_eq!(kit.relay_url.as_deref(), Some("https://sync.example.org"));
    let dir = tempfile::tempdir().expect("dir");
    finish(&relay, &device, dir.path()).await.expect("setup");
    assert_eq!(
        keys::credentials(&device.lock())
            .expect("credentials")
            .expect("stored")
            .relay_url,
        "https://sync.example.org"
    );
}

#[tokio::test]
async fn the_relay_address_is_checked_normalized_and_kept_until_setup_ends() {
    let relay = FakeRelay::new();
    let device = fresh();
    for refused in [
        "ftp://relay.example.org",
        "https://relay.example.org/path",
        "",
    ] {
        assert_eq!(
            key_of(create_account(&relay, &device, refused).await),
            "validation.syncRelayUrlInvalid",
            "{refused}"
        );
    }
    assert!(relay.calls().is_empty(), "nothing reached the relay");
    assert_eq!(keys::account(&device.lock()).expect("account"), None);

    let first = create_account(&relay, &device, "https://Relay.Example.org:443/")
        .await
        .expect("account");
    assert_eq!(
        keys::account(&device.lock()).expect("account"),
        Some((RELAY.to_string(), first.account_number.clone()))
    );
    // Before sync is on, another relay address means a new account there.
    let moved = create_account(&relay, &device, DEFAULT_RELAY_URL)
        .await
        .expect("account on another relay");
    assert_ne!(moved.account_number, first.account_number);
    assert_eq!(
        keys::account(&device.lock()).expect("account"),
        Some((DEFAULT_RELAY_URL.to_string(), moved.account_number.clone()))
    );
}

#[tokio::test]
async fn a_kit_carries_a_custom_relay_and_not_the_default_one() {
    let relay = FakeRelay::new();
    let custom = fresh();
    create_account(&relay, &custom, RELAY)
        .await
        .expect("account");
    let kit = recovery_kit(&custom.lock()).expect("kit");
    assert_eq!(kit.relay_url.as_deref(), Some(RELAY));

    let default = fresh();
    create_account(&relay, &default, DEFAULT_RELAY_URL)
        .await
        .expect("account");
    let kit = recovery_kit(&default.lock()).expect("kit");
    assert_eq!(kit.relay_url, None);
    assert_eq!(kit.recovery_code.len(), 95, "16 groups of five");
    assert_eq!(kit.recovery_code.matches('-').count(), 15);
    assert!(kit.qr_svg.contains("</svg>"));
    let (vault_id, keys) = keys::ensure_vault_secrets(&default.lock()).expect("secrets");
    let code = crate::services::sync::recovery::encode_recovery_code(&vault_id, keys.current());
    assert_eq!(
        kit.recovery_code,
        *crate::services::sync::recovery::format_recovery_code(&code)
    );
}

#[tokio::test]
async fn turning_off_keeps_local_data_and_forgets_every_secret() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    set_up(&relay, &device, dir.path()).await;
    institution(&device.lock(), "kept");
    institution(&device.lock(), "also kept");
    crate::services::sync::binding::write_rejoin_marker(dir.path()).expect("marker");
    assert!(dir.path().join(BINDING_FILE).exists());
    let finance_before = institutions(&device.lock());
    assert_eq!(
        count(&device.lock(), "SELECT COUNT(*) FROM sync_devices"),
        1
    );

    turn_off_between_rounds(&device, dir.path())
        .await
        .expect("turn off");

    let conn = device.lock();
    assert!(has_institution(&conn, "kept"), "local data stays");
    assert_eq!(
        institutions(&conn),
        finance_before,
        "finance rows unchanged"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM sync_devices"),
        0,
        "the device list is sync metadata of the old vault"
    );
    assert!(state::load(&conn).expect("state").is_none());
    assert_eq!(
        keys::account(&conn).expect("account"),
        None,
        "relay address forgotten"
    );
    assert!(keys::credentials(&conn).expect("credentials").is_none());
    for table in [
        "sync_state",
        "sync_clock",
        "sync_outbox",
        "sync_heads",
        "sync_own_log",
        "sync_unsent",
        "sync_typed",
        "sync_devices",
    ] {
        assert_eq!(
            count(&conn, &format!("SELECT COUNT(*) FROM {table}")),
            0,
            "{table}"
        );
    }
    assert!(!capture::is_installed(&conn).expect("capture"));
    assert!(!dir.path().join(BINDING_FILE).exists());
    assert!(!dir.path().join(REJOIN_MARKER).exists());
    assert!(
        relay.snapshot_seq().is_some(),
        "the vault stays for other devices"
    );
}

#[tokio::test]
async fn deleting_synced_data_wipes_the_vault_and_turns_off() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let creds = set_up(&relay, &device, dir.path()).await;

    // A relay that does not answer keeps everything as it was.
    relay.fail_next(Route::DeleteVault, RelayError::Network("timeout".into()));
    let failed = delete_synced_data(&relay, &device, dir.path()).await;
    assert!(matches!(failed, Err(SyncError::Transient(_))), "{failed:?}");
    assert!(enabled(&device.lock()));
    assert!(relay.vault_info(&creds.auth()).await.is_ok());

    delete_synced_data(&relay, &device, dir.path())
        .await
        .expect("delete");

    assert_eq!(
        relay.vault_info(&creds.auth()).await,
        Err(RelayError::VaultNotFound)
    );
    let conn = device.lock();
    assert!(state::load(&conn).expect("state").is_none());
    assert!(!capture::is_installed(&conn).expect("capture"));
    assert!(!dir.path().join(BINDING_FILE).exists());
}

#[tokio::test]
async fn deleting_an_already_gone_vault_still_turns_off() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let creds = set_up(&relay, &device, dir.path()).await;
    relay.wipe_vault(&creds.vault_id.to_b64());

    delete_synced_data(&relay, &device, dir.path())
        .await
        .expect("delete");

    assert!(state::load(&device.lock()).expect("state").is_none());
    assert_eq!(
        relay
            .calls()
            .iter()
            .filter(|r| **r == Route::DeleteVault)
            .count(),
        1
    );
}

#[tokio::test]
async fn the_best_effort_delete_reports_whether_the_vault_is_gone() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let creds = set_up(&relay, &device, dir.path()).await;

    relay.fail_next(Route::DeleteVault, RelayError::Network("connect".into()));
    assert!(!delete_vault_best_effort(&relay, &device).await, "offline");
    assert!(relay.vault_info(&creds.auth()).await.is_ok());

    assert!(delete_vault_best_effort(&relay, &device).await, "deleted");
    assert_eq!(
        relay.vault_info(&creds.auth()).await,
        Err(RelayError::VaultNotFound)
    );
    assert!(
        delete_vault_best_effort(&relay, &device).await,
        "already gone"
    );
    assert!(
        enabled(&device.lock()),
        "Delete Account wipes the profile itself; this only reaches the relay"
    );
    assert!(
        delete_vault_best_effort(&relay, &fresh()).await,
        "a profile without sync has no vault"
    );
}

#[test]
fn recovery_kit_debug_output_hides_the_code() {
    let conn = migrated();
    keys::save_account(&conn, RELAY, "4417938205561273").expect("account");
    let kit = recovery_kit(&conn).expect("kit");
    let printed = format!("{kit:?} {kit:#?}");
    assert_eq!(format!("{kit:?}"), "RecoveryKit { .. }");
    assert!(!printed.contains(&kit.recovery_code[..10]));
    assert!(!printed.contains("4417938205561273"));
}

#[tokio::test]
async fn a_new_setup_after_turning_off_publishes_only_this_device() {
    let old_relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    set_up(&old_relay, &device, dir.path()).await;
    let old_device = device_of(&device.lock());
    turn_off_between_rounds(&device, dir.path())
        .await
        .expect("turn off");

    let relay = FakeRelay::new();
    let creds = set_up(&relay, &device, dir.path()).await;

    let this = device_of(&device.lock());
    assert_ne!(this, old_device, "a new vault, a new device ID");
    let only_this = vec![format!("Single({:?})", this.to_hex())];
    let ops = relay.ops();
    assert_eq!(ops.len(), 1);
    let json = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &ops[0].ct).expect("open");
    let cs = from_json(&json).expect("changeset");
    assert_eq!(device_keys(&cs.rows), only_this, "the first envelope");

    let blob = relay.get_snapshot(&creds.auth()).await.expect("snapshot");
    let plain = open_sealed(&creds, &blob).expect("opens");
    let mut stream = &plain[..];
    let mut devs = Vec::new();
    let mut rows = Vec::new();
    while let Some(record) = read_record(&mut stream, &devs).expect("record") {
        match record {
            Record::Devices(list) => devs = list,
            Record::Row(row) => rows.push(row),
            _ => {}
        }
    }
    assert_eq!(device_keys(&rows), only_this, "the first snapshot");
}

#[tokio::test]
async fn a_first_round_that_pushed_nothing_fails_setup_and_resumes() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let account = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    relay.set_paid(&account.account_number, true);
    recovery_kit(&device.lock()).expect("kit");
    relay.fail_next(Route::Push, RelayError::PaymentRequired);

    let lapsed = finish(&relay, &device, dir.path()).await;
    assert!(matches!(lapsed, Err(SyncError::Lapsed)), "{lapsed:?}");
    assert_eq!(
        command_key(Err::<(), _>(lapsed.expect_err("lapsed"))),
        "validation.syncPaymentRequired"
    );
    assert!(enabled(&device.lock()), "sync stays on; a retry resumes");
    assert!(relay.ops().is_empty());
    assert_eq!(relay.snapshot_seq(), None);

    finish(&relay, &device, dir.path()).await.expect("resumed");
    assert_eq!(relay.ops().len(), 1);
    assert_eq!(relay.snapshot_seq(), Some(1));
    assert_eq!(cursor(&device.lock()), 1, "S is the applied cursor");
}

/// A setup that stopped after sync was turned on, on a profile that then turned out to be a copy
/// (a restore left the marker): the resumed setup re-joins under a new ID before its first push.
#[tokio::test]
async fn a_resumed_setup_on_a_restored_copy_rejoins_before_its_first_push() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let account = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    relay.set_paid(&account.account_number, true);
    recovery_kit(&device.lock()).expect("kit");
    relay.fail_next(Route::Pull, RelayError::Network("timeout".into()));
    let stopped = finish(&relay, &device, dir.path()).await;
    assert!(
        matches!(stopped, Err(SyncError::Transient(_))),
        "{stopped:?}"
    );
    let original = device_of(&device.lock());
    write_rejoin_marker(dir.path()).expect("marker");

    finish(&relay, &device, dir.path()).await.expect("resumed");

    let rejoined = device_of(&device.lock());
    assert!(rejoined > original, "a new device ID above the original");
    let creds = keys::credentials(&device.lock())
        .expect("credentials")
        .expect("stored");
    let ops = relay.ops();
    assert_eq!(ops.len(), 1);
    let json = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &ops[0].ct).expect("open");
    let cs = from_json(&json).expect("changeset");
    assert_eq!(
        (cs.device, cs.seq),
        (rejoined, 1),
        "the first push is under the new ID"
    );
    assert!(
        !dir.path().join(REJOIN_MARKER).exists(),
        "the re-join cleared the marker"
    );
    assert_eq!(
        startup_check(&device.lock(), dir.path(), &XorNative).expect("check"),
        StartupCheck::Fine,
        "the binding is written for the new ID"
    );
}

#[tokio::test]
async fn a_vault_the_relay_lost_during_setup_is_found_again_on_retry() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let account = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    relay.set_paid(&account.account_number, true);
    recovery_kit(&device.lock()).expect("kit");
    relay.fail_next(Route::Pull, RelayError::VaultNotFound);

    let gone = finish(&relay, &device, dir.path()).await;
    assert!(matches!(gone, Err(SyncError::VaultNotFound)), "{gone:?}");

    finish(&relay, &device, dir.path()).await.expect("resumed");
    assert_eq!(relay.snapshot_seq(), Some(1));
}

#[tokio::test]
async fn a_kit_for_the_longest_relay_address_renders_and_reads_back() {
    let relay = FakeRelay::new();
    let device = fresh();
    let longest = relay_of_len(256);
    create_account(&relay, &device, &longest)
        .await
        .expect("a 256-character address");
    let kit = recovery_kit(&device.lock()).expect("kit");
    assert_eq!(kit.relay_url.as_deref(), Some(longest.as_str()));
    assert!(kit.qr_svg.contains("</svg>"));

    let payload = crate::services::sync::recovery::recovery_qr_payload(
        &kit.account_number,
        &kit.recovery_code,
        Some(&longest),
    );
    assert!(payload.len() <= 512, "{}", payload.len());
    let qr = crate::services::sync::recovery::parse_recovery_qr_payload(&payload).expect("reads");
    assert_eq!(qr.relay_url.as_deref(), Some(longest.as_str()));

    assert_eq!(
        key_of(create_account(&relay, &fresh(), &relay_of_len(257)).await),
        "validation.syncRelayUrlInvalid"
    );
}

fn code_for(account: &str) -> crate::services::sync::connection_code::ConnectionCode {
    parse_connection_code(&encode_connection_code(RELAY, account)).expect("code")
}

#[tokio::test]
async fn an_own_server_code_adopts_the_account_and_skips_payment() {
    let fake = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let acct = fake.add_person();

    let account = connect_own_server(&fake, &device, &code_for(&acct))
        .await
        .expect("connect");
    assert_eq!(
        account,
        SyncAccount {
            account_number: acct.clone(),
            active: true,
            paid_until: Some(253_402_300_799),
        }
    );
    assert_eq!(
        keys::account(&device.lock()).expect("account"),
        Some((RELAY.to_string(), acct.clone()))
    );
    assert_eq!(fake.calls(), [Route::ServerInfo, Route::AccountStatus]);

    let kit = recovery_kit(&device.lock()).expect("kit");
    assert_eq!(kit.relay_url.as_deref(), Some(RELAY));
    finish(&fake, &device, dir.path()).await.expect("finish");
    assert!(enabled(&device.lock()));
}

#[tokio::test]
async fn a_blocked_or_deleted_person_is_refused_before_anything_is_stored() {
    let fake = FakeRelay::new();
    let device = fresh();
    let blocked = fake.add_person();
    fake.block(&blocked, NOW - 10);
    assert_eq!(
        command_key(connect_own_server(&fake, &device, &code_for(&blocked)).await),
        "validation.syncAccountBlocked"
    );
    assert_eq!(keys::account(&device.lock()).expect("account"), None);

    let gone = fake.add_person();
    fake.remove_account(&gone);
    assert_eq!(
        command_key(connect_own_server(&fake, &device, &code_for(&gone)).await),
        "validation.syncConnectionCodeRevoked"
    );
    assert_eq!(keys::account(&device.lock()).expect("account"), None);
}

#[tokio::test]
async fn a_server_that_fails_the_check_stores_nothing() {
    let fake = FakeRelay::new();
    let device = fresh();
    let acct = fake.add_person();
    fake.set_server_info(None);
    assert_eq!(
        command_key(connect_own_server(&fake, &device, &code_for(&acct)).await),
        "validation.syncServerNotFound"
    );
    assert_eq!(keys::account(&device.lock()).expect("account"), None);
    assert!(!fake.calls().contains(&Route::AccountStatus));
}

#[tokio::test]
async fn the_same_code_resumes_and_another_replaces_only_while_off() {
    let fake = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let (first, second) = (fake.add_person(), fake.add_person());

    let a = connect_own_server(&fake, &device, &code_for(&first))
        .await
        .expect("first");
    let again = connect_own_server(&fake, &device, &code_for(&first))
        .await
        .expect("again");
    assert_eq!(a.account_number, again.account_number);
    let rows: i64 = device
        .lock()
        .query_row("SELECT COUNT(*) FROM sync_state", [], |r| r.get(0))
        .expect("count");
    assert_eq!(rows, 1);

    let replaced = connect_own_server(&fake, &device, &code_for(&second))
        .await
        .expect("replaced");
    assert_eq!(replaced.account_number, second);
    assert_eq!(
        keys::account(&device.lock()).expect("account"),
        Some((RELAY.to_string(), second.clone()))
    );

    recovery_kit(&device.lock()).expect("kit");
    finish(&fake, &device, dir.path()).await.expect("finish");
    assert_eq!(
        command_key(connect_own_server(&fake, &device, &code_for(&first)).await),
        "validation.syncAlreadyEnabled"
    );
    // The same code while on is the same account: it only reads the status.
    connect_own_server(&fake, &device, &code_for(&second))
        .await
        .expect("same while on");
}

#[tokio::test]
async fn a_person_blocked_before_finish_reads_the_own_server_key() {
    let fake = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let acct = fake.add_person();
    connect_own_server(&fake, &device, &code_for(&acct))
        .await
        .expect("connect");
    recovery_kit(&device.lock()).expect("kit");
    fake.block(&acct, NOW - 10);
    let error = finish(&fake, &device, dir.path())
        .await
        .expect_err("blocked");
    assert!(matches!(error, SyncError::Lapsed), "{error:?}");
    match error.to_own_server_app_error() {
        AppError::Validation(key) => assert_eq!(key, "validation.syncAccountBlocked"),
        other => panic!("not a validation key: {other:?}"),
    }
}
