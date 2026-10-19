//! Statement files that reach the phone from outside the app: shared into Moony from another app's
//! share sheet, opened with "Open in Moony", or picked in the Files picker.
//!
//! iOS hands such a file over as a copy inside the app's own container, and the copy stays there
//! until somebody removes it: `Documents/Inbox/` for a file opened in Moony and
//! `<tmp>/<bundle identifier>-Inbox/` for the document picker's copy. A bank statement is a plaintext
//! file, so this module keeps the copies short-lived: [`ReceivedFiles`] queues the paths the system
//! announced (`RunEvent::Opened`) until the webview asks for them, [`discard`] deletes one copy once
//! it is imported or abandoned, and [`sweep`] removes what an interrupted import left behind.
//!
//! The commands that expose this are phones only (`commands::received_files`); the logic here is
//! plain Rust so the desktop build compiles and tests it. The paths name the person's bank files
//! and are never logged.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::Url;

use crate::error::{AppError, Result};

/// The paths the system opened in Moony that the webview has not picked up yet.
///
/// `RunEvent::Opened` can fire before the webview listens (a cold start from the share sheet, a
/// share while the app is locked), so the paths wait here and the webview drains them with
/// [`ReceivedFiles::take`] at startup and whenever the `received-files` event says there are more.
#[derive(Default)]
pub struct ReceivedFiles {
    queue: Mutex<Vec<PathBuf>>,
    startup_sweep_claimed: AtomicBool,
}

impl ReceivedFiles {
    /// Queue the `file` URLs among `urls` (anything else, a web link or another app's scheme, is
    /// dropped) and return how many paths were added. A path that is already queued is not queued
    /// twice.
    pub fn push_urls(&self, urls: &[Url]) -> usize {
        let mut queue = self.lock();
        let mut added = 0;
        for url in urls {
            if url.scheme() != "file" {
                continue;
            }
            let Ok(path) = url.to_file_path() else {
                continue;
            };
            if !queue.contains(&path) {
                queue.push(path);
                added += 1;
            }
        }
        added
    }

    /// Hand the queued paths out, oldest first, and forget them.
    pub fn take(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.lock())
    }

    /// `true` for the first call of the process and `false` after: whoever gets `true` does the
    /// startup sweep. The sweep waits for the webview's first drain instead of running in `setup`:
    /// iOS has already copied the file a cold start is opening into the Inbox by the time `setup`
    /// runs, and `RunEvent::Opened` arrives after it, so a sweep in `setup` could delete the file
    /// that is about to be queued. By the first drain it is queued, and [`sweep`] is told to keep it.
    pub fn claim_startup_sweep(&self) -> bool {
        !self.startup_sweep_claimed.swap(true, Ordering::SeqCst)
    }

    fn lock(&self) -> MutexGuard<'_, Vec<PathBuf>> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The two folders iOS copies a received file into: `<Documents>/Inbox` (a file opened in Moony)
/// and `<tmp>/<identifier>-Inbox` (the document picker's copy).
pub fn inbox_dirs(document_dir: &Path, temp_dir: &Path, identifier: &str) -> [PathBuf; 2] {
    [
        document_dir.join("Inbox"),
        temp_dir.join(format!("{identifier}-Inbox")),
    ]
}

/// The canonical path of `path` when it is a received copy: a regular file (not a link) that sits
/// directly inside one of `inbox_dirs`, with no `..` in the way. A folder that does not exist
/// never matches.
fn received_copy(path: &Path, inbox_dirs: &[PathBuf]) -> Option<PathBuf> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return None;
    }
    if !fs::symlink_metadata(path).ok()?.file_type().is_file() {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    let parent = canonical.parent()?;
    inbox_dirs
        .iter()
        .filter_map(|dir| fs::canonicalize(dir).ok())
        .any(|dir| dir == parent)
        .then_some(canonical)
}

/// Whether `path` is a received copy: the one thing `discard` is allowed to delete. Both the path and
/// the inbox folders are canonicalized first, so `..` segments and links cannot lead out.
pub fn is_received_copy(path: &Path, inbox_dirs: &[PathBuf]) -> bool {
    received_copy(path, inbox_dirs).is_some()
}

