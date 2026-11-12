//! The Sync Recovery Kit as a file on the desktop (audit M11): the webview renders the page or the
//! text, and Rust writes it where the person chose in the native save dialog, owner-only and
//! atomically (`write_kit_file`), never into the profile's data folder, and says whether that
//! folder looks synced to a cloud service. The phone saves through the Files picker
//! (`stage_text_export`) instead.
//!
//! "Owner-only" means mode 0600, set when the file is created, on macOS and Linux. On Windows the
//! file takes the permissions the chosen folder gives its files (usually the person's own folders
//! are theirs alone).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::error::{AppError, Result};
use crate::models::sync::KitFileKind;
use crate::services::attachments::ensure_outside_data_dir;
use crate::services::crypto;

/// The most a Kit file may hold: the page with its inline QR is far below it.
pub const MAX_KIT_FILE_BYTES: usize = 256 * 1024;

/// A base64 payload (the page's QR data URI): data, never markup, so it is left out of the markup
/// checks (its characters can happen to read like `/onXyz=`).
static BASE64_PAYLOAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)base64,[a-z0-9+/=]*").expect("static pattern is valid"));

/// What a page that runs nothing never holds: a script element, an inline event handler
/// (`onload=`, `<svg/onload=`, `"onerror=` right after a value) or a `javascript:` URL.
static ACTIVE_CONTENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<\s*script|<[^>]*[\s/"']on[a-z]+\s*=|javascript:"#)
        .expect("static pattern is valid")
});

/// The file name the save dialog proposes, the filter's label and its extension.
pub fn file_type(kind: KitFileKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        KitFileKind::Page => ("moony-sync-recovery-kit.html", "HTML", "html"),
        KitFileKind::Text => ("moony-sync-recovery-kit.txt", "Text", "txt"),
    }
}

/// Whether `contents` is a Kit file of this kit: at most `MAX_KIT_FILE_BYTES`, the recovery code's
/// groups appear in order (the page wraps each group in its own element; the text holds the code
/// as it is), and a page runs nothing (`ACTIVE_CONTENT`; `recoveryKitHtml` writes inline CSS and
/// a data-URI image only). Anything else is a programming error of the caller (or a compromised
/// webview), not something to save.
pub fn check_contents(kind: KitFileKind, contents: &str, recovery_code: &str) -> Result<()> {
    let refused = || AppError::Internal("The Recovery Kit file was refused".into());
    if contents.len() > MAX_KIT_FILE_BYTES {
        return Err(refused());
    }
    if kind == KitFileKind::Page
        && ACTIVE_CONTENT.is_match(&BASE64_PAYLOAD.replace_all(contents, "base64,"))
    {
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
/// otherwise), owner-only (see the module), replacing a file the person chose to overwrite in one
/// step. Returns whether the folder looks cloud-synced (`looks_cloud_synced`).
///
/// The person may pick a folder other accounts can write to (`/tmp`, `/Users/Shared`), so nothing
/// here opens a name someone could have planted: the content is staged under a random name that
/// is created new (`create_new`: an existing file, link or pipe there fails the save instead of
/// being followed), flushed, then renamed onto `path` (a link at `path` is replaced, never written
/// through) and the folder flushed. Any failure removes the staging file. Unlike
/// `crypto::replace_secret_file`, which serves the app's private data folder, no predictable
/// `<name>.tmp` is used.
pub fn write_kit_file(
    path: &Path,
    data_dir: &Path,
    kind: KitFileKind,
    contents: &str,
    recovery_code: &str,
) -> Result<bool> {
    check_contents(kind, contents, recovery_code)?;
    ensure_outside_data_dir(data_dir, path)?;
    let staging = staging_path(path)?;
    let staged = create_new_owner_only(&staging)
        .and_then(|mut file| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&staging, path));
    if let Err(e) = staged {
        if let Err(cleanup) = fs::remove_file(&staging) {
            if cleanup.kind() != std::io::ErrorKind::NotFound {
                log::warn!("[SYNC] Could not remove the Recovery Kit's staging file: {cleanup}");
            }
        }
        return Err(e.into());
    }
    if let Err(e) = crypto::sync_parent_dir(path) {
        log::warn!("[SYNC] Could not flush the folder after saving the Recovery Kit: {e}");
    }
    Ok(looks_cloud_synced(path))
}

/// `.<file name>.<random UUID>.tmp` next to `path`, so the rename never crosses file systems and
/// nobody can plant something at the name beforehand.
fn staging_path(path: &Path) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| AppError::Internal("The Recovery Kit path has no file name".into()))?;
    let mut staging = std::ffi::OsString::from(".");
    staging.push(name);
    staging.push(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    Ok(path.with_file_name(staging))
}

