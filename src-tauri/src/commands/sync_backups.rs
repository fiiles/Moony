//! What a replace (a join, a repair, a re-sync) puts at stake, and the backups those make (audit
//! H4, M8, M9): the impact a confirmation shows and the consent it sends back, the sync backups
//! Restore lists and the person may delete, and on phones a copy of one in Documents for the Files
//! sheet. Thin: the flag, the input, `services::sync::{impact, backups}`.
//!
//! Every command starts with `require_feature()`. No command logs its arguments. Clocks cross IPC
//! as decimal strings (they exceed 2^53).

use std::path::Path;
#[cfg(any(mobile, test))]
use std::path::PathBuf;

#[cfg(any(mobile, test))]
use chrono::{DateTime, TimeZone};
use tauri::{AppHandle, State};

use super::sync::{data_dir, require_feature, sync_change_running, CHANGE_IN_PROGRESS};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{
    ReplaceConsentView, ReplaceImpactView, ReplaceOutcomeView, SyncBackupEntry,
};
use crate::services::sync::backups::{self, BackupMark, SyncBackup};
use crate::services::sync::impact::{self, ReplaceConsent, ReplaceImpact, IMPACT_CHANGED};
use crate::services::sync::join::ReplaceOutcome;

/// The name a staged copy of a sync backup is refused under, as the phone's backup export.
#[cfg(any(mobile, test))]
const EXPORT_INVALID: &str = "validation.backupExportInvalid";

// ============================================================================
// Helpers
// ============================================================================

/// The impact as a confirmation receives it: the watermark as a decimal string.
pub(super) fn impact_view(impact: &ReplaceImpact) -> ReplaceImpactView {
    ReplaceImpactView {
        pending_items: impact.pending_items,
        pending_batches: impact.pending_batches,
        documents: impact.documents,
        photos: impact.photos,
        accounts: impact.accounts,
        transactions: impact.transactions,
        changes_through: impact.changes_through.to_string(),
    }
}

/// The consent a confirmation sent. A watermark that is not a decimal `u64` (digits only) is no
/// impact this app showed: `validation.syncReplaceImpactChanged`, so the confirmation reloads it.
pub(super) fn consent_of(view: &ReplaceConsentView) -> Result<ReplaceConsent> {
    let clock = &view.changes_through;
    let changes_through = (!clock.is_empty() && clock.bytes().all(|b| b.is_ascii_digit()))
        .then(|| clock.parse::<u64>().ok())
        .flatten()
        .ok_or_else(|| AppError::Validation(IMPACT_CHANGED.into()))?;
    Ok(ReplaceConsent {
        changes_through,
        attachments: view.attachments,
    })
}

/// What a join, repair or re-sync did, as the UI shows it.
pub(super) fn outcome_view(outcome: &ReplaceOutcome) -> ReplaceOutcomeView {
    ReplaceOutcomeView {
        backup_path: outcome.backup.display().to_string(),
        attachments_kept: outcome.attachments.kept,
        attachments_in_backup_only: outcome.attachments.orphaned,
        unsent_items_in_backup_only: outcome.unsent_items_in_backup_only,
        unsent_batches_in_backup_only: outcome.unsent_batches_in_backup_only,
        kept: outcome.kept,
    }
}

fn entry_of(backup: &SyncBackup) -> SyncBackupEntry {
    SyncBackupEntry {
        path: backup.path.display().to_string(),
        created_at: backup.created_at,
        bytes: backup.bytes,
        kept: backup.mark == BackupMark::Kept,
    }
}

/// A backup is deleted only while no sync change runs: a join, repair or re-sync may have just
/// written and marked the backup it is about to rely on (`validation.syncChangeInProgress`).
fn deletion_allowed(change_running: bool) -> Result<()> {
    if change_running {
        return Err(AppError::Validation(CHANGE_IN_PROGRESS.into()));
    }
    Ok(())
}

