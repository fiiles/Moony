//! One conversion policy for the whole backend (ADR 0011, ADR 0012). Quotes are units of a
//! currency per 1 unit of the reference currency; the reference is an implementation detail.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

/// Seconds in one UTC day.
pub const DAY: i64 = 86_400;
/// How many days back a historical conversion may look for a common quote day.
pub const MAX_LOOKBACK_DAYS: i64 = 10;

type SupportedTable = (Vec<String>, HashMap<String, (String, f64)>);

fn table() -> &'static SupportedTable {
    static TABLE: OnceLock<SupportedTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../../../shared/fx-supported.json"))
                .expect("fx-supported.json is embedded and valid");
        let source = raw["source"]
            .as_array()
            .expect("fx-supported.json is embedded and valid")
            .iter()
            .map(|v| {
                v.as_str()
                    .expect("fx-supported.json is embedded and valid")
                    .to_string()
            })
            .collect();
        let derived = raw["derived"]
            .as_object()
            .expect("fx-supported.json is embedded and valid")
            .iter()
            .map(|(code, v)| {
                let anchor = v["anchor"]
                    .as_str()
                    .expect("fx-supported.json is embedded and valid")
                    .to_string();
                let peg = v["unitsPerAnchor"]
                    .as_f64()
                    .expect("fx-supported.json is embedded and valid");
                (code.clone(), (anchor, peg))
            })
            .collect();
        (source, derived)
    })
}

/// Trim + uppercase; `Some` only for exactly three ASCII letters.
pub fn normalize(code: &str) -> Option<String> {
    let code = code.trim().to_ascii_uppercase();
    (code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())).then_some(code)
}

/// Codes that have a published source quote.
pub fn source_codes() -> &'static [String] {
    &table().0
}

/// `(anchor, units per anchor)` for a currency pegged to another one.
pub fn derived(code: &str) -> Option<(&'static str, f64)> {
    let code = normalize(code)?;
    table()
        .1
        .get(&code)
        .map(|(anchor, peg)| (anchor.as_str(), *peg))
}

/// Source codes plus derived (pegged) codes.
pub fn is_supported(code: &str) -> bool {
    match normalize(code) {
        Some(code) => source_codes().contains(&code) || table().1.contains_key(&code),
        None => false,
    }
}

/// Every supported code, sorted.
pub fn supported_codes() -> Vec<String> {
    let mut codes: Vec<String> = source_codes()
        .iter()
        .cloned()
        .chain(table().1.keys().cloned())
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

/// Reference-agnostic quotes: units of each currency per 1 unit of the reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Quotes {
    reference: String,
    units: HashMap<String, f64>,
}

impl Quotes {
    pub fn new(reference: &str) -> Self {
        Self {
            reference: normalize(reference).unwrap_or_else(|| reference.trim().to_uppercase()),
            units: HashMap::new(),
        }
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Stores a source quote. Invalid codes, non-finite or non-positive values, derived codes
    /// and the reference itself are ignored.
    pub fn insert(&mut self, code: &str, units_per_reference: f64) {
        let Some(code) = normalize(code) else {
            return;
        };
        if code == self.reference
            || table().1.contains_key(&code)
            || !units_per_reference.is_finite()
            || units_per_reference <= 0.0
        {
            return;
        }
        self.units.insert(code, units_per_reference);
    }

    /// Units of `code` per 1 unit of the reference (reference -> 1.0, derived -> anchor * peg).
    pub fn units(&self, code: &str) -> Option<f64> {
        let code = normalize(code)?;
        if code == self.reference {
            Some(1.0)
        } else if let Some((anchor, peg)) = derived(&code) {
            self.units(anchor).map(|a| a * peg)
        } else {
            self.units.get(&code).copied()
        }
    }

    /// Converts `amount` between currencies; `None` when a quote is missing or the result is
    /// not finite. Same currency is identity and needs no quote.
    pub fn convert(&self, amount: f64, from: &str, to: &str) -> Option<f64> {
        if from.trim().to_uppercase() == to.trim().to_uppercase() {
            return amount.is_finite().then_some(amount);
        }
        let from_units = self.units(from)?;
        let to_units = self.units(to)?;
        let converted = amount * to_units / from_units;
        converted.is_finite().then_some(converted)
    }

    /// True when there is no quote besides the reference.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Stored plus resolvable derived codes, sorted, including the reference.
    pub fn codes(&self) -> Vec<String> {
        let mut codes: Vec<String> = self.units.keys().cloned().collect();
        codes.push(self.reference.clone());
        codes.extend(
            table()
                .1
                .keys()
                .filter(|code| self.units(code).is_some())
                .cloned(),
        );
        codes.sort();
        codes.dedup();
        codes
    }

    /// Phase-1 adapter: reference CZK, `units = 1 / rate` for a CZK-per-unit map.
    pub fn from_czk_per_unit(rates: &HashMap<String, f64>) -> Self {
        let mut quotes = Self::new("CZK");
        for (code, rate) in rates {
            if rate.is_finite() && *rate > 0.0 {
                quotes.insert(code, 1.0 / rate);
            }
        }
        quotes
    }

    /// CZK per 1 unit of every resolvable code (incl. derived and CZK itself). Empty when CZK
    /// cannot be resolved.
    pub fn to_czk_per_unit(&self) -> HashMap<String, f64> {
        let Some(czk_units) = self.units("CZK") else {
            return HashMap::new();
        };
        self.codes()
            .into_iter()
            .filter_map(|code| {
                let units = self.units(&code)?;
                Some((code, czk_units / units))
            })
            .collect()
    }
}

/// Collects conversions that could not be made, so reports can declare what they excluded.
#[derive(Debug, Clone, Default)]
pub struct Quality {
    missing: BTreeSet<String>,
    missing_count: usize,
}

impl Quality {
    /// Passes `converted` through; records `from` as missing when it is `None`.
    pub fn track(&mut self, from: &str, converted: Option<f64>) -> Option<f64> {
        if converted.is_none() {
            let code = normalize(from).unwrap_or_else(|| from.trim().to_uppercase());
            self.missing.insert(code);
            self.missing_count += 1;
        }
        converted
    }

