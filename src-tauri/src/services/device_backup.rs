//! Keeps what the phone app stores out of device backups (iCloud and the computer backup).
//!
//! The app data folder holds the whole encrypted profile: `moony.db`, `attachments.db`, the key
//! files. Moony's own backup (Settings, Backup and transfer; the first-run Backup step) is meant to
//! be the only copy that leaves the phone: the "data stays with you" promise, the multi-megabyte
//! databases re-uploaded to iCloud after every change, and a Keychain item (`ThisDeviceOnly`) that
//! would not travel with a device backup anyway are the reasons (owner's decision R25, 2026-10-18).
//!
//! What is flagged ([`Target`]), each by the `moony-keychain` plugin's `exclude_from_backup` (iOS
//! `isExcludedFromBackup`, which on a folder covers everything below it):
//!
//! - the app data folder (the profile), at every start and again after a first-run setup and a
//!   restore, in case the folder was recreated;
//! - the top `Library/WebKit` folder, at every start: the webview's localStorage keeps the greeting
//!   name and the device preferences there, in plain text (flagging the top folder covers both
//!   WebKit layouts);
//! - the files staged in `Documents/` for the Files export (the backup archive, the recovery key's
//!   text), right after each is written;
//! - `Documents/Inbox`, where iOS copies a statement opened in Moony, at every start and whenever a
//!   received file is taken. (The picker's copy lives in the tmp folder, which is never backed up.)
//!
//! The app's adapter lives in `commands::device_backup` (phones only); [`BackupExcluder`] keeps this
//! module plain Rust, so the desktop build compiles and tests it. No path is ever logged.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};
use crate::services::received_files;

/// Marks a file or folder as excluded from device backups and says whether the mark holds
/// afterwards. A call may block until the native side answers: run it off the main thread.
pub trait BackupExcluder {
    /// `Ok(true)` when `path` (an existing file or folder) is excluded after the call, `Ok(false)`
    /// when the platform could not (or does not yet) exclude it.
    fn exclude_from_backup(&self, path: &Path) -> Result<bool>;
}

/// What is being excluded, for the log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The app data folder: the encrypted profile.
    Profile,
    /// `Library/WebKit`: the webview's storage.
    WebKit,
    /// `Documents/Inbox`: statements opened in Moony.
    ReceivedFiles,
    /// A file staged in `Documents/` for the Files export.
    StagedExport,
}

impl Target {
    fn label(self) -> &'static str {
        match self {
            Target::Profile => "Profile folder",
            Target::WebKit => "WebKit storage folder",
            Target::ReceivedFiles => "Received files folder",
            Target::StagedExport => "Staged export file",
        }
    }
}

/// The folders to flag at every start. [`exclude_at_start`] creates the first two when they are
/// missing and skips the last one when it does not exist (iOS creates it when the first statement
/// is opened in Moony). A folder whose location could not be worked out is `None` and skipped, so
/// one failed lookup never keeps the profile from being flagged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartFolders {
    /// The app data folder.
    pub data_dir: PathBuf,
    /// `Library/WebKit` (iOS only: `None` where there is no such folder).
    pub webkit_dir: Option<PathBuf>,
    /// `Documents/Inbox`.
    pub received_files_dir: Option<PathBuf>,
}

/// The webview's storage folder: `<home>/Library/WebKit`, where `home` is the app's container (the
/// iOS home folder). Tauri has no path API for the `Library` folder itself.
pub fn webkit_dir(home_dir: &Path) -> PathBuf {
    home_dir.join("Library").join("WebKit")
}

/// The folder iOS copies a statement opened in Moony into: `<Documents>/Inbox`.
pub fn received_files_dir(document_dir: &Path) -> PathBuf {
    received_files::documents_inbox(document_dir)
}

/// Makes sure the folder `dir` exists (a first launch has no data folder yet, and the flag needs
/// something to attach to), then excludes it. Returns whether it is excluded afterwards.
pub fn exclude_dir(excluder: &impl BackupExcluder, dir: &Path) -> Result<bool> {
    fs::create_dir_all(dir)
        .map_err(|e| AppError::Internal(format!("Failed to create the folder: {e}")))?;
    excluder.exclude_from_backup(dir)
}

