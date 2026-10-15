//! One-time removal of plaintext photo copies that Moony up to 0.12.2 left in the OS temp dir.
//!
//! Until the raw-IPC photo upload, the gallery wrote every uploaded photo (HEIC converted to JPEG
//! included) to `$TMPDIR/photo_<Date.now()>_<base36>.<ext>` and never deleted the copy. macOS
//! purges its temp dir after a few days, Windows `%TEMP%` never does, so the photos stayed on disk
//! next to an encrypted database.
//!
//! The temp dir is shared with other programs, so this is deliberately narrow: only regular files
//! directly inside the dir whose name matches that exact pattern. It never recurses, never follows
//! a symlink and never touches anything else.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::error::{AppError, Result};

/// `photo_` + `Date.now()` (13 digits) + `_` + `Math.random().toString(36).slice(2)` (base36, at
/// most about 11 characters; the cap leaves room) + the lowercased extension the gallery accepted.
const LEGACY_PHOTO_COPY_PATTERN: &str =
    r"^photo_[0-9]{13}_[0-9a-z]{1,12}\.(?:jpg|jpeg|png|gif|webp|bmp|heic|heif)$";

/// What one sweep did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CleanupReport {
    /// Matching files deleted.
    pub removed: usize,
    /// Matching files that could not be deleted (in use, no permission).
    pub failed: usize,
}

fn legacy_photo_copy_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(LEGACY_PHOTO_COPY_PATTERN).expect("static pattern is valid"))
}

/// Whether `name` is exactly what the old gallery code wrote.
pub fn is_legacy_photo_copy(name: &str) -> bool {
    legacy_photo_copy_regex().is_match(name)
}

/// Delete the legacy photo copies directly inside `temp_dir`. A missing dir is a clean sweep.
///
/// `DirEntry::file_type` does not follow symlinks, so a link named like a photo copy is skipped
/// instead of being resolved, and `remove_file` on a path swapped for a link in the meantime
/// removes the link, never its target.
pub fn remove_legacy_photo_copies(temp_dir: &Path) -> Result<CleanupReport> {
    let entries = match fs::read_dir(temp_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(CleanupReport::default()),
        Err(e) => {
            return Err(AppError::Internal(format!(
                "Failed to list the temp folder: {e}"
            )))
        }
    };

    let mut report = CleanupReport::default();
    for entry in entries.flatten() {
        let is_regular_file = entry.file_type().is_ok_and(|t| t.is_file());
        let matches = entry.file_name().to_str().is_some_and(is_legacy_photo_copy);
        if !is_regular_file || !matches {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => report.removed += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                log::warn!("[TEMP] Could not remove a leftover photo copy: {e}");
                report.failed += 1;
            }
        }
    }
    Ok(report)
}

/// Sweep `temp_dir` once: `None` when `marker` says it already ran, otherwise the report.
///
/// The marker is written only after a sweep that left nothing behind, so a file that could not be
/// removed (still open on Windows, say) is retried on the next start. The marker lives in the app
/// data dir because the encrypted database is still locked when the app starts.
pub fn run_once(temp_dir: &Path, marker: &Path) -> Result<Option<CleanupReport>> {
    if marker.exists() {
        return Ok(None);
    }
    let report = remove_legacy_photo_copies(temp_dir)?;
    if report.failed == 0 {
        if let Some(parent) = marker.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError::Internal(format!("Failed to create the app data folder: {e}"))
            })?;
        }
        fs::write(marker, "1").map_err(|e| {
            AppError::Internal(format!("Failed to write the temp cleanup marker: {e}"))
        })?;
    }
    Ok(Some(report))
}

