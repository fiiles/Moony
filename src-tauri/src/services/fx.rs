//! One conversion policy for the whole backend (ADR 0011, ADR 0012). Quotes are units of a
//! currency per 1 unit of the reference currency; the reference is an implementation detail.

use crate::error::Result;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{OnceLock, RwLock};

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

/// Day-keyed quotes (UTC midnight keys). Historical conversions use one common day for both
/// currencies, looking back at most `MAX_LOOKBACK_DAYS`; separate last-known days are never mixed.
#[derive(Debug, Clone, Default)]
pub struct History {
    days: BTreeMap<i64, Quotes>,
}

impl History {
    /// Loads `exchange_rate_history` for `[floor(start_day) - MAX_LOOKBACK_DAYS * DAY,
    /// floor(end_day)]`; `convert_on` is supported for days in `[start_day, end_day]`.
    /// Phase-1 storage: rows are CZK per unit, so each day becomes `Quotes::new("CZK")` with
    /// `units = 1 / rate`; unusable rows are skipped.
    pub fn load(conn: &Connection, start_day: i64, end_day: i64) -> Result<History> {
        let lower = floor_day(start_day).saturating_sub(MAX_LOOKBACK_DAYS * DAY);
        let upper = floor_day(end_day);
        let mut stmt = conn.prepare(
            "SELECT date, currency, rate FROM exchange_rate_history WHERE date >= ?1 AND date <= ?2",
        )?;
        let mut rows = stmt.query(params![lower, upper])?;
        let mut days: BTreeMap<i64, Quotes> = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let date: i64 = row.get(0)?;
            let currency: String = row.get(1)?;
            let rate: f64 = row.get(2)?;
            let quotes = days
                .entry(floor_day(date))
                .or_insert_with(|| Quotes::new("CZK"));
            if rate.is_finite() && rate > 0.0 {
                quotes.insert(&currency, 1.0 / rate);
            }
        }
        Ok(History { days })
    }

    /// For tests and adapters.
    pub fn from_days(days: BTreeMap<i64, Quotes>) -> History {
        History { days }
    }

    /// Walks back day by day from `floor(day)` for at most `MAX_LOOKBACK_DAYS` and uses the
    /// first day on which both currencies resolve. Same currency is identity without lookup.
    pub fn convert_on(&self, amount: f64, from: &str, to: &str, day: i64) -> Option<f64> {
        let from = normalize(from)?;
        let to = normalize(to)?;
        if from == to {
            return amount.is_finite().then_some(amount);
        }
        let start = floor_day(day);
        (0..=MAX_LOOKBACK_DAYS).find_map(|back| {
            let quotes = self.days.get(&start.saturating_sub(back * DAY))?;
            quotes.units(&from)?;
            quotes.units(&to)?;
            // Both quotes resolve on this day: this day decides, even when the result overflows.
            Some(quotes.convert(amount, &from, &to))
        })?
    }

    /// Quotes of exactly this stored day.
    pub fn quotes_on(&self, day: i64) -> Option<&Quotes> {
        self.days.get(&day)
    }

    pub fn days(&self) -> impl Iterator<Item = (&i64, &Quotes)> {
        self.days.iter()
    }
}

fn floor_day(ts: i64) -> i64 {
    ts.div_euclid(DAY) * DAY
}

/// The in-memory snapshot of the latest quotes, when it was fetched, and the currencies that
/// failed a current conversion since it was last replaced.
struct CurrentStore {
    quotes: Quotes,
    /// Unix seconds the quotes were fetched/saved; doubles as the snapshot revision.
    fetched_at: Option<i64>,
    missing: BTreeSet<String>,
}

impl CurrentStore {
    fn empty() -> Self {
        Self {
            quotes: Quotes::new("CZK"),
            fetched_at: None,
            missing: BTreeSet::new(),
        }
    }
}

/// Production: one process-wide store.
#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&RwLock<CurrentStore>) -> R) -> R {
    static STORE: OnceLock<RwLock<CurrentStore>> = OnceLock::new();
    f(STORE.get_or_init(|| RwLock::new(CurrentStore::empty())))
}