/// `moony-sync-backup-<local date of created_at>.zip`, a name `services::backup::is_export_file_name`
/// accepts (so `finish_backup_export` and the sweep of an interrupted export remove it).
#[cfg(any(mobile, test))]
fn export_name(created_at: i64) -> String {
    export_name_in(created_at, &chrono::Local)
}

#[cfg(any(mobile, test))]
fn export_name_in<Tz: TimeZone>(created_at: i64, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let made = DateTime::from_timestamp(created_at, 0).unwrap_or_default();
    format!(
        "moony-sync-backup-{}.zip",
        made.with_timezone(zone).format("%Y-%m-%d")
    )
}

/// Copies the finished sync backup `path` (one `backups::list` finds in `data_dir`) to
/// `documents/<export_name>`, replacing a file of that name. Any other path is
/// `validation.backupExportInvalid`. A copy that fails leaves nothing behind.
#[cfg(any(mobile, test))]
fn stage_copy(data_dir: &Path, documents: &Path, path: &Path) -> Result<PathBuf> {
    let backup = backups::list(data_dir)?
        .into_iter()
        .find(|backup| backup.path == path && backups::is_sync_backup_path(data_dir, path))
        .ok_or_else(|| AppError::Validation(EXPORT_INVALID.into()))?;
    let name = export_name(backup.created_at);
    if !crate::services::backup::is_export_file_name(&name) {
        return Err(AppError::Validation(EXPORT_INVALID.into()));
    }
    let dest = documents.join(name);
    if let Err(error) = std::fs::copy(&backup.path, &dest) {
        if let Err(removal) = crate::services::backup::remove_staged_file(&dest) {
            log::warn!("[SYNC] Could not remove a partial copy of a sync backup: {removal}");
        }
        return Err(AppError::Internal(format!(
            "Failed to copy the sync backup: {error}"
        )));
    }
    Ok(dest)
}

// ============================================================================
// Commands
// ============================================================================

/// What a join, repair or re-sync would put at stake on this profile now. A confirmation loads it
/// when it opens and sends back `ReplaceConsentView` with what it showed.
#[tauri::command]
pub async fn sync_replace_impact(db: State<'_, Database>) -> Result<ReplaceImpactView> {
    require_feature()?;
    let impact = db.with_conn(impact::replace_impact)?;
    Ok(impact_view(&impact))
}

/// The backups joins, repairs and re-syncs made, newest first (an unlocked profile only).
#[tauri::command]
pub async fn sync_list_backups(
    app: AppHandle,
    db: State<'_, Database>,
) -> Result<Vec<SyncBackupEntry>> {
    require_feature()?;
    db.current_session()?;
    let dir = data_dir(&app)?;
    Ok(backups::list(&dir)?.iter().map(entry_of).collect())
}

/// The person deletes a listed sync backup (the UI confirms first: permanent), while the profile
/// is unlocked and no sync change runs (`deletion_allowed`).
#[tauri::command]
pub async fn sync_delete_backup(
    app: AppHandle,
    db: State<'_, Database>,
    path: String,
) -> Result<()> {
    require_feature()?;
    db.current_session()?;
    deletion_allowed(sync_change_running())?;
    let dir = data_dir(&app)?;
    backups::delete(&dir, Path::new(&path))
}

