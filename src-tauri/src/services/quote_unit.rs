//! The unit a Yahoo stock quote is expressed in.
//!
//! Every Yahoo chart response names the currency of its prices in `meta.currency`, and for many
//! listings that is not the one the ticker suffix suggests: `CSPX.L` and `VWRA.L` are London
//! listings of dollar ETFs and quote in `USD`. Exchanges that quote in a minor unit report a code
//! of their own: `GBp` (pence, also `GBX`) in London, `ZAc` (cents) in Johannesburg, `ILA`
//! (agorot) in Tel Aviv. Such a price is a hundred times the major currency's, so it is scaled by
//! 0.01 and stored in `GBP` / `ZAR` / `ILS`.
//!
//! `GBp` (pence) and `GBP` (pounds) differ only in the case of one letter: the code must never be
//! compared case-insensitively, or a London share is valued a hundred times too high.
//!
//! [`quote_unit`] is the one place that decides. Every Yahoo price path in `price_api` goes
//! through it; the ticker suffix (`price_api::get_currency_from_ticker`) is only the fallback for
//! a response that names no currency.

use crate::services::price_api::get_currency_from_ticker;

/// Ten decimals: far finer than any quote.
const DECIMALS_FACTOR: f64 = 1e10;

/// Digits after the decimal point of a formatted number.
fn decimals_of(text: &str) -> usize {
    text.split_once('.')
        .map_or(0, |(_, decimals)| decimals.len())
}

/// What a quoted price is stored as.
#[derive(Debug, Clone, PartialEq)]
pub struct QuoteUnit {
    /// ISO 4217 code of the currency the stored price is in (the major unit).
    pub currency: String,
    /// What a quoted price is multiplied by to get the stored one: 1.0, or 0.01 for a minor unit.
    pub scale: f64,
}

impl QuoteUnit {
    /// A quoted price as the stored one. Without a scale the price is returned untouched; a
    /// scaled one is rounded to ten decimals, which drops the binary float noise of `443.65 *
    /// 0.01` (4.4365000000000006) and is far finer than any quote.
    pub fn apply(&self, quoted: f64) -> f64 {
        if self.scale == 1.0 {
            quoted
        } else {
            ((quoted * self.scale) * DECIMALS_FACTOR).round() / DECIMALS_FACTOR
        }
    }

    /// The decimal text a stored price is written as. Prices keep two decimals, as they always
    /// have; a scaled price keeps up to six (trailing zeros dropped, two at least), because two
    /// decimals of a price in pounds would round 3.2 pence to 0.03.
    pub fn price_text(&self, price: f64) -> String {
        if self.scale == 1.0 {
            return format!("{price:.2}");
        }
        let mut text = format!("{price:.6}");
        while text.ends_with('0') && decimals_of(&text) > 2 {
            text.pop();
        }
        text
    }
}

/// The unit of a quote: from the currency Yahoo reported, else from the ticker's suffix.
///
/// - `GBp` (exactly) and `GBX` in any case: pence, i.e. `GBP` at a scale of 0.01. `GBP` itself
///   is pounds, and so is `gbp`.
/// - `ZAc` (South African cents) and `ILA` (Israeli agorot): `ZAR` / `ILS` at 0.01.
/// - Any other code: itself, trimmed and in upper case, at 1.0.
/// - No code (or an empty one): the suffix guess at 1.0, for a response that has none.
pub fn quote_unit(reported: Option<&str>, ticker: &str) -> QuoteUnit {
    let Some(code) = reported.map(str::trim).filter(|code| !code.is_empty()) else {
        return QuoteUnit {
            currency: get_currency_from_ticker(ticker).to_string(),
            scale: 1.0,
        };
    };
    let hundredth = |currency: &str| QuoteUnit {
        currency: currency.to_string(),
        scale: 0.01,
    };
    // `GBp` is the only code that needs its case: `GBP` is a currency of its own. `GBX`, `ZAC`
    // and `ILA` are not ISO codes in any spelling, so they are read in any case.
    if code == "GBp" || code.eq_ignore_ascii_case("GBX") {
        hundredth("GBP")
    } else if code.eq_ignore_ascii_case("ZAc") {
        hundredth("ZAR")
    } else if code.eq_ignore_ascii_case("ILA") {
        hundredth("ILS")
    } else {
        QuoteUnit {
            currency: code.to_ascii_uppercase(),
            scale: 1.0,
        }
    }
}

