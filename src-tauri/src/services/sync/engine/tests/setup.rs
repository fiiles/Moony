//! First-device setup (spec §11.5, ruling P4), turning sync off and deleting synced data (§11.6),
//! and the self-hosted relay's address and checkout (rulings SH-1, SH-2), on a fresh profile
//! against a `FakeRelay`.

use std::collections::BTreeSet;
use std::path::Path;

use zeroize::Zeroizing;

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
use crate::services::sync::identity::trust::{self, TrustState};
use crate::services::sync::identity::{self, CertKind, KitSecret};
use crate::services::sync::keys::VaultId;
use crate::services::sync::rekey::kit::KitStorage;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::http::DEFAULT_RELAY_URL;
use crate::services::sync::relay::RelayError;
use crate::services::sync::setup::{
    account_status, checkout_url, connect_own_server, create_account, delete_synced_data,
    delete_vault_best_effort, finish_setup, kit_view, note_kit_storage, prepare_kit,
    turn_off_between_rounds, KitDraft,
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

/// The setup's Kit draft, printed: one Kit per vault (a resumed setup finishes with the same
/// one), or a Kit of no vault before the vault's secrets exist.
pub(super) fn printed_kit(conn: &Connection) -> KitDraft {
    let vault_id = keys::credentials(conn)
        .expect("credentials")
        .map_or(VaultId([0xee; 16]), |creds| creds.vault_id);
    let seed = Zeroizing::new([vault_id.0[0] ^ 0x5a; 32]);
    let mut draft = KitDraft::new(vault_id, KitSecret::from_bytes(seed), NOW);
    note_kit_storage(conn, &mut draft, KitStorage::Paper).expect("printed");
    draft
}

async fn finish(relay: &FakeRelay, device: &TestConn, dir: &Path) -> SyncResult<()> {
    let kit = printed_kit(&device.lock());
    finish_setup(relay, device, "Laptop", &kit, dir, &XorNative, NOW).await
}

/// The Kit as setup shows it, of a new draft.
fn view_of(conn: &Connection) -> crate::models::sync::RecoveryKit {
    let draft = prepare_kit(conn, None, NOW).expect("draft");
    kit_view(conn, &draft).expect("kit")
}

/// A profile with sync set up on `relay`: account, payment, kit, finish.
pub(super) async fn set_up(relay: &FakeRelay, device: &TestConn, dir: &Path) -> Credentials {
    let account = create_account(relay, device, RELAY).await.expect("account");
    relay.set_paid(&account.account_number, true);
    prepare_kit(&device.lock(), None, NOW).expect("kit");
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

    let kit = view_of(&device.lock());
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
        // The Recovery Kit's row (revision §2.3).
        (
            "sync_devices",
            format!(
                "Single({:?})",
                printed_kit(&device.lock()).keys().id().to_hex()
            ),
        ),
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

/// Phase 5 §5.2–§5.3: setup gives the first device its signing key and a genesis certificate,
/// pinned in its own store and carried by its row in the first push.
#[tokio::test]
async fn setup_signs_and_pins_a_genesis_certificate_that_syncs() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let creds = set_up(&relay, &device, dir.path()).await;

    let conn = device.lock();
    let this = device_of(&conn);
    let seed = identity::own_seed(&conn)
        .expect("seed")
        .expect("a signing key");
    let entry = trust::entry(&conn, this).expect("entry").expect("pinned");
    assert_eq!(entry.kind, CertKind::Genesis);
    assert_eq!(entry.state, TrustState::Trusted);
    assert_eq!(entry.sign_pub, identity::sign_pub(&seed));
    assert!(trust::is_supported(&conn).expect("supported"));
    let row_cert: Option<String> = conn
        .query_row(
            "SELECT cert FROM sync_devices WHERE id = ?1",
            [this.to_hex()],
            |r| r.get(0),
        )
        .expect("row");
    assert_eq!(row_cert, Some(entry.cert.to_b64()));

    let ops = relay.ops();
    let json = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &ops[0].ct).expect("open");
    let cs = from_json(&json).expect("changeset");
    let pushed = cs
        .rows
        .iter()
        .find(|row| row.table == "sync_devices")
        .and_then(|row| row.columns.get("cert"))
        .map(|(value, _)| value.clone());
    assert_eq!(
        pushed,
        Some(SqlValue::Text(entry.cert.to_b64())),
        "the certificate went out with the row"
    );
}

