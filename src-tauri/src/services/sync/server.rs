//! The server check (spec §9.3): before an address is used for sync, the server is asked what it
//! is (`GET /v1/server`). Only a Moony sync server of the person's own, speaking a protocol this
//! app knows, passes; everything else is a refusal with a reading of why, before any account call.

use super::error::{SyncError, SyncResult};
use super::relay::http::{display_host, is_insecure_address, normalize_base_url};
use super::relay::{Relay, RelayError, ServerMode};
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