/// Tests: cargo runs every test on its own thread, so a thread-local store isolates tests
/// from each other without any locking.
#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&RwLock<CurrentStore>) -> R) -> R {
    thread_local! {
        static STORE: RwLock<CurrentStore> = RwLock::new(CurrentStore::empty());
    }
    STORE.with(f)
}

/// Clone of the current quote snapshot. Empty (reference only) until rates are loaded.
pub fn current() -> Quotes {
    with_store(|s| s.read().unwrap_or_else(|e| e.into_inner()).quotes.clone())
}

/// Unix seconds the current snapshot was fetched; `0` while no rates have been loaded.
pub fn current_revision() -> i64 {
    current_fetched_at().unwrap_or(0)
}

/// Unix seconds the current snapshot was fetched, if known.
pub fn current_fetched_at() -> Option<i64> {
    with_store(|s| s.read().unwrap_or_else(|e| e.into_inner()).fetched_at)
}

/// Replaces the current snapshot (keeping its revision) and forgets previously recorded
/// missing currencies.
pub fn set_current(quotes: Quotes) {
    with_store(|s| {
        let mut guard = s.write().unwrap_or_else(|e| e.into_inner());
        guard.quotes = quotes;
        guard.missing.clear();
    });
}

/// Replaces the snapshot and its fetched-at revision together.
pub fn set_current_with_revision(quotes: Quotes, fetched_at: i64) {
    with_store(|s| {
        let mut guard = s.write().unwrap_or_else(|e| e.into_inner());
        guard.quotes = quotes;
        guard.fetched_at = Some(fetched_at);
        guard.missing.clear();
    });
}

/// Codes that failed a current conversion since the last `set_current`, sorted.
pub fn missing_now() -> Vec<String> {
    with_store(|s| {
        s.read()
            .unwrap_or_else(|e| e.into_inner())
            .missing
            .iter()
            .cloned()
            .collect()
    })
}

/// Records a currency that could not be converted with the current snapshot (warns once).
pub fn record_missing(code: &str) {
    let code = normalize(code).unwrap_or_else(|| code.trim().to_uppercase());
    with_store(|s| {
        let mut guard = s.write().unwrap_or_else(|e| e.into_inner());
        if guard.missing.insert(code.clone()) {
            log::warn!("[FX] no current exchange rate for {code}; amounts in {code} are excluded");
        }
    });
}

/// Records every currency of `quality` as missing in the current snapshot (for conversions
/// that were made elsewhere, e.g. cost basis, and must still reach `get_price_status`).
pub fn record_missing_all(quality: &Quality) {
    for code in quality.missing_currencies() {
        record_missing(&code);
    }
}