/// Phase 5 §6.1: setup records epoch 0 at relay position 0.
#[tokio::test]
async fn setup_records_epoch_zero() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    set_up(&relay, &device, dir.path()).await;
    assert_eq!(
        crate::services::sync::state::epochs(&device.lock()).expect("epochs"),
        std::collections::BTreeMap::from([(0, 0)])
    );
}

/// A setup resumed after sync was turned on keeps the certificate it signed the first time.
#[tokio::test]
async fn a_resumed_setup_keeps_its_certificate() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    let account = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    relay.set_paid(&account.account_number, true);
    prepare_kit(&device.lock(), None, NOW).expect("kit");
    relay.fail_next(Route::Pull, RelayError::Network("timeout".into()));
    assert!(finish(&relay, &device, dir.path()).await.is_err());
    let first = trust::entries(&device.lock()).expect("entries");
    assert_eq!(first.len(), 2, "this device and the Kit");

    finish(&relay, &device, dir.path()).await.expect("resumed");

    assert_eq!(trust::entries(&device.lock()).expect("entries"), first);
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

    // While setup runs, the open draft is shown again, as often as asked (revision §2.2).
    let draft = prepare_kit(&device.lock(), None, NOW).expect("kit during setup");
    let kit = kit_view(&device.lock(), &draft).expect("kit");
    let again = prepare_kit(&device.lock(), Some(draft), NOW + 60).expect("again");
    let during = kit_view(&device.lock(), &again).expect("kit");
    assert_eq!(during.recovery_code, kit.recovery_code);
    assert_eq!(during.account_number, account.account_number);
    let other = prepare_kit(&device.lock(), None, NOW).expect("new");
    assert_ne!(
        kit_view(&device.lock(), &other).expect("kit").recovery_code,
        kit.recovery_code,
        "a closed draft is never shown again: a new Kit"
    );

    // Once sync is on, a Kit comes only from a key change.
    finish(&relay, &device, dir.path()).await.expect("setup");
    assert_eq!(
        key_of(prepare_kit(&device.lock(), None, NOW).map_err(SyncError::from)),
        "validation.syncAlreadyEnabled"
    );
}