    pub fn merge(&mut self, other: &Quality) {
        self.missing.extend(other.missing.iter().cloned());
        self.missing_count += other.missing_count;
    }

    pub fn missing_currencies(&self) -> Vec<String> {
        self.missing.iter().cloned().collect()
    }

    pub fn missing_count(&self) -> usize {
        self.missing_count
    }

    pub fn is_complete(&self) -> bool {
        self.missing_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur_quotes() -> Quotes {
        let mut q = Quotes::new("EUR");
        q.insert("CZK", 25.0);
        q.insert("USD", 1.1);
        q.insert("JPY", 160.0);
        q
    }

    #[test]
    fn shared_vectors_hold() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../../../shared/fx-test-vectors.json"))
                .expect("json");
        let mut q = Quotes::new(raw["quotes"]["reference"].as_str().expect("ref"));
        for (code, units) in raw["quotes"]["units"].as_object().expect("units") {
            q.insert(code, units.as_f64().expect("f64"));
        }
        for case in raw["cases"].as_array().expect("cases") {
            let got = q.convert(
                case["amount"].as_f64().expect("amount"),
                case["from"].as_str().expect("from"),
                case["to"].as_str().expect("to"),
            );
            match case["expected"].as_f64() {
                Some(want) => {
                    let got = got.unwrap_or_else(|| panic!("case {case} returned None"));
                    assert!((got - want).abs() < 1e-9, "case {case}: {got}");
                }
                None => assert_eq!(got, None, "case {case}"),
            }
        }
    }

    #[test]
    fn invalid_stored_quotes_are_ignored() {
        let mut q = Quotes::new("EUR");
        q.insert("USD", 0.0);
        q.insert("GBP", f64::NAN);
        q.insert("AED", 4.0); // derived codes are never stored
        q.insert("usd!", 1.0);
        assert_eq!(q.units("USD"), None);
        assert_eq!(q.units("GBP"), None);
        assert_eq!(q.units("AED"), None); // no USD quote -> no AED
        assert!(q.is_empty());
    }

    #[test]
    fn czk_adapter_round_trips() {
        let czk_per_unit = HashMap::from([
            ("EUR".to_string(), 25.0),
            ("USD".to_string(), 22.727272727272727),
        ]);
        let q = Quotes::from_czk_per_unit(&czk_per_unit);
        assert_eq!(q.reference(), "CZK");
        assert!((q.convert(1.0, "EUR", "USD").expect("conv") - 1.1).abs() < 1e-9);
        let back = q.to_czk_per_unit();
        assert!((back["EUR"] - 25.0).abs() < 1e-9);
        assert!((back["AED"] - 22.727272727272727 / 3.6725).abs() < 1e-9);
        assert_eq!(back["CZK"], 1.0);
    }

    #[test]
    fn quality_tracks_missing_once_per_currency() {
        let q = eur_quotes();
        let mut quality = Quality::default();
        assert_eq!(quality.track("KWD", q.convert(1.0, "KWD", "EUR")), None);
        assert_eq!(quality.track("KWD", q.convert(2.0, "KWD", "EUR")), None);
        assert!(quality.track("USD", q.convert(1.1, "USD", "EUR")).is_some());
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
        assert_eq!(quality.missing_count(), 2);
        assert!(!quality.is_complete());
    }

    #[test]
    fn supported_set_matches_shared_table() {
        assert_eq!(supported_codes().len(), 36);
        assert!(is_supported("aed"));
        assert!(!is_supported("KWD"));
        assert_eq!(derived("BGN"), Some(("EUR", 1.95583)));
    }
}
