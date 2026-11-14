//! The server check (spec §9.3): before an address is used for sync, the server is asked what it
//! is (`GET /v1/server`). Only a Moony sync server of the person's own, speaking a protocol this
//! app knows, passes; everything else is a refusal with a reading of why, before any account call.

use super::engine::{self, ConnAccess};
use super::error::{SyncError, SyncResult};
use super::keys;
use super::relay::http::{display_host, is_default_relay, is_insecure_address, normalize_base_url};
use super::relay::{Relay, RelayError, ServerMode};
use super::setup::on_db;
use super::state;
use crate::models::sync::SyncServerCheck;

/// The oldest server protocol this app works with (the contract of spec §5).
pub const MIN_PROTOCOL: u32 = 2;

/// Asks `relay` (the server at `address`) what it is. `RateLimited` stays itself (the person may
/// try again); any other failure to answer reads as "no sync server here"; an old protocol
/// asks for an update; the cloud or an unknown mode is not an own server.
pub async fn check_own_server<R: Relay>(relay: &R, address: &str) -> SyncResult<SyncServerCheck> {
    let address = normalize_base_url(address)?;
    let info = match relay.server_info().await {
        Ok(info) => info,
        Err(RelayError::RateLimited) => return Err(SyncError::RateLimited),
        Err(_) => return Err(refusal("validation.syncServerNotFound")),
    };
    if info.protocol < MIN_PROTOCOL {
        return Err(refusal("validation.syncServerNeedsUpdate"));
    }
    if info.mode != ServerMode::SelfHosted {
        return Err(refusal("validation.syncServerNotOwn"));
    }
    Ok(SyncServerCheck {
        host: display_host(&address).unwrap_or_default(),
        insecure: is_insecure_address(&address),
        address,
        version: info.version,
    })
}

fn refusal(key: &str) -> SyncError {
    SyncError::Local(crate::error::AppError::Validation(key.into()))
}

/// Changing the server address of a device that syncs (spec §9.3a). Under the round lock
/// (`engine::exclusive`, so no round runs on the old address while it is swapped); `relay_for`
/// builds the client from the normalised address, so the client and the stored value cannot differ:
///
/// 1. the stored credentials and `enabled` (sync off or no credentials: `syncSetupIncomplete`);
/// 2. the same address as stored (after normalisation) is `Ok` without a call or a write; the
///    Moony cloud is refused as a target (`syncServerNotOwn`, no call) while it is not built;
/// 3. the server check; then, with the STORED account, vault and token (never a connection
///    code's account), `account_status` and `vault_info`: an account, vault or token the server
///    does not know is `syncServerAddressOtherServer`. A blocked person is not refused for being
///    blocked (their vault still reads for 30 days, spec §5.4); past that the vault answers 402
///    and the change is refused as `SyncError::Lapsed` until the admin unblocks them;
/// 4. a vault head below this device's cursor is `syncServerAddressWentBack`. The device's own
///    went-back mark (`state::note_vault_went_back`) belongs to the current server and is not set;
/// 5. `keys::save_relay_url` and the end of the vault-gone pause (`state::set_vault_gone`), one
///    transaction. Returns the stored, normalized address. Any failure
///    leaves the old address and writes nothing.
pub async fn change_server_address<R: Relay, D: ConnAccess>(
    relay_for: impl FnOnce(&str) -> SyncResult<R>,
    db: &D,
    new_address: &str,
) -> SyncResult<String> {
    // The one normalisation point: the client, the checks and the stored value all use it.
    let new_address = normalize_base_url(new_address)?;
    let relay = &relay_for(&new_address)?;
    let _no_round = engine::exclusive(db).await;
    let (creds, cursor) = on_db(db, |conn| {
        let sync = state::load(conn)?.filter(|sync| sync.enabled);
        let creds = keys::credentials(conn)?;
        match (sync, creds) {
            (Some(sync), Some(creds)) => Ok((creds, sync.cursor)),
            _ => Err(crate::error::AppError::Validation(
                "validation.syncSetupIncomplete".into(),
            )),
        }
    })
    .await?;
    if normalize_base_url(&creds.relay_url).is_ok_and(|stored| stored == new_address) {
        return Ok(new_address);
    }
    if is_default_relay(&new_address) {
        return Err(refusal("validation.syncServerNotOwn"));
    }
    check_own_server(relay, &new_address).await?;
    match relay.account_status(&creds.account).await {
        Ok(_) => {}
        Err(RelayError::AccountNotFound) => return Err(refusal(OTHER_SERVER)),
        Err(error) => return Err(error.into()),
    }
    let info = match relay.vault_info(&creds.auth()).await {
        Ok(info) => info,
        Err(RelayError::VaultNotFound | RelayError::Unauthorized | RelayError::TokenReadOnly) => {
            return Err(refusal(OTHER_SERVER))
        }
        Err(error) => return Err(error.into()),
    };
    if info.head < cursor {
        return Err(refusal("validation.syncServerAddressWentBack"));
    }
    let address = new_address.clone();
    // The vault was just proven to exist on the new server: the old server's "vault gone" pause
    // ends with the swap, in the same transaction.
    on_db(db, move |conn| {
        let tx = conn.unchecked_transaction()?;
        keys::save_relay_url(&tx, &address)?;
        state::set_vault_gone(&tx, None)?;
        tx.commit()?;
        Ok(())
    })
    .await?;
    log::info!("[SYNC] Changed the sync server address");
    Ok(new_address)
}

const OTHER_SERVER: &str = "validation.syncServerAddressOtherServer";
