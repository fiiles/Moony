//! The runtime flag for device sync (plan's call P1): everything compiles and is tested in every
//! build, but nothing of sync runs unless the flag is on. `RELEASED` is on since 2026-11-22, so
//! every build runs sync. Before that a debug build had sync on by default (owner's call,
//! 2026-11-14) and `MOONY_DEV_SYNC=0` turned it off; with `RELEASED` on the variable no longer
//! turns it off. `MOONY_RELAY_URL` points a debug build at a dev relay (read by
//! `relay::http::default_base_url`).
//! An iPhone app gets no environment variables, so a debug build made with `MOONY_DEV_SYNC=0` (or
//! `MOONY_RELAY_URL`) in the build environment carries them in the binary (`option_env!`; cargo
//! rebuilds when they change); the run-time value wins over the built-in one. A release build
//! never reads the variables, at run time or at build time.
//!
//! The flag gates the engine, the commands and the UI, not change capture: a build without the flag
//! still records an enabled profile's changes (`sync::on_open`, audit M12, ADR 0027) and can turn
//! sync off (`sync_turn_off`), so it never writes synced tables uncaptured.
//! CI runs `cargo test --release --lib services::sync::feature` with both variables set.

/// Device sync is released to everyone (owner's call, 2026-11-22: own servers only; the Moony
/// cloud stays a disabled choice in the setup until it runs).
pub const RELEASED: bool = true;

/// The environment variable that turns sync off in a debug build (`0`).
pub const DEV_VARIABLE: &str = "MOONY_DEV_SYNC";

/// `MOONY_DEV_SYNC` as it was when this debug binary was built.
#[cfg(debug_assertions)]
const BUILT_WITH: Option<&str> = option_env!("MOONY_DEV_SYNC");
#[cfg(not(debug_assertions))]
const BUILT_WITH: Option<&str> = None;

/// `MOONY_RELAY_URL` as it was when this debug binary was built.
#[cfg(debug_assertions)]
const BUILT_RELAY: Option<&str> = option_env!("MOONY_RELAY_URL");
#[cfg(not(debug_assertions))]
const BUILT_RELAY: Option<&str> = None;

/// Whether sync runs in this process.
pub fn enabled() -> bool {
    let dev = if cfg!(debug_assertions) {
        std::env::var(DEV_VARIABLE).ok()
    } else {
        None
    };
    decide(RELEASED, cfg!(debug_assertions), dev.as_deref(), BUILT_WITH)
}

/// Whether the developer tools of sync (the measurements) are offered.
pub fn dev_tools() -> bool {
    cfg!(debug_assertions) && enabled()
}

/// The dev sync server of a debug build: the run-time `MOONY_RELAY_URL`, else the one the binary
/// was built with. `None` in a release build, which never reads either.
pub fn dev_relay_base() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    pick_dev_base(
        std::env::var("MOONY_RELAY_URL").ok().as_deref(),
        BUILT_RELAY,
    )
}

/// The first non-empty of the run-time and the build-time value, the run-time one first.
fn pick_dev_base(runtime: Option<&str>, built_with: Option<&str>) -> Option<String> {
    [runtime, built_with]
        .into_iter()
        .flatten()
        .find(|url| !url.is_empty())
        .map(str::to_string)
}

/// The flag: released, or a debug build whose variable is not exactly `0`. The run-time value
/// wins over the one the binary was built with.
fn decide(released: bool, debug: bool, runtime: Option<&str>, built_with: Option<&str>) -> bool {
    released || (debug && runtime.or(built_with) != Some("0"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_debug_build_has_sync_on_unless_the_dev_variable_is_zero() {
        assert!(
            decide(RELEASED, false, None, None),
            "a release build runs sync (released to everyone, owner's call 2026-11-22)"
        );
        // (released, debug, run-time variable, built with) → enabled
        let table = [
            (false, true, None, None, true),
            (false, true, Some("1"), None, true),
            (false, true, Some(""), None, true),
            (false, true, Some("0"), None, false),
            (false, true, None, Some("0"), false),
            (false, true, Some("1"), Some("0"), true),
            (false, true, Some("0"), Some("1"), false),
            (false, true, None, Some("1"), true),
            (false, false, Some("1"), None, false),
            (false, false, None, Some("1"), false),
            (false, false, None, None, false),
            (true, false, None, None, true),
            (true, true, Some("0"), None, true),
        ];
        for (released, debug, variable, built_with, expected) in table {
            assert_eq!(
                decide(released, debug, variable, built_with),
                expected,
                "released {released}, debug {debug}, {DEV_VARIABLE} {variable:?}, built with {built_with:?}"
            );
        }
        // This process: sync runs whatever the variable says; the developer tools only in debug.
        assert!(enabled());
        assert_eq!(dev_tools(), cfg!(debug_assertions));
    }

    #[test]
    fn the_dev_base_prefers_the_runtime_variable() {
        assert_eq!(
            pick_dev_base(Some("http://localhost:1"), Some("http://127.0.0.1:8787")),
            Some("http://localhost:1".to_string())
        );
        assert_eq!(
            pick_dev_base(None, Some("http://127.0.0.1:8787")),
            Some("http://127.0.0.1:8787".to_string())
        );
        assert_eq!(
            pick_dev_base(Some(""), Some("http://127.0.0.1:8787")),
            Some("http://127.0.0.1:8787".to_string())
        );
        assert_eq!(pick_dev_base(Some(""), None), None);
        assert_eq!(pick_dev_base(None, None), None);
    }

    /// A release build ignores both variables, whether they were set at build time or are set
    /// now (run with `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=... cargo test --release`).
    #[cfg(not(debug_assertions))]
    #[test]
    fn a_release_build_ignores_the_dev_variables() {
        assert!(enabled(), "released: a release build runs sync");
        assert!(!dev_tools());
        assert_eq!(dev_relay_base(), None);
        assert_eq!(
            crate::services::sync::relay::http::default_base_url(),
            crate::services::sync::relay::http::DEFAULT_RELAY_URL
        );
    }
}
