//! Keeps the phone's profile folder out of device backups (iCloud and the computer backup).
//!
//! The app data folder holds the whole encrypted profile: `moony.db`, `attachments.db`, the key
//! files. Moony's own backup (Settings, Backup and transfer; the first-run Backup step) is meant to
//! be the only copy that leaves the phone: the "data stays with you" promise, the multi-megabyte
//! databases re-uploaded to iCloud after every change, and a Keychain item (`ThisDeviceOnly`) that
//! would not travel with a device backup anyway are the reasons (owner's decision R25, 2026-10-18).
//!
//! The flag is set on the folder by the `moony-keychain` plugin (`exclude_from_backup`; iOS
//! `isExcludedFromBackup`, which covers everything below the folder). The app's adapter lives in
//! `commands::device_backup` (phones only); [`BackupExcluder`] keeps this module plain Rust, so the
//! desktop build compiles and tests it. The folder is excluded at every start and again after a
//! first-run setup and a restore, in case the folder was recreated.
//!
//! The data folder's path is never logged.

use std::fs;
use std::path::Path;

use crate::error::{AppError, Result};

/// Marks a folder as excluded from device backups and says whether the mark holds afterwards.
/// A call may block until the native side answers: run it off the main thread.
pub trait BackupExcluder {
    /// `Ok(true)` when `folder` is excluded after the call, `Ok(false)` when the platform could not
    /// (or does not yet) exclude it.
    fn exclude_from_backup(&self, folder: &Path) -> Result<bool>;
}

/// Makes sure `data_dir` exists (a first launch has no folder yet, and the flag needs one), then
/// excludes it. Returns whether it is excluded afterwards.
pub fn exclude_data_dir(excluder: &impl BackupExcluder, data_dir: &Path) -> Result<bool> {
    fs::create_dir_all(data_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create the data folder: {e}")))?;
    excluder.exclude_from_backup(data_dir)
}

/// The log level and the (path-free) line for what [`exclude_data_dir`] answered. Only a folder
/// that is excluded is `Info`: anything else means the profile may still reach a device backup.
fn outcome_line(outcome: &Result<bool>) -> (log::Level, String) {
    match outcome {
        Ok(excluded) => (
            if *excluded {
                log::Level::Info
            } else {
                log::Level::Warn
            },
            format!("[BACKUP] Profile folder excluded from device backups: {excluded}"),
        ),
        Err(e) => (
            log::Level::Warn,
            format!("[BACKUP] Could not exclude the profile folder from device backups: {e}"),
        ),
    }
}

/// [`exclude_data_dir`] and a log line with the outcome. Never fails: a phone whose folder cannot
/// be excluded still starts and works (it just keeps the folder in device backups).
pub fn exclude_data_dir_and_log(excluder: &impl BackupExcluder, data_dir: &Path) {
    let (level, line) = outcome_line(&exclude_data_dir(excluder, data_dir));
    log::log!(level, "{line}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    /// Records the folders it was asked about and answers what it was told to.
    struct StubExcluder {
        answer: std::result::Result<bool, String>,
        calls: RefCell<Vec<(PathBuf, bool)>>,
    }

    impl StubExcluder {
        fn answering(answer: std::result::Result<bool, String>) -> Self {
            Self {
                answer,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl BackupExcluder for StubExcluder {
        fn exclude_from_backup(&self, folder: &Path) -> Result<bool> {
            self.calls
                .borrow_mut()
                .push((folder.to_path_buf(), folder.is_dir()));
            self.answer.clone().map_err(AppError::Internal)
        }
    }

    #[test]
    fn creates_the_missing_folder_before_excluding_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data_dir = tmp.path().join("Library").join("com.moonyfinance.mobile");
        let excluder = StubExcluder::answering(Ok(true));

        let excluded = exclude_data_dir(&excluder, &data_dir).expect("excluded");

        assert!(excluded);
        assert!(data_dir.is_dir());
        // The folder existed when the platform was asked: the flag needs something to attach to.
        assert_eq!(*excluder.calls.borrow(), vec![(data_dir, true)]);
    }

    #[test]
    fn leaves_an_existing_folder_and_its_files_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("moony.db"), b"encrypted").expect("write");
        let excluder = StubExcluder::answering(Ok(true));

        assert!(exclude_data_dir(&excluder, tmp.path()).expect("excluded"));

        assert_eq!(
            fs::read(tmp.path().join("moony.db")).expect("read"),
            b"encrypted"
        );
        assert_eq!(excluder.calls.borrow().len(), 1);
    }

    #[test]
    fn reports_a_flag_that_did_not_take() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let excluder = StubExcluder::answering(Ok(false));

        assert!(!exclude_data_dir(&excluder, tmp.path()).expect("answer"));
    }

    #[test]
    fn passes_the_platform_error_on() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let excluder = StubExcluder::answering(Err("NSCocoaErrorDomain 513".into()));

        let error = exclude_data_dir(&excluder, tmp.path()).expect_err("error");

        assert!(error.to_string().contains("NSCocoaErrorDomain 513"));
    }

    #[test]
    fn a_folder_that_cannot_be_created_never_reaches_the_platform() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("a-file");
        fs::write(&file, b"x").expect("write");
        let excluder = StubExcluder::answering(Ok(true));

        let error = exclude_data_dir(&excluder, &file.join("data")).expect_err("error");

        assert!(matches!(error, AppError::Internal(_)));
        assert!(excluder.calls.borrow().is_empty());
    }

    #[test]
    fn only_an_excluded_folder_is_logged_as_info() {
        let (level, line) = outcome_line(&Ok(true));
        assert_eq!(level, log::Level::Info);
        assert_eq!(
            line,
            "[BACKUP] Profile folder excluded from device backups: true"
        );

        let (level, line) = outcome_line(&Ok(false));
        assert_eq!(level, log::Level::Warn);
        assert!(line.ends_with("device backups: false"));

        let (level, line) = outcome_line(&Err(AppError::Internal("boom".into())));
        assert_eq!(level, log::Level::Warn);
        assert!(line.contains("Could not exclude") && line.contains("boom"));
    }

    #[test]
    fn exclude_and_log_survives_every_outcome() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for answer in [Ok(true), Ok(false), Err("boom".to_string())] {
            exclude_data_dir_and_log(&StubExcluder::answering(answer), tmp.path());
        }
    }
}
