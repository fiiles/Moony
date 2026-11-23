//! Which folder holds the profile that is open (ADR 0037): the real profile in the app data folder,
//! or the demo profile in `<app data folder>/demo` while the demo is open.
//!
//! One process-wide flag says whether the demo is the open profile. Everything a running session
//! reads or writes next to its database (the attachments store's folder, legacy attachment files,
//! the attachment URI scheme) goes through [`open_profile_dir`]; everything that belongs to the
//! real profile alone (`check_setup`, the key files, biometric unlock, the sync binding, backups'
//! restore target, startup sweeps) keeps using the real folder. The call-site audit is in the ADR.
//!
//! While the demo is open, the features that would make synthetic data leave the device or touch
//! the real profile's keys refuse with [`NOT_IN_DEMO`] ([`ensure_not_demo`]).

use std::path::{Path, PathBuf};

use tauri::Manager;

use crate::error::{AppError, Result};

/// The demo profile's folder name inside the app data folder.
pub const DEMO_DIR_NAME: &str = "demo";

/// The error key of every feature the demo switches off (`common.json` → `auth.notInDemo`).
pub const NOT_IN_DEMO: &str = "auth.notInDemo";

// Production: one flag for the whole process. Tests: one per test thread, so a test that opens
// the demo never switches the gates on for the tests running beside it.
#[cfg(not(test))]
static DEMO_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    static DEMO_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the demo profile is the open profile.
pub fn demo_open() -> bool {
    #[cfg(not(test))]
    {
        DEMO_OPEN.load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(test)]
    {
        DEMO_OPEN.with(std::cell::Cell::get)
    }
}

/// Mark the demo profile open (after it was opened) or closed (when its session ends).
pub fn set_demo_open(open: bool) {
    #[cfg(not(test))]
    DEMO_OPEN.store(open, std::sync::atomic::Ordering::SeqCst);
    #[cfg(test)]
    DEMO_OPEN.with(|flag| flag.set(open));
}

/// `auth.notInDemo` while the demo is open: the gate of sync, the MCP server, biometric unlock,
/// backups, the password, recovery and account deletion.
pub fn ensure_not_demo() -> Result<()> {
    if demo_open() {
        Err(AppError::Auth(NOT_IN_DEMO.into()))
    } else {
        Ok(())
    }
}

/// The demo folder that belongs to the real profile folder `real_dir`.
pub fn demo_dir_in(real_dir: &Path) -> PathBuf {
    real_dir.join(DEMO_DIR_NAME)
}

/// The folder of the open profile: the demo folder while `demo` is open, else `real_dir`.
pub fn open_dir_in(real_dir: &Path, demo: bool) -> PathBuf {
    if demo {
        demo_dir_in(real_dir)
    } else {
        real_dir.to_path_buf()
    }
}

/// The real profile's folder: the app data folder.
pub fn real_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {e}")))
}

/// The demo profile's folder: `<app data folder>/demo`.
pub fn demo_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    real_dir(app).map(|dir| demo_dir_in(&dir))
}

/// The open profile's folder (see the module docs for who uses it).
pub fn open_profile_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    real_dir(app).map(|dir| open_dir_in(&dir, demo_open()))
}

/// Test helper: the demo flag of this test thread set to `open` until the guard drops.
#[cfg(test)]
pub(crate) fn demo_flag_for_test(open: bool) -> DemoFlagGuard {
    set_demo_open(open);
    DemoFlagGuard
}

/// Clears this test thread's demo flag when dropped.
#[cfg(test)]
pub(crate) struct DemoFlagGuard;

#[cfg(test)]
impl Drop for DemoFlagGuard {
    fn drop(&mut self) {
        set_demo_open(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_folder_is_the_demo_folder_only_while_the_demo_is_open() {
        let real = Path::new("/data/com.moonyfinance.desktop");
        assert_eq!(open_dir_in(real, false), real);
        assert_eq!(
            open_dir_in(real, true),
            Path::new("/data/com.moonyfinance.desktop/demo")
        );
    }

    #[test]
    fn the_gate_refuses_with_its_key_while_the_demo_is_open() {
        assert!(ensure_not_demo().is_ok(), "closed by default");
        {
            let _demo = demo_flag_for_test(true);
            assert!(demo_open());
            match ensure_not_demo() {
                Err(AppError::Auth(key)) => assert_eq!(key, NOT_IN_DEMO),
                other => panic!("expected the demo gate, got {other:?}"),
            }
        }
        assert!(!demo_open(), "the guard closes it again");
        assert!(ensure_not_demo().is_ok());
    }
}
