//! Changes from another device that this device holds because something they need never arrived
//! (audit M3's way out, Task 17): the list Settings shows and skipping one after 14 days. Thin:
//! the flag, the password, the round lock, `services::sync::chain::gaps`.
//!
//! Every command starts with `require_feature()`. No command logs its arguments.

use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use super::sync::{now, password_given, require_feature, verify_password, wake_engine};
use crate::db::Database;
use crate::error::Result;
use crate::models::sync::HeldGapView;
use crate::services::sync::chain::gaps;
use crate::services::sync::engine;
use crate::services::sync::hlc::DeviceId;

/// The changes held because something they need has not arrived, the oldest first, each with its
/// author's name and from when it may be skipped.
#[tauri::command]
pub async fn sync_held_gaps(db: State<'_, Database>) -> Result<Vec<HeldGapView>> {
    require_feature()?;
    db.with_conn(gaps::held_gap_views)
}

/// Skips the held change of `device_id` once the device password is checked
/// (`gaps::skip_missing_change`: `validation.syncGapNotSkippableYet` before 14 days), between two
/// rounds, then asks for a round so the changes after it apply. A device ID that names no held
/// change (it arrived meanwhile, or the ID is not one) is nothing to skip.
#[tauri::command]
pub async fn sync_skip_missing_change(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
    device_id: String,
) -> Result<()> {
    let password = Zeroizing::new(password);
    require_feature()?;
    password_given(&password)?;
    verify_password(&app, password).await?;
    let Ok(device) = DeviceId::from_hex(&device_id) else {
        return Ok(());
    };
    {
        let _between_rounds = engine::exclusive(db.inner()).await;
        db.with_conn(|conn| gaps::skip_missing_change(conn, device, now()))?;
    }
    wake_engine(&app);
    Ok(())
}
