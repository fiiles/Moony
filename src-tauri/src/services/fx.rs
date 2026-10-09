//! One conversion policy for the whole backend (ADR 0011, ADR 0012). Quotes are units of a
//! currency per 1 unit of the reference currency; the reference is an implementation detail.

use crate::error::Result;
use crate::models::fx::{FxDayQuotes, FxHistoryQuotes, FxSnapshot};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{OnceLock, RwLock};

/// Seconds in one UTC day.
pub const DAY: i64 = 86_400;
/// How many days back a historical conversion may look for a common quote day.
pub const MAX_LOOKBACK_DAYS: i64 = 10;
/// The reference currency of every stored quote: `exchange_rates` and `exchange_rate_history`
/// rows are units of a currency per 1 EUR, as the ECB publishes them (migration 010).
pub const REFERENCE: &str = "EUR";

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

/// Whether a stored quote may exist for `code`: a valid code that is neither the storage
/// [`REFERENCE`] (1.0 by definition) nor derived (resolved through its anchor at read time).
pub fn is_stored_code(code: &str) -> bool {
    normalize(code).is_some_and(|code| code != REFERENCE && !table().1.contains_key(&code))
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

    /// Quotes against the storage [`REFERENCE`] from stored `(code, units per EUR)` pairs;
    /// unusable pairs are ignored (see [`Quotes::insert`]).
    pub fn from_stored<'a>(rates: impl IntoIterator<Item = (&'a str, f64)>) -> Self {
        let mut quotes = Self::new(REFERENCE);
        for (code, units) in rates {
            quotes.insert(code, units);
        }
        quotes
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
    /// Rows are units per 1 EUR ([`REFERENCE`]), so each day becomes `Quotes::new(REFERENCE)`
    /// with `units = rate`; unusable rows are skipped.
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
                .or_insert_with(|| Quotes::new(REFERENCE));
            quotes.insert(&currency, rate);
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
        let code = from.trim();
        if !code.is_empty() && code.to_uppercase() == to.trim().to_uppercase() {
            return amount.is_finite().then_some(amount);
        }
        let from = normalize(from)?;
        let to = normalize(to)?;
        let start = floor_day(day);
        (0..=MAX_LOOKBACK_DAYS).find_map(|back| {
            let quotes = self.days.get(&start.saturating_sub(back * DAY))?;
            quotes.units(&from)?;
            quotes.units(&to)?;
            // Both quotes resolve on this day: this day decides, even when the result overflows.
            Some(quotes.convert(amount, &from, &to))
        })?
    }

    /// Like `convert_on`, recording a failed conversion in `quality` (one miss per conversion):
    /// the currencies with no quote within the lookback window are blamed, `from` when both
    /// are quoted but never on one common day.
    pub fn convert_on_tracked(
        &self,
        quality: &mut Quality,
        amount: f64,
        from: &str,
        to: &str,
        day: i64,
    ) -> Option<f64> {
        let result = self.convert_on(amount, from, to, day);
        if result.is_none() {
            let mut unresolved: Vec<&str> = [from, to]
                .into_iter()
                .filter(|code| !self.is_quoted_near(code, day))
                .collect();
            if unresolved.is_empty() {
                unresolved.push(from);
            }
            quality.record_missing_conversion(unresolved);
        }
        result
    }

    /// Whether `code` has a quote on `day` or within the lookback window before it.
    fn is_quoted_near(&self, code: &str, day: i64) -> bool {
        let Some(code) = normalize(code) else {
            return false;
        };
        // The storage reference is quoted on every stored day by definition
        if code == REFERENCE {
            return true;
        }
        let start = floor_day(day);
        (0..=MAX_LOOKBACK_DAYS).any(|back| {
            self.days
                .get(&start.saturating_sub(back * DAY))
                .is_some_and(|quotes| quotes.units(&code).is_some())
        })
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

/// The in-memory snapshot of the latest quotes and when it was fetched.
struct CurrentStore {
    quotes: Quotes,
    /// Unix seconds the quotes were fetched/saved; doubles as the snapshot revision.
    fetched_at: Option<i64>,
}

impl CurrentStore {
    fn empty() -> Self {
        Self {
            quotes: Quotes::new(REFERENCE),
            fetched_at: None,
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

/// Replaces the current snapshot (keeping its revision).
pub fn set_current(quotes: Quotes) {
    with_store(|s| {
        let mut guard = s.write().unwrap_or_else(|e| e.into_inner());
        guard.quotes = quotes;
    });
}

/// Replaces the snapshot and its fetched-at revision together.
pub fn set_current_with_revision(quotes: Quotes, fetched_at: i64) {
    with_store(|s| {
        let mut guard = s.write().unwrap_or_else(|e| e.into_inner());
        guard.quotes = quotes;
        guard.fetched_at = Some(fetched_at);
    });
}

/// Test helper: replaces the (thread-local) snapshot with these units-per-EUR quotes.
#[cfg(test)]
pub(crate) fn set_current_for_test(units_per_eur: &[(&str, f64)]) {
    set_current(Quotes::from_stored(units_per_eur.iter().copied()));
}

/// Units per 1 unit of the reference for every resolvable code (stored, derived and the
/// reference itself at 1.0), as exposed to the frontend.
pub fn snapshot_units(quotes: &Quotes) -> HashMap<String, f64> {
    quotes
        .codes()
        .into_iter()
        .filter_map(|code| {
            let units = quotes.units(&code)?;
            Some((code, units))
        })
        .collect()
}

/// The current quotes as a wire snapshot (`revision` 0 while nothing has been loaded).
pub fn snapshot() -> FxSnapshot {
    let quotes = current();
    FxSnapshot {
        reference: quotes.reference().to_string(),
        units: snapshot_units(&quotes),
        revision: current_revision(),
    }
}

/// Stored quote days within `[floor(start_day), floor(end_day)]`, oldest first, with derived
/// codes resolved, together with their reference. Days without any usable quote are skipped;
/// the lookback margin that `History::load` reads is not returned.
pub fn history_quotes(conn: &Connection, start_day: i64, end_day: i64) -> Result<FxHistoryQuotes> {
    let mut result = FxHistoryQuotes {
        reference: REFERENCE.to_string(),
        days: Vec::new(),
    };
    if start_day > end_day {
        return Ok(result);
    }
    let (first, last) = (floor_day(start_day), floor_day(end_day));
    let history = History::load(conn, start_day, end_day)?;
    result.days = history
        .days()
        .filter(|(day, quotes)| **day >= first && **day <= last && !quotes.is_empty())
        .map(|(day, quotes)| FxDayQuotes {
            day: *day,
            units: snapshot_units(quotes),
        })
        .collect();
    Ok(result)
}

/// Collects conversions that could not be made, so reports can declare what they excluded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quality {
    missing: BTreeSet<String>,
    missing_count: usize,
    estimated: bool,
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
            self.record_missing_conversion(unresolved);
        }
        result
    }

    /// Records one missing conversion that involves `codes` (each is listed as missing).
    pub fn record_missing_conversion<'a>(&mut self, codes: impl IntoIterator<Item = &'a str>) {
        for code in codes {
            self.missing
                .insert(normalize(code).unwrap_or_else(|| code.trim().to_uppercase()));
        }
        self.missing_count += 1;
    }

    pub fn merge(&mut self, other: &Quality) {
        self.missing.extend(other.missing.iter().cloned());
        self.missing_count += other.missing_count;
        self.estimated |= other.estimated;
    }

    /// Flags that some converted value used an estimated (non-historical) rate.
    pub fn mark_estimated(&mut self) {
        self.estimated = true;
    }

    pub fn is_estimated(&self) -> bool {
        self.estimated
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
    use crate::models::fx::ConversionQuality;

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
    fn stored_quotes_are_units_per_eur() {
        let q = Quotes::from_stored([("CZK", 25.0), ("USD", 1.1), ("AED", 4.0), ("EUR", 2.0)]);
        assert_eq!(q.reference(), "EUR");
        assert!((q.convert(1.0, "EUR", "USD").expect("conv") - 1.1).abs() < 1e-9);
        assert!((q.convert(1.0, "EUR", "CZK").expect("EUR") - 25.0).abs() < 1e-9);
        // AED is derived from USD (a stored AED quote is ignored), EUR is the reference.
        let aed = q.convert(1.0, "AED", "CZK").expect("AED via USD");
        assert!((aed - 25.0 / (1.1 * 3.6725)).abs() < 1e-9);
        assert_eq!(q.units("EUR"), Some(1.0));
        assert_eq!(q.convert(1.0, "CZK", "CZK"), Some(1.0));
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

    #[test]
    fn snapshot_units_lists_every_resolvable_code() {
        let mut quotes = Quotes::new("CZK");
        quotes.insert("USD", 0.05);
        let units = snapshot_units(&quotes);
        assert_eq!(units["CZK"], 1.0);
        assert_eq!(units["USD"], 0.05);
        assert!((units["AED"] - 0.05 * 3.6725).abs() < 1e-12);
        assert!(!units.contains_key("EUR"));
        assert!(snapshot_units(&Quotes::new("CZK")).len() == 1);
    }

    #[test]
    fn snapshot_reports_reference_units_and_revision() {
        assert_eq!(snapshot().revision, 0);
        let mut quotes = Quotes::new("CZK");
        quotes.insert("EUR", 0.04);
        set_current_with_revision(quotes, 1_700_000_000);
        let snap = snapshot();
        assert_eq!(snap.reference, "CZK");
        assert_eq!(snap.revision, 1_700_000_000);
        assert_eq!(snap.units["EUR"], 0.04);
        assert_eq!(snap.units["CZK"], 1.0);
    }

    #[test]
    fn history_quotes_skip_empty_days_and_the_lookback_margin() {
        let d = 20_000 * DAY;
        let conn = history_db(&[
            (d - 2 * DAY, "USD", 1.1), // before the requested range: lookback only
            (d, "USD", 0.0),           // unusable row: day ends up empty
            (d + DAY, "USD", 1.25),
            (d + DAY, "AED", 6.0), // derived codes are never stored
        ]);
        let history = history_quotes(&conn, d, d + 2 * DAY).expect("history");
        assert_eq!(history.reference, "EUR");
        let days = history.days;
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].day, d + DAY);
        assert!((days[0].units["USD"] - 1.25).abs() < 1e-12);
        assert!((days[0].units["AED"] - 1.25 * 3.6725).abs() < 1e-12);
        assert_eq!(days[0].units["EUR"], 1.0);
        let reversed = history_quotes(&conn, d + 2 * DAY, d).expect("reversed");
        assert!(reversed.days.is_empty());
        assert_eq!(reversed.reference, "EUR");
    }

    #[test]
    fn quality_estimated_flag_merges_and_maps_to_the_wire() {
        let mut quality = Quality::default();
        assert!(!quality.is_estimated());
        let mut other = Quality::default();
        other.mark_estimated();
        quality.merge(&other);
        assert!(quality.is_estimated());
        quality.track("KWD", None);
        let wire = ConversionQuality::from(&quality);
        assert_eq!(wire.missing_currencies, vec!["KWD".to_string()]);
        assert_eq!(wire.missing_count, 1);
        assert!(wire.estimated);
        assert_eq!(
            ConversionQuality::from(&Quality::default()),
            ConversionQuality::default()
        );
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
        // day d: only CZK; day d-1: CZK and USD -> USD->CZK must use d-1 for both
        let conn = history_db(&[
            (d, "CZK", 24.0),
            (d - DAY, "CZK", 25.0),
            (d - DAY, "USD", 1.25),
        ]);
        let h = History::load(&conn, d, d).expect("load");
        let got = h
            .convert_on(100.0, "USD", "CZK", d + 3600)
            .expect("converted");
        assert!((got - 2000.0).abs() < 1e-9, "{got}");
    }

    #[test]
    fn history_rows_are_units_per_eur() {
        let d = 20_000 * DAY;
        let conn = history_db(&[(d, "CZK", 25.0), (d, "USD", 1.25)]);
        let h = History::load(&conn, d, d).expect("load");
        let czk = h.convert_on(100.0, "USD", "CZK", d).expect("USD->CZK");
        assert!((czk - 2000.0).abs() < 1e-9, "{czk}");
        let eur = h.convert_on(100.0, "USD", "EUR", d).expect("USD->EUR");
        assert!((eur - 80.0).abs() < 1e-9, "{eur}");
    }

    #[test]
    fn convert_on_same_code_is_identity_even_for_non_iso_codes() {
        let h = History::default();
        assert_eq!(h.convert_on(5.0, "USDT", "usdt ", 0), Some(5.0));
        assert_eq!(h.convert_on(5.0, " eur", "EUR", 0), Some(5.0));
        assert_eq!(h.convert_on(5.0, "USDT", "USD", 0), None);
        // An empty code is no currency, not an identity
        assert_eq!(h.convert_on(5.0, "", "", 0), None);
        assert_eq!(h.convert_on(5.0, "  ", " ", 0), None);
    }

    #[test]
    fn convert_on_gives_up_after_ten_days() {
        let d = 20_000 * DAY;
        let conn = history_db(&[(d - 11 * DAY, "CZK", 25.0)]);
        let h = History::load(&conn, d - 10 * DAY, d).expect("load");
        assert_eq!(h.convert_on(1.0, "EUR", "CZK", d), None);
        assert_eq!(h.convert_on(1.0, "EUR", "CZK", d - 10 * DAY), Some(25.0));
    }

    #[test]
    fn convert_on_tracked_blames_the_currency_without_a_quote() {
        let d = 20_000 * DAY;
        let conn = history_db(&[(d, "CZK", 25.0)]);
        let h = History::load(&conn, d, d).expect("load");
        let mut quality = Quality::default();
        assert_eq!(
            h.convert_on_tracked(&mut quality, 1.0, "EUR", "CZK", d),
            Some(25.0)
        );
        assert!(quality.is_complete());
        assert_eq!(
            h.convert_on_tracked(&mut quality, 1.0, "USD", "CZK", d),
            None
        );
        assert_eq!(
            h.convert_on_tracked(&mut quality, 1.0, "USD", "JPY", d),
            None
        );
        assert_eq!(quality.missing_currencies(), vec!["JPY", "USD"]);
        assert_eq!(quality.missing_count(), 2);
    }

    #[test]
    fn convert_on_tracked_never_blames_the_storage_reference() {
        let h = History::default();
        let mut quality = Quality::default();
        assert_eq!(
            h.convert_on_tracked(&mut quality, 1.0, "USD", "EUR", 20_000 * DAY),
            None
        );
        assert_eq!(quality.missing_currencies(), vec!["USD"]);
    }

    #[test]
    fn convert_on_tracked_blames_from_when_no_common_day_exists() {
        let d = 20_000 * DAY;
        // CZK and USD are each quoted within ten days, but never on the same day
        let conn = history_db(&[(d, "CZK", 25.0), (d - 3 * DAY, "USD", 1.25)]);
        let h = History::load(&conn, d, d).expect("load");
        let mut quality = Quality::default();
        assert_eq!(
            h.convert_on_tracked(&mut quality, 1.0, "USD", "CZK", d),
            None
        );
        assert_eq!(quality.missing_currencies(), vec!["USD"]);
        assert_eq!(quality.missing_count(), 1);
    }

    #[test]
    fn convert_on_derives_pegs_and_skips_bad_rows() {
        let d = 20_000 * DAY;
        let conn = history_db(&[
            (d, "USD", 0.0),
            (d - DAY, "USD", 1.25),
            (d - DAY, "CZK", 25.0),
        ]);
        let h = History::load(&conn, d, d).expect("load");
        let aed = h.convert_on(3.6725, "AED", "CZK", d).expect("peg");
        assert!((aed - 20.0).abs() < 1e-9);
        assert_eq!(h.convert_on(5.0, "KWD", "KWD", d), Some(5.0));
    }
}
