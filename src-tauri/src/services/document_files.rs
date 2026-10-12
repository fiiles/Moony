//! Locating and opening attached document files.
//!
//! The DB stores the absolute path the file had when it was attached. That path dies when the
//! app data directory moves (the 0.9.0 identifier change copied the folder but not the rows), so
//! the file is looked up under the current app data dir first.

use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};

/// Resolve a stored document path to an existing file.
///
/// Files live at `<app_data_dir>/<subdir>/<owner id>/<file>`; the stored path is tried as-is
/// first, then re-rooted under `app_data_dir` using its last two components.
pub fn resolve_document_path(app_data_dir: &Path, subdir: &str, stored: &str) -> Result<PathBuf> {
    let stored_path = Path::new(stored);
    if stored_path.is_file() {
        return Ok(stored_path.to_path_buf());
    }

    let file_name = stored_path.file_name();
    let owner_dir = stored_path.parent().and_then(|p| p.file_name());
    if let (Some(owner_dir), Some(file_name)) = (owner_dir, file_name) {
        let rerooted = app_data_dir.join(subdir).join(owner_dir).join(file_name);
        if rerooted.is_file() {
            return Ok(rerooted);
        }
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
    fn missing_file_is_not_found() {
        let dir = temp_dir("missing");
        let err = resolve_document_path(&dir, "insurance_documents", "/old/x/pol-1/gone.pdf")
            .expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
