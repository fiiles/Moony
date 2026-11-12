//! The backups a join, repair or re-sync makes before it replaces the profile (audit M9, L11).
//!
//! A backup is written to `sync-repair-<unix>[-<n>].zip.partial`, flushed through a write handle,
//! read back and only then renamed to its plain `.zip` name, so a file under that name is always
//! complete. The mark lives in the name (`-kept`, `-unused`; no sidecar files), and pruning never
//! removes the only copy of something: see [`prune`].

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::services::{backup, crypto};

pub const PREFIX: &str = "sync-repair-";
pub const SUFFIX: &str = ".zip";
pub const PARTIAL_SUFFIX: &str = ".zip.partial";
pub const KEEP_NEWEST: usize = 3;
pub const KEEP_DAYS: i64 = 30;

const KEPT_TAG: &str = "-kept";
const UNUSED_TAG: &str = "-unused";
const DAY: i64 = 86_400;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// In the file name: `sync-repair-<unix>[-<n>][-kept|-unused].zip`. No sidecar files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupMark {
    Plain,
    /// It holds something found nowhere else (changes the replace could not send, documents or
    /// photos it could not re-attach). Never pruned; only the person deletes it.
    Kept,
    /// Its replace never happened (a refused or failed join, repair or re-sync). Final.
    Unused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncBackup {
    pub path: PathBuf,
    pub created_at: i64,
    pub bytes: u64,
    pub mark: BackupMark,
}

/// `(created_at, n, mark)` of a finished backup name, `None` for any other name.
fn parse_finished(name: &str) -> Option<(i64, u32, BackupMark)> {
    let stem = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let (stem, mark) = if let Some(s) = stem.strip_suffix(KEPT_TAG) {
        (s, BackupMark::Kept)
    } else if let Some(s) = stem.strip_suffix(UNUSED_TAG) {
        (s, BackupMark::Unused)
    } else {
        (stem, BackupMark::Plain)
    };
    let (unix, n) = parse_stamp(stem)?;
    Some((unix, n, mark))
}

/// `<unix>` or `<unix>-<n>` with `n >= 2`, both as `free_stem` writes them (digits, no leading
/// zero), so one backup has one name and `mark` can never rename onto another backup.
fn parse_stamp(stem: &str) -> Option<(i64, u32)> {
    fn canonical<T: std::str::FromStr + ToString>(s: &str) -> Option<T> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let value: T = s.parse().ok()?;
        (value.to_string() == s).then_some(value)
    }
    match stem.split_once('-') {
        None => Some((canonical(stem)?, 1)),
        Some((unix, n)) => {
            let n: u32 = canonical(n)?;
            (n >= 2).then_some(())?;
            Some((canonical(unix)?, n))
        }
    }
}

/// The unix second of a `.zip.partial` name.
fn parse_partial(name: &str) -> Option<i64> {
    let stem = name.strip_prefix(PREFIX)?.strip_suffix(PARTIAL_SUFFIX)?;
    parse_stamp(stem).map(|(unix, _)| unix)
}

fn name_of(path: &Path) -> Option<&str> {
    path.file_name().and_then(|n| n.to_str())
}

/// A stem (`sync-repair-<now>[-<n>]`) no file in any state uses.
fn free_stem(data_dir: &Path, now: i64) -> Result<String> {
    let taken = |stem: &str| -> Result<bool> {
        for tail in [
            SUFFIX,
            PARTIAL_SUFFIX,
            &format!("{KEPT_TAG}{SUFFIX}"),
            &format!("{UNUSED_TAG}{SUFFIX}"),
        ] {
            if data_dir.join(format!("{stem}{tail}")).try_exists()? {
                return Ok(true);
            }
        }
        Ok(false)
    };
    let first = format!("{PREFIX}{now}");
    if !taken(&first)? {
        return Ok(first);
    }
    for n in 2u32.. {
        let next = format!("{PREFIX}{now}-{n}");
        if !taken(&next)? {
            return Ok(next);
        }
    }
    Err(AppError::Internal("sync backup: no free name".into()))
}