/// Whether history stored before a refresh was written in another unit than the refresh's.
///
/// `stored` is the code Yahoo reported at the previous refresh. Without one (a ticker that has
/// not been refreshed since the unit was recorded) the old assumption stands: the suffix guess
/// at 1.0, which is what every earlier version stored. Two codes of one unit (`GBp`, `GBX`) are
/// no change.
pub fn unit_changed(stored: Option<&str>, new: &QuoteUnit, ticker: &str) -> bool {
    quote_unit(stored, ticker) != *new
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::currency::is_iso_4217;

    fn unit(reported: Option<&str>, ticker: &str) -> (String, f64) {
        let unit = quote_unit(reported, ticker);
        (unit.currency, unit.scale)
    }

    fn expect(currency: &str, scale: f64) -> (String, f64) {
        (currency.to_string(), scale)
    }

    // ---- quote_unit ---------------------------------------------------------------

    #[test]
    fn pence_codes_are_pounds_at_a_hundredth() {
        for code in ["GBp", "GBX", "GBx", "gbx", " GBp ", "\tGBX\n"] {
            assert_eq!(unit(Some(code), "BARC.L"), expect("GBP", 0.01), "{code:?}");
        }
    }

    #[test]
    fn pounds_and_pence_differ_only_by_case() {
        assert_eq!(unit(Some("GBP"), "VUSA.L"), expect("GBP", 1.0));
        assert_eq!(unit(Some("GBp"), "VUSA.L"), expect("GBP", 0.01));
        // Only the exact `GBp` is pence; any other spelling of the pound is the pound.
        assert_eq!(unit(Some("gbp"), "VUSA.L"), expect("GBP", 1.0));
        assert_eq!(unit(Some("Gbp"), "VUSA.L"), expect("GBP", 1.0));
        assert_eq!(unit(Some(" GBP "), "VUSA.L"), expect("GBP", 1.0));
    }

    #[test]
    fn south_african_cents_and_israeli_agorot_are_hundredths() {
        assert_eq!(unit(Some("ZAc"), "NPN.JO"), expect("ZAR", 0.01));
        assert_eq!(unit(Some("ILA"), "TEVA.TA"), expect("ILS", 0.01));
        // The majors themselves are untouched.
        assert_eq!(unit(Some("ZAR"), "NPN.JO"), expect("ZAR", 1.0));
        assert_eq!(unit(Some("ILS"), "TEVA.TA"), expect("ILS", 1.0));
    }

    #[test]
    fn any_other_code_is_itself_trimmed_and_in_upper_case() {
        for (code, expected) in [
            ("USD", "USD"),
            ("EUR", "EUR"),
            ("eur", "EUR"),
            (" chf ", "CHF"),
            ("CZK", "CZK"),
            ("XYZ", "XYZ"),
        ] {
            assert_eq!(unit(Some(code), "ANY.L"), expect(expected, 1.0), "{code:?}");
        }
    }

    #[test]
    fn the_reported_code_wins_over_the_ticker_suffix() {
        // London listings of dollar ETFs (live Yahoo values, 2026-10-04).
        for ticker in ["CSPX.L", "VWRA.L", "EIMI.L", "IWDA.L", "VUSD.L"] {
            assert_eq!(unit(Some("USD"), ticker), expect("USD", 1.0), "{ticker}");
        }
        assert_eq!(unit(Some("GBP"), "VWRL.L"), expect("GBP", 1.0));
        assert_eq!(unit(Some("EUR"), "SXR8.DE"), expect("EUR", 1.0));
        assert_eq!(unit(Some("CZK"), "CEZ.PR"), expect("CZK", 1.0));
    }

    #[test]
    fn without_a_reported_code_the_suffix_decides_at_scale_one() {
        for reported in [None, Some(""), Some("   ")] {
            assert_eq!(unit(reported, "VUSA.L"), expect("GBP", 1.0), "{reported:?}");
            assert_eq!(unit(reported, "SXR8.DE"), expect("EUR", 1.0));
            assert_eq!(unit(reported, "CEZ.PR"), expect("CZK", 1.0));
            assert_eq!(unit(reported, "AAPL"), expect("USD", 1.0));
        }
    }

    #[test]
    fn every_currency_a_unit_can_name_is_one_the_app_accepts() {
        // The minor units resolve to these majors; the CSV import and the forms reject codes
        // that are not ISO 4217, so each has to pass.
        for code in ["GBP", "ZAR", "ILS"] {
            assert!(is_iso_4217(code), "{code}");
        }
        for reported in ["GBp", "GBX", "ZAc", "ILA"] {
            let unit = quote_unit(Some(reported), "X.L");
            assert!(
                is_iso_4217(&unit.currency),
                "{reported} -> {}",
                unit.currency
            );
        }
    }

    // ---- apply and price_text ---------------------------------------------------------

    #[test]
    fn a_price_without_a_scale_is_returned_untouched() {
        let usd = quote_unit(Some("USD"), "CSPX.L");
        for price in [832.99, 0.0, 0.1 + 0.2, 443.6499938964844] {
            assert_eq!(usd.apply(price).to_bits(), price.to_bits());
        }
    }

    #[test]
    fn pence_become_pounds_without_float_noise() {
        let pence = quote_unit(Some("GBp"), "BARC.L");
        assert_eq!(pence.apply(443.65), 4.4365);
        assert_eq!(pence.apply(104.0), 1.04);
        assert_eq!(pence.apply(4912.35), 49.1235);
        assert_eq!(
            pence.apply(0.0525),
            0.000525,
            "a sub-penny price keeps its digits"
        );
        // Yahoo's own float32 noise is cleaned up too.
        assert_eq!(pence.apply(443.6499938964844), 4.4364999390);
        // The same for cents and agorot.
        assert_eq!(quote_unit(Some("ZAc"), "NPN.JO").apply(69551.0), 695.51);
        assert_eq!(quote_unit(Some("ILA"), "TEVA.TA").apply(12160.0), 121.6);
    }

    #[test]
    fn prices_without_a_scale_keep_two_decimals_as_ever() {
        let usd = quote_unit(Some("USD"), "AAPL");
        assert_eq!(usd.price_text(443.65), "443.65");
        assert_eq!(usd.price_text(832.989999), "832.99");
        assert_eq!(usd.price_text(1.0), "1.00");
        assert_eq!(usd.price_text(2000.0), "2000.00");
    }

    #[test]
    fn scaled_prices_keep_up_to_six_decimals_and_at_least_two() {
        let pence = quote_unit(Some("GBp"), "BARC.L");
        assert_eq!(pence.price_text(4.4365), "4.4365");
        assert_eq!(pence.price_text(1.04), "1.04");
        assert_eq!(pence.price_text(1.0), "1.00");
        assert_eq!(pence.price_text(12.0), "12.00");
        assert_eq!(pence.price_text(0.000525), "0.000525");
        assert_eq!(pence.price_text(0.115), "0.115");
    }

    // ---- unit_changed ------------------------------------------------------------------

    fn changed(stored: Option<&str>, new: Option<&str>, ticker: &str) -> bool {
        unit_changed(stored, &quote_unit(new, ticker), ticker)
    }

    #[test]
    fn a_ticker_never_recorded_is_compared_with_the_old_suffix_assumption() {
        // Pence stored as pounds, dollars stored as pounds: history to repair.
        assert!(changed(None, Some("GBp"), "BARC.L"));
        assert!(changed(None, Some("GBX"), "LLOY.L"));
        assert!(changed(None, Some("USD"), "CSPX.L"));
        assert!(changed(None, Some("USD"), "VWRA.L"));
        // The suffix was right: nothing to repair.
        assert!(!changed(None, Some("GBP"), "VUSA.L"));
        assert!(!changed(None, Some("GBP"), "VWRL.L"));
        assert!(!changed(None, Some("EUR"), "SXR8.DE"));
        assert!(!changed(None, Some("CZK"), "CEZ.PR"));
        assert!(!changed(None, Some("USD"), "AAPL"));
        // A response without a currency keeps the old assumption, so it cannot change anything.
        assert!(!changed(None, None, "CSPX.L"));
    }

    #[test]
    fn a_recorded_ticker_is_compared_with_the_new_code() {
        assert!(!changed(Some("GBp"), Some("GBp"), "BARC.L"));
        assert!(!changed(Some("USD"), Some("USD"), "CSPX.L"));
        assert!(!changed(Some("GBP"), Some("GBP"), "VUSA.L"));
        // Another listing currency, or pence turned into pounds: the stored rows are off.
        assert!(changed(Some("USD"), Some("GBP"), "CSPX.L"));
        assert!(changed(Some("GBp"), Some("GBP"), "BARC.L"));
        assert!(changed(Some("GBP"), Some("GBp"), "BARC.L"));
        assert!(changed(Some("EUR"), Some("USD"), "SXR8.DE"));
    }

    #[test]
    fn two_codes_of_one_unit_are_no_change() {
        assert!(!changed(Some("GBp"), Some("GBX"), "BARC.L"));
        assert!(!changed(Some("GBX"), Some("GBp"), "BARC.L"));
        assert!(!changed(Some("usd"), Some("USD"), "CSPX.L"));
        assert!(!changed(Some(" USD "), Some("USD"), "CSPX.L"));
    }

    #[test]
    fn an_empty_stored_code_counts_as_none() {
        assert!(changed(Some(""), Some("USD"), "CSPX.L"));
        assert!(!changed(Some("  "), Some("GBP"), "VUSA.L"));
    }
}