/// Excludes `path` (a file or a folder) when it exists; `Ok(None)` when there is nothing there, and
/// then the platform is not asked.
pub fn exclude_existing(excluder: &impl BackupExcluder, path: &Path) -> Result<Option<bool>> {
    match fs::symlink_metadata(path) {
        Ok(_) => excluder.exclude_from_backup(path).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::Internal(format!(
            "Failed to look at the path: {e}"
        ))),
    }
}

/// The log level and the (path-free) line for what an exclusion answered. Only something excluded
/// is `Info`: a `false` or an error means it may still reach a device backup.
fn outcome_line(target: Target, outcome: &Result<Option<bool>>) -> (log::Level, String) {
    let label = target.label();
    match outcome {
        Ok(Some(excluded)) => (
            if *excluded {
                log::Level::Info
            } else {
                log::Level::Warn
            },
            format!("[BACKUP] {label} excluded from device backups: {excluded}"),
        ),
        Ok(None) => (
            log::Level::Debug,
            format!("[BACKUP] {label} not there, nothing to exclude"),
        ),
        Err(e) => (
            log::Level::Warn,
            format!("[BACKUP] {label}: could not exclude it from device backups: {e}"),
        ),
    }
}

fn log_outcome(target: Target, outcome: &Result<Option<bool>>) {
    let (level, line) = outcome_line(target, outcome);
    log::log!(level, "{line}");
}

/// [`exclude_dir`] and a log line with the outcome. Never fails: a phone whose folder cannot be
/// excluded still starts and works (it just keeps the folder in device backups).
pub fn exclude_dir_and_log(excluder: &impl BackupExcluder, target: Target, dir: &Path) {
    log_outcome(target, &exclude_dir(excluder, dir).map(Some));
}

/// [`exclude_existing`] and a log line with the outcome. Never fails, like [`exclude_dir_and_log`].
pub fn exclude_existing_and_log(excluder: &impl BackupExcluder, target: Target, path: &Path) {
    log_outcome(target, &exclude_existing(excluder, path));
}

/// Every exclusion of a start, in order, with its outcome. One that fails does not keep the next
/// from being tried.
fn run_at_start(
    excluder: &impl BackupExcluder,
    folders: &StartFolders,
) -> Vec<(Target, Result<Option<bool>>)> {
    let mut outcomes = vec![(
        Target::Profile,
        exclude_dir(excluder, &folders.data_dir).map(Some),
    )];
    if let Some(webkit_dir) = &folders.webkit_dir {
        outcomes.push((Target::WebKit, exclude_dir(excluder, webkit_dir).map(Some)));
    }
    if let Some(received_files_dir) = &folders.received_files_dir {
        outcomes.push((
            Target::ReceivedFiles,
            exclude_existing(excluder, received_files_dir),
        ));
    }
    outcomes
}