/// Startup entry point: best effort, logs the outcome, never returns an error.
pub fn run_once_and_log(temp_dir: &Path, marker: &Path) {
    match run_once(temp_dir, marker) {
        Ok(None) => log::debug!("[TEMP] Leftover photo copies were already cleaned up"),
        Ok(Some(report)) if report.removed == 0 && report.failed == 0 => {
            log::debug!("[TEMP] No leftover photo copies in the temp folder")
        }
        Ok(Some(report)) => log::info!(
            "[TEMP] Removed {} leftover photo copies from the temp folder ({} could not be removed)",
            report.removed,
            report.failed
        ),
        Err(e) => log::warn!("[TEMP] Leftover photo cleanup skipped: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "photo_1760000000000_abc123def.jpg";

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").expect("write file");
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn matches_the_names_the_old_gallery_wrote() {
        for name in [
            GOOD,
            "photo_1760000000000_0.png",
            "photo_1760000000000_k3j9x0q2m1z.jpeg",
            "photo_1700000000123_zzzzzzzzzzzz.gif",
            "photo_1760000000000_abc.webp",
            "photo_1760000000000_abc.bmp",
            "photo_1760000000000_abc.heic",
            "photo_1760000000000_abc.heif",
        ] {
            assert!(is_legacy_photo_copy(name), "{name} should match");
        }
    }

    #[test]
    fn rejects_anything_that_is_not_exactly_that_shape() {
        for name in [
            "photo_notours.jpg",
            "photo_1760000000000_abc.txt",
            "photo_1760000000000_abc.svg",
            "photo_1760000000000_abc.jpg.exe",
            "photo_1760000000000_abc",
            "photo_1760000000000_abc.",
            "photo_1760000000000_.jpg",
            "photo_176000000000_abc.jpg",   // 12 digits
            "photo_17600000000000_abc.jpg", // 14 digits
            "photo_1760000000000_ABC.jpg",  // base36 is lowercase
            "photo_1760000000000_abc.JPG",  // the extension was lowercased
            "photo_1760000000000_ab-c.jpg",
            "photo_1760000000000_ab c.jpg",
            "photo_1760000000000_abcdefghijklm.jpg", // 13 random characters
            "Photo_1760000000000_abc.jpg",
            "my_photo_1760000000000_abc.jpg",
            "photo_1760000000000_abc.jpg ",
            "photo_1760000000000_abc.jpg\n",
            "photo_1760000000000_abc.jpg.part",
            "photo__1760000000000_abc.jpg",
            "",
        ] {
            assert!(!is_legacy_photo_copy(name), "{name:?} should not match");
        }
    }

    #[test]
    fn removes_only_matching_files_and_counts_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(dir.path(), GOOD);
        touch(dir.path(), "photo_1760000000001_xyz.heic");
        touch(dir.path(), "photo_notours.jpg");
        touch(dir.path(), "holiday.jpg");
        touch(dir.path(), "photo_1760000000000_abc.txt");

        let report = remove_legacy_photo_copies(dir.path()).expect("sweep");

        assert_eq!(
            report,
            CleanupReport {
                removed: 2,
                failed: 0
            }
        );
        assert_eq!(
            names(dir.path()),
            vec![
                "holiday.jpg",
                "photo_1760000000000_abc.txt",
                "photo_notours.jpg"
            ]
        );
    }

    #[test]
    fn leaves_a_directory_with_a_matching_name_and_its_contents_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lookalike = dir.path().join(GOOD);
        fs::create_dir(&lookalike).expect("mkdir");
        touch(&lookalike, GOOD);

        let report = remove_legacy_photo_copies(dir.path()).expect("sweep");

        assert_eq!(report, CleanupReport::default());
        assert!(lookalike.is_dir());
        assert!(lookalike.join(GOOD).is_file());
    }

    #[test]
    fn does_not_recurse_into_subdirectories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("sub");
        fs::create_dir(&nested).expect("mkdir");
        touch(&nested, GOOD);

        let report = remove_legacy_photo_copies(dir.path()).expect("sweep");

        assert_eq!(report, CleanupReport::default());
        assert!(nested.join(GOOD).is_file());
    }

    #[cfg(unix)]
    #[test]
    fn leaves_symlinks_and_their_targets_alone() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        // A link to a file outside the dir, named like a photo copy.
        let target_file = elsewhere.path().join("precious.jpg");
        fs::write(&target_file, b"keep").expect("write");
        symlink(&target_file, dir.path().join(GOOD)).expect("symlink to file");
        // A link to a directory holding a file with a matching name.
        touch(elsewhere.path(), "photo_1760000000002_dir.png");
        symlink(
            elsewhere.path(),
            dir.path().join("photo_1760000000001_lnk.jpg"),
        )
        .expect("symlink to dir");
        // A dangling link.
        symlink(
            elsewhere.path().join("gone"),
            dir.path().join("photo_1760000000003_gone.gif"),
        )
        .expect("dangling symlink");

        let report = remove_legacy_photo_copies(dir.path()).expect("sweep");

        assert_eq!(report, CleanupReport::default());
        assert_eq!(fs::read(&target_file).expect("target"), b"keep");
        assert!(elsewhere
            .path()
            .join("photo_1760000000002_dir.png")
            .is_file());
        assert_eq!(names(dir.path()).len(), 3, "the links themselves stay too");
    }

    #[test]
    fn an_empty_or_missing_dir_is_a_clean_sweep() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            remove_legacy_photo_copies(dir.path()).expect("empty"),
            CleanupReport::default()
        );
        assert_eq!(
            remove_legacy_photo_copies(&dir.path().join("does-not-exist")).expect("missing"),
            CleanupReport::default()
        );
    }

    #[test]
    fn a_temp_path_that_is_a_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(dir.path(), "not-a-dir");
        assert!(remove_legacy_photo_copies(&dir.path().join("not-a-dir")).is_err());
    }

    #[test]
    fn run_once_sweeps_then_writes_the_marker_and_skips_afterwards() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data = tempfile::tempdir().expect("tempdir");
        let marker = data.path().join("nested").join("legacy-temp-cleanup.done");
        touch(temp.path(), GOOD);

        let first = run_once(temp.path(), &marker).expect("first run");
        assert_eq!(
            first,
            Some(CleanupReport {
                removed: 1,
                failed: 0
            })
        );
        assert!(marker.is_file());
        assert!(names(temp.path()).is_empty());

        // A copy that shows up later (say, another install) is not touched: it ran once.
        touch(temp.path(), GOOD);
        assert_eq!(run_once(temp.path(), &marker).expect("second run"), None);
        assert_eq!(names(temp.path()), vec![GOOD]);
    }

    #[test]
    fn run_once_still_writes_the_marker_when_the_temp_dir_is_missing() {
        let data = tempfile::tempdir().expect("tempdir");
        let marker = data.path().join("legacy-temp-cleanup.done");

        let report = run_once(&data.path().join("no-temp"), &marker).expect("run");

        assert_eq!(report, Some(CleanupReport::default()));
        assert!(marker.is_file());
    }

    #[test]
    fn run_once_without_a_marker_does_not_mark_a_failed_listing_as_done() {
        let data = tempfile::tempdir().expect("tempdir");
        touch(data.path(), "not-a-dir");
        let marker = data.path().join("legacy-temp-cleanup.done");

        assert!(run_once(&data.path().join("not-a-dir"), &marker).is_err());
        assert!(
            !marker.exists(),
            "a failed sweep is retried on the next start"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_removed_is_counted_and_the_marker_waits() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let data = tempfile::tempdir().expect("tempdir");
        let marker = data.path().join("legacy-temp-cleanup.done");
        touch(temp.path(), GOOD);
        // Without write permission on the dir, unlinking fails.
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o500)).expect("chmod");
        let enforced = fs::write(temp.path().join("probe"), b"x").is_err();

        let report = run_once(temp.path(), &marker).expect("run");
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).expect("chmod back");

        // Root ignores the mode; there is nothing to assert about a denial then.
        if enforced {
            assert_eq!(
                report,
                Some(CleanupReport {
                    removed: 0,
                    failed: 1
                })
            );
            assert!(!marker.exists(), "retry on the next start");
            assert!(temp.path().join(GOOD).is_file());
        }
    }
}
