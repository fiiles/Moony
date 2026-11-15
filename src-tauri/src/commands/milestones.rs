//! Milestones commands ("Co vás čeká") — thin wrappers over `services::milestones`.

use tauri::{AppHandle, State};

use crate::commands::sync::current_status;
use crate::db::Database;
use crate::error::Result;
use crate::models::{
    validate_milestone_key, validate_milestone_kinds, validate_milestone_state, Milestone,
};
use crate::services::loan_amortization::today_utc_day;
use crate::services::milestones;
use crate::services::sync::feature;

/// Every milestone as of today, without the done and snoozed occurrences.
#[tauri::command]
pub async fn get_milestones(app: AppHandle, db: State<'_, Database>) -> Result<Vec<Milestone>> {
    // The stall reminder exists only while the sync flag is on: without it no engine runs.
    let sync = feature::enabled()
        .then(|| current_status(&app, &db))
        .and_then(|status| match status {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("[MILESTONES] sync status skipped: {e}");
                None
            }
        });
    db.with_conn(|conn| milestones::list_milestones(conn, today_utc_day(), sync.as_ref()))
}

/// Mark one occurrence `done` or `snoozed` (a week; a month for upkeep).
#[tauri::command]
pub async fn set_milestone_state(
    db: State<'_, Database>,
    key: String,
    state: String,
) -> Result<()> {
    validate_milestone_state(&key, &state)?;
    db.with_conn(|conn| milestones::set_milestone_state(conn, &key, &state, today_utc_day()))
}

/// Undo `set_milestone_state`.
#[tauri::command]
pub async fn clear_milestone_state(db: State<'_, Database>, key: String) -> Result<()> {
    validate_milestone_key(&key)?;
    db.with_conn(|conn| milestones::clear_milestone_state(conn, &key))
}

/// Kinds turned off in Settings → Obecné → Připomínky na přehledu.
#[tauri::command]
pub async fn get_milestone_muted_kinds(db: State<'_, Database>) -> Result<Vec<String>> {
    db.with_conn(milestones::muted_kinds)
}

/// Replace the muted kinds; returns what was stored (sorted, without duplicates).
#[tauri::command]
pub async fn set_milestone_muted_kinds(
    db: State<'_, Database>,
    kinds: Vec<String>,
) -> Result<Vec<String>> {
    validate_milestone_kinds(&kinds)?;
    db.with_conn(|conn| milestones::set_muted_kinds(conn, &kinds))
}