/// The exclusions of a start (the profile, the webview's storage, the Inbox when it exists), each
/// with a log line. Never fails.
pub fn exclude_at_start(excluder: &impl BackupExcluder, folders: &StartFolders) {
    for (target, outcome) in run_at_start(excluder, folders) {
        log_outcome(target, &outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// What the platform was asked about, and what the path was at that moment.
    #[derive(Debug, PartialEq, Eq)]
    struct Call {
        path: PathBuf,
        is_dir: bool,
        is_file: bool,
    }

    /// Records the paths it was asked about; answers `answer`, except for the paths in `fail_for`.
    struct StubExcluder {
        answer: std::result::Result<bool, String>,
        fail_for: Vec<PathBuf>,
        calls: RefCell<Vec<Call>>,
    }

    impl StubExcluder {
        fn answering(answer: std::result::Result<bool, String>) -> Self {
            Self {
                answer,
                fail_for: Vec::new(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn failing_for(mut self, path: &Path) -> Self {
            self.fail_for.push(path.to_path_buf());
            self
        }

        fn asked(&self) -> Vec<PathBuf> {
            self.calls
                .borrow()
                .iter()
                .map(|call| call.path.clone())
                .collect()
        }
    }

    impl BackupExcluder for StubExcluder {
        fn exclude_from_backup(&self, path: &Path) -> Result<bool> {
            self.calls.borrow_mut().push(Call {
                path: path.to_path_buf(),
                is_dir: path.is_dir(),
                is_file: path.is_file(),
            });
            if self.fail_for.iter().any(|failing| failing == path) {
                return Err(AppError::Internal("NSCocoaErrorDomain 513".into()));
            }
            self.answer.clone().map_err(AppError::Internal)
        }
    }

    fn start_folders(root: &Path) -> StartFolders {
        StartFolders {
            data_dir: root.join("Library").join("Application Support").join("app"),
            webkit_dir: Some(webkit_dir(root)),
            received_files_dir: Some(received_files_dir(&root.join("Documents"))),
        }
    }

    fn inbox_of(folders: &StartFolders) -> PathBuf {
        folders.received_files_dir.clone().expect("inbox")
    }

    #[test]
    fn creates_the_missing_folder_before_excluding_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data_dir = tmp.path().join("Library").join("com.moonyfinance.mobile");
        let excluder = StubExcluder::answering(Ok(true));

        let excluded = exclude_dir(&excluder, &data_dir).expect("excluded");

        assert!(excluded);
        assert!(data_dir.is_dir());
        // The folder existed when the platform was asked: the flag needs something to attach to.
        assert_eq!(
            *excluder.calls.borrow(),
            vec![Call {
                path: data_dir,
                is_dir: true,
                is_file: false
            }]
        );
    }

    #[test]
    fn leaves_an_existing_folder_and_its_files_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("moony.db"), b"encrypted").expect("write");
        let excluder = StubExcluder::answering(Ok(true));

        assert!(exclude_dir(&excluder, tmp.path()).expect("excluded"));

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

        assert!(!exclude_dir(&excluder, tmp.path()).expect("answer"));
    }

    #[test]
    fn passes_the_platform_error_on() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let excluder = StubExcluder::answering(Err("NSCocoaErrorDomain 513".into()));

        let error = exclude_dir(&excluder, tmp.path()).expect_err("error");

        assert!(error.to_string().contains("NSCocoaErrorDomain 513"));
    }

    #[test]
    fn a_folder_that_cannot_be_created_never_reaches_the_platform() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("a-file");
        fs::write(&file, b"x").expect("write");
        let excluder = StubExcluder::answering(Ok(true));

        let error = exclude_dir(&excluder, &file.join("data")).expect_err("error");

        assert!(matches!(error, AppError::Internal(_)));
        assert!(excluder.calls.borrow().is_empty());
    }

    #[test]
    fn a_staged_file_is_excluded_as_a_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let archive = tmp.path().join("moony-backup-2026-10-18.zip");
        fs::write(&archive, b"zip").expect("write");
        let excluder = StubExcluder::answering(Ok(true));

        let outcome = exclude_existing(&excluder, &archive).expect("outcome");

        assert_eq!(outcome, Some(true));
        assert_eq!(
            *excluder.calls.borrow(),
            vec![Call {
                path: archive,
                is_dir: false,
                is_file: true
            }]
        );
    }

    #[test]
    fn something_that_is_not_there_is_not_created_and_not_asked_about() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("Inbox");
        let excluder = StubExcluder::answering(Ok(true));

        assert_eq!(
            exclude_existing(&excluder, &missing).expect("outcome"),
            None
        );

        assert!(!missing.exists());
        assert!(excluder.calls.borrow().is_empty());
    }

    #[test]
    fn existing_passes_the_platform_error_on() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let excluder = StubExcluder::answering(Err("boom".into()));

        let error = exclude_existing(&excluder, tmp.path()).expect_err("error");

        assert!(error.to_string().contains("boom"));
    }

    #[test]
    fn the_webview_storage_is_library_webkit_under_the_home_folder() {
        let home = Path::new("/var/mobile/Containers/Data/Application/ABC");
        assert_eq!(
            webkit_dir(home),
            PathBuf::from("/var/mobile/Containers/Data/Application/ABC/Library/WebKit")
        );
    }

    #[test]
    fn the_received_files_folder_is_the_documents_inbox() {
        assert_eq!(
            received_files_dir(Path::new("/c/Documents")),
            PathBuf::from("/c/Documents/Inbox")
        );
    }

    #[test]
    fn a_start_excludes_the_profile_the_webview_storage_and_an_existing_inbox_in_that_order() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let folders = start_folders(tmp.path());
        let inbox = inbox_of(&folders);
        fs::create_dir_all(&inbox).expect("inbox");
        let excluder = StubExcluder::answering(Ok(true));

        let outcomes = run_at_start(&excluder, &folders);

        let webkit = folders.webkit_dir.clone().expect("webkit");
        assert_eq!(
            excluder.asked(),
            vec![folders.data_dir.clone(), webkit, inbox]
        );
        let targets: Vec<Target> = outcomes.iter().map(|(target, _)| *target).collect();
        assert_eq!(
            targets,
            vec![Target::Profile, Target::WebKit, Target::ReceivedFiles]
        );
        assert!(outcomes
            .iter()
            .all(|(_, outcome)| matches!(outcome, Ok(Some(true)))));
        // The data folder and the webview storage were created for the flag to attach to.
        assert!(folders.data_dir.is_dir());
        assert!(folders.webkit_dir.expect("webkit").is_dir());
    }

    #[test]
    fn a_start_without_an_inbox_does_not_create_one() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let folders = start_folders(tmp.path());
        let excluder = StubExcluder::answering(Ok(true));

        let outcomes = run_at_start(&excluder, &folders);

        assert!(!inbox_of(&folders).exists());
        assert_eq!(excluder.asked().len(), 2);
        assert!(matches!(
            outcomes.last(),
            Some((Target::ReceivedFiles, Ok(None)))
        ));
    }

    #[test]
    fn a_start_without_a_webkit_folder_skips_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let folders = StartFolders {
            webkit_dir: None,
            ..start_folders(tmp.path())
        };
        let excluder = StubExcluder::answering(Ok(true));

        let outcomes = run_at_start(&excluder, &folders);

        assert_eq!(excluder.asked(), vec![folders.data_dir.clone()]);
        assert_eq!(outcomes.len(), 2);
        assert!(!tmp.path().join("Library").join("WebKit").exists());
    }

    #[test]
    fn a_start_without_a_known_inbox_location_skips_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let folders = StartFolders {
            received_files_dir: None,
            ..start_folders(tmp.path())
        };
        let excluder = StubExcluder::answering(Ok(true));

        let outcomes = run_at_start(&excluder, &folders);

        assert_eq!(excluder.asked().len(), 2);
        assert_eq!(outcomes.len(), 2);
    }

    #[test]
    fn a_failure_at_start_does_not_keep_the_rest_from_being_tried() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let folders = start_folders(tmp.path());
        fs::create_dir_all(inbox_of(&folders)).expect("inbox");
        let excluder = StubExcluder::answering(Ok(true)).failing_for(&folders.data_dir);

        let outcomes = run_at_start(&excluder, &folders);

        assert_eq!(excluder.asked().len(), 3);
        assert!(matches!(outcomes[0], (Target::Profile, Err(_))));
        assert!(matches!(outcomes[1], (Target::WebKit, Ok(Some(true)))));
        assert!(matches!(
            outcomes[2],
            (Target::ReceivedFiles, Ok(Some(true)))
        ));
    }

    #[test]
    fn only_an_excluded_target_is_logged_as_info() {
        let (level, line) = outcome_line(Target::Profile, &Ok(Some(true)));
        assert_eq!(level, log::Level::Info);
        assert_eq!(
            line,
            "[BACKUP] Profile folder excluded from device backups: true"
        );

        let (level, line) = outcome_line(Target::WebKit, &Ok(Some(false)));
        assert_eq!(level, log::Level::Warn);
        assert_eq!(
            line,
            "[BACKUP] WebKit storage folder excluded from device backups: false"
        );

        let (level, line) = outcome_line(Target::StagedExport, &Ok(None));
        assert_eq!(level, log::Level::Debug);
        assert!(line.contains("Staged export file") && line.contains("not there"));

        let (level, line) = outcome_line(
            Target::ReceivedFiles,
            &Err(AppError::Internal("boom".into())),
        );
        assert_eq!(level, log::Level::Warn);
        assert!(line.contains("Received files folder") && line.contains("boom"));
    }

    #[test]
    fn exclude_and_log_survives_every_outcome() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let staged = tmp.path().join("moony-recovery-key.txt");
        fs::write(&staged, b"key").expect("write");
        for answer in [Ok(true), Ok(false), Err("boom".to_string())] {
            let excluder = StubExcluder::answering(answer);
            exclude_dir_and_log(&excluder, Target::Profile, tmp.path());
            exclude_existing_and_log(&excluder, Target::StagedExport, &staged);
            exclude_existing_and_log(&excluder, Target::ReceivedFiles, &tmp.path().join("none"));
            exclude_at_start(&excluder, &start_folders(tmp.path()));
        }
    }
}
