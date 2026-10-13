//! One-time move of the data folder from the pre-0.10.1 identifier to the current one.
//!
//! Tauri derives the data folder from the bundle identifier. Releases 0.9.0–0.10.0 used
//! `com.filipkral.moony`; from 0.10.1 the identifier is `com.moonyfinance.desktop`, so the
//! folder of an existing install sits next to the new one under the old name. On every start,
//! before any database access, `migrate_legacy_data_dir` moves it into place exactly once.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::services::backup::{DB_FILE_NAME, KEY_FILES};

/// Identifier of releases 0.9.0–0.10.0. Its data folder sits next to the current one.
pub const LEGACY_IDENTIFIER: &str = "com.filipkral.moony";
/// Written into the new folder after the move, as a trace of where the profile came from.
const MARKER: &str = ".moved-from-com.filipkral.moony";
/// SQLite side files of the database. They travel right before `moony.db`.
const DB_SIDE_FILES: [&str; 3] = ["moony.db-wal", "moony.db-shm", "moony.db-journal"];
/// Finder litter, not data: never moved, and removed together with an otherwise empty folder.
const FINDER_FILE: &str = ".DS_Store";

/// Outcome of [`migrate_legacy_data_dir`].
#[derive(Debug)]
pub enum Moved {
    /// Nothing to do: the new folder already holds a profile, or there is no legacy profile.
    Nothing,
    /// Every entry of the legacy folder was renamed into place.
    Moved(PathBuf),
    /// At least one entry could not be renamed (another volume, a locked file) and was copied;
    /// the copied originals stay in the legacy folder.
    Copied(PathBuf),
    /// The profile moved, but the named entries (attachments, backups, ...) could neither be
    /// renamed nor copied and stay in the legacy folder. Stored document paths still find them.
    Partial(PathBuf, Vec<String>),
}

/// Move the pre-0.10.1 data folder into `new_dir` once.
///
/// A `new_dir` that already holds `moony.db` is a real profile and is never touched. Without
/// a `moony.db` in the legacy folder (next to `new_dir`, named [`LEGACY_IDENTIFIER`]) there is
/// nothing to move. Otherwise the legacy folder is merged into `new_dir` entry by entry: other
/// things may already sit in `new_dir` before this runs (on Linux the log plugin creates
/// `logs/` there), so an entry whose name exists in `new_dir` is left in the legacy folder and
/// the new one wins. Each other entry is renamed into place, or copied when the rename fails;
/// a copy leaves the original behind and is built under a temporary name, so an entry in
/// `new_dir` is always whole. The legacy folder is removed afterwards only if it is empty.
///
/// Crash safety: the check above looks only at `moony.db`, so it is the LAST entry to move.
/// First come all other entries (key files, attachments, backups, ...), then the `moony.db-wal`
/// and `moony.db-shm` side files, then `moony.db`. A crash anywhere before the last rename
/// leaves `moony.db` in the legacy folder, so the next start repeats the merge and moves
/// whatever is still missing; a crash after it leaves nothing behind.
///
/// Only an *essential* entry ([`is_essential`]: the database, its side files, the key files)
/// that can be neither renamed nor copied aborts the move with an error, before the database is
/// touched; the next start retries. Any other entry that cannot be moved (attachments, backups)
/// is logged, left in the legacy folder and reported as [`Moved::Partial`]: the user keeps the
/// profile, and the documents left behind stay reachable through their stored paths.
pub fn migrate_legacy_data_dir(new_dir: &Path) -> io::Result<Moved> {
    if new_dir.join(DB_FILE_NAME).exists() {
        return Ok(Moved::Nothing);
    }
    let Some(parent) = new_dir.parent() else {
        return Ok(Moved::Nothing);
    };
    let legacy = parent.join(LEGACY_IDENTIFIER);
    if !legacy.join(DB_FILE_NAME).is_file() {
        return Ok(Moved::Nothing);
    }
    merge_legacy_into(
        &legacy,
        new_dir,
        &|from, to| fs::rename(from, to),
        &copy_entry_into_place,
    )
}