/// Phones: copies a finished sync backup (`backups::is_sync_backup_path`) to
/// `Documents/moony-sync-backup-<date>.zip` for the Files sheet; `finish_backup_export` removes it.
/// The copy is kept out of a device backup while it waits.
#[cfg(mobile)]
#[tauri::command]
pub async fn stage_sync_backup_export(
    app: AppHandle,
    db: State<'_, Database>,
    path: String,
) -> Result<String> {
    require_feature()?;
    db.current_session()?;
    let dir = data_dir(&app)?;
    let documents = super::backup::documents_dir(&app)?;
    let dest = tauri::async_runtime::spawn_blocking(move || {
        stage_copy(&dir, &documents, Path::new(&path))
    })
    .await
    .map_err(|e| AppError::Internal(format!("The copy did not finish: {e}")))??;
    crate::commands::device_backup::exclude_staged_file(&app, &dest).await;
    super::backup::path_string(&dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    /// 2026-11-12 12:00 and 23:00 UTC.
    const NOON: i64 = 1_794_484_800;
    const LATE: i64 = 1_794_524_400;

    #[test]
    fn export_name_is_the_backup_day() {
        assert_eq!(
            export_name_in(NOON, &Utc),
            "moony-sync-backup-2026-11-12.zip"
        );
        let east = FixedOffset::east_opt(2 * 3600).expect("offset");
        assert_eq!(
            export_name_in(LATE, &east),
            "moony-sync-backup-2026-11-13.zip",
            "the local day"
        );
        for at in [NOON, LATE, 0, i64::MAX] {
            assert!(
                crate::services::backup::is_export_file_name(&export_name(at)),
                "{at}"
            );
        }
    }

    #[test]
    fn a_backup_is_not_deleted_while_a_sync_change_runs() {
        match deletion_allowed(true) {
            Err(AppError::Validation(key)) => assert_eq!(key, "validation.syncChangeInProgress"),
            other => panic!("{other:?}"),
        }
        assert!(deletion_allowed(false).is_ok());
    }

    fn view(clock: &str) -> ReplaceConsentView {
        ReplaceConsentView {
            changes_through: clock.into(),
            attachments: 3,
        }
    }

    #[test]
    fn consent_of_reads_a_decimal_clock() {
        assert_eq!(
            consent_of(&view("123")).expect("consent"),
            ReplaceConsent {
                changes_through: 123,
                attachments: 3,
            }
        );
        assert_eq!(
            consent_of(&view("18446744073709551615"))
                .expect("u64::MAX")
                .changes_through,
            u64::MAX
        );
        assert_eq!(consent_of(&view("0")).expect("0").changes_through, 0);
        for bad in [
            "",
            "-1",
            "+5",
            " 5",
            "5 ",
            "1e3",
            "0x10",
            "18446744073709551616",
        ] {
            match consent_of(&view(bad)) {
                Err(AppError::Validation(key)) => assert_eq!(key, IMPACT_CHANGED, "{bad:?}"),
                other => panic!("{bad:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn an_impact_and_its_consent_round_trip_through_the_wire() {
        let impact = ReplaceImpact {
            pending_items: 2,
            pending_batches: 1,
            documents: 4,
            photos: 5,
            accounts: 6,
            transactions: 7,
            changes_through: u64::MAX - 1,
        };
        let shown = impact_view(&impact);
        assert_eq!(shown.changes_through, "18446744073709551614");
        let sent = ReplaceConsentView {
            changes_through: shown.changes_through.clone(),
            attachments: shown.documents + shown.photos,
        };
        assert_eq!(consent_of(&sent).expect("consent"), impact.consent());
    }

    #[test]
    fn only_a_listed_sync_backup_is_staged_and_under_its_export_name() {
        let data = tempfile::tempdir().expect("data");
        let documents = tempfile::tempdir().expect("documents");
        let backup = data.path().join(format!("sync-repair-{NOON}-kept.zip"));
        std::fs::write(&backup, b"zip").expect("backup");
        std::fs::write(data.path().join("moony.db"), b"db").expect("db");

        let staged = stage_copy(data.path(), documents.path(), &backup).expect("staged");
        assert_eq!(staged, documents.path().join(export_name(NOON)));
        assert_eq!(std::fs::read(&staged).expect("copy"), b"zip");
        assert!(backup.is_file(), "the backup stays");

        for refused in [
            data.path().join("moony.db"),
            data.path().join(format!("sync-repair-{LATE}.zip")),
            documents
                .path()
                .join(format!("sync-repair-{NOON}-kept.zip")),
            data.path()
                .join("sub")
                .join("..")
                .join(format!("sync-repair-{NOON}-kept.zip")),
        ] {
            match stage_copy(data.path(), documents.path(), &refused) {
                Err(AppError::Validation(key)) => assert_eq!(key, EXPORT_INVALID, "{refused:?}"),
                other => panic!("{refused:?}: {other:?}"),
            }
        }
    }
}