/// Test helper: replaces the (thread-local) snapshot with these CZK-per-unit rates.
#[cfg(test)]
pub(crate) fn set_current_for_test(czk_per_unit: &[(&str, f64)]) {
    let rates: HashMap<String, f64> = czk_per_unit
        .iter()
        .map(|(code, rate)| (code.to_string(), *rate))
        .collect();
    set_current(Quotes::from_czk_per_unit(&rates));
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

    /// Passes the `result` of converting `from` into `to` through; on `None` records every
    /// currency the quotes cannot resolve (both, or just the unquoted side), counting one
    /// missing conversion. When both quotes exist (e.g. a non-finite amount) `from` is blamed.
    pub fn track_conversion(
        &mut self,
        quotes: &Quotes,
        from: &str,
        to: &str,
        result: Option<f64>,
    ) -> Option<f64> {
        if result.is_none() {
            let mut unresolved: Vec<&str> = [from, to]
                .into_iter()
                .filter(|code| quotes.units(code).is_none())
                .collect();
            if unresolved.is_empty() {
                unresolved.push(from);
            }
            for code in unresolved {
                self.missing
                    .insert(normalize(code).unwrap_or_else(|| code.trim().to_uppercase()));
            }
            self.missing_count += 1;
        }
        result
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
    fn track_conversion_blames_the_unquoted_sides() {
        let q = eur_quotes();
        let mut quality = Quality::default();
        // only the target is unquoted
        assert_eq!(
            quality.track_conversion(&q, "USD", "KWD", q.convert(1.0, "USD", "KWD")),
            None
        );
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
        // only the source is unquoted
        quality.track_conversion(&q, "ZZA", "EUR", q.convert(1.0, "ZZA", "EUR"));
        // both unquoted: one conversion, two currencies
        quality.track_conversion(&q, "ZZB", "KWD", q.convert(1.0, "ZZB", "KWD"));
        assert_eq!(
            quality.missing_currencies(),
            vec!["KWD".to_string(), "ZZA".to_string(), "ZZB".to_string()]
        );
        assert_eq!(quality.missing_count(), 3);
        // success passes through untouched; both quoted but not finite blames the source
        assert!(quality
            .track_conversion(&q, "USD", "EUR", q.convert(1.1, "USD", "EUR"))
            .is_some());
        assert_eq!(quality.missing_count(), 3);
        quality.track_conversion(&q, "JPY", "EUR", q.convert(f64::NAN, "JPY", "EUR"));
        assert!(quality.missing_currencies().contains(&"JPY".to_string()));
        assert_eq!(quality.missing_count(), 4);
    }

    #[test]
    fn current_store_sets_quotes_and_revision_together() {
        assert_eq!(current_revision(), 0);
        assert_eq!(current_fetched_at(), None);
        let mut q = Quotes::new("CZK");
        q.insert("EUR", 0.04);
        set_current_with_revision(q, 1_700_000_000);
        assert_eq!(current_revision(), 1_700_000_000);
        // plain set_current keeps the revision of the data it replaces
        set_current(Quotes::new("CZK"));
        assert_eq!(current_fetched_at(), Some(1_700_000_000));
    }

    #[test]
    fn supported_set_matches_shared_table() {
        assert_eq!(supported_codes().len(), 36);
        assert!(is_supported("aed"));
        assert!(!is_supported("KWD"));
        assert_eq!(derived("BGN"), Some(("EUR", 1.95583)));
    }

    fn history_db(rows: &[(i64, &str, f64)]) -> Connection {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL, PRIMARY KEY (date, currency));",
        ).expect("schema");
        for (date, currency, rate) in rows {
            conn.execute(
                "INSERT INTO exchange_rate_history VALUES (?1, ?2, ?3)",
                rusqlite::params![date, currency, rate],
            )
            .expect("row");
        }
        conn
    }

    #[test]
    fn convert_on_uses_a_day_that_quotes_both_currencies() {
        let d = 20_000 * DAY;
        // day d: only EUR; day d-1: EUR and USD -> USD->EUR must use d-1 for both
        let conn = history_db(&[
            (d, "EUR", 24.0),
            (d - DAY, "EUR", 25.0),
            (d - DAY, "USD", 20.0),
        ]);
        let h = History::load(&conn, d, d).expect("load");
        let got = h
            .convert_on(100.0, "USD", "EUR", d + 3600)
            .expect("converted");
        assert!((got - 80.0).abs() < 1e-9, "{got}");
    }

    #[test]
    fn convert_on_gives_up_after_ten_days() {
        let d = 20_000 * DAY;
        let conn = history_db(&[(d - 11 * DAY, "EUR", 25.0)]);
        let h = History::load(&conn, d - 10 * DAY, d).expect("load");
        assert_eq!(h.convert_on(1.0, "EUR", "CZK", d), None);
        assert_eq!(h.convert_on(1.0, "EUR", "CZK", d - 10 * DAY), Some(25.0));
    }

    #[test]
    fn convert_on_derives_pegs_and_skips_bad_rows() {
        let d = 20_000 * DAY;
        let conn = history_db(&[(d, "USD", 0.0), (d - DAY, "USD", 20.0)]);
        let h = History::load(&conn, d, d).expect("load");
        let aed = h.convert_on(3.6725, "AED", "CZK", d).expect("peg");
        assert!((aed - 20.0).abs() < 1e-9);
        assert_eq!(h.convert_on(5.0, "KWD", "KWD", d), Some(5.0));
    }
}