/// The merge behind [`migrate_legacy_data_dir`]. `move_entry` is `fs::rename` and `copy_entry`
/// is [`copy_entry_into_place`] in the app; the tests pass stand-ins that fail (a cross-volume
/// rename or a locked file cannot be staged in a temp dir).
fn merge_legacy_into(
    legacy: &Path,
    new_dir: &Path,
    move_entry: &dyn Fn(&Path, &Path) -> io::Result<()>,
    copy_entry: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<Moved> {
    fs::create_dir_all(new_dir)?;
    let names = fs::read_dir(legacy)?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<io::Result<Vec<_>>>()?;

    let mut copied = false;
    let mut left_behind: Vec<String> = Vec::new();
    for name in in_move_order(names) {
        if name == FINDER_FILE {
            continue;
        }
        let target = new_dir.join(&name);
        // `symlink_metadata`: a dangling link in the way still counts as "exists".
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        let source = legacy.join(&name);
        if let Err(rename_error) = move_entry(&source, &target) {
            log::warn!(
                "[DATA] could not rename {} to {} ({rename_error}); copying instead",
                source.display(),
                target.display()
            );
            match copy_entry(&source, &target) {
                Ok(()) => copied = true,
                // Without an essential entry the profile cannot work: stop before `moony.db`.
                Err(copy_error) if is_essential(&name) => return Err(copy_error),
                Err(copy_error) => {
                    log::warn!(
                        "[DATA] {} could be neither renamed nor copied ({copy_error}); it stays in {}",
                        name.to_string_lossy(),
                        legacy.display()
                    );
                    left_behind.push(name.to_string_lossy().into_owned());
                }
            }
        }
    }

    remove_if_empty(legacy);
    // The profile is already in place; a missing marker must not be reported as a failed move.
    if let Err(e) = fs::write(new_dir.join(MARKER), b"") {
        log::warn!("[DATA] could not write {MARKER}: {e}");
    }
    let from = legacy.to_path_buf();
    Ok(if !left_behind.is_empty() {
        Moved::Partial(from, left_behind)
    } else if copied {
        Moved::Copied(from)
    } else {
        Moved::Moved(from)
    })
}

/// Entries without which the profile cannot work: the database, its side files and the key
/// files. Everything else (attachments, backups, logs) is expendable for the move.
fn is_essential(name: &OsStr) -> bool {
    KEY_FILES.iter().any(|key_file| name == *key_file)
        || DB_SIDE_FILES.iter().any(|side| name == *side)
}

/// Entries in the order they must move: everything else (alphabetical), then the database side
/// files, then `moony.db` itself.
fn in_move_order(mut names: Vec<OsString>) -> Vec<OsString> {
    let rank = |name: &OsStr| -> u8 {
        if name == DB_FILE_NAME {
            2
        } else if DB_SIDE_FILES.iter().any(|side| name == *side) {
            1
        } else {
            0
        }
    };
    names.sort_by(|a, b| (rank(a), a).cmp(&(rank(b), b)));
    names
}

/// Copy `source` (file or folder) to `target` so that `target` appears whole or not at all:
/// build it under a temporary name next to it, then rename.
fn copy_entry_into_place(source: &Path, target: &Path) -> io::Result<()> {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(io::Error::other(format!(
            "no parent folder for {}",
            target.display()
        )));
    };
    let temp = dir.join(copy_temp_name(name));
    remove_path(&temp)?; // left by an earlier, interrupted start
    let result = if fs::symlink_metadata(source)?.is_dir() {
        copy_dir(source, &temp)
    } else {
        fs::copy(source, &temp).map(|_| ())
    }
    .and_then(|()| fs::rename(&temp, target));
    if result.is_err() {
        let _ = remove_path(&temp);
    }
    result
}

