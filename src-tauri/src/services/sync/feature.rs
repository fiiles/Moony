//! The runtime flag for device sync (plan's call P1): everything compiles and is tested in every
//! build, but nothing of sync runs unless the flag is on. `RELEASED` flips in phase 6; until then
//! a debug build turns sync on with `MOONY_DEV_SYNC=1` (and points at a dev relay with
//! `MOONY_RELAY_URL`, read by `relay::http::default_base_url`). An iPhone app gets no environment
//! variables, so a debug build made with `MOONY_DEV_SYNC=1` (and `MOONY_RELAY_URL`) in the build
//! environment carries them in the binary (`option_env!`; cargo rebuilds when they change). A
//! release build never reads the variables, at run time or at build time.

/// Device sync is released to everyone (phase 6).
pub const RELEASED: bool = false;

/// The environment variable that turns sync on in a debug build.
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

/// Whether the developer tools of sync (the dev grant, the measurements) are offered.
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

/// The flag: released, or a debug build with the variable set to exactly `1` at run time or at
/// build time.
fn decide(released: bool, debug: bool, runtime: Option<&str>, built_with: Option<&str>) -> bool {
    released || (debug && (runtime == Some("1") || built_with == Some("1")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_off_in_this_build_unless_the_dev_variable_is_set() {
        assert!(
            !decide(RELEASED, false, None, None),
            "a release build of this version runs no sync (released in phase 6)"
        );
        // (released, debug, run-time variable, built with) → enabled
        let table = [
            (false, true, None, Some("1"), true),
            (false, true, Some("0"), Some("1"), true),
            (false, false, None, Some("1"), false),
            (false, true, None, Some("yes"), false),
            (false, true, Some("1"), None, true),
            (false, true, None, None, false),
            (false, true, Some("0"), None, false),
            (false, true, Some("true"), None, false),
            (false, true, Some(""), None, false),
            (false, false, Some("1"), None, false),
            (false, false, None, None, false),
            (true, false, None, None, true),
            (true, true, None, None, true),
        ];
        for (released, debug, variable, built_with, expected) in table {
            assert_eq!(
                decide(released, debug, variable, built_with),
                expected,
                "released {released}, debug {debug}, {DEV_VARIABLE} {variable:?}, built with {built_with:?}"
            );
        }
        // This process: a debug build follows the variable, a release build ignores it.
        let variable = std::env::var(DEV_VARIABLE).ok();
        let expected =
            cfg!(debug_assertions) && (variable.as_deref() == Some("1") || BUILT_WITH == Some("1"));
        assert_eq!(enabled(), expected);
        assert_eq!(dev_tools(), expected);
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
        assert!(!enabled());
        assert!(!dev_tools());
        assert_eq!(dev_relay_base(), None);
        assert_eq!(
            crate::services::sync::relay::http::default_base_url(),
            crate::services::sync::relay::http::DEFAULT_RELAY_URL
        );
    }
}
