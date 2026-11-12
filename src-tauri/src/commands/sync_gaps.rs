//! Changes from another device that this device holds because something they need never arrived
//! (audit M3's way out, Task 17): the list Settings shows and skipping one after 14 days. Thin:
//! the flag, the password, the round lock, `services::sync::chain::gaps`.
//!
//! Every command starts with `require_feature()`. No command logs its arguments.

use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use super::sync::{
    guarded_change, now, password_given, require_feature, verify_password, wake_engine, AppEngine,
};
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
/// (`gaps::skip_missing_change`: `validation.syncGapNotSkippableYet` before 14 days), as a sync
/// change (`guarded_change`: one at a time, the engine stopped and started again) and between two
/// rounds, then asks for a round so the changes after it apply. `true` when a change was skipped;
/// `false` when there was nothing to skip (it arrived meanwhile, or the ID names no held change).
#[tauri::command]
pub async fn sync_skip_missing_change(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
    device_id: String,
) -> Result<bool> {
    let password = Zeroizing::new(password);
    require_feature()?;
    password_given(&password)?;
    verify_password(&app, password).await?;
    let Ok(device) = DeviceId::from_hex(&device_id) else {
        return Ok(false);
    };
    let database = db.inner();
    let skip = async {
        let _between_rounds = engine::exclusive(database).await;
        Ok(database.with_conn(|conn| gaps::skip_missing_change(conn, device, now()))?)
    };
    let skipped = guarded_change(
        &AppEngine {
            app: &app,
            db: database,
        },
        skip,
    )
    .await?;
    wake_engine(&app);
    Ok(skipped)
}
