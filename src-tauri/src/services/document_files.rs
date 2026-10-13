//! Locating and opening attached document files.
//!
//! The DB stores the absolute path the file had when it was attached. That path goes stale when
//! the app data directory changes (the 0.9.0 identifier change copied the folder but not the
//! rows, 0.10.1 moves the folder to a new identifier), so the file is looked up under the
//! current app data dir first and the stored path is only the fallback.

use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};

/// Resolve a stored document path to an existing file.
///
/// Files live at `<app_data_dir>/<subdir>/<owner id>/<file>`. The stored path is re-rooted under
/// `app_data_dir` using its last two components and that file is preferred: after a data folder
/// move that fell back to copying, the old folder still holds a stale copy at the stored path,
/// and the live one is in the current folder. The stored path itself is the fallback, for a file
/// that did not move and so still sits where the database says.
pub fn resolve_document_path(app_data_dir: &Path, subdir: &str, stored: &str) -> Result<PathBuf> {
    let stored_path = Path::new(stored);

    let file_name = stored_path.file_name();
    let owner_dir = stored_path.parent().and_then(|p| p.file_name());
    if let (Some(owner_dir), Some(file_name)) = (owner_dir, file_name) {
        let rerooted = app_data_dir.join(subdir).join(owner_dir).join(file_name);
        if rerooted.is_file() {
            return Ok(rerooted);
        }
    }

    if stored_path.is_file() {
        return Ok(stored_path.to_path_buf());
    }

    Err(AppError::NotFound(format!(
        "Document file is missing on disk: {}",
        stored
    )))
}

/// Open a file with the system default application and report a failed launch.
pub fn open_with_system(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");

    let status = cmd
        .arg(path)
        .status()
        .map_err(|e| AppError::Internal(format!("Failed to open file: {}", e)))?;

    // `explorer` exits 1 even on success, so only the other platforms are checked.
    #[cfg(not(target_os = "windows"))]
    if !status.success() {
        return Err(AppError::Internal(format!(
            "Failed to open file: {} exited with {}",
            path.display(),
            status
        )));
    }
    #[cfg(target_os = "windows")]
    let _ = status;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("moony-docfiles-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn uses_the_stored_path_when_it_exists() {
        let dir = temp_dir("stored");
        let file = dir.join("a.pdf");
        fs::write(&file, b"x").expect("write");
        let got = resolve_document_path(&dir, "insurance_documents", file.to_str().expect("utf8"))
            .expect("resolves");
        assert_eq!(got, file);
    }

    #[test]
    fn reroots_a_stale_path_under_the_current_app_data_dir() {
        let dir = temp_dir("reroot");
        let owner = dir.join("insurance_documents").join("pol-1");
        fs::create_dir_all(&owner).expect("mkdir");
        fs::write(owner.join("doc_x.pdf"), b"x").expect("write");
        let stale = "/old/com.filipkral.moony-tauri/insurance_documents/pol-1/doc_x.pdf";
        let got = resolve_document_path(&dir, "insurance_documents", stale).expect("resolves");
        assert_eq!(got, owner.join("doc_x.pdf"));
    }

    #[test]
    fn the_rerooted_path_wins_when_the_stored_path_also_exists() {
        // After a copy-style move of the data folder the old folder still holds the file the
        // database points at; the copy in the current data folder is the live one.
        let current = temp_dir("both-current");
        let old = temp_dir("both-old");
        let live = current.join("insurance_documents").join("pol-1");
        let stale = old.join("insurance_documents").join("pol-1");
        fs::create_dir_all(&live).expect("mkdir live");
        fs::create_dir_all(&stale).expect("mkdir stale");
        fs::write(live.join("doc_x.pdf"), b"live").expect("write live");
        fs::write(stale.join("doc_x.pdf"), b"stale").expect("write stale");
        let stored = stale.join("doc_x.pdf");

        let got = resolve_document_path(
            &current,
            "insurance_documents",
            stored.to_str().expect("utf8"),
        )
        .expect("resolves");

        assert_eq!(got, live.join("doc_x.pdf"));
    }

    #[test]
    fn the_stored_path_is_the_fallback_when_nothing_sits_under_the_current_folder() {
        // A file that failed to move stays in the old folder; the stored path still finds it.
        let current = temp_dir("fallback-current");
        let old = temp_dir("fallback-old");
        let stale = old.join("insurance_documents").join("pol-1");
        fs::create_dir_all(&stale).expect("mkdir stale");
        fs::write(stale.join("doc_x.pdf"), b"stale").expect("write stale");
        let stored = stale.join("doc_x.pdf");

        let got = resolve_document_path(
            &current,
            "insurance_documents",
            stored.to_str().expect("utf8"),
        )
        .expect("resolves");

        assert_eq!(got, stored);
    }

    #[test]
    fn missing_file_is_not_found() {
        let dir = temp_dir("missing");
        let err = resolve_document_path(&dir, "insurance_documents", "/old/x/pol-1/gone.pdf")
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
