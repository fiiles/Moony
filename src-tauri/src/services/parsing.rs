//! Money/quantity parsing helper for the portfolio metrics calculation paths.
//!
//! Wire values for money/quantity fields are stored as TEXT (per ADR 0001) and
//! parsed to `f64` at read time. A plain `.parse().unwrap_or(default)` silently
//! turns a corrupt value into a plausible-looking default, which is exactly the
//! failure mode a finance app should avoid (audit L3). `parse_money` keeps the
//! same fallback behavior but logs loudly when the value was actually corrupt
//! (non-empty but unparseable), so a bad row shows up in logs instead of just
//! silently distorting a total.
//!
//! No dedup/rate-limiting of the warning: corrupt DB values are expected to be
//! rare, and the metric paths that use this helper (live dashboard metrics,
//! day-by-day snapshot calculation, historical backfill) can legitimately call
//! it many times for the same field across many rows/days. Repeated warnings
//! during something like a backfill run are acceptable noise, not a reason to
//! add tracking state here.

/// Parse a monetary/quantity string value, defaulting to `default` on failure.
///
/// Behavior matches `value.parse().unwrap_or(default)` with one addition: if
/// `value` is non-empty after trimming and still fails to parse, a warning is
/// printed naming the corrupt value and the calling context. Empty or
/// whitespace-only values do NOT warn — legacy rows may legitimately have them,
/// and that case is already handled silently by the `default`.
pub fn parse_money(value: &str, default: f64, context: &str) -> f64 {
    match value.parse::<f64>() {
        Ok(parsed) => parsed,
        Err(_) => {
            if !value.trim().is_empty() {
                log::warn!(
                    "[METRICS] WARNING: corrupt money value '{}' in {}, using {}",
                    value,
                    context,
                    default
                );
            }
            default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_money_valid_number() {
        assert_eq!(parse_money("123.45", 0.0, "test.field"), 123.45);
    }

    #[test]
    fn test_parse_money_negative_decimal() {
        assert_eq!(parse_money("-42.5", 0.0, "test.field"), -42.5);
    }

    #[test]
    fn test_parse_money_corrupt_non_empty_returns_default() {
        assert_eq!(parse_money("not-a-number", 0.0, "test.field"), 0.0);
    }

    #[test]
    fn test_parse_money_empty_returns_default() {
        assert_eq!(parse_money("", 0.0, "test.field"), 0.0);
    }

    #[test]
    fn test_parse_money_whitespace_only_returns_default() {
        assert_eq!(parse_money("   ", 0.0, "test.field"), 0.0);
    }

    #[test]
    fn test_parse_money_padded_numeric_is_treated_as_corrupt() {
        // Pins the "parse the raw string" contract: `f64::from_str` does not
        // allow surrounding whitespace, so a padded numeric like "  42  " is
        // NOT trimmed-then-parsed to 42.0 — it falls into the corrupt-value
        // branch and returns `default`. If a future refactor trims before
        // parsing, this test breaks and calls out the behavior change.
        assert_eq!(parse_money("  42  ", 0.0, "test.field"), 0.0);
    }

    #[test]
    fn test_parse_money_default_one_variant() {
        // Bonds quantity parsing uses default 1.0 instead of 0.0.
        assert_eq!(parse_money("3", 1.0, "bonds.quantity"), 3.0);
        assert_eq!(parse_money("", 1.0, "bonds.quantity"), 1.0);
        assert_eq!(parse_money("garbage", 1.0, "bonds.quantity"), 1.0);
    }
}