/// The archive is one the app restores (`backup::inspect_backup`: its layout, its manifest, every
/// listed file present, not newer than this build), and every entry reads to its end, so the zip
/// checks each one's CRC.
fn read_back(path: &Path) -> Result<()> {
    let unreadable = || AppError::Internal("sync repair: the backup does not read back".into());
    let inspection = backup::inspect_backup(path, &crate::db::known_migration_names())?;
    if inspection.newer_than_app {
        return Err(unreadable());
    }
    let mut archive = zip::ZipArchive::new(File::open(path)?).map_err(|_| unreadable())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|_| unreadable())?;
        io::copy(&mut entry, &mut io::sink()).map_err(|_| unreadable())?;
    }
    Ok(())
}

/// `backup::create_backup` into `<PREFIX><now>[-n]<PARTIAL_SUFFIX>`, `flush_file`, the read-back,
/// rename to the plain `.zip` name, `crypto::sync_parent_dir`, then `prune`. A failure up to the
/// rename removes the partial file; after it the backup is complete and verified, so a directory
/// sync or a prune that fails is only logged.
pub fn write_verified(conn: &Connection, data_dir: &Path, now: i64) -> Result<PathBuf> {
    write_verified_with(conn, data_dir, now, read_back)
}

/// `write_verified` with the read-back as a seam.
pub fn write_verified_with(
    conn: &Connection,
    data_dir: &Path,
    now: i64,
    read_back: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    write_with(
        conn,
        data_dir,
        now,
        read_back,
        crypto::sync_parent_dir,
        prune,
    )
}

/// `write_verified_with` with the directory sync and the prune as seams too.
fn write_with(
    conn: &Connection,
    data_dir: &Path,
    now: i64,
    read_back: impl FnOnce(&Path) -> Result<()>,
    sync_dir: impl FnOnce(&Path) -> io::Result<()>,
    prune: impl FnOnce(&Path, i64) -> Result<usize>,
) -> Result<PathBuf> {
    let stem = free_stem(data_dir, now)?;
    let partial = data_dir.join(format!("{stem}{PARTIAL_SUFFIX}"));
    let finish = |partial: &Path| -> Result<PathBuf> {
        backup::create_backup(conn, data_dir, partial, APP_VERSION)?;
        flush_file(partial)?;
        read_back(partial)?;
        let done = data_dir.join(format!("{stem}{SUFFIX}"));
        fs::rename(partial, &done)?;
        Ok(done)
    };
    let done = match finish(&partial) {
        Ok(done) => done,
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
    };
    if let Err(error) = sync_dir(&done) {
        log::warn!("[SYNC] The data folder could not be synced after a sync backup: {error}");
    }
    if let Err(error) = prune(data_dir, now) {
        log::warn!("[SYNC] Pruning old sync backups failed: {error}");
    }
    Ok(done)
}

/// Opens the file for reading and writing and `sync_all`s it (Windows' FlushFileBuffers needs
/// write access).
pub fn flush_file(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()
}

/// Renames a finished backup to carry `mark`; returns the new path. A Plain backup takes any
/// mark. A Kept one may become Plain or Unused: a replace marks its backup Kept before it commits,
/// so a crash before the outcome is known never leaves it prunable, and settles it after. An
/// Unused one keeps its mark. Never renames onto an existing file.
pub fn mark(path: &Path, mark: BackupMark) -> Result<PathBuf> {
    let name = name_of(path).ok_or_else(|| AppError::Internal("sync backup: bad path".into()))?;
    let (unix, n, current) = parse_finished(name)
        .ok_or_else(|| AppError::Internal("sync backup: not a finished backup".into()))?;
    if current == mark {
        return Ok(path.to_path_buf());
    }
    if current == BackupMark::Unused {
        return Err(AppError::Internal(
            "sync backup: an unused backup keeps its mark".into(),
        ));
    }
    let tag = match mark {
        BackupMark::Plain => "",
        BackupMark::Kept => KEPT_TAG,
        BackupMark::Unused => UNUSED_TAG,
    };
    let stem = if n > 1 {
        format!("{PREFIX}{unix}-{n}")
    } else {
        format!("{PREFIX}{unix}")
    };
    let target = path.with_file_name(format!("{stem}{tag}{SUFFIX}"));
    if target.try_exists()? {
        return Err(AppError::Internal(
            "sync backup: the marked name is taken".into(),
        ));
    }
    fs::rename(path, &target)?;
    if let Err(error) = crypto::sync_parent_dir(&target) {
        log::warn!("[SYNC] The data folder could not be synced after marking a backup: {error}");
    }
    Ok(target)
}