#[tokio::test]
async fn setup_resumes_with_the_stored_account_and_vault_after_a_restart() {
    let relay = FakeRelay::new();
    let device = fresh();
    let first = create_account(&relay, &device, RELAY)
        .await
        .expect("account");
    let kit = prepare_kit(&device.lock(), None, NOW).expect("kit");

    // The app restarts: a new relay handle on the same relay, the same profile.
    let again = relay.clone();
    let second = create_account(&again, &device, &format!("{RELAY}/"))
        .await
        .expect("account again");
    let kit_again = prepare_kit(&device.lock(), None, NOW).expect("kit again");

    assert_eq!(second.account_number, first.account_number);
    assert!(
        kit_again.vault_id() == kit.vault_id(),
        "the vault's ID and key are made once"
    );
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

    // A Kit the person did nothing with (not printed, copied or saved).
    let unsaved = KitDraft::new(
        VaultId([0xee; 16]),
        KitSecret::generate().expect("seed"),
        NOW,
    );
    let unconfirmed = finish_setup(
        &relay,
        &device,
        "Laptop",
        &unsaved,
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
    prepare_kit(&device.lock(), None, NOW).expect("kit");
    let kit = printed_kit(&device.lock());
    let unnamed = finish_setup(&relay, &device, "  ", &kit, dir.path(), &XorNative, NOW);
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

    let kit = view_of(&device.lock());
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
    let kit = view_of(&custom.lock());
    assert_eq!(kit.relay_url.as_deref(), Some(RELAY));

    let default = fresh();
    create_account(&relay, &default, DEFAULT_RELAY_URL)
        .await
        .expect("account");
    let kit = view_of(&default.lock());
    assert_eq!(kit.relay_url, None);
    assert_eq!(
        kit.recovery_code.len(),
        113,
        "19 groups of five (revision §2.2)"
    );
    assert_eq!(kit.recovery_code.matches('-').count(), 18);
    assert!(kit.qr_svg.contains("</svg>"));
    let code = crate::services::sync::recovery::decode_kit_code(&kit.recovery_code).expect("v2");
    let (vault_id, _) = keys::ensure_vault_secrets(&default.lock()).expect("secrets");
    assert!(code.vault_id == vault_id);
    assert_eq!(
        code.account, kit.account_number,
        "the account is in the code"
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
        2,
        "this device and the Kit"
    );
    device
        .lock()
        .execute_batch(
            "INSERT INTO sync_trust (device, cert, kind, issuer, sign_pub, box_pub, state, \
               pinned_at) VALUES ('d', 'c', 1, 'd', x'01', x'02', 'trusted', 1); \
             INSERT OR IGNORE INTO sync_epochs (epoch, rotation_head, adopted_at) VALUES (0, 0, 1);",
        )
        .expect("trust and epochs");

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
        "sync_trust",
        "sync_epochs",
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
    let kit = view_of(&conn);
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
    let kit = printed_kit(&device.lock()).keys().id();
    let mut only_this = vec![
        format!("Single({:?})", this.to_hex()),
        format!("Single({:?})", kit.to_hex()),
    ];
    only_this.sort();
    let ops = relay.ops();
    assert_eq!(ops.len(), 1);
    let json = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &ops[0].ct).expect("open");
    let cs = from_json(&json).expect("changeset");
    let mut published = device_keys(&cs.rows);
    published.sort();
    assert_eq!(
        published, only_this,
        "the first envelope: this device and its Kit"
    );

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
    prepare_kit(&device.lock(), None, NOW).expect("kit");
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
    prepare_kit(&device.lock(), None, NOW).expect("kit");
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
    prepare_kit(&device.lock(), None, NOW).expect("kit");
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
    let kit = view_of(&device.lock());
    assert_eq!(kit.relay_url.as_deref(), Some(longest.as_str()));
    assert!(kit.qr_svg.contains("</svg>"));

    let payload = crate::services::sync::recovery::kit_qr_payload(
        &kit.recovery_code.replace('-', ""),
        Some(&longest),
    );
    assert!(payload.len() <= 512, "{}", payload.len());
    let qr = crate::services::sync::recovery::parse_kit_qr_payload(&payload).expect("reads");
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

    let kit = view_of(&device.lock());
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

    prepare_kit(&device.lock(), None, NOW).expect("kit");
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
    prepare_kit(&device.lock(), None, NOW).expect("kit");
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

// ── Revision §2.2, §2.7, §2.8: the Recovery Kit at setup and after ──────────────────────────

/// Every value of every table, as text, for looking for a secret in it.
fn every_value(conn: &Connection) -> Vec<String> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .expect("tables")
        .query_map([], |r| r.get(0))
        .expect("rows")
        .map(|t| t.expect("table"))
        .collect();
    let mut values = Vec::new();
    for table in tables {
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM \"{table}\""))
            .expect("select");
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                (0..columns)
                    .map(|i| {
                        r.get::<_, Value>(i).map(|v| match v {
                            Value::Text(text) => text,
                            Value::Blob(blob) => hex::encode(blob),
                            other => format!("{other:?}"),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("rows");
        for row in rows {
            values.extend(row.expect("row"));
        }
    }
    values
}

/// A profile with an account at `RELAY`, paid, and its vault's secrets: ready for the Kit step.
async fn ready_for_the_kit(relay: &FakeRelay, device: &TestConn) {
    let account = create_account(relay, device, RELAY).await.expect("account");
    relay.set_paid(&account.account_number, true);
    prepare_kit(&device.lock(), None, NOW).expect("vault secrets");
}

/// Revision §2.2: the Kit's seed lives in the draft only, never in the database.
#[tokio::test]
async fn the_kit_seed_is_never_stored() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    ready_for_the_kit(&relay, &device).await;
    let kit = printed_kit(&device.lock());

    finish_setup(&relay, &device, "Laptop", &kit, dir.path(), &XorNative, NOW)
        .await
        .expect("setup");

    let seed = *KitSecret::from_bytes(Zeroizing::new([kit.vault_id().0[0] ^ 0x5a; 32])).expose();
    let keys = kit.keys();
    let secrets = [
        hex::encode(seed),
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, seed),
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, seed),
        keys.token().to_string(),
        hex::encode(keys.sign_seed().expose()),
    ];
    for value in every_value(&device.lock()) {
        for secret in &secrets {
            assert!(!value.contains(secret.as_str()), "a Kit secret is stored");
        }
    }
}