fn copy_temp_name(name: &OsStr) -> OsString {
    let mut temp = OsString::from(".moony-moving-");
    temp.push(name);
    temp
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Remove a file or folder tree; a missing path is fine. Only ever called on our own temporary
/// names.
fn remove_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Remove `dir` when nothing but Finder litter is left in it. `remove_dir` (not `remove_dir_all`)
/// refuses a folder that still holds anything, so a failure here is expected and ignored.
fn remove_if_empty(dir: &Path) {
    let only_finder_litter = fs::read_dir(dir)
        .map(|entries| entries.flatten().all(|e| e.file_name() == FINDER_FILE))
        .unwrap_or(false);
    if only_finder_litter {
        let _ = fs::remove_file(dir.join(FINDER_FILE));
        let _ = fs::remove_dir(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io;
    use std::path::Path;

    const NEW_NAME: &str = "com.moonyfinance.desktop";

    fn profile(dir: &Path) {
        fs::create_dir_all(dir.join("insurance_documents")).unwrap();
        fs::write(dir.join("moony.db"), b"db").unwrap();
        fs::write(dir.join("salt"), b"s").unwrap();
        fs::write(dir.join("key.enc"), b"k").unwrap();
        fs::write(dir.join("insurance_documents/a.pdf"), b"pdf").unwrap();
    }

    /// A rename that always fails, as when the two folders sit on different volumes.
    fn rename_never_works(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("cross-device link"))
    }

    /// A copy that always fails, as when the disk is full or a file is locked.
    fn copy_never_works(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("file is locked"))
    }

    /// Names of the entries of a folder, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn moves_the_legacy_folder_when_the_new_one_does_not_exist() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        let result = migrate_legacy_data_dir(&new).unwrap();
        assert!(matches!(result, Moved::Moved(ref from) if *from == legacy));
        assert!(new.join("moony.db").is_file());
        assert!(new.join("salt").is_file());
        assert!(new.join("key.enc").is_file());
        assert!(new.join("insurance_documents/a.pdf").is_file());
        assert!(new.join(MARKER).is_file());
        assert!(!legacy.exists());
    }

    #[test]
    fn leaves_an_existing_profile_alone() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        profile(&new);
        fs::write(new.join("moony.db"), b"newer").unwrap();
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Nothing
        ));
        assert_eq!(fs::read(new.join("moony.db")).unwrap(), b"newer");
        assert!(legacy.join("moony.db").is_file());
        assert!(!new.join(MARKER).exists());
    }

    #[test]
    fn an_empty_or_ds_store_only_new_folder_receives_the_profile() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join(".DS_Store"), b"").unwrap();
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Moved(_)
        ));
        assert!(new.join("moony.db").is_file());
        assert!(new.join("insurance_documents/a.pdf").is_file());
    }

    #[test]
    fn a_new_folder_with_a_log_folder_still_receives_the_profile() {
        // On Linux the log plugin creates `<data dir>/logs/` before `setup` runs.
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        fs::create_dir_all(legacy.join("logs")).unwrap();
        fs::write(legacy.join("logs/old.log"), b"old").unwrap();
        fs::create_dir_all(new.join("logs")).unwrap();
        fs::write(new.join("logs/moony.log"), b"current").unwrap();

        let result = migrate_legacy_data_dir(&new).unwrap();

        assert!(matches!(result, Moved::Moved(_)));
        assert!(new.join("moony.db").is_file());
        assert!(new.join("salt").is_file());
        assert!(new.join("insurance_documents/a.pdf").is_file());
        // The new log folder wins; the old logs are not merged and stay where they were.
        assert_eq!(fs::read(new.join("logs/moony.log")).unwrap(), b"current");
        assert!(!new.join("logs/old.log").exists());
        assert!(legacy.join("logs/old.log").is_file());
        assert!(!legacy.join("moony.db").exists());
    }

    #[test]
    fn nothing_to_do_without_a_legacy_folder() {
        let root = tempfile::tempdir().unwrap();
        let new = root.path().join(NEW_NAME);
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Nothing
        ));
        assert!(!new.exists());
    }

    #[test]
    fn a_legacy_folder_without_a_database_is_not_a_profile() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        fs::create_dir_all(legacy.join("logs")).unwrap();
        fs::write(legacy.join("salt"), b"s").unwrap();
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Nothing
        ));
        assert!(legacy.join("salt").is_file());
        assert!(!new.exists());
    }

    #[test]
    fn a_second_start_does_nothing() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Moved(_)
        ));
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Nothing
        ));
        assert_eq!(fs::read(new.join("moony.db")).unwrap(), b"db");
        assert_eq!(fs::read(new.join("salt")).unwrap(), b"s");
        assert_eq!(
            fs::read(new.join("insurance_documents/a.pdf")).unwrap(),
            b"pdf"
        );
    }

    #[test]
    fn a_half_finished_move_is_resumed() {
        // An earlier, interrupted start moved `salt` and `insurance_documents/` and then died
        // before it reached `moony.db`, so the legacy folder still holds the database.
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("moony.db"), b"db").unwrap();
        fs::write(legacy.join("key.enc"), b"k").unwrap();
        fs::create_dir_all(new.join("insurance_documents")).unwrap();
        fs::write(new.join("insurance_documents/a.pdf"), b"pdf").unwrap();
        fs::write(new.join("salt"), b"s").unwrap();

        let result = migrate_legacy_data_dir(&new).unwrap();

        assert!(matches!(result, Moved::Moved(_)));
        assert_eq!(
            names(&new),
            [MARKER, "insurance_documents", "key.enc", "moony.db", "salt"]
        );
        assert_eq!(fs::read(new.join("moony.db")).unwrap(), b"db");
        assert_eq!(fs::read(new.join("key.enc")).unwrap(), b"k");
        assert_eq!(fs::read(new.join("salt")).unwrap(), b"s");
        assert_eq!(
            fs::read(new.join("insurance_documents/a.pdf")).unwrap(),
            b"pdf"
        );
        assert!(!legacy.exists());
    }

    #[test]
    fn the_database_is_moved_last_and_its_side_files_just_before_it() {
        let order = in_move_order(
            [
                "moony.db",
                "moony.db-wal",
                "salt",
                "moony.db-shm",
                "insurance_documents",
                "key.enc",
            ]
            .map(OsString::from)
            .to_vec(),
        );
        let order: Vec<&str> = order.iter().map(|n| n.to_str().unwrap()).collect();
        assert_eq!(order.last(), Some(&"moony.db"));
        let first_side = order
            .iter()
            .position(|n| n.starts_with("moony.db-"))
            .unwrap();
        assert!(order[..first_side].contains(&"salt"));
        assert!(order[..first_side].contains(&"key.enc"));
        assert!(order[..first_side].contains(&"insurance_documents"));
        assert_eq!(order.len(), 6);
    }

    #[test]
    fn a_failed_rename_copies_and_leaves_the_originals() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);

        let result =
            merge_legacy_into(&legacy, &new, &rename_never_works, &copy_entry_into_place).unwrap();

        assert!(matches!(result, Moved::Copied(ref from) if *from == legacy));
        assert_eq!(fs::read(new.join("moony.db")).unwrap(), b"db");
        assert_eq!(
            fs::read(new.join("insurance_documents/a.pdf")).unwrap(),
            b"pdf"
        );
        assert!(new.join(MARKER).is_file());
        // Nothing is deleted after a copy, and no temporary names are left in the new folder.
        assert!(legacy.join("moony.db").is_file());
        assert!(legacy.join("insurance_documents/a.pdf").is_file());
        assert_eq!(
            names(&new),
            [MARKER, "insurance_documents", "key.enc", "moony.db", "salt"]
        );
        // The next start sees a finished profile and leaves both folders alone.
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Nothing
        ));
    }

    #[test]
    fn one_failed_rename_among_successful_ones_reports_a_copy() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        let fail_for_documents = |from: &Path, to: &Path| {
            if from.file_name().is_some_and(|n| n == "insurance_documents") {
                Err(io::Error::other("busy"))
            } else {
                fs::rename(from, to)
            }
        };

        let result =
            merge_legacy_into(&legacy, &new, &fail_for_documents, &copy_entry_into_place).unwrap();

        assert!(matches!(result, Moved::Copied(_)));
        assert!(new.join("insurance_documents/a.pdf").is_file());
        assert!(new.join("moony.db").is_file());
        // The renamed entries left the legacy folder; the copied one stayed.
        assert!(!legacy.join("moony.db").exists());
        assert!(!legacy.join("salt").exists());
        assert!(legacy.join("insurance_documents/a.pdf").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_key_file_that_cannot_be_moved_keeps_the_database_in_the_legacy_folder() {
        // `moony.db` must not reach the new folder before everything it needs does, or the
        // next start would take a half-filled folder for a finished profile. Make `key.enc`
        // impossible to copy (a folder holding a dangling link) and the rename fail as well.
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        fs::remove_file(legacy.join("key.enc")).unwrap();
        fs::create_dir(legacy.join("key.enc")).unwrap();
        std::os::unix::fs::symlink(root.path().join("nowhere"), legacy.join("key.enc/link"))
            .unwrap();

        let result = merge_legacy_into(&legacy, &new, &rename_never_works, &copy_entry_into_place);

        assert!(result.is_err());
        assert!(legacy.join("moony.db").is_file());
        assert!(!new.join("moony.db").exists());
        assert!(!new.join("key.enc").exists());
        assert!(!new.join(MARKER).exists());
        // The next start (with the same obstacle) repeats the attempt instead of declaring the
        // profile finished ...
        assert!(
            merge_legacy_into(&legacy, &new, &rename_never_works, &copy_entry_into_place).is_err()
        );
        assert!(!new.join("moony.db").exists());
        // ... and once the obstacle is gone it completes the move.
        fs::remove_dir_all(legacy.join("key.enc")).unwrap();
        fs::write(legacy.join("key.enc"), b"k").unwrap();
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Moved(_) | Moved::Copied(_)
        ));
        assert_eq!(fs::read(new.join("moony.db")).unwrap(), b"db");
        assert_eq!(fs::read(new.join("key.enc")).unwrap(), b"k");
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_copy_leaves_no_partial_entry_behind() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("entry");
        let new = root.path().join(NEW_NAME);
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("ok.txt"), b"ok").unwrap();
        std::os::unix::fs::symlink(root.path().join("nowhere"), source.join("bad")).unwrap();
        fs::create_dir_all(&new).unwrap();

        assert!(copy_entry_into_place(&source, &new.join("entry")).is_err());

        assert!(!new.join("entry").exists());
        assert_eq!(names(&new), Vec::<String>::new());
    }

    #[test]
    fn a_copy_replaces_a_temporary_left_by_an_earlier_crash() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("salt");
        let new = root.path().join(NEW_NAME);
        fs::write(&source, b"fresh").unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join(copy_temp_name(OsStr::new("salt"))), b"half").unwrap();

        copy_entry_into_place(&source, &new.join("salt")).unwrap();

        assert_eq!(fs::read(new.join("salt")).unwrap(), b"fresh");
        assert_eq!(names(&new), ["salt"]);
    }

    #[test]
    fn a_non_essential_entry_that_cannot_be_moved_stays_behind_and_the_profile_moves() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let new = root.path().join(NEW_NAME);
        profile(&legacy);
        fs::write(legacy.join("moony.db.bak-1"), b"backup").unwrap();
        // `insurance_documents` can neither be renamed nor copied; the backup file moves fine.
        let fail_for_documents = |from: &Path, to: &Path| {
            if from.file_name().is_some_and(|n| n == "insurance_documents") {
                Err(io::Error::other("busy"))
            } else {
                fs::rename(from, to)
            }
        };
        let copy_fails_for_documents = |from: &Path, to: &Path| {
            if from.file_name().is_some_and(|n| n == "insurance_documents") {
                Err(io::Error::other("busy"))
            } else {
                copy_entry_into_place(from, to)
            }
        };

        let result = merge_legacy_into(
            &legacy,
            &new,
            &fail_for_documents,
            &copy_fails_for_documents,
        )
        .unwrap();

        assert!(
            matches!(&result, Moved::Partial(from, left) if *from == legacy
                && left.as_slice() == ["insurance_documents"]),
            "{result:?}"
        );
        // The profile is complete in the new folder, including the database ...
        assert_eq!(fs::read(new.join("moony.db")).unwrap(), b"db");
        assert_eq!(fs::read(new.join("salt")).unwrap(), b"s");
        assert_eq!(fs::read(new.join("key.enc")).unwrap(), b"k");
        assert_eq!(fs::read(new.join("moony.db.bak-1")).unwrap(), b"backup");
        assert!(new.join(MARKER).is_file());
        // ... and the entry that failed is untouched in the legacy folder.
        assert!(!new.join("insurance_documents").exists());
        assert_eq!(
            fs::read(legacy.join("insurance_documents/a.pdf")).unwrap(),
            b"pdf"
        );
        assert!(!legacy.join("moony.db").exists());
        // The next start sees a finished profile and leaves both folders alone.
        assert!(matches!(
            migrate_legacy_data_dir(&new).unwrap(),
            Moved::Nothing
        ));
        assert!(legacy.join("insurance_documents/a.pdf").is_file());
    }

    #[test]
    fn every_essential_entry_that_cannot_be_moved_aborts_before_the_database() {
        for essential in [
            "salt",
            "key.enc",
            "recovery.enc",
            "moony.db-wal",
            "moony.db-shm",
            "moony.db-journal",
        ] {
            let root = tempfile::tempdir().unwrap();
            let legacy = root.path().join(LEGACY_IDENTIFIER);
            let new = root.path().join(NEW_NAME);
            profile(&legacy);
            fs::write(legacy.join(essential), b"x").unwrap();
            let fail_for_it = move |from: &Path, to: &Path| {
                if from.file_name().is_some_and(|n| n == essential) {
                    Err(io::Error::other("busy"))
                } else {
                    fs::rename(from, to)
                }
            };

            let result = merge_legacy_into(&legacy, &new, &fail_for_it, &copy_never_works);

            assert!(result.is_err(), "{essential} must abort the move");
            assert!(legacy.join("moony.db").is_file(), "{essential}");
            assert!(!new.join("moony.db").exists(), "{essential}");
            assert!(!new.join(MARKER).exists(), "{essential}");
        }
    }

    #[test]
    fn only_the_database_its_side_files_and_the_key_files_are_essential() {
        for name in [
            "moony.db",
            "moony.db-wal",
            "moony.db-shm",
            "moony.db-journal",
            "salt",
            "key.enc",
            "recovery.enc",
        ] {
            assert!(is_essential(OsStr::new(name)), "{name}");
        }
        for name in [
            "insurance_documents",
            "real_estate_photos",
            "real_estate_documents",
            "moony.db.bak-1700000000",
            "pre-restore-1700000000",
            "logs",
            ".seed_demo",
        ] {
            assert!(!is_essential(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn copy_dir_copies_nested_files() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        let to = root.path().join("to");
        profile(&from);
        copy_dir(&from, &to).unwrap();
        assert_eq!(
            fs::read(to.join("insurance_documents/a.pdf")).unwrap(),
            b"pdf"
        );
        assert_eq!(fs::read(to.join("salt")).unwrap(), b"s");
    }
}
