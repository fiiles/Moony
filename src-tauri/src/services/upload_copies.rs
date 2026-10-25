//! Plaintext copies that WebKit makes of the files a person picks in an `<input type="file">`.
//!
//! On the phone, "Vybrat soubor", "Vybrat z fotek" and "Vyfotit" all go through the system picker,
//! and WKWebView copies each picked file into the app container's tmp folder as
//! `<tmp>/WKFileUploadPanel-<random>/<name>` before the page can read it. The copy is never removed
//! by WebKit, so a photo of a property or a scanned policy would sit there in plaintext next to the
//! encrypted database, while Moony's own copy is stored encrypted.
//!
//! The tmp folder is shared with the system and other components (`<identifier>-Inbox`,
//! `com.apple.WebKit.*`, pasteboard files), so this is deliberately narrow: only folders directly
//! inside it whose name starts with [`UPLOAD_PANEL_PREFIX`], and only the regular files inside
//! those. It never follows a link and never recurses. The names are the person's file names and are
//! never logged, only counts.
//!
//! It runs once at startup and when the app is locked, never after an upload: another picked file
//! may still be waiting in an open sheet.

use std::fs;
use std::io;
use std::path::Path;

/// What WebKit names the folder of one picker session: this prefix and a random suffix.
const UPLOAD_PANEL_PREFIX: &str = "WKFileUploadPanel-";

/// Remove WebKit's upload copies from `temp_dir`: for every real folder (not a link) directly inside
/// it whose name starts with `WKFileUploadPanel-`, the regular files inside and then the folder.
/// Everything else in `temp_dir` is left alone. A missing `temp_dir` is an empty sweep, and a file
/// or folder that cannot be removed is logged and skipped. Returns how many files were removed.
///
/// `DirEntry::file_type` does not follow links, so a link named like a panel folder is skipped, and
/// a link inside a panel folder is never removed or followed (the folder then stays, as it is not
/// empty).
pub fn sweep(temp_dir: &Path) -> usize {
    let entries = match fs::read_dir(temp_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return 0,
        Err(e) => {
            log::warn!("[UPLOAD] Could not list the temp folder: {e}");
            return 0;
        }
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        let is_panel = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(UPLOAD_PANEL_PREFIX));
        if !is_panel || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        removed += remove_panel_folder(&entry.path());
    }
    removed
}

/// Remove the regular files directly inside one panel folder and then the folder. Returns how many
/// files went.
fn remove_panel_folder(folder: &Path) -> usize {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return 0,
        Err(e) => {
            log::warn!("[UPLOAD] Could not list an upload folder: {e}");
            return 0;
        }
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("[UPLOAD] Could not remove an upload copy: {e}"),
        }
    }
    match fs::remove_dir(folder) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("[UPLOAD] Could not remove an upload folder: {e}"),
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel_with(tmp: &Path, folder: &str, files: &[&str]) -> std::path::PathBuf {
        let dir = tmp.join(folder);
        fs::create_dir(&dir).expect("panel folder");
        for name in files {
            fs::write(dir.join(name), b"\xff\xd8\xff photo").expect("picked file");
        }
        dir
    }

    #[test]
    fn removes_the_files_and_the_folder() {
        let tmp = tempfile::tempdir().expect("tmp");
        let first = panel_with(tmp.path(), "WKFileUploadPanel-yR5EWGMY", &["IMG_0005.jpeg"]);
        let second = panel_with(
            tmp.path(),
            "WKFileUploadPanel-uq8mgK1j",
            &["IMG_0111.jpeg", "Výpis (1).pdf"],
        );
        assert_eq!(sweep(tmp.path()), 3);
        assert!(!first.exists() && !second.exists());
        assert_eq!(sweep(tmp.path()), 0);
    }

    #[test]
    fn removes_an_empty_panel_folder_without_counting_it() {
        let tmp = tempfile::tempdir().expect("tmp");
        let empty = panel_with(tmp.path(), "WKFileUploadPanel-empty", &[]);
        assert_eq!(sweep(tmp.path()), 0);
        assert!(!empty.exists());
    }

    #[test]
    fn leaves_the_other_tmp_entries_alone() {
        let tmp = tempfile::tempdir().expect("tmp");
        let inbox = panel_with(tmp.path(), "com.moonyfinance.mobile-Inbox", &["a.csv"]);
        let webkit = panel_with(tmp.path(), "com.apple.WebKit.Networking", &["x"]);
        let temporary_items = panel_with(tmp.path(), "TemporaryItems", &["y"]);
        let pasteboard = tmp.path().join(".com.apple.Pasteboard.abc");
        fs::write(&pasteboard, b"p").expect("pasteboard");
        // A plain file named like a panel folder, and a folder that only contains the prefix
        let plain = tmp.path().join("WKFileUploadPanel-x");
        fs::write(&plain, b"not a folder").expect("plain file");
        let lookalike = panel_with(tmp.path(), "old-WKFileUploadPanel-z", &["z.jpeg"]);
        let panel = panel_with(tmp.path(), "WKFileUploadPanel-real", &["IMG_0002.jpeg"]);

        assert_eq!(sweep(tmp.path()), 1);

        assert!(!panel.exists());
        assert!(inbox.join("a.csv").exists());
        assert!(webkit.join("x").exists());
        assert!(temporary_items.join("y").exists());
        assert!(pasteboard.exists());
        assert!(plain.is_file());
        assert!(lookalike.join("z.jpeg").exists());
    }

    #[test]
    fn leaves_what_is_not_a_regular_file_inside_a_panel_folder() {
        let tmp = tempfile::tempdir().expect("tmp");
        let panel = panel_with(tmp.path(), "WKFileUploadPanel-a", &["IMG_1.jpeg"]);
        let nested = panel.join("nested");
        fs::create_dir(&nested).expect("nested");
        fs::write(nested.join("deep.jpeg"), b"d").expect("deep file");

        assert_eq!(sweep(tmp.path()), 1);

        // The folder is not empty, so it stays with what it still holds
        assert!(!panel.join("IMG_1.jpeg").exists());
        assert!(nested.join("deep.jpeg").exists());
    }

    #[test]
    fn a_missing_folder_is_an_empty_sweep() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert_eq!(sweep(&tmp.path().join("no-such-tmp")), 0);
    }

    #[test]
    fn an_empty_tmp_folder_is_an_empty_sweep() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert_eq!(sweep(tmp.path()), 0);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_a_link_out_of_the_tmp_folder() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().expect("tmp");
        let outside = tempfile::tempdir().expect("outside");
        let secret = outside.path().join("moony.db");
        fs::write(&secret, b"database").expect("secret");

        // A link named like a panel folder that points at a folder outside tmp
        symlink(outside.path(), tmp.path().join("WKFileUploadPanel-link")).expect("folder link");
        // A real panel folder holding a link to a file outside tmp and a link to a folder
        let panel = panel_with(tmp.path(), "WKFileUploadPanel-real", &["IMG_1.jpeg"]);
        symlink(&secret, panel.join("to-db.jpeg")).expect("file link");
        symlink(outside.path(), panel.join("to-dir")).expect("inner folder link");

        assert_eq!(sweep(tmp.path()), 1);

        assert!(secret.exists());
        assert!(outside.path().is_dir());
        // The link to the folder is untouched, and so is everything it points at
        assert!(tmp.path().join("WKFileUploadPanel-link").exists());
        assert!(!panel.join("IMG_1.jpeg").exists());
        assert!(fs::symlink_metadata(panel.join("to-db.jpeg")).is_ok());
    }
}