/// Delete one received copy. Anything that is not a received copy is refused with
/// `validation.receivedFileOutside` and nothing is touched; a file that is already gone is fine
/// (the import and the sheet closing can both discard it).
pub fn discard(path: &Path, inbox_dirs: &[PathBuf]) -> Result<()> {
    match received_copy(path, inbox_dirs) {
        Some(copy) => match fs::remove_file(&copy) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::Internal(format!(
                "Failed to remove the received file: {e}"
            ))),
        },
        None => match fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            _ => Err(AppError::Validation(
                "validation.receivedFileOutside".into(),
            )),
        },
    }
}

/// Remove every regular file directly inside `inbox_dirs` except the paths in `keep` (what the
/// webview has just been handed): the copies an interrupted import left behind. Folders and links
/// are left alone; a folder that does not exist is an empty sweep; a file that cannot be removed is
/// logged and skipped. Returns how many files went.
pub fn sweep(inbox_dirs: &[PathBuf], keep: &[PathBuf]) -> Result<usize> {
    let keep: HashSet<PathBuf> = keep
        .iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .collect();
    let mut removed = 0;
    let mut listing_failed = None;
    for dir in inbox_dirs {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                log::warn!("[IMPORT] Could not list a received-files folder: {e}");
                listing_failed.get_or_insert(e);
                continue;
            }
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            if fs::canonicalize(&path).is_ok_and(|canonical| keep.contains(&canonical)) {
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(e) => log::warn!("[IMPORT] Could not remove a leftover received file: {e}"),
            }
        }
    }
    match listing_failed {
        Some(e) => Err(AppError::Internal(format!(
            "Failed to list a received-files folder: {e}"
        ))),
        None => Ok(removed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn file_url(path: &Path) -> Url {
        Url::from_file_path(path).expect("a file url")
    }

    /// An app container with both inbox folders created: `Documents/Inbox` and
    /// `tmp/<identifier>-Inbox`, plus a file next to each of them that must never be touched.
    struct Container {
        root: TempDir,
        inboxes: [PathBuf; 2],
    }

    impl Container {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("container");
            let inboxes = inbox_dirs(
                &root.path().join("Documents"),
                &root.path().join("tmp"),
                "com.example.moony",
            );
            for dir in &inboxes {
                fs::create_dir_all(dir).expect("inbox dir");
            }
            Container { root, inboxes }
        }

        fn documents(&self) -> PathBuf {
            self.root.path().join("Documents")
        }

        fn write(&self, dir: &Path, name: &str) -> PathBuf {
            let path = dir.join(name);
            fs::write(&path, b"Date;Amount\n").expect("write");
            path
        }
    }

    #[test]
    fn inbox_dirs_name_the_two_places_ios_copies_a_file_to() {
        let dirs = inbox_dirs(
            Path::new("/c/Documents"),
            Path::new("/c/tmp"),
            "com.example.moony",
        );
        assert_eq!(
            dirs,
            [
                Path::new("/c/Documents").join("Inbox"),
                Path::new("/c/tmp").join("com.example.moony-Inbox"),
            ]
        );
    }

    #[test]
    fn push_keeps_file_urls_and_drops_others() {
        let dir = tempfile::tempdir().expect("dir");
        let csv = dir.path().join("Výpis 2026 (1).csv");
        let received = ReceivedFiles::default();
        let urls = [
            Url::parse("https://example.com/statement.csv").expect("https url"),
            file_url(&csv),
            Url::parse("moony://open/statement.csv").expect("custom scheme url"),
        ];
        assert_eq!(received.push_urls(&urls), 1);
        // Spaces and diacritics come back as the person's file name, not percent-encoded
        assert_eq!(received.take(), vec![csv]);
    }

    #[test]
    fn push_queues_a_path_once() {
        let dir = tempfile::tempdir().expect("dir");
        let first = file_url(&dir.path().join("a.csv"));
        let second = file_url(&dir.path().join("b.csv"));
        let received = ReceivedFiles::default();
        // The same file twice in one call, then again in a later call
        assert_eq!(
            received.push_urls(&[first.clone(), second.clone(), first.clone()]),
            2
        );
        assert_eq!(received.push_urls(&[first, second]), 0);
        assert_eq!(received.take().len(), 2);
    }

    #[test]
    fn take_drains_in_order_and_twice_is_empty() {
        let dir = tempfile::tempdir().expect("dir");
        let paths: Vec<PathBuf> = ["a.csv", "b.ofx", "c.qif"]
            .iter()
            .map(|name| dir.path().join(name))
            .collect();
        let received = ReceivedFiles::default();
        assert!(received.take().is_empty());
        received.push_urls(&[file_url(&paths[0]), file_url(&paths[1])]);
        received.push_urls(&[file_url(&paths[2])]);
        assert_eq!(received.take(), paths);
        assert!(received.take().is_empty());
        // A path that was taken can be received again (the person shares the file a second time)
        assert_eq!(received.push_urls(&[file_url(&paths[0])]), 1);
        assert_eq!(received.take(), vec![paths[0].clone()]);
    }

    #[test]
    fn the_startup_sweep_is_claimed_once() {
        let received = ReceivedFiles::default();
        assert!(received.claim_startup_sweep());
        assert!(!received.claim_startup_sweep());
        assert!(!received.claim_startup_sweep());
    }

    #[test]
    fn is_received_copy_accepts_a_file_in_either_inbox() {
        let c = Container::new();
        for dir in &c.inboxes {
            let file = c.write(dir, "export (1).csv");
            assert!(is_received_copy(&file, &c.inboxes), "{}", file.display());
        }
    }

    #[test]
    fn is_received_copy_refuses_parent_segments_siblings_nested_and_symlinks() {
        let c = Container::new();
        let inside = c.write(&c.inboxes[0], "a.csv");
        assert!(is_received_copy(&inside, &c.inboxes));

        // A file next to the inbox, reached with `..` and directly
        let sibling = c.write(&c.documents(), "moony.db");
        let dotdot = c.inboxes[0].join("..").join("moony.db");
        assert!(!is_received_copy(&dotdot, &c.inboxes));
        assert!(!is_received_copy(&sibling, &c.inboxes));
        // `..` is refused even when it lands back in the inbox
        fs::create_dir(c.inboxes[0].join("sub")).expect("sub");
        assert!(!is_received_copy(
            &c.inboxes[0].join("sub").join("..").join("a.csv"),
            &c.inboxes
        ));

        // A folder whose name merely starts like the inbox's
        let lookalike = c.documents().join("Inbox-old");
        fs::create_dir(&lookalike).expect("lookalike");
        assert!(!is_received_copy(&c.write(&lookalike, "a.csv"), &c.inboxes));

        // A file one level down, the inbox folder itself, and a file that is not there
        let nested = c.write(&c.inboxes[0].join("sub"), "a.csv");
        assert!(!is_received_copy(&nested, &c.inboxes));
        assert!(!is_received_copy(&c.inboxes[0], &c.inboxes));
        assert!(!is_received_copy(
            &c.inboxes[0].join("gone.csv"),
            &c.inboxes
        ));
        // Nothing to match against
        assert!(!is_received_copy(&inside, &[]));
        assert!(!is_received_copy(
            &inside,
            &[c.root.path().join("no-such-inbox")]
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            // A link in the inbox to a file outside it, to a folder outside it, and to a file
            // inside it; and a link outside the inbox to a file inside it
            let outside_dir = c.root.path().join("elsewhere");
            fs::create_dir(&outside_dir).expect("outside dir");
            let secret = c.write(&outside_dir, "secret.txt");
            symlink(&secret, c.inboxes[0].join("to-file.csv")).expect("file link");
            symlink(&outside_dir, c.inboxes[0].join("to-dir")).expect("dir link");
            symlink(&inside, c.inboxes[0].join("alias.csv")).expect("alias link");
            symlink(&inside, c.documents().join("outside-alias.csv")).expect("outside link");
            assert!(!is_received_copy(
                &c.inboxes[0].join("to-file.csv"),
                &c.inboxes
            ));
            assert!(!is_received_copy(
                &c.inboxes[0].join("to-dir").join("secret.txt"),
                &c.inboxes
            ));
            assert!(!is_received_copy(
                &c.inboxes[0].join("alias.csv"),
                &c.inboxes
            ));
            assert!(!is_received_copy(
                &c.documents().join("outside-alias.csv"),
                &c.inboxes
            ));
            // The real file is still accepted
            assert!(is_received_copy(&inside, &c.inboxes));
        }
    }

    #[test]
    fn discard_removes_a_received_copy_and_nothing_else() {
        let c = Container::new();
        let first = c.write(&c.inboxes[0], "a.csv");
        let second = c.write(&c.inboxes[1], "b.csv");
        let neighbour = c.write(&c.inboxes[0], "c.csv");
        let sibling = c.write(&c.documents(), "moony.db");
        discard(&first, &c.inboxes).expect("discard from Documents/Inbox");
        discard(&second, &c.inboxes).expect("discard from tmp");
        assert!(!first.exists() && !second.exists());
        assert!(neighbour.exists() && sibling.exists());
    }

    #[test]
    fn discard_refuses_outside_with_the_validation_key() {
        let c = Container::new();
        let sibling = c.write(&c.documents(), "moony.db");
        let dotdot = c.inboxes[0].join("..").join("moony.db");
        let nested_dir = c.inboxes[0].join("sub");
        fs::create_dir(&nested_dir).expect("sub");
        let nested = c.write(&nested_dir, "a.csv");
        for path in [&sibling, &dotdot, &nested, &c.inboxes[0]] {
            let err = discard(path, &c.inboxes).expect_err("refused");
            assert!(
                matches!(&err, AppError::Validation(key) if key == "validation.receivedFileOutside"),
                "{}: {err:?}",
                path.display()
            );
        }
        // Nothing was deleted
        assert!(sibling.exists() && nested.exists() && c.inboxes[0].is_dir());

        #[cfg(unix)]
        {
            let link = c.inboxes[0].join("to-db.csv");
            std::os::unix::fs::symlink(&sibling, &link).expect("link");
            assert!(matches!(
                discard(&link, &c.inboxes),
                Err(AppError::Validation(key)) if key == "validation.receivedFileOutside"
            ));
            assert!(sibling.exists());
        }
    }

    #[test]
    fn discard_of_a_missing_file_is_ok() {
        let c = Container::new();
        let file = c.write(&c.inboxes[0], "a.csv");
        discard(&file, &c.inboxes).expect("first discard");
        // Imported, then abandoned again: the file is already gone
        discard(&file, &c.inboxes).expect("second discard");
        discard(&c.inboxes[1].join("never-there.csv"), &c.inboxes).expect("never there");
    }

    #[test]
    fn sweep_removes_files_and_keeps_folders() {
        let c = Container::new();
        c.write(&c.inboxes[0], "a.csv");
        c.write(&c.inboxes[0], "b.ofx");
        c.write(&c.inboxes[1], "c.qif");
        let sub = c.inboxes[0].join("sub");
        fs::create_dir(&sub).expect("sub");
        let nested = c.write(&sub, "nested.csv");
        let sibling = c.write(&c.documents(), "moony.db");

        assert_eq!(sweep(&c.inboxes, &[]).expect("sweep"), 3);

        for dir in &c.inboxes {
            let files: Vec<_> = fs::read_dir(dir)
                .expect("list")
                .map(|e| e.expect("entry").path())
                .filter(|p| p.is_file())
                .collect();
            assert!(files.is_empty(), "{files:?}");
        }
        assert!(sub.is_dir() && nested.exists() && sibling.exists());
        assert_eq!(sweep(&c.inboxes, &[]).expect("sweep again"), 0);
    }

    #[test]
    fn sweep_of_missing_folders_is_empty() {
        let dir = tempfile::tempdir().expect("dir");
        let missing = [dir.path().join("Inbox"), dir.path().join("x-Inbox")];
        assert_eq!(sweep(&missing, &[]).expect("sweep"), 0);
    }

    #[test]
    fn sweep_keeps_the_paths_it_is_told_to() {
        let c = Container::new();
        let kept = c.write(&c.inboxes[0], "opened.csv");
        let gone = c.write(&c.inboxes[0], "leftover.csv");
        let kept_in_tmp = c.write(&c.inboxes[1], "picked.csv");
        assert_eq!(
            sweep(&c.inboxes, &[kept.clone(), kept_in_tmp.clone()]).expect("sweep"),
            1
        );
        assert!(kept.exists() && kept_in_tmp.exists() && !gone.exists());
    }

    #[cfg(unix)]
    #[test]
    fn sweep_does_not_follow_links_out_of_the_inbox() {
        let c = Container::new();
        let secret = c.write(&c.documents(), "moony.db");
        std::os::unix::fs::symlink(&secret, c.inboxes[0].join("to-db.csv")).expect("link");
        c.write(&c.inboxes[0], "a.csv");
        assert_eq!(sweep(&c.inboxes, &[]).expect("sweep"), 1);
        assert!(secret.exists());
    }
}
