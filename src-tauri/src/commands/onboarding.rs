//! First-run onboarding commands (thin wrappers over `services::onboarding`).

use tauri::State;

use crate::db::Database;
use crate::error::Result;
use crate::models::OnboardingProgress;
use crate::services::onboarding;

/// Done-state for the dashboard checklist.
#[tauri::command]
pub async fn get_onboarding_progress(db: State<'_, Database>) -> Result<OnboardingProgress> {
    db.with_conn(onboarding::get_progress)
}

/// Set or clear one whitelisted onboarding flag
/// (`onboarding.completedAt`, `onboarding.checklistDismissed`).
#[tauri::command]
pub async fn set_onboarding_flag(
    db: State<'_, Database>,
    key: String,
    value: Option<String>,
) -> Result<()> {
    db.with_conn(|conn| onboarding::set_flag(conn, &key, value.as_deref()))
}