/// Revision §2.2, §2.3: `finish_setup` puts the Kit on the server and writes and pins its row; a
/// retry finishes with the same Kit.
#[tokio::test]
async fn finishing_setup_registers_the_kit_once_even_when_retried() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    ready_for_the_kit(&relay, &device).await;
    let kit = printed_kit(&device.lock());
    let keys = kit.keys();
    // The Kit's answer is lost: the server stored it (put here by hand), the device did not
    // hear so. Nothing is registered here before the server holds the Kit.
    relay.fail_next(Route::PutKit, RelayError::Network("offline".into()));
    assert!(
        finish_setup(&relay, &device, "Laptop", &kit, dir.path(), &XorNative, NOW)
            .await
            .is_err()
    );
    assert!(trust::entry(&device.lock(), keys.id())
        .expect("entry")
        .is_none());
    let auth = keys::credentials(&device.lock())
        .expect("creds")
        .expect("bound")
        .auth();
    assert!(relay
        .put_kit(&auth, keys.token(), b"stored before the answer was lost")
        .await
        .expect("stored"));

    finish_setup(&relay, &device, "Laptop", &kit, dir.path(), &XorNative, NOW)
        .await
        .expect("resumed");
    let puts = |relay: &FakeRelay| {
        relay
            .calls()
            .iter()
            .filter(|r| **r == Route::PutKit)
            .count()
    };
    assert_eq!(
        puts(&relay),
        3,
        "the lost one, by hand, the retry's (idempotent)"
    );

    let creds = keys::credentials(&device.lock())
        .expect("creds")
        .expect("bound");
    let read = relay
        .get_kit(&creds.vault_id.to_b64(), keys.token())
        .await
        .expect("the Kit reads its envelope");
    let opened =
        crate::services::sync::rekey::envelope::open(&read.envelope, &keys).expect("opens");
    let me = device_of(&device.lock());
    assert_eq!(opened.header.epoch, 0);
    assert_eq!(opened.by, me);
    let confirmed: Vec<DeviceId> = opened.confirmed.iter().map(|c| c.device).collect();
    let mut expected = vec![me, keys.id()];
    expected.sort();
    assert_eq!(confirmed, expected);
    let conn = device.lock();
    let rows: Vec<(String, String, Option<String>)> = conn
        .prepare("SELECT id, platform, kit_storage FROM sync_devices WHERE platform = ?1")
        .expect("rows")
        .query_map([crate::services::sync::rekey::kit::KIT_PLATFORM], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .expect("rows")
        .map(|r| r.expect("row"))
        .collect();
    assert_eq!(
        rows,
        vec![(
            keys.id().to_hex(),
            "recoveryKit".to_string(),
            Some("paper".to_string())
        )]
    );
    let entry = trust::entry(&conn, keys.id())
        .expect("entry")
        .expect("pinned");
    assert_eq!(entry.state, TrustState::Trusted);
    assert_eq!(entry.kind, CertKind::Kit);
    assert!(entry.confirmed);
    assert!(
        trust::entry(&conn, me)
            .expect("entry")
            .expect("pinned")
            .confirmed
    );
    assert_eq!(
        trust::recipients(&conn, &[])
            .expect("recipients")
            .kit
            .map(|k| k.device),
        Some(keys.id())
    );
}

/// Revision §2.9: setup refuses a sync server that does not keep a Recovery Kit, before it
/// creates anything there.
#[tokio::test]
async fn a_server_without_the_kit_refuses_setup() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    ready_for_the_kit(&relay, &device).await;
    relay.set_server_info(Some(crate::services::sync::relay::ServerInfo {
        version: "1.0.0".into(),
        protocol: 2,
        mode: crate::services::sync::relay::ServerMode::SelfHosted,
        blobs: true,
        kit: false,
    }));

    let refused = finish(&relay, &device, dir.path()).await;

    assert_eq!(key_of(refused), "validation.syncServerNeedsUpdate");
    assert!(!relay.calls().contains(&Route::CreateVault));
    assert!(!enabled(&device.lock()));
}

