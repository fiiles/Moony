//! Changing the server address of a device that syncs (spec §9.3a): the new address must be the
//! same server state for this device, then the stored address is swapped in one write; on any
//! failure the old address stays.

use super::join::validation_key;
use super::setup::{enabled, fresh, set_up, RELAY};
use super::*;
use crate::services::sync::error::SyncResult;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;
use crate::services::sync::server::change_server_address;
use crate::services::sync::setup::recovery_kit;

const NEW: &str = "http://192.168.1.20:8787";

fn key_of(result: SyncResult<String>) -> String {
    let error = result.expect_err("refused");
    validation_key(&error)
        .unwrap_or_else(|| panic!("not a local validation error: {error:?}"))
        .to_string()
}

fn stored(device: &TestConn) -> (String, String) {
    keys::account(&device.lock())
        .expect("account")
        .expect("stored")
}

fn vault_gone_at(conn: &Connection) -> Option<i64> {
    state::marks(conn)
        .expect("marks")
        .expect("row")
        .vault_gone_at
}

/// One change pushed and pulled through a round, so the device's cursor is past zero.
async fn advance_cursor(vault: &Vault) -> u64 {
    exec(
        &vault.a.lock(),
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    let cts = vec![seal_from(&vault.a.lock(), &vault.creds)];
    push(&vault.relay, &vault.creds, &cts).await;
    crate::services::sync::engine::run_round(
        &vault.relay,
        &vault.a,
        &mut crate::services::sync::engine::snapshots::SnapshotPolicy::default(),
        crate::services::sync::engine::RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round");
    let n = cursor(&vault.a.lock());
    assert!(n > 0, "the cursor moved");
    n
}

#[tokio::test]
async fn a_new_address_of_the_same_server_is_stored_in_one_write() {
    let vault = vault().await;
    let relay = &vault.relay;
    let a = &vault.a;
    advance_cursor(&vault).await;
    // The old server had said the vault was gone; the new one proves it exists.
    exec(&a.lock(), "UPDATE sync_state SET vault_gone_at = 11");
    let (_, account) = stored(a);
    let state_before = state::load(&a.lock()).expect("s").expect("s");
    let calls_before = relay.calls().len();

    let new = change_server_address(|_| Ok(relay.clone()), a, NEW)
        .await
        .expect("changed");
    assert_eq!(new, NEW);
    assert_eq!(stored(a), (NEW.to_string(), account));
    let after = keys::credentials(&a.lock()).expect("c").expect("c");
    assert_eq!(after.vault_id.0, vault.creds.vault_id.0);
    assert_eq!(after.keys.current(), vault.creds.keys.current());
    assert_eq!(after.keys.current_epoch(), vault.creds.keys.current_epoch());
    assert_eq!(state::load(&a.lock()).expect("s").expect("s"), state_before);
    assert_eq!(vault_gone_at(&a.lock()), None, "the old pause is over");
    assert_eq!(state::vault_went_back_at(&a.lock()).expect("mark"), None);
    assert_eq!(
        relay.calls()[calls_before..],
        [Route::ServerInfo, Route::AccountStatus, Route::VaultInfo]
    );
    assert_eq!(
        recovery_kit(&a.lock()).expect("kit").relay_url,
        Some(NEW.to_string())
    );
}

#[tokio::test]
async fn another_server_leaves_the_old_address() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    set_up(&relay, &device, dir.path()).await;
    exec(&device.lock(), "UPDATE sync_state SET vault_gone_at = 11");
    let rows = sync_rows(&device.lock());

    // A server that has never heard of this person.
    let empty = FakeRelay::new();
    assert_eq!(
        key_of(change_server_address(|_| Ok(empty.clone()), &device, NEW).await),
        "validation.syncServerAddressOtherServer"
    );
    assert_eq!(
        empty.calls(),
        [Route::ServerInfo, Route::AccountStatus],
        "no vault call with an unknown account"
    );

    // A server that knows the account but not the vault.
    let partial = FakeRelay::new();
    partial.set_paid(&stored(&device).1, true);
    assert_eq!(
        key_of(change_server_address(|_| Ok(partial.clone()), &device, NEW).await),
        "validation.syncServerAddressOtherServer"
    );

    // The same server, but the vault's token no longer opens it.
    relay.fail_next(Route::VaultInfo, RelayError::Unauthorized);
    assert_eq!(
        key_of(change_server_address(|_| Ok(relay.clone()), &device, NEW).await),
        "validation.syncServerAddressOtherServer"
    );

    // A network failure on the vault call passes through as itself.
    relay.fail_next(Route::VaultInfo, RelayError::Network("down".into()));
    let error = change_server_address(|_| Ok(relay.clone()), &device, NEW)
        .await
        .expect_err("offline");
    assert!(validation_key(&error).is_none(), "{error:?}");

    assert_eq!(stored(&device).0, RELAY);
    assert_eq!(sync_rows(&device.lock()), rows);
    assert_eq!(vault_gone_at(&device.lock()), Some(11), "the pause stays");
}

#[tokio::test]
async fn an_older_copy_of_the_server_is_refused() {
    let vault = vault().await;
    let relay = &vault.relay;
    let a = &vault.a;
    let n = advance_cursor(&vault).await;
    let id = vault.creds.vault_id.to_b64();

    // A head exactly at the cursor is not behind it.
    assert_eq!(
        change_server_address(|_| Ok(relay.clone()), a, NEW)
            .await
            .expect("changed"),
        NEW
    );

    relay.roll_back_head(&id, n - 1);
    let rows = sync_rows(&a.lock());
    assert_eq!(
        key_of(change_server_address(|_| Ok(relay.clone()), a, "http://192.168.1.21:8787").await),
        "validation.syncServerAddressWentBack"
    );
    assert_eq!(stored(a).0, NEW);
    assert_eq!(
        state::vault_went_back_at(&a.lock()).expect("mark"),
        None,
        "the refusal does not mark the current server as gone back"
    );
    assert_eq!(sync_rows(&a.lock()), rows);
}

#[tokio::test]
async fn a_server_that_fails_the_check_or_sync_off_changes_nothing() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    set_up(&relay, &device, dir.path()).await;
    let rows = sync_rows(&device.lock());

    relay.set_server_info(None);
    assert_eq!(
        key_of(change_server_address(|_| Ok(relay.clone()), &device, NEW).await),
        "validation.syncServerNotFound"
    );
    relay.set_server_info(Some(crate::services::sync::relay::ServerInfo {
        version: "1.0.0".into(),
        protocol: 2,
        mode: crate::services::sync::relay::ServerMode::SelfHosted,
    }));

    // A bad address is refused before any call.
    let calls = relay.calls().len();
    assert_eq!(
        key_of(change_server_address(|_| Ok(relay.clone()), &device, "ftp://x.example").await),
        "validation.syncRelayUrlInvalid"
    );
    // The Moony cloud is not a target while it is not built.
    assert_eq!(
        key_of(
            change_server_address(
                |_| Ok(relay.clone()),
                &device,
                "https://Sync.MoonyFinance.com/"
            )
            .await
        ),
        "validation.syncServerNotOwn"
    );
    assert_eq!(relay.calls().len(), calls, "refused without a call");

    // A rate limit stays itself.
    relay.fail_next(Route::AccountStatus, RelayError::RateLimited);
    let error = change_server_address(|_| Ok(relay.clone()), &device, NEW)
        .await
        .expect_err("limited");
    assert!(matches!(
        error,
        crate::services::sync::error::SyncError::RateLimited
    ));

    // The same address (spelled differently) is a success without a call or a write.
    let calls = relay.calls().len();
    assert_eq!(
        change_server_address(|_| Ok(relay.clone()), &device, "https://Relay.Example.org/")
            .await
            .expect("no-op"),
        RELAY
    );
    assert_eq!(relay.calls().len(), calls);
    assert_eq!(sync_rows(&device.lock()), rows);

    // Sync off, or no credentials at all.
    exec(&device.lock(), "UPDATE sync_state SET enabled = 0");
    assert!(!enabled(&device.lock()));
    assert_eq!(
        key_of(change_server_address(|_| Ok(relay.clone()), &device, NEW).await),
        "validation.syncSetupIncomplete"
    );
    let bare = fresh();
    assert_eq!(
        key_of(change_server_address(|_| Ok(relay.clone()), &bare, NEW).await),
        "validation.syncSetupIncomplete"
    );
    assert_eq!(relay.calls().len(), calls);
}

#[tokio::test]
async fn a_blocked_person_may_still_move_the_server() {
    let relay = FakeRelay::new();
    let device = fresh();
    let dir = tempfile::tempdir().expect("dir");
    set_up(&relay, &device, dir.path()).await;
    let account = stored(&device).1;

    // Blocked a day ago: reads work for 30 days.
    relay.set_paid_until(&account, chrono::Utc::now().timestamp() - 86_400);
    assert_eq!(
        change_server_address(|_| Ok(relay.clone()), &device, NEW)
            .await
            .expect("changed"),
        NEW
    );

    // Past the grace the vault answers 402: refused, nothing written.
    relay.set_paid_until(&account, chrono::Utc::now().timestamp() - 31 * 86_400);
    let error = change_server_address(|_| Ok(relay.clone()), &device, "http://192.168.1.21:8787")
        .await
        .expect_err("lapsed");
    assert!(matches!(
        error,
        crate::services::sync::error::SyncError::Lapsed
    ));
    assert_eq!(stored(&device).0, NEW);
}