/// A new file at `path` (failing if anything exists there, a link included), mode 0600 from its
/// creation on Unix.
fn create_new_owner_only(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
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

    const TEXT: KitFileKind = KitFileKind::Text;
    const PAGE: KitFileKind = KitFileKind::Page;

    /// The shape `recoveryKitHtml` (src/utils/recovery-kit.ts) produces: inline CSS, the code in
    /// one span per group, the QR as a base64 data URI, no script and no handler.
    fn page(qr_base64: &str) -> String {
        format!(
            r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Moony Sync Recovery Kit</title>
<style>
  @page {{ size: A4; margin: 18mm; }}
  .code .group {{ white-space: nowrap; }}
</style>
</head>
<body>
<main>
<h1>Moony Sync Recovery Kit</h1>
<section><h2>Recovery code</h2><div class="value code"><span class="group">ABCDE</span>-<wbr><span class="group">FGHIJ</span>-<wbr><span class="group">KLMNP</span></div><p class="note">Keep it safe.</p></section>
<section class="qr"><img src="data:image/svg+xml;base64,{qr_base64}" alt="QR code"><p class="note">QR code</p></section>
</main>
</body>
</html>
"#
        )
    }

    #[test]
    fn the_contents_must_hold_this_kits_code_and_stay_small() {
        assert!(check_contents(TEXT, "Recovery code: ABCDE-FGHIJ-KLMNP\n", CODE).is_ok());
        assert!(check_contents(PAGE, &page("PHN2Zz48L3N2Zz4="), CODE).is_ok());
        assert!(check_contents(TEXT, "Recovery code: ABCDE-KLMNP-FGHIJ", CODE).is_err());
        assert!(check_contents(TEXT, "nothing here", CODE).is_err());
        let huge = format!("{CODE}{}", "x".repeat(MAX_KIT_FILE_BYTES));
        assert!(check_contents(TEXT, &huge, CODE).is_err());
    }

    #[test]
    fn a_page_with_a_script_or_an_inline_handler_is_refused() {
        // A base64 payload may happen to read "/onXyz=": it is data, not an attribute.
        assert!(check_contents(PAGE, &page("PHN2Zz4/onLoad="), CODE).is_ok());
        for planted in [
            "<script>fetch('https://example.com')</script>",
            "<SCRIPT src=x></SCRIPT>",
            "< script>",
            r#"<img src="x" onerror="alert(1)">"#,
            r#"<img src="x"ONERROR=alert(1)>"#,
            "<svg/onload=alert(1)>",
            "<body onLoad = alert(1)>",
            r#"<a href="javascript:alert(1)">"#,
        ] {
            let contents = page("PHN2Zz48L3N2Zz4=").replace("<main>", &format!("<main>{planted}"));
            assert!(check_contents(PAGE, &contents, CODE).is_err(), "{planted}");
        }
        // The text file is never rendered: the same words in it are only words.
        let text = format!("Recovery code: {CODE}\n<script> onload=x");
        assert!(check_contents(TEXT, &text, CODE).is_ok());
    }

    #[test]
    fn the_kit_file_is_written_whole_and_never_into_the_data_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("data");
        std::fs::create_dir(&data).expect("data dir");
        let path = dir.path().join("kit.txt");
        std::fs::write(&path, "an older file").expect("older");
        let synced = write_kit_file(&path, &data, TEXT, "Recovery code: ABCDE-FGHIJ-KLMNP", CODE)
            .expect("write");
        assert!(!synced);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "Recovery code: ABCDE-FGHIJ-KLMNP"
        );

        let inside = data.join("key.enc");
        let refused = write_kit_file(&inside, &data, TEXT, CODE, CODE).expect_err("data folder");
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
        write_kit_file(&path, &data, TEXT, CODE, CODE).expect("write");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// Entries of `dir` whose name ends in `.tmp`.
    fn staging_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect()
    }

    /// In a folder others can write to (`/tmp`, `/Users/Shared`), another account can plant a link
    /// where the file is staged or saved: the save must never write through it.
    #[cfg(unix)]
    #[test]
    fn a_planted_link_never_redirects_the_save() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("data");
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, "the victim's own file").expect("victim");
        let path = dir.path().join("kit.txt");
        // The predictable staging name `crypto::replace_secret_file` uses.
        std::os::unix::fs::symlink(&victim, dir.path().join("kit.txt.tmp")).expect("staging link");
        write_kit_file(&path, &data, TEXT, CODE, CODE).expect("write");
        assert_eq!(
            std::fs::read_to_string(&victim).expect("victim"),
            "the victim's own file"
        );

        // A link at the chosen name itself is replaced, not written through.
        std::fs::remove_file(&path).expect("remove kit");
        std::os::unix::fs::symlink(&victim, &path).expect("final link");
        write_kit_file(&path, &data, TEXT, CODE, CODE).expect("write over the link");
        assert_eq!(
            std::fs::read_to_string(&victim).expect("victim"),
            "the victim's own file"
        );
        assert!(!std::fs::symlink_metadata(&path)
            .expect("kit")
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&path).expect("kit"), CODE);
    }

    #[test]
    fn a_failed_save_leaves_no_staging_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("data");
        // The chosen name is a folder with something in it: the final rename fails.
        let path = dir.path().join("kit.txt");
        std::fs::create_dir(&path).expect("folder");
        std::fs::write(path.join("keep"), "x").expect("content");
        assert!(write_kit_file(&path, &data, TEXT, CODE, CODE).is_err());
        assert_eq!(staging_files(dir.path()), Vec::<String>::new());
        assert!(path.join("keep").exists());
    }
}