/// Revision §2.7: where the Kit is kept only gets stronger (`cloud` > `file` > `copied` >
/// `paper`), in the draft and, once the Kit has a row, in the row (which syncs).
#[tokio::test]
async fn the_kit_storage_keeps_the_strongest_value() {
    use KitStorage::{Cloud, Copied, File, Paper};
    assert!(Paper < Copied && Copied < File && File < Cloud);
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    ready_for_the_kit(&relay, &device).await;
    let vault_id = keys::credentials(&device.lock())
        .expect("creds")
        .expect("bound")
        .vault_id;
    let kit_of = || {
        KitDraft::new(
            vault_id,
            KitSecret::from_bytes(Zeroizing::new([7; 32])),
            NOW,
        )
    };
    let mut draft = kit_of();
    assert_eq!(draft.storage(), None);
    for (noted, kept) in [
        (Copied, Copied),
        (Paper, Copied),
        (File, File),
        (Copied, File),
        (Cloud, Cloud),
        (Paper, Cloud),
    ] {
        note_kit_storage(&device.lock(), &mut draft, noted).expect("noted");
        assert_eq!(draft.storage(), Some(kept), "after {noted:?}");
    }
    finish_setup(
        &relay,
        &device,
        "Laptop",
        &draft,
        dir.path(),
        &XorNative,
        NOW,
    )
    .await
    .expect("setup");

    let conn = device.lock();
    let id = draft.keys().id();
    assert_eq!(
        crate::services::sync::rekey::kit::storage(&conn, id).expect("row"),
        Some(Cloud),
        "the row is written with the draft's value"
    );
    // A Kit with a row (one a key change made): the row follows, never weaker.
    conn.execute(
        "UPDATE sync_devices SET kit_storage = 'copied' WHERE id = ?1",
        [id.to_hex()],
    )
    .expect("weaker");
    let mut registered = kit_of();
    assert_eq!(registered.keys().id(), id, "the same Kit");
    note_kit_storage(&conn, &mut registered, Paper).expect("noted");
    assert_eq!(
        crate::services::sync::rekey::kit::storage(&conn, id).expect("row"),
        Some(Copied),
        "never weaker"
    );
    note_kit_storage(&conn, &mut registered, File).expect("noted");
    assert_eq!(
        crate::services::sync::rekey::kit::storage(&conn, id).expect("row"),
        Some(File)
    );
}

/// Revision §2.8: "Ověřit sadu" tells a valid Kit, a replaced one, one of another sync and one
/// this vault does not know, with no network.
#[tokio::test]
async fn checking_a_kit_tells_valid_replaced_other_and_unknown() {
    use crate::services::sync::recovery::{encode_kit_code, format_recovery_code, kit_qr_payload};
    use crate::services::sync::setup::{check_kit, KitCheck};
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    ready_for_the_kit(&relay, &device).await;
    let kit = printed_kit(&device.lock());
    let code = kit_view(&device.lock(), &kit)
        .expect("view")
        .recovery_code
        .clone();
    finish_setup(&relay, &device, "Laptop", &kit, dir.path(), &XorNative, NOW)
        .await
        .expect("setup");
    let calls = relay.calls().len();
    let conn = device.lock();
    let creds = keys::credentials(&conn).expect("creds").expect("bound");

    assert_eq!(check_kit(&conn, &code).expect("valid"), KitCheck::Valid);
    let raw = code.replace('-', "");
    assert_eq!(
        check_kit(&conn, &kit_qr_payload(&raw, Some(RELAY))).expect("scanned"),
        KitCheck::Valid
    );
    let stranger = KitSecret::generate().expect("seed");
    let unknown = format_recovery_code(
        &encode_kit_code(&creds.vault_id, &stranger, &creds.account).expect("code"),
    );
    assert_eq!(
        check_kit(&conn, &unknown).expect("unknown"),
        KitCheck::Unknown
    );
    let other = format_recovery_code(
        &encode_kit_code(&VaultId([0x44; 16]), &stranger, &creds.account).expect("code"),
    );
    assert_eq!(
        check_kit(&conn, &other).expect("other"),
        KitCheck::OtherSync
    );
    assert_eq!(
        check_kit(&conn, "ABCDE-FGHJK")
            .map_err(|e| e.to_string())
            .expect_err("not a code"),
        AppError::Validation("validation.syncRecoveryCodeInvalid".into()).to_string()
    );
    assert_eq!(relay.calls().len(), calls, "nothing reaches the server");

    // A Kit a key change replaced (by hand here: the old Kit's removal, the new Kit pinned).
    let new_keys = KitSecret::generate()
        .expect("seed")
        .derive(&creds.vault_id.0);
    let seed = identity::own_seed(&conn).expect("seed").expect("seed");
    let me = device_of(&conn);
    let removal = identity::Record::sign(
        identity::RecordType::Removed,
        creds.vault_id.0,
        kit.keys().id(),
        me,
        NOW + 100,
        0,
        &seed,
    )
    .expect("record");
    trust::mark_removed(&conn, &removal, Some(1)).expect("removed");
    let new_cert = identity::cert::issue_kit(&seed, me, &new_keys, NOW + 100).expect("new Kit");
    trust::pin_created_kit(&conn, &new_cert, NOW + 100).expect("pinned");

    assert_eq!(
        check_kit(&conn, &code).expect("replaced"),
        KitCheck::Replaced {
            at: Some(NOW + 100),
            current_created_at: Some(NOW + 100),
        }
    );
}