/// The finished sync backups, newest first; partial and foreign names, symbolic links, folders
/// and entries that cannot be read are left out.
pub fn list(data_dir: &Path) -> Result<Vec<SyncBackup>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(data_dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((created_at, n, mark)) = parse_finished(&name) else {
            continue;
        };
        // `DirEntry::metadata` does not follow a symbolic link.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        found.push((
            (created_at, n),
            SyncBackup {
                path: entry.path(),
                created_at,
                bytes: meta.len(),
                mark,
            },
        ));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    Ok(found.into_iter().map(|(_, b)| b).collect())
}

/// Never removes a Kept backup. Unused ones go once older than `KEEP_DAYS` and never count toward
/// `KEEP_NEWEST`. Of the Plain ones it keeps the newest `KEEP_NEWEST` and every one younger than
/// `KEEP_DAYS`. Partial files older than a day go. Returns how many went.
pub fn prune(data_dir: &Path, now: i64) -> Result<usize> {
    let mut removed = 0;
    let mut plain_seen = 0;
    for backup in list(data_dir)? {
        let old = now - backup.created_at >= KEEP_DAYS * DAY;
        let drop = match backup.mark {
            BackupMark::Kept => false,
            BackupMark::Unused => old,
            BackupMark::Plain => {
                plain_seen += 1;
                plain_seen > KEEP_NEWEST && old
            }
        };
        if drop && fs::remove_file(&backup.path).is_ok() {
            removed += 1;
        }
    }
    for entry in fs::read_dir(data_dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if parse_partial(&name).is_some_and(|at| now - at > DAY)
            && entry.file_type().is_ok_and(|t| t.is_file())
            && fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

/// `path` is a finished sync backup (any mark) directly inside `data_dir`.
pub fn is_sync_backup_path(data_dir: &Path, path: &Path) -> bool {
    let Some(name) = name_of(path) else {
        return false;
    };
    path.parent() == Some(data_dir)
        && parse_finished(name).is_some()
        && fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

/// The person's own deletion from Restore (the UI confirms first); only `is_sync_backup_path`.
pub fn delete(data_dir: &Path, path: &Path) -> Result<()> {
    if !is_sync_backup_path(data_dir, path) {
        return Err(AppError::Internal(
            "sync backup: not a finished sync backup in the data folder".into(),
        ));
    }
    fs::remove_file(path)?;
    if let Err(error) = crypto::sync_parent_dir(path) {
        log::warn!("[SYNC] The data folder could not be synced after deleting a backup: {error}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const NOW: i64 = 1_800_000_000;

    fn account(dir: &Path) -> Connection {
        let conn = Connection::open(dir.join(backup::DB_FILE_NAME)).expect("db");
        conn.execute_batch(
            "CREATE TABLE _migrations (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, applied_at INTEGER NOT NULL DEFAULT (unixepoch()));
             CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .expect("schema");
        fs::write(dir.join("salt"), b"saltsaltsaltsalt").expect("salt");
        fs::write(dir.join("key.enc"), b"key").expect("key");
        conn
    }

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").expect("file");
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .expect("dir")
            .map(|e| e.expect("e").file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(PREFIX))
            .collect();
        v.sort();
        v
    }

    fn aged(days: i64) -> String {
        format!("{PREFIX}{}{SUFFIX}", NOW - days * DAY)
    }

    #[test]
    fn a_backup_appears_only_complete() {
        let dir = tempdir().expect("dir");
        let conn = account(dir.path());

        let first = write_verified_with(&conn, dir.path(), NOW, |_| Ok(())).expect("first");
        assert_eq!(first, dir.path().join(format!("sync-repair-{NOW}.zip")));
        assert_eq!(names(dir.path()), vec![format!("sync-repair-{NOW}.zip")]);

        let second = write_verified_with(&conn, dir.path(), NOW, |_| Ok(())).expect("second");
        assert_eq!(second, dir.path().join(format!("sync-repair-{NOW}-2.zip")));
        assert_eq!(names(dir.path()).len(), 2);
    }

    #[test]
    fn a_backup_that_does_not_read_back_leaves_nothing_under_the_final_name() {
        let dir = tempdir().expect("dir");
        let conn = account(dir.path());

        let result = write_verified_with(&conn, dir.path(), NOW, |_| {
            Err(AppError::Internal("does not read back".into()))
        });

        assert!(result.is_err());
        assert_eq!(names(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn the_real_read_back_accepts_a_fresh_backup_of_a_known_schema() {
        let dir = tempdir().expect("dir");
        let conn = account(dir.path());
        // The real read-back refuses migrations this build does not know.
        let known = crate::db::known_migration_names();
        for name in known {
            conn.execute("INSERT INTO _migrations (name) VALUES (?1)", [name])
                .expect("row");
        }

        let zip = write_verified(&conn, dir.path(), NOW).expect("verified");

        assert!(zip.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn flush_file_asks_for_write_access() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("dir");
        touch(dir.path(), "a");
        let path = dir.path().join("a");
        assert!(flush_file(&path).is_ok());

        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("chmod");
        if OpenOptions::new().write(true).open(&path).is_ok() {
            // Running as root: permissions do not apply, so there is nothing to see.
            return;
        }
        let error = flush_file(&path).expect_err("read-only");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn prune_keeps_the_newest_three_and_anything_recent() {
        let dir = tempdir().expect("dir");
        for days in [1, 10, 40, 50, 60, 70] {
            touch(dir.path(), &aged(days));
        }
        assert_eq!(prune(dir.path(), NOW).expect("prune"), 3);
        let mut want: Vec<String> = [1, 10, 40].map(aged).to_vec();
        want.sort();
        assert_eq!(names(dir.path()), want);

        let dir = tempdir().expect("dir");
        for days in [1, 5, 10, 20, 29] {
            touch(dir.path(), &aged(days));
        }
        touch(
            dir.path(),
            &format!("{PREFIX}{}{PARTIAL_SUFFIX}", NOW - 2 * DAY),
        );
        touch(
            dir.path(),
            &format!("{PREFIX}{}-2{PARTIAL_SUFFIX}", NOW - 3600),
        );
        assert_eq!(prune(dir.path(), NOW).expect("prune"), 1);
        assert_eq!(names(dir.path()).len(), 6);
        assert!(!names(dir.path())
            .iter()
            .any(|n| n.contains(&format!("{}", NOW - 2 * DAY))));
    }

    #[test]
    fn prune_keeps_a_backup_that_holds_unsent_changes() {
        let dir = tempdir().expect("dir");
        let kept = format!("{PREFIX}{}{KEPT_TAG}{SUFFIX}", NOW - 400 * DAY);
        touch(dir.path(), &kept);
        for days in [1, 2, 3, 4] {
            touch(dir.path(), &aged(days));
        }

        prune(dir.path(), NOW).expect("prune");

        assert!(names(dir.path()).contains(&kept));
    }

    #[test]
    fn unused_backups_do_not_push_out_older_ones() {
        let dir = tempdir().expect("dir");
        touch(dir.path(), &aged(40));
        for n in ["", "-2", "-3"] {
            touch(dir.path(), &format!("{PREFIX}{NOW}{n}{UNUSED_TAG}{SUFFIX}"));
        }
        let old_unused = format!("{PREFIX}{}{UNUSED_TAG}{SUFFIX}", NOW - 31 * DAY);
        touch(dir.path(), &old_unused);

        assert_eq!(prune(dir.path(), NOW).expect("prune"), 1);

        let left = names(dir.path());
        assert!(left.contains(&aged(40)));
        assert!(!left.contains(&old_unused));
        assert_eq!(left.len(), 4);
    }

    #[test]
    fn mark_renames_and_list_reads_the_mark() {
        let dir = tempdir().expect("dir");
        let a = dir.path().join(format!("{PREFIX}{NOW}{SUFFIX}"));
        let b = dir.path().join(format!("{PREFIX}{NOW}-2{SUFFIX}"));
        let c = dir.path().join(format!("{PREFIX}{}{SUFFIX}", NOW - 5));
        for p in [&a, &b, &c] {
            fs::write(p, b"xyz").expect("file");
        }

        let a = mark(&a, BackupMark::Kept).expect("kept");
        let b = mark(&b, BackupMark::Kept).expect("kept 2");
        let c = mark(&c, BackupMark::Unused).expect("unused");
        assert_eq!(
            name_of(&a),
            Some(format!("sync-repair-{NOW}-kept.zip").as_str())
        );
        assert_eq!(
            name_of(&b),
            Some(format!("sync-repair-{NOW}-2-kept.zip").as_str())
        );
        assert!(
            mark(&c, BackupMark::Kept).is_err(),
            "an unused backup stays unused"
        );

        let listed = list(dir.path()).expect("list");
        let got: Vec<(PathBuf, i64, BackupMark)> = listed
            .iter()
            .map(|s| (s.path.clone(), s.created_at, s.mark))
            .collect();
        assert_eq!(
            got,
            vec![
                (b, NOW, BackupMark::Kept),
                (a, NOW, BackupMark::Kept),
                (c, NOW - 5, BackupMark::Unused),
            ]
        );
        assert_eq!(listed[0].bytes, 3);
    }

    #[test]
    fn list_reads_names_newest_first_and_ignores_others() {
        let dir = tempdir().expect("dir");
        touch(dir.path(), &aged(3));
        touch(dir.path(), &aged(1));
        touch(dir.path(), &format!("{PREFIX}{NOW}{PARTIAL_SUFFIX}"));
        touch(dir.path(), "pre-restore-1790000000.zip");
        touch(dir.path(), "sync-repair-x.zip");
        touch(dir.path(), "sync-repair-.zip");

        let listed = list(dir.path()).expect("list");

        let at: Vec<i64> = listed.iter().map(|s| s.created_at).collect();
        assert_eq!(at, vec![NOW - DAY, NOW - 3 * DAY]);
    }

    #[test]
    fn is_sync_backup_path_wants_a_finished_backup_in_the_folder() {
        let dir = tempdir().expect("dir");
        let other = tempdir().expect("other");
        let name = aged(1);
        touch(dir.path(), &name);
        touch(other.path(), &name);
        fs::create_dir(dir.path().join("sub")).expect("sub");
        touch(&dir.path().join("sub"), &name);
        let partial = dir.path().join(format!("{PREFIX}{NOW}{PARTIAL_SUFFIX}"));
        fs::write(&partial, b"x").expect("partial");
        fs::create_dir(dir.path().join(aged(2))).expect("dir named like a backup");

        let good = dir.path().join(&name);
        assert!(is_sync_backup_path(dir.path(), &good));
        for bad in [
            dir.path().join("sub").join(&name),
            dir.path().join("sub").join("..").join(&name),
            other.path().join(&name),
            partial.clone(),
            dir.path().join(aged(2)),
            dir.path().join(aged(9)),
        ] {
            assert!(!is_sync_backup_path(dir.path(), &bad), "{bad:?}");
            assert!(delete(dir.path(), &bad).is_err(), "{bad:?}");
        }
        assert!(partial.exists() && other.path().join(&name).exists());

        delete(dir.path(), &good).expect("delete");
        assert!(!good.exists());
    }

    #[test]
    fn a_kept_backup_settles_to_plain_or_unused() {
        let dir = tempdir().expect("dir");
        let a = dir.path().join(format!("{PREFIX}{NOW}{SUFFIX}"));
        let b = dir.path().join(format!("{PREFIX}{NOW}-2{SUFFIX}"));
        fs::write(&a, b"a").expect("a");
        fs::write(&b, b"b").expect("b");

        let a = mark(&a, BackupMark::Kept).expect("kept");
        let a = mark(&a, BackupMark::Plain).expect("settled plain");
        let b = mark(&b, BackupMark::Kept).expect("kept");
        let b = mark(&b, BackupMark::Unused).expect("settled unused");

        assert_eq!(name_of(&a), Some(format!("sync-repair-{NOW}.zip").as_str()));
        assert_eq!(
            name_of(&b),
            Some(format!("sync-repair-{NOW}-2-unused.zip").as_str())
        );
        assert_eq!(fs::read(&a).expect("a"), b"a");
    }

    #[test]
    fn mark_never_renames_onto_another_backup() {
        let dir = tempdir().expect("dir");
        // Not names `free_stem` writes: `-1`, a leading zero.
        for odd in [
            format!("{PREFIX}{NOW}-1{SUFFIX}"),
            format!("{PREFIX}{NOW}-02{SUFFIX}"),
            format!("{PREFIX}0{NOW}{SUFFIX}"),
        ] {
            touch(dir.path(), &odd);
            assert!(
                mark(&dir.path().join(&odd), BackupMark::Kept).is_err(),
                "{odd}"
            );
        }
        assert!(list(dir.path()).expect("list").is_empty());

        // A taken target (never written by this module, but possible by hand).
        let plain = dir.path().join(format!("{PREFIX}{NOW}{SUFFIX}"));
        let kept = dir.path().join(format!("{PREFIX}{NOW}{KEPT_TAG}{SUFFIX}"));
        fs::write(&plain, b"plain").expect("plain");
        fs::write(&kept, b"kept").expect("kept");

        assert!(mark(&kept, BackupMark::Plain).is_err());
        assert!(mark(&plain, BackupMark::Kept).is_err());
        assert_eq!(fs::read(&plain).expect("plain"), b"plain");
        assert_eq!(fs::read(&kept).expect("kept"), b"kept");
    }

    #[test]
    fn a_failing_directory_sync_after_the_rename_does_not_fail_the_backup() {
        let dir = tempdir().expect("dir");
        let conn = account(dir.path());

        let zip = write_with(
            &conn,
            dir.path(),
            NOW,
            |_| Ok(()),
            |_| Err(io::Error::other("fsync failed")),
            prune,
        )
        .expect("the backup is complete and verified");

        assert_eq!(zip, dir.path().join(format!("sync-repair-{NOW}.zip")));
        assert!(zip.is_file());
    }

    #[test]
    fn write_verified_prunes_and_a_failing_prune_does_not_fail_the_backup() {
        let dir = tempdir().expect("dir");
        let conn = account(dir.path());
        for days in [40, 50, 60, 70] {
            touch(dir.path(), &aged(days));
        }

        write_verified_with(&conn, dir.path(), NOW, |_| Ok(())).expect("backup");

        let left = names(dir.path());
        assert_eq!(
            left.len(),
            3,
            "the new one and the two newest before it: {left:?}"
        );
        assert!(left.contains(&aged(40)) && left.contains(&aged(50)));

        let zip = write_with(
            &conn,
            dir.path(),
            NOW,
            |_| Ok(()),
            crypto::sync_parent_dir,
            |_, _| Err(AppError::Internal("prune failed".into())),
        )
        .expect("a failing prune is only logged");
        assert!(zip.is_file());
    }

    #[test]
    fn the_real_read_back_refuses_a_corrupt_zip() {
        let dir = tempdir().expect("dir");
        let path = dir.path().join("corrupt.zip");
        fs::write(&path, b"PK\x03\x04 not really a zip").expect("file");

        assert!(read_back(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_and_folders_named_like_backups_are_ignored() {
        let dir = tempdir().expect("dir");
        let elsewhere = tempdir().expect("elsewhere");
        let target = elsewhere.path().join("precious.zip");
        fs::write(&target, b"precious").expect("target");
        let link = dir.path().join(aged(400));
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let folder = dir.path().join(aged(500));
        fs::create_dir(&folder).expect("folder");
        let partial_link = dir
            .path()
            .join(format!("{PREFIX}{}{PARTIAL_SUFFIX}", NOW - 9 * DAY));
        std::os::unix::fs::symlink(&target, &partial_link).expect("partial symlink");
        for days in [1, 2, 3] {
            touch(dir.path(), &aged(days));
        }

        assert_eq!(list(dir.path()).expect("list").len(), 3);
        assert_eq!(prune(dir.path(), NOW).expect("prune"), 0);
        assert!(delete(dir.path(), &link).is_err());
        assert!(delete(dir.path(), &folder).is_err());
        assert!(fs::symlink_metadata(&link).is_ok() && folder.is_dir());
        assert!(fs::symlink_metadata(&partial_link).is_ok());
        assert_eq!(fs::read(&target).expect("target"), b"precious");
    }
}
