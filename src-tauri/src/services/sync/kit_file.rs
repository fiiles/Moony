//! The Sync Recovery Kit as a file on the desktop (audit M11): the webview renders the page or the
//! text, and Rust writes it where the person chose in the native save dialog, owner-only and
//! atomically (`crypto::replace_secret_file`), never into the profile's data folder, and says
//! whether that folder looks synced to a cloud service. The phone saves through the Files picker
//! (`stage_text_export`) instead.

use std::path::Path;

use crate::error::{AppError, Result};
use crate::models::sync::KitFileKind;
use crate::services::attachments::ensure_outside_data_dir;
use crate::services::crypto;

/// The most a Kit file may hold: the page with its inline QR is far below it.
pub const MAX_KIT_FILE_BYTES: usize = 256 * 1024;

/// The file name the save dialog proposes, the filter's label and its extension.
pub fn file_type(kind: KitFileKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        KitFileKind::Page => ("moony-sync-recovery-kit.html", "HTML", "html"),
        KitFileKind::Text => ("moony-sync-recovery-kit.txt", "Text", "txt"),
    }
}

/// Whether `contents` is a Kit file of this kit: at most `MAX_KIT_FILE_BYTES`, and the recovery
/// code's groups appear in order (the page wraps each group in its own element; the text holds the
/// code as it is). Anything else is a programming error of the caller, not something to save.
pub fn check_contents(contents: &str, recovery_code: &str) -> Result<()> {
    let refused = || AppError::Internal("The Recovery Kit file was refused".into());
    if contents.len() > MAX_KIT_FILE_BYTES {
        return Err(refused());
    }
    let mut rest = contents;
    for group in recovery_code.split('-') {
        if group.is_empty() {
            return Err(refused());
        }
        let Some(at) = rest.find(group) else {
            return Err(refused());
        };
        rest = &rest[at + group.len()..];
    }
    Ok(())
}

/// Writes the Kit file at `path` (outside `data_dir`, `validation.exportIntoDataFolder`
/// otherwise) readable and writable by its owner only, replacing a file the person chose to
/// overwrite in one step. Returns whether the folder looks cloud-synced (`looks_cloud_synced`).
pub fn write_kit_file(
    path: &Path,
    data_dir: &Path,
    contents: &str,
    recovery_code: &str,
) -> Result<bool> {
    check_contents(contents, recovery_code)?;
    ensure_outside_data_dir(data_dir, path)?;
    crypto::replace_secret_file(path, contents.as_bytes())?;
    Ok(looks_cloud_synced(path))
}

/// Folder names a desktop sync client creates (compared lowercased, a whole path component, or
/// its start where the client adds an account or a company: "OneDrive - Contoso",
/// "Dropbox (Personal)", "Google Drive (a@b.c)").
const SYNCED_PREFIXES: &[&str] = &[
    "onedrive",
    "dropbox",
    "google drive",
    "googledrive",
    "nextcloud",
    "owncloud",
    "pcloud drive",
    "icloud drive",
    "iclouddrive",
];

/// Folder names that only count whole (a prefix would match too much: "boxes", "my drives").
const SYNCED_NAMES: &[&str] = &["box", "box sync", "my drive", "com~apple~clouddocs"];

/// Whether `path` lies in a folder a cloud service syncs: iCloud Drive (`Library/Mobile
/// Documents`), macOS File Provider folders (`Library/CloudStorage`: OneDrive, Dropbox, Google
/// Drive, Box), or a folder named after OneDrive, Dropbox, Google Drive (`My Drive`), Box, pCloud,
/// Nextcloud or ownCloud. Case-insensitive, `/` and `\` alike. A hint only: a synced Documents
/// folder (macOS "Desktop & Documents" in iCloud, Windows' OneDrive backup) keeps its usual path.
pub fn looks_cloud_synced(path: &Path) -> bool {
    let text = path.to_string_lossy().to_lowercase().replace('\\', "/");
    if text.contains("/library/mobile documents/") || text.contains("/library/cloudstorage/") {
        return true;
    }
    text.split('/').any(|component| {
        let component = component.trim();
        SYNCED_NAMES.contains(&component)
            || SYNCED_PREFIXES
                .iter()
                .any(|prefix| component.starts_with(prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "ABCDE-FGHIJ-KLMNP";

    #[test]
    fn cloud_folders_are_recognised() {
        for synced in [
            "/Users/a/Library/Mobile Documents/com~apple~CloudDocs/kit.html",
            "/Users/a/Library/CloudStorage/OneDrive-Personal/kit.html",
            "C:\\Users\\a\\OneDrive\\Documents\\kit.html",
            "C:\\Users\\a\\OneDrive - Contoso\\kit.txt",
            "/Users/a/Dropbox/kit.txt",
            "/users/a/DROPBOX (Personal)/kit.txt",
            "/Users/a/Google Drive/kit.html",
            "G:\\My Drive\\kit.html",
            "/Users/a/Box/kit.html",
            "/Users/a/pCloud Drive/kit.txt",
            "/home/a/Nextcloud/Documents/kit.txt",
            "C:\\Users\\a\\iCloudDrive\\kit.txt",
        ] {
            assert!(looks_cloud_synced(Path::new(synced)), "{synced}");
        }
        for local in [
            "/Users/a/Documents/kit.html",
            "/Users/a/Downloads/kit.txt",
            "C:\\Users\\a\\Desktop\\kit.html",
            "/home/a/boxes/kit.txt",
            "/Users/a/Documents/my drives/kit.txt",
        ] {
            assert!(!looks_cloud_synced(Path::new(local)), "{local}");
        }
    }

    #[test]
    fn the_contents_must_hold_this_kits_code_and_stay_small() {
        assert!(check_contents("Recovery code: ABCDE-FGHIJ-KLMNP\n", CODE).is_ok());
        let page = r#"<span class="group">ABCDE</span>-<wbr><span class="group">FGHIJ</span>-<wbr><span class="group">KLMNP</span>"#;
        assert!(check_contents(page, CODE).is_ok());
        assert!(check_contents("Recovery code: ABCDE-KLMNP-FGHIJ", CODE).is_err());
        assert!(check_contents("nothing here", CODE).is_err());
        let huge = format!("{CODE}{}", "x".repeat(MAX_KIT_FILE_BYTES));
        assert!(check_contents(&huge, CODE).is_err());
    }

    #[test]
    fn the_kit_file_is_written_whole_and_never_into_the_data_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("data");
        std::fs::create_dir(&data).expect("data dir");
        let path = dir.path().join("kit.txt");
        std::fs::write(&path, "an older file").expect("older");
        let synced =
            write_kit_file(&path, &data, "Recovery code: ABCDE-FGHIJ-KLMNP", CODE).expect("write");
        assert!(!synced);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "Recovery code: ABCDE-FGHIJ-KLMNP"
        );

        let inside = data.join("key.enc");
        let refused = write_kit_file(&inside, &data, CODE, CODE).expect_err("data folder");
        assert!(
            matches!(&refused, AppError::Validation(key) if key == "validation.exportIntoDataFolder")
        );
        assert!(!inside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_kit_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("data");
        let path = dir.path().join("kit.html");
        write_kit_file(&path, &data, CODE, CODE).expect("write");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