/// Revision §2.8: a Kit read the server reports is `kitUsed` once it is more than an hour old,
/// newer than what the person knows of, and no Kit join was pinned here since an hour before it;
/// the join is judged by when this device pinned it, never by the date its certificate claims.
#[test]
fn kit_used_judges_a_kit_join_by_when_it_was_pinned_here() {
    use crate::services::sync::engine::status::{dismiss_kit_used, kit_used};
    let conn = migrated();
    let read = NOW - 2 * 3600;
    assert_eq!(kit_used(&conn, None, NOW).expect("none"), None);
    assert_eq!(kit_used(&conn, Some(read), NOW).expect("used"), Some(read));
    assert_eq!(
        kit_used(&conn, Some(NOW - 1800), NOW).expect("too fresh"),
        None,
        "an hour's grace for the join to show up"
    );
    // A Kit join whose certificate claims a time after the read but that this device pinned
    // well before it does not explain the read.
    let vault = [0x11; 16];
    let kit = KitSecret::from_bytes(Zeroizing::new([3; 32])).derive(&vault);
    let joiner = identity::new_sign_seed().expect("seed");
    let joined = identity::cert::issue_kit_join(
        &kit,
        &identity::cert::CertFields {
            vault_id: vault,
            device: DeviceId([9; 16]),
            box_pub: [9; 32],
            issued_at: read + 600,
        },
        &joiner,
    )
    .expect("Kit join");
    let pin = |pinned_at: i64| {
        conn.execute("DELETE FROM sync_trust", []).expect("clear");
        conn.execute(
            "INSERT INTO sync_trust (device, cert, kind, issuer, sign_pub, box_pub, state, \
               pinned_at) VALUES (?1, ?2, 3, ?3, ?4, ?5, 'trusted', ?6)",
            rusqlite::params![
                joined.body.device.to_hex(),
                joined.to_b64(),
                kit.id().to_hex(),
                joined.body.sign_pub.to_vec(),
                joined.body.box_pub.to_vec(),
                pinned_at
            ],
        )
        .expect("pinned");
    };
    pin(read - 3 * 3600);
    assert_eq!(kit_used(&conn, Some(read), NOW).expect("used"), Some(read));
    pin(read - 1800);
    assert_eq!(
        kit_used(&conn, Some(read), NOW).expect("explained"),
        None,
        "a join within the hour before the read explains it"
    );
    pin(read + 600);
    assert_eq!(kit_used(&conn, Some(read), NOW).expect("explained"), None);

    conn.execute("DELETE FROM sync_trust", []).expect("clear");
    dismiss_kit_used(&conn, read).expect("dismissed");
    assert_eq!(
        kit_used(&conn, Some(read), NOW).expect("known"),
        None,
        "Vím o tom"
    );
    assert_eq!(
        kit_used(&conn, Some(read + 60), NOW).expect("newer"),
        Some(read + 60),
        "a newer read is news again"
    );
}
