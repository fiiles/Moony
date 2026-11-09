//! The runtime flag for device sync (plan's call P1): everything compiles and is tested in every
//! build, but nothing of sync runs unless the flag is on. `RELEASED` flips in phase 6; until then
//! a debug build turns sync on with `MOONY_DEV_SYNC=1` (and points at a dev relay with
//! `MOONY_RELAY_URL`, read by `relay::http::default_base_url`). A release build never reads the
//! variable.

/// Device sync is released to everyone (phase 6).
pub const RELEASED: bool = false;

/// The environment variable that turns sync on in a debug build.
pub const DEV_VARIABLE: &str = "MOONY_DEV_SYNC";

/// Whether sync runs in this process.
pub fn enabled() -> bool {
    let dev = if cfg!(debug_assertions) {
        std::env::var(DEV_VARIABLE).ok()
    } else {
        None
    };
    decide(RELEASED, cfg!(debug_assertions), dev.as_deref())
}

/// Whether the developer tools of sync (the dev grant, the measurements) are offered.
pub fn dev_tools() -> bool {
    cfg!(debug_assertions) && enabled()
}

/// The flag: released, or a debug build with the variable set to exactly `1`.
fn decide(released: bool, debug: bool, dev_variable: Option<&str>) -> bool {
    released || (debug && dev_variable == Some("1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_off_in_this_build_unless_the_dev_variable_is_set() {
        assert!(
            !decide(RELEASED, false, None),
            "a release build of this version runs no sync (released in phase 6)"
        );
        // (released, debug, variable) → enabled
        let table = [
            (false, true, Some("1"), true),
            (false, true, None, false),
            (false, true, Some("0"), false),
            (false, true, Some("true"), false),
            (false, true, Some(""), false),
            (false, false, Some("1"), false),
            (false, false, None, false),
            (true, false, None, true),
            (true, true, None, true),
        ];
        for (released, debug, variable, expected) in table {
            assert_eq!(
                decide(released, debug, variable),
                expected,
                "released {released}, debug {debug}, {DEV_VARIABLE} {variable:?}"
            );
        }
        // This process: a debug build follows the variable, a release build ignores it.
        let variable = std::env::var(DEV_VARIABLE).ok();
        let expected = cfg!(debug_assertions) && variable.as_deref() == Some("1");
        assert_eq!(enabled(), expected);
        assert_eq!(dev_tools(), expected);
    }
}
