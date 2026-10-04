//! Application logging.
//!
//! Everything goes through the `log` facade; `tauri-plugin-log` writes it to the
//! per-OS app log directory so a bug report can attach a real log file:
//!
//! | OS      | Log file                                                          |
//! |---------|-------------------------------------------------------------------|
//! | macOS   | `~/Library/Logs/com.filipkral.moony/moony.log`              |
//! | Windows | `%LOCALAPPDATA%\com.filipkral.moony\logs\moony.log`         |
//! | Linux   | `$XDG_DATA_HOME/com.filipkral.moony/logs/moony.log`         |
//! |         | (`~/.local/share/com.filipkral.moony/logs/moony.log`)       |
//!
//! Timestamps are UTC. The file is capped at [`MAX_LOG_FILE_BYTES`] and rotated
//! to dated archives; only [`ARCHIVED_LOGS_TO_KEEP`] archives are kept, so the
//! whole log folder stays under ~6 MB. Release builds log `info` and above,
//! debug builds `debug` plus stdout.

use std::path::PathBuf;

use log::LevelFilter;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::error::{AppError, Result};

/// Name of the active log file (`<name>.log`).
pub const LOG_FILE_NAME: &str = "moony";
/// Rotate once the active file would grow past this many bytes.
pub const MAX_LOG_FILE_BYTES: u128 = 2_000_000;
/// Dated archives kept next to the active file.
pub const ARCHIVED_LOGS_TO_KEEP: usize = 2;

// Compile-time guards: the log folder stays bounded, and `KeepSome(0)` would
// underflow inside the plugin's rotation.
const _: () = assert!(MAX_LOG_FILE_BYTES * (ARCHIVED_LOGS_TO_KEEP as u128 + 1) <= 6_000_000);
const _: () = assert!(ARCHIVED_LOGS_TO_KEEP >= 1);

/// Chatty dependencies that would drown the app's own lines at `debug`.
const QUIET_MODULES: [&str; 5] = ["hyper", "hyper_util", "reqwest", "rustls", "h2"];

/// Maximum level written to the log: `info` in release, `debug` in debug builds.
pub fn level_for_build(debug_build: bool) -> LevelFilter {
    if debug_build {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    }
}

/// Logger configuration shared by the real plugin and the tests: given targets,
/// level and rotation size, with the project's rotation policy and quiet modules.
fn configure(
    targets: Vec<Target>,
    level: LevelFilter,
    max_file_bytes: u128,
) -> tauri_plugin_log::Builder {
    let mut builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .targets(targets)
        .level(level)
        .max_file_size(max_file_bytes)
        .rotation_strategy(RotationStrategy::KeepSome(ARCHIVED_LOGS_TO_KEEP));
    for module in QUIET_MODULES {
        builder = builder.level_for(module, LevelFilter::Warn);
    }
    builder
}

/// The `tauri-plugin-log` plugin: rotated file in the app log dir, plus stdout
/// in debug builds. Register it before every other plugin so their logs land
/// in the file too.
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let debug_build = cfg!(debug_assertions);

    let mut targets = vec![Target::new(TargetKind::LogDir {
        file_name: Some(LOG_FILE_NAME.to_string()),
    })];
    if debug_build {
        targets.push(Target::new(TargetKind::Stdout));
    }

    configure(targets, level_for_build(debug_build), MAX_LOG_FILE_BYTES).build()
}

/// Directory that holds the log file(s). Created if it does not exist yet.
pub fn logs_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::Internal(format!("Failed to resolve the log folder: {e}")))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Internal(format!("Failed to create the log folder: {e}")))?;
    Ok(dir)
}

/// First line of every session: version and platform, so a pasted log is
/// self-describing.
pub fn log_startup_banner() {
    log::info!(
        "Moony {} starting on {} {} (log level {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        level_for_build(cfg!(debug_assertions))
    );
}

/// Human-readable text of a panic payload (`&str` or `String`; anything else is opaque).
fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "<non-string panic payload>"
    }
}

/// `panic at <file>:<line>: <message>` for the log.
fn format_panic(payload: &(dyn std::any::Any + Send), location: Option<(&str, u32)>) -> String {
    let message = panic_payload_text(payload);
    match location {
        Some((file, line)) => format!("panic at {file}:{line}: {message}"),
        None => format!("panic: {message}"),
    }
}

