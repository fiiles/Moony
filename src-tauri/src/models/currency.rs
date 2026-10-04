//! ISO 4217 currency codes.
//!
//! Only the alphabetic codes of currencies in circulation: no fund codes
//! (CHE, CLF, …), metals (XAU, XAG, …), SDR (XDR), test (XTS) or "no currency"
//! (XXX) codes, because none of those can hold a bank balance.

/// Alphabetic codes of the currencies in circulation, sorted (binary search).
const ISO_4217_CODES: [&str; 157] = [
    "AED", "AFN", "ALL", "AMD", "ANG", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT",
    "BGN", "BHD", "BIF", "BMD", "BND", "BOB", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD", "CAD",
    "CDF", "CHF", "CLP", "CNY", "COP", "CRC", "CUP", "CVE", "CZK", "DJF", "DKK", "DOP", "DZD",
    "EGP", "ERN", "ETB", "EUR", "FJD", "FKP", "GBP", "GEL", "GHS", "GIP", "GMD", "GNF", "GTQ",
    "GYD", "HKD", "HNL", "HTG", "HUF", "IDR", "ILS", "INR", "IQD", "IRR", "ISK", "JMD", "JOD",
    "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP", "LKR",
    "LRD", "LSL", "LYD", "MAD", "MDL", "MGA", "MKD", "MMK", "MNT", "MOP", "MRU", "MUR", "MVR",
    "MWK", "MXN", "MYR", "MZN", "NAD", "NGN", "NIO", "NOK", "NPR", "NZD", "OMR", "PAB", "PEN",
    "PGK", "PHP", "PKR", "PLN", "PYG", "QAR", "RON", "RSD", "RUB", "RWF", "SAR", "SBD", "SCR",
    "SDG", "SEK", "SGD", "SHP", "SLE", "SOS", "SRD", "SSP", "STN", "SVC", "SYP", "SZL", "THB",
    "TJS", "TMT", "TND", "TOP", "TRY", "TTD", "TWD", "TZS", "UAH", "UGX", "USD", "UYU", "UZS",
    "VED", "VES", "VND", "VUV", "WST", "XAF", "XCD", "XCG", "XOF", "XPF", "YER", "ZAR", "ZMW",
    "ZWG",
];

/// Whether `code` is the ISO 4217 alphabetic code of a circulating currency.
/// Tolerant of case and surrounding whitespace ("eur " is EUR).
pub fn is_iso_4217(code: &str) -> bool {
    let code = code.trim();
    if code.len() != 3 {
        return false;
    }
    let upper = code.to_ascii_uppercase();
    ISO_4217_CODES.binary_search(&upper.as_str()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_common_codes() {
        for code in [
            "USD", "EUR", "CZK", "JPY", "GBP", "CHF", "XAF", "XOF", "XPF",
        ] {
            assert!(is_iso_4217(code), "{code}");
        }
    }

    #[test]
    fn is_tolerant_of_case_and_whitespace() {
        assert!(is_iso_4217("eur"));
        assert!(is_iso_4217(" Czk "));
        assert!(is_iso_4217("\tusd\n"));
    }

    #[test]
    fn rejects_unknown_well_formed_codes() {
        for code in ["XYZ", "ABC", "AAA", "ZZZ", "EUU"] {
            assert!(!is_iso_4217(code), "{code}");
        }
    }

    #[test]
    fn rejects_funds_metals_and_special_codes() {
        for code in [
            "XAU", "XAG", "XPT", "XPD", "XDR", "XTS", "XXX", "CHE", "CHW", "CLF",
        ] {
            assert!(!is_iso_4217(code), "{code}");
        }
    }

    #[test]
    fn rejects_withdrawn_currencies() {
        // Replaced long ago: HRK (euro 2023), ZWL (ZWG 2024), CUC, VEF, MRO, STD, SLL.
        for code in ["HRK", "ZWL", "CUC", "VEF", "MRO", "STD", "SLL"] {
            assert!(!is_iso_4217(code), "{code}");
        }
    }

    #[test]
    fn rejects_malformed_input() {
        for code in ["", " ", "US", "USDD", "U$D", "12E", "€", "ČKK", "EU R"] {
            assert!(!is_iso_4217(code), "{code:?}");
        }
    }

    #[test]
    fn table_is_sorted_unique_and_well_formed() {
        // The binary search relies on this.
        for pair in ISO_4217_CODES.windows(2) {
            assert!(
                pair[0] < pair[1],
                "{} must sort before {}",
                pair[0],
                pair[1]
            );
        }
        for code in ISO_4217_CODES {
            assert!(
                code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()),
                "{code}"
            );
        }
        assert!(
            ISO_4217_CODES.len() > 150,
            "about 160 circulating currencies"
        );
    }

    #[test]
    fn covers_every_currency_the_app_supports() {
        // Mirrors CURRENCY_CODES in shared/currencies.ts: a currency the UI offers must
        // never be rejected by the CSV import.
        let app_currencies = [
            "AED", "ARS", "AUD", "BGN", "BHD", "BRL", "CAD", "CHF", "CLP", "CNY", "COP", "CZK",
            "DKK", "EGP", "EUR", "GBP", "HKD", "HUF", "IDR", "ILS", "INR", "ISK", "JPY", "KRW",
            "KWD", "MXN", "MYR", "NOK", "NZD", "OMR", "PHP", "PLN", "QAR", "RON", "RSD", "SAR",
            "SEK", "SGD", "THB", "TRY", "TWD", "UAH", "USD", "VND", "ZAR",
        ];
        for code in app_currencies {
            assert!(is_iso_4217(code), "{code}");
        }
    }
}