/// Route panics to the log file (then to the previous hook, which still prints
/// to stderr). Without this a crash leaves no trace on a release build, where
/// there is no console.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| (l.file(), l.line()));
        log::error!("{}", format_panic(info.payload(), location));
        log::logger().flush();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_builds_log_info_debug_builds_log_debug() {
        assert_eq!(level_for_build(false), LevelFilter::Info);
        assert_eq!(level_for_build(true), LevelFilter::Debug);
    }

    #[test]
    fn panic_text_covers_static_and_formatted_payloads() {
        let from_str = std::panic::catch_unwind(|| panic!("static boom")).unwrap_err();
        assert_eq!(panic_payload_text(from_str.as_ref()), "static boom");

        let code = 7;
        let from_string = std::panic::catch_unwind(|| panic!("formatted {code}")).unwrap_err();
        assert_eq!(panic_payload_text(from_string.as_ref()), "formatted 7");

        let opaque = std::panic::catch_unwind(|| std::panic::panic_any(42u8)).unwrap_err();
        assert_eq!(
            panic_payload_text(opaque.as_ref()),
            "<non-string panic payload>"
        );
    }

    #[test]
    fn panic_line_names_the_location_when_known() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(
            format_panic(payload.as_ref(), Some(("src/lib.rs", 12))),
            "panic at src/lib.rs:12: boom"
        );
        assert_eq!(format_panic(payload.as_ref(), None), "panic: boom");
    }

    // ------------------------------------------------------------------
    // The configured logger really writes and rotates a file
    // ------------------------------------------------------------------

    use log::{Level, Log, Record};
    use std::fs;
    use std::path::Path;

    /// Build the project's logger writing into `dir` (instead of the OS log dir)
    /// without installing it as the process-wide logger.
    fn logger_in(dir: &Path, level: LevelFilter, max_file_bytes: u128) -> Box<dyn Log> {
        let app = tauri::test::mock_app();
        let targets = vec![Target::new(TargetKind::Folder {
            path: dir.to_path_buf(),
            file_name: Some(LOG_FILE_NAME.to_string()),
        })];
        let (_plugin, _max_level, logger) = configure(targets, level, max_file_bytes)
            .split(app.handle())
            .expect("split logger");
        logger
    }

    fn emit(logger: &dyn Log, level: Level, target: &str, message: &str) {
        logger.log(
            &Record::builder()
                .args(format_args!("{message}"))
                .level(level)
                .target(target)
                .build(),
        );
        logger.flush();
    }

    #[test]
    fn writes_formatted_lines_to_moony_log_and_honours_the_level() {
        let dir = tempfile::tempdir().unwrap();
        let logger = logger_in(dir.path(), LevelFilter::Info, MAX_LOG_FILE_BYTES);

        emit(logger.as_ref(), Level::Info, "moony", "[TEST] hello log");
        emit(logger.as_ref(), Level::Debug, "moony", "[TEST] too chatty");
        emit(
            logger.as_ref(),
            Level::Info,
            "hyper",
            "[TEST] quiet module info",
        );
        emit(
            logger.as_ref(),
            Level::Warn,
            "hyper",
            "[TEST] quiet module warn",
        );

        let text = fs::read_to_string(dir.path().join("moony.log")).expect("moony.log exists");
        assert!(text.contains("[INFO] [TEST] hello log"), "{text}");
        assert!(!text.contains("too chatty"), "{text}");
        assert!(!text.contains("quiet module info"), "{text}");
        assert!(text.contains("[WARN] [TEST] quiet module warn"), "{text}");
    }

    #[test]
    fn rotation_keeps_the_active_file_and_a_bounded_number_of_archives() {
        let dir = tempfile::tempdir().unwrap();
        // Every 2.5 KB line overflows a 4 KB file, so each emit after the first
        // rotates once. Archive names carry a seconds-resolution timestamp (two
        // rotations in one second would collide, which cannot happen with the
        // real 2 MB limit), hence the sleep between lines.
        let logger = logger_in(dir.path(), LevelFilter::Info, 4_000);
        let line = "x".repeat(2_500);
        let emits = ARCHIVED_LOGS_TO_KEEP + 3;
        for i in 0..emits {
            emit(
                logger.as_ref(),
                Level::Info,
                "moony",
                &format!("{i} {line}"),
            );
            std::thread::sleep(std::time::Duration::from_millis(1_050));
        }

        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"moony.log".to_string()), "{names:?}");
        // Active file + at most ARCHIVED_LOGS_TO_KEEP dated archives.
        assert!(
            names.len() <= ARCHIVED_LOGS_TO_KEEP + 1,
            "too many log files kept: {names:?}"
        );
        let newest = fs::read_to_string(dir.path().join("moony.log")).unwrap();
        assert!(
            newest.contains(&format!("{} x", emits - 1)),
            "the latest line must be in the active log"
        );
    }
}
