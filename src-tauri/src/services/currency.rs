//! Currency conversion service
//!
//! Handles exchange rate caching and currency conversion
//! Base currency is CZK

use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

lazy_static! {
    /// Exchange rates relative to CZK (how many CZK = 1 unit of currency)
    /// These rates convert X CURRENCY to CZK via: amount * rate
    /// Starts with fallback rates - will be updated from database on startup, then from ECB
    static ref EXCHANGE_RATES: RwLock<HashMap<String, f64>> = {
        let mut m = HashMap::new();
        m.insert("CZK".to_string(), 1.0);
        m.insert("EUR".to_string(), 25.0);
        m.insert("USD".to_string(), 23.0);
        m.insert("GBP".to_string(), 29.0);
        m.insert("JPY".to_string(), 0.15);
        m.insert("AUD".to_string(), 14.5);
        m.insert("CAD".to_string(), 17.0);
        m.insert("CHF".to_string(), 26.0);
        m.insert("HKD".to_string(), 3.0);
        m.insert("CNY".to_string(), 3.2);
        m.insert("SEK".to_string(), 2.2);
        m.insert("NOK".to_string(), 2.1);
        m.insert("DKK".to_string(), 3.4);
        m.insert("SGD".to_string(), 17.5);
        m.insert("NZD".to_string(), 13.5);
        RwLock::new(m)
    };

    /// Timestamp when exchange rates were last fetched (Unix timestamp in seconds)
    static ref EXCHANGE_RATES_FETCHED_AT: RwLock<Option<i64>> = RwLock::new(None);

    /// Currencies currently missing an exchange rate (audit M2). Doubles as
    /// the dedup guard so the warning fires only on first occurrence. Cleared
    /// per-currency by `update_exchange_rates` when a rate arrives. Never
    /// contains CZK or known currencies.
    static ref MISSING_CURRENCIES: RwLock<HashSet<String>> = RwLock::new(HashSet::new());
}

/// Record an unknown currency and warn loudly — once per currency per session.
/// Returns the documented 1.0 last-resort rate (1 unit = 1 CZK), which is
/// almost certainly wrong; the recording makes that visible via the console
/// and `get_missing_currencies()` (surfaced in the UI through `get_price_status`).
fn record_missing_currency(currency: &str) -> f64 {
    let currency = currency.to_uppercase();
    if currency != "CZK" {
        let mut missing = MISSING_CURRENCIES
            .write()
            .expect("Missing currencies lock poisoned");
        if missing.insert(currency.clone()) {
            log::warn!(
                "[CURRENCY] WARNING: no exchange rate known for {currency}; \
                 falling back to 1 {currency} = 1 CZK — amounts in {currency} \
                 will be wrong until a rate is available"
            );
        }
    }
    1.0
}

/// Currencies currently missing an exchange rate (recorded when a conversion
/// hit the 1.0 fallback, cleared by `update_exchange_rates` once a rate
/// arrives), sorted for deterministic output. Surfaced through the
/// `get_price_status` command so the UI can show a warning badge for 1:1
/// fallback conversions.
pub fn get_missing_currencies() -> Vec<String> {
    let missing = MISSING_CURRENCIES
        .read()
        .expect("Missing currencies lock poisoned");
    let mut list: Vec<String> = missing.iter().cloned().collect();
    list.sort();
    list
}

/// Convert an amount from a currency to CZK.
/// Unknown currencies fall back to 1.0 as the documented last resort and are
/// recorded + warned about once per session (see `record_missing_currency`).
#[allow(dead_code)]
pub fn convert_to_czk(amount: f64, currency: &str) -> f64 {
    let currency = currency.to_uppercase();
    if currency == "CZK" {
        return amount;
    }

    let rate = {
        let rates = EXCHANGE_RATES.read().expect("Exchange rates lock poisoned");
        rates.get(&currency).copied()
    };
    let rate = rate.unwrap_or_else(|| record_missing_currency(&currency));
    amount * rate
}

/// Convert an amount from CZK to another currency.
/// Unknown currencies fall back to 1.0 as the documented last resort and are
/// recorded + warned about once per session (see `record_missing_currency`).
#[allow(dead_code)]
pub fn convert_from_czk(amount: f64, currency: &str) -> f64 {
    let currency = currency.to_uppercase();
    if currency == "CZK" {
        return amount;
    }

    let rate = {
        let rates = EXCHANGE_RATES.read().expect("Exchange rates lock poisoned");
        rates.get(&currency).copied()
    };
    let rate = rate.unwrap_or_else(|| record_missing_currency(&currency));
    if rate == 0.0 {
        return amount;
    }
    amount / rate
}

/// Convert an amount from one currency to another (via CZK as intermediary)
/// Example: convert_between(100.0, "EUR", "USD") converts 100 EUR to USD
#[allow(dead_code)]
pub fn convert_between(amount: f64, from_currency: &str, to_currency: &str) -> f64 {
    let from = from_currency.to_uppercase();
    let to = to_currency.to_uppercase();

    // If same currency, no conversion needed
    if from == to {
        return amount;
    }

    // Convert to CZK first, then to target currency
    let amount_in_czk = convert_to_czk(amount, &from);
    convert_from_czk(amount_in_czk, &to)
}

/// Update exchange rates in memory (called after fetching from ECB or loading from DB).
/// Currencies that now have a rate are cleared from the missing set, so
/// `get_missing_currencies()` reflects current state; if a currency later goes
/// missing again it is re-recorded (and warned about again).
pub fn update_exchange_rates(new_rates: HashMap<String, f64>) {
    {
        let mut rates = EXCHANGE_RATES
            .write()
            .expect("Exchange rates lock poisoned");
        for (currency, rate) in &new_rates {
            rates.insert(currency.clone(), *rate);
        }
    }

    // Rates inserted first, missing set cleared second: a concurrent conversion
    // between the two steps finds the new rate and cannot re-record the currency.
    let mut missing = MISSING_CURRENCIES
        .write()
        .expect("Missing currencies lock poisoned");
    for currency in new_rates.keys() {
        missing.remove(&currency.to_uppercase());
    }
}

/// Update the fetched_at timestamp
pub fn set_exchange_rates_fetched_at(timestamp: i64) {
    let mut fetched_at = EXCHANGE_RATES_FETCHED_AT
        .write()
        .expect("Exchange rates fetched_at lock poisoned");
    *fetched_at = Some(timestamp);
}

/// Get the timestamp when exchange rates were last fetched
pub fn get_exchange_rates_fetched_at() -> Option<i64> {
    let fetched_at = EXCHANGE_RATES_FETCHED_AT
        .read()
        .expect("Exchange rates fetched_at lock poisoned");
    *fetched_at
}

/// Get current exchange rate for a currency (to CZK).
/// Unknown currencies fall back to 1.0 as the documented last resort and are
/// recorded + warned about once per session (see `record_missing_currency`).
#[allow(dead_code)]
pub fn get_exchange_rate(currency: &str) -> f64 {
    let currency = currency.to_uppercase();
    let rate = {
        let rates = EXCHANGE_RATES.read().expect("Exchange rates lock poisoned");
        rates.get(&currency).copied()
    };
    rate.unwrap_or_else(|| record_missing_currency(&currency))
}

/// Get all current exchange rates
pub fn get_all_rates() -> HashMap<String, f64> {
    let rates = EXCHANGE_RATES.read().expect("Exchange rates lock poisoned");
    rates.clone()
}

/// Load exchange rates from database on startup
/// This ensures we have rates available even when offline
pub fn load_rates_from_db(conn: &rusqlite::Connection) -> crate::error::Result<()> {
    // Check if exchange_rates table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='exchange_rates'",
            [],
            |row| row.get::<_, i32>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);

    if !table_exists {
        log::info!("[CURRENCY] exchange_rates table does not exist yet, skipping load");
        return Ok(());
    }

    let mut stmt = conn.prepare("SELECT currency, rate, fetched_at FROM exchange_rates")?;
    let mut rates = HashMap::new();
    let mut oldest_fetched_at: Option<i64> = None;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    for row in rows.flatten() {
        let (currency, rate, fetched_at) = row;
        rates.insert(currency, rate);

        // Track the most recent fetch time
        match oldest_fetched_at {
            None => oldest_fetched_at = Some(fetched_at),
            Some(existing) => {
                if fetched_at > existing {
                    oldest_fetched_at = Some(fetched_at);
                }
            }
        }
    }

    if !rates.is_empty() {
        log::info!(
            "[CURRENCY] Loaded {} exchange rates from database",
            rates.len()
        );
        update_exchange_rates(rates);

        if let Some(ts) = oldest_fetched_at {
            set_exchange_rates_fetched_at(ts);
        }
    } else {
        log::info!("[CURRENCY] No exchange rates in database, will fetch from ECB");
    }

    Ok(())
}

/// Save exchange rates to database for offline use
pub fn save_rates_to_db(
    conn: &rusqlite::Connection,
    rates: &HashMap<String, f64>,
) -> crate::error::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;

    for (currency, rate) in rates {
        conn.execute(
            "INSERT INTO exchange_rates (currency, rate, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(currency) DO UPDATE SET rate = ?2, fetched_at = ?3",
            rusqlite::params![currency, rate, now],
        )?;
    }

    log::info!(
        "[CURRENCY] Saved {} exchange rates to database",
        rates.len()
    );
    set_exchange_rates_fetched_at(now);

    Ok(())
}

/// The ECB daily reference rates: rates relative to CZK plus the day they were
/// published *for* (the `time` attribute of the feed). On weekends and
/// holidays the feed keeps serving the last business day, so the reference day
/// is not the day the app fetched it.
#[derive(Debug, Clone)]
pub struct EcbRates {
    /// Currency -> CZK per 1 unit, including `EUR` and `CZK` (= 1.0).
    pub rates: HashMap<String, f64>,
    /// Midnight UTC of the ECB reference date.
    pub reference_day: i64,
}

/// Fetch exchange rates from the ECB feed (rates relative to CZK).
pub async fn fetch_ecb_rates() -> crate::error::Result<EcbRates> {
    let url = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";

    let response = crate::services::http::client().get(url).send().await?;
    let body = response.text().await?;
    let ecb = parse_ecb_xml(&body)?;

    // Update global rates
    update_exchange_rates(ecb.rates.clone());

    // Update fetched_at timestamp
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;
    set_exchange_rates_fetched_at(now);

    Ok(ecb)
}

/// Parse the ECB daily XML (`<Cube time='2026-10-02'>` with
/// `<Cube currency='USD' rate='1.0843'/>` rows, all quoted per 1 EUR) into CZK
/// based rates and the reference day.
///
/// Fails — instead of guessing — when the feed has no usable CZK quote (every
/// other rate is converted through it; the old fallback of 25.0 silently
/// mispriced everything) or no parseable reference date.
pub fn parse_ecb_xml(body: &str) -> crate::error::Result<EcbRates> {
    use crate::error::AppError;

    let reference_day = extract_reference_day(body).ok_or_else(|| {
        AppError::ExternalApi("ECB feed has no usable reference date (time attribute)".into())
    })?;

    // CZK per 1 EUR
    let czk_per_eur = extract_rate(body, "CZK")
        .filter(|r| r.is_finite() && *r > 0.0)
        .ok_or_else(|| AppError::ExternalApi("ECB feed has no usable CZK rate".into()))?;

    let mut rates = HashMap::new();
    rates.insert("CZK".to_string(), 1.0);
    rates.insert("EUR".to_string(), czk_per_eur);

    for currency in extract_all_currencies(body) {
        if currency == "CZK" {
            continue;
        }
        if let Some(rate_vs_eur) = extract_rate(body, &currency) {
            if rate_vs_eur.is_finite() && rate_vs_eur > 0.0 {
                // rate_vs_eur units of the currency per 1 EUR = czk_per_eur CZK,
                // so 1 unit = czk_per_eur / rate_vs_eur CZK
                rates.insert(currency, czk_per_eur / rate_vs_eur);
            }
        }
    }

    Ok(EcbRates {
        rates,
        reference_day,
    })
}

/// The `time='YYYY-MM-DD'` reference date of the ECB feed, as midnight UTC.
fn extract_reference_day(xml: &str) -> Option<i64> {
    let after = &xml[xml.find("time=")? + "time=".len()..];
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &after[1..];
    let value = &value[..value.find(quote)?];
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp())
}

/// Extract all currency codes from ECB XML response
fn extract_all_currencies(xml: &str) -> Vec<String> {
    let mut currencies = Vec::new();

    // Find all occurrences of currency="XXX" or currency='XXX'
    let mut search_start = 0;
    while let Some(pos) = xml[search_start..].find("currency=") {
        let abs_pos = search_start + pos;
        let after = &xml[abs_pos + 9..]; // Skip "currency="

        // Determine quote character
        let quote_char = after.chars().next();
        if let Some(q) = quote_char {
            if q == '"' || q == '\'' {
                let after_quote = &after[1..]; // Skip opening quote
                if let Some(end_pos) = after_quote.find(q) {
                    let currency = after_quote[..end_pos].to_string();
                    if currency.len() == 3
                        && currency.chars().all(|c| c.is_ascii_uppercase())
                        && !currencies.contains(&currency)
                    {
                        currencies.push(currency);
                    }
                }
            }
        }
        search_start = abs_pos + 10; // Move past this occurrence
    }

    currencies
}

/// Extract rate from ECB XML response
fn extract_rate(xml: &str, currency: &str) -> Option<f64> {
    // Try double quotes first: currency="USD"
    let mut pos = xml.find(&format!("currency=\"{}\"", currency));

    // If not found, try single quotes: currency='USD'
    if pos.is_none() {
        pos = xml.find(&format!("currency='{}'", currency));
    }

    if let Some(p) = pos {
        let after = &xml[p..];

        // Try double quotes for rate: rate="1.23"
        // Check finding it relatively close to the currency to avoid reading a different line
        // But the XML structure <Cube currency='...' rate='...'/> is consistent
        if let Some(rate_start) = after.find("rate=\"") {
            // Ensure this rate belongs to this currency (is close)
            // A simple check is that it should be within < 50 chars usually
            if rate_start < 50 {
                let rate_str = &after[rate_start + 6..];
                if let Some(rate_end) = rate_str.find('"') {
                    let rate_value = &rate_str[..rate_end];
                    return rate_value.parse().ok();
                }
            }
        }

        // Try single quotes for rate: rate='1.23'
        if let Some(rate_start) = after.find("rate='") {
            if rate_start < 50 {
                let rate_str = &after[rate_start + 6..];
                if let Some(rate_end) = rate_str.find('\'') {
                    let rate_value = &rate_str[..rate_end];
                    return rate_value.parse().ok();
                }
            }
        }
    }
    None
}

/// Save rates into the exchange_rate_history table (one row per currency per
/// day), under `reference_day` — the ECB reference date of the rates, *not* the
/// day the app fetched them: on a Saturday the feed still carries Friday's
/// rates, which belong to Friday. Overwrites an existing row for that
/// day, so a refresh after the ECB's afternoon publication replaces the earlier
/// fetch of the same day.
pub fn save_rates_to_history_db(
    conn: &rusqlite::Connection,
    rates: &HashMap<String, f64>,
    reference_day: i64,
) -> crate::error::Result<()> {
    // Normalise to midnight UTC
    let day = (reference_day / 86400) * 86400;

    for (currency, rate) in rates {
        if currency == "CZK" {
            continue; // CZK is always 1.0 — no need to store
        }
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(date, currency) DO UPDATE SET rate = ?3",
            rusqlite::params![day, currency, rate],
        )?;
    }

    Ok(())
}

/// Return rates for a specific date (Unix timestamp, normalised to midnight UTC).
/// Falls back to the nearest earlier date that has a snapshot (up to 10 days back).
/// Falls back to today's in-memory rates if no history exists at all.
pub fn get_rates_for_date(conn: &rusqlite::Connection, date: i64) -> HashMap<String, f64> {
    let day = (date / 86400) * 86400;

    // Try exact date first, then walk backwards up to 10 days (covers weekends + holidays)
    let found_rows: Vec<(String, f64)> = (0..=10i64)
        .find_map(|offset| {
            let target = day - offset * 86400;
            let result: rusqlite::Result<Vec<(String, f64)>> = conn
                .prepare("SELECT currency, rate FROM exchange_rate_history WHERE date = ?1")
                .and_then(|mut stmt| {
                    stmt.query_map([target], |row| Ok((row.get(0)?, row.get(1)?)))
                        .map(|rows| rows.filter_map(|r| r.ok()).collect())
                });
            match result {
                Ok(rows) if !rows.is_empty() => Some(rows),
                _ => None,
            }
        })
        .unwrap_or_default();

    if found_rows.is_empty() {
        // No history at all — fall back to current in-memory rates
        return get_all_rates();
    }

    let mut map = HashMap::new();
    map.insert("CZK".to_string(), 1.0);
    for (currency, rate) in found_rows {
        map.insert(currency, rate);
    }
    map
}

/// Return rates for all dates in a range that have history snapshots.
/// Keys are midnight-normalised Unix timestamps.
pub fn get_rates_for_date_range(
    conn: &rusqlite::Connection,
    start_date: i64,
    end_date: i64,
) -> HashMap<i64, HashMap<String, f64>> {
    let start = (start_date / 86400) * 86400;
    let end = (end_date / 86400) * 86400;

    let query_result: rusqlite::Result<Vec<(i64, String, f64)>> = conn
        .prepare(
            "SELECT date, currency, rate FROM exchange_rate_history
             WHERE date >= ?1 AND date <= ?2
             ORDER BY date",
        )
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![start, end], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        });

    let mut result: HashMap<i64, HashMap<String, f64>> = HashMap::new();

    for (date, currency, rate) in query_result.unwrap_or_default() {
        let entry = result.entry(date).or_insert_with(|| {
            let mut m = HashMap::new();
            m.insert("CZK".to_string(), 1.0);
            m
        });
        entry.insert(currency, rate);
    }

    result
}

/// Convert an amount to CZK using a prefetched rates map (e.g. one day's snapshot from
/// `get_rates_for_date`). Falls back to the current in-memory rate when `currency` is
/// missing from the map, and to 1.0 as the last resort — mirroring `convert_to_czk`'s
/// fallback semantics. The 1.0 last resort (via `get_exchange_rate`) records the
/// currency and warns once per session (see `record_missing_currency`).
pub fn convert_to_czk_with_rates(amount: f64, currency: &str, rates: &HashMap<String, f64>) -> f64 {
    let currency = currency.to_uppercase();
    if currency == "CZK" {
        return amount;
    }

    let rate = rates
        .get(&currency)
        .copied()
        .unwrap_or_else(|| get_exchange_rate(&currency));
    amount * rate
}

/// Convert an amount to CZK using a specific day's exchange rate.
/// Reads `exchange_rate_history` for the day (closest date ≤ day, walking back up to 10
/// days to cover weekends/holidays), falling back to the current in-memory rates when no
/// history exists for that day at all. This is the composition of `get_rates_for_date` +
/// `convert_to_czk_with_rates`.
///
/// Note: this queries the database on every call. Callers converting several amounts for
/// the same day should call `get_rates_for_date` once and reuse `convert_to_czk_with_rates`
/// instead.
pub fn convert_to_czk_at(
    conn: &rusqlite::Connection,
    amount: f64,
    currency: &str,
    day_ts: i64,
) -> f64 {
    let rates = get_rates_for_date(conn, day_ts);
    convert_to_czk_with_rates(amount, currency, &rates)
}

/// Maximum relative difference tolerated between a per-currency breakdown's CZK
/// equivalent and the CZK total stored alongside it. Rate drift between the two
/// sources is a few percent at most; a larger gap means the breakdown does not
/// cover the whole asset class (e.g. it was derived from incomplete per-ticker
/// history). Mirrors BREAKDOWN_CONSISTENCY_TOLERANCE in src/utils/historical-rates.ts.
pub const BREAKDOWN_CONSISTENCY_TOLERANCE: f64 = 0.25;

/// Parse a `*_by_currency` JSON column ({"USD": 123.4, ...}) into a map.
/// Invalid JSON, non-object values, and non-numeric entries yield an empty
/// map / are skipped — mirroring the frontend's parseBreakdown.
pub fn breakdown_from_json(json: &str) -> HashMap<String, f64> {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let Some(object) = parsed.as_object() else {
        return HashMap::new();
    };
    object
        .iter()
        .filter_map(|(currency, value)| {
            value
                .as_f64()
                .filter(|v| v.is_finite())
                .map(|v| (currency.clone(), v))
        })
        .collect()
}

/// CZK equivalent of a breakdown at the given day's rates, or None when any
/// bucket currency lacks a usable (finite, positive) rate — in which case the
/// breakdown cannot be valued like-for-like against a stored CZK total.
pub fn breakdown_czk_at_rates(
    breakdown: &HashMap<String, f64>,
    rates: &HashMap<String, f64>,
) -> Option<f64> {
    let mut sum = 0.0;
    for (currency, amount) in breakdown {
        let rate = rates.get(currency).copied()?;
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        sum += amount * rate;
    }
    Some(sum)
}

/// True when a breakdown's CZK sum is close enough to the stored CZK total for
/// the breakdown to represent the whole asset class (see the tolerance above).
pub fn breakdown_covers_total(czk_sum: f64, czk_total: f64) -> bool {
    if czk_total <= 0.0 {
        return czk_sum.abs() <= f64::EPSILON;
    }
    ((czk_sum - czk_total) / czk_total).abs() <= BREAKDOWN_CONSISTENCY_TOLERANCE
}

/// Repair policy for a stored `*_by_currency` column checked against its CZK
/// total: an empty breakdown with a non-zero total needs repair, a valuable
/// breakdown that does not cover the total needs repair, and a breakdown that
/// cannot be valued at the day's rates is kept (nothing better is available).
pub fn needs_breakdown_repair(
    json: &str,
    czk_total: f64,
    rates: Option<&HashMap<String, f64>>,
) -> bool {
    let breakdown = breakdown_from_json(json);
    if breakdown.is_empty() {
        return czk_total != 0.0;
    }
    match rates.and_then(|r| breakdown_czk_at_rates(&breakdown, r)) {
        Some(czk_sum) => !breakdown_covers_total(czk_sum, czk_total),
        None => false,
    }
}

/// Resolve rates for a day from a prefetched `get_rates_for_date_range` map:
/// the exact day, else the closest earlier day within a 10-day walk-back
/// (weekends + holidays) — the in-memory mirror of `get_rates_for_date`.
pub fn resolve_rates_for_day_from_range(
    rates_by_day: &HashMap<i64, HashMap<String, f64>>,
    day: i64,
) -> Option<&HashMap<String, f64>> {
    let day = (day / 86400) * 86400;
    (0..=10i64).find_map(|offset| rates_by_day.get(&(day - offset * 86400)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_to_czk() {
        // CZK to CZK should be identity
        let czk = convert_to_czk(100.0, "CZK");
        assert_eq!(czk, 100.0);
    }

    #[test]
    fn test_currency_case_sensitivity() {
        // Case sensitivity test - both should work the same
        let val_upper = convert_to_czk(100.0, "CZK");
        let val_lower = convert_to_czk(100.0, "czk");

        assert_eq!(
            val_upper, val_lower,
            "Currency conversion should be case insensitive"
        );
    }

    #[test]
    fn test_convert_between_same_currency() {
        assert_eq!(convert_between(100.0, "EUR", "EUR"), 100.0);
    }

    #[test]
    fn test_convert_between_via_czk() {
        update_exchange_rates(
            [("EUR".to_string(), 25.0), ("USD".to_string(), 23.0)]
                .into_iter()
                .collect(),
        );
        let result = convert_between(100.0, "EUR", "USD");
        let expected = 100.0 * 25.0 / 23.0;
        assert!((result - expected).abs() < 0.01);
    }

    #[test]
    fn test_convert_to_czk_unknown_currency_fallback() {
        let result = convert_to_czk(42.0, "XYZ");
        assert_eq!(result, 42.0);
    }

    #[test]
    fn test_convert_to_czk_case_insensitive() {
        update_exchange_rates([("EUR".to_string(), 25.0)].into_iter().collect());
        let upper = convert_to_czk(10.0, "EUR");
        let lower = convert_to_czk(10.0, "eur");
        assert!((upper - lower).abs() < 0.001);
    }

    fn setup_history_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );",
        )
        .expect("schema");
        conn
    }

    #[test]
    fn test_save_and_get_rates_for_date_exact() {
        let conn = setup_history_db();
        let mut rates = HashMap::new();
        rates.insert("EUR".to_string(), 25.0);
        rates.insert("USD".to_string(), 23.0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let today = (now / 86400) * 86400;
        save_rates_to_history_db(&conn, &rates, today).expect("save");

        let loaded = get_rates_for_date(&conn, today);
        assert!((loaded["EUR"] - 25.0).abs() < 0.001);
        assert!((loaded["USD"] - 23.0).abs() < 0.001);
        assert_eq!(loaded["CZK"], 1.0);
    }

    #[test]
    fn test_get_rates_for_date_falls_back_to_nearest_earlier() {
        let conn = setup_history_db();
        // Insert rates for 5 days ago
        let five_days_ago = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            / 86400
            - 5)
            * 86400;
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 24.5)",
            [five_days_ago],
        )
        .expect("insert");

        // Query for 2 days ago (no exact match) — should fall back to 5-days-ago row
        let two_days_ago = five_days_ago + 3 * 86400;
        let loaded = get_rates_for_date(&conn, two_days_ago);
        assert!((loaded["EUR"] - 24.5).abs() < 0.001);
    }

    #[test]
    fn test_get_rates_for_date_falls_back_to_memory_when_no_history() {
        let conn = setup_history_db(); // empty table
        update_exchange_rates([("EUR".to_string(), 27.0)].into_iter().collect());
        // Any date — table is empty, should return in-memory rates
        let loaded = get_rates_for_date(&conn, 1_700_000_000);
        assert!((loaded["EUR"] - 27.0).abs() < 0.001);
    }

    #[test]
    fn test_get_rates_for_date_range() {
        let conn = setup_history_db();
        let base: i64 = 1_700_000_000 / 86400 * 86400;
        conn.execute_batch(&format!(
            "INSERT INTO exchange_rate_history VALUES ({}, 'EUR', 25.0);
             INSERT INTO exchange_rate_history VALUES ({}, 'EUR', 25.5);",
            base,
            base + 86400,
        ))
        .expect("insert");

        let result = get_rates_for_date_range(&conn, base, base + 86400);
        assert_eq!(result.len(), 2);
        assert!((result[&base]["EUR"] - 25.0).abs() < 0.001);
        assert!((result[&(base + 86400)]["EUR"] - 25.5).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_at_uses_exact_day_rate() {
        let conn = setup_history_db();
        let day: i64 = 1_700_000_000 / 86400 * 86400;
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 24.0)",
            [day],
        )
        .expect("insert");

        let result = convert_to_czk_at(&conn, 10.0, "EUR", day);
        assert!((result - 240.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_at_falls_back_to_closest_earlier_day() {
        let conn = setup_history_db();
        let day: i64 = 1_700_000_000 / 86400 * 86400;
        let earlier_day = day - 3 * 86400;
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 24.0)",
            [earlier_day],
        )
        .expect("insert");

        // Query on `day` — no exact-day row, should fall back to the row 3 days earlier.
        let result = convert_to_czk_at(&conn, 10.0, "EUR", day);
        assert!((result - 240.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_at_falls_back_to_current_rates_when_no_history() {
        let conn = setup_history_db(); // empty table, no history at all
        update_exchange_rates([("NOK".to_string(), 21.0)].into_iter().collect());

        let result = convert_to_czk_at(&conn, 5.0, "NOK", 1_700_000_000);
        assert!((result - 105.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_at_czk_identity() {
        let conn = setup_history_db();
        let result = convert_to_czk_at(&conn, 500.0, "CZK", 1_700_000_000);
        assert_eq!(result, 500.0);
    }

    #[test]
    fn test_convert_to_czk_at_missing_currency_in_days_map_falls_back_to_current_rate() {
        let conn = setup_history_db();
        let day: i64 = 1_700_000_000 / 86400 * 86400;
        // The day has history, but only for EUR — NOK is absent from that day's map.
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 24.0)",
            [day],
        )
        .expect("insert");
        update_exchange_rates([("NOK".to_string(), 21.0)].into_iter().collect());

        let result = convert_to_czk_at(&conn, 5.0, "NOK", day);
        assert!((result - 105.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_at_case_insensitive() {
        let conn = setup_history_db();
        let day: i64 = 1_700_000_000 / 86400 * 86400;
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 24.0)",
            [day],
        )
        .expect("insert");

        let upper = convert_to_czk_at(&conn, 10.0, "EUR", day);
        let lower = convert_to_czk_at(&conn, 10.0, "eur", day);
        assert!((upper - lower).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_with_rates_uses_provided_map() {
        let rates: HashMap<String, f64> = [("EUR".to_string(), 24.0)].into_iter().collect();
        let result = convert_to_czk_with_rates(10.0, "EUR", &rates);
        assert!((result - 240.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_with_rates_missing_currency_falls_back_to_current_rate() {
        update_exchange_rates([("NOK".to_string(), 21.0)].into_iter().collect());
        let rates: HashMap<String, f64> = [("EUR".to_string(), 24.0)].into_iter().collect();

        let result = convert_to_czk_with_rates(5.0, "NOK", &rates);
        assert!((result - 105.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_czk_with_rates_czk_identity() {
        let rates: HashMap<String, f64> = HashMap::new();
        let result = convert_to_czk_with_rates(100.0, "CZK", &rates);
        assert_eq!(result, 100.0);
    }

    // ------------------------------------------------------------------
    // Unknown-currency recording (audit M2). Tests run in parallel and the
    // missing-currency set is global, so each test uses unique fake codes.
    // ------------------------------------------------------------------

    #[test]
    fn test_unknown_currency_is_recorded_as_missing() {
        let _ = convert_to_czk(10.0, "ZZA1");
        assert!(get_missing_currencies().contains(&"ZZA1".to_string()));
    }

    #[test]
    fn test_known_currency_is_not_recorded_as_missing() {
        update_exchange_rates([("ZZB2".to_string(), 5.0)].into_iter().collect());
        let result = convert_to_czk(10.0, "ZZB2");
        assert!((result - 50.0).abs() < 0.001);
        assert!(!get_missing_currencies().contains(&"ZZB2".to_string()));
    }

    #[test]
    fn test_czk_is_never_recorded_as_missing() {
        let _ = convert_to_czk(10.0, "CZK");
        let _ = convert_from_czk(10.0, "czk");
        let _ = get_exchange_rate("CZK");
        assert!(!get_missing_currencies().contains(&"CZK".to_string()));
    }

    #[test]
    fn test_unknown_currency_recorded_once_per_session() {
        let _ = convert_to_czk(1.0, "ZZC3");
        let _ = convert_to_czk(2.0, "ZZC3");
        let _ = convert_from_czk(3.0, "ZZC3");
        let count = get_missing_currencies()
            .iter()
            .filter(|c| c.as_str() == "ZZC3")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_unknown_currency_conversion_still_falls_back_to_1_to_1() {
        assert_eq!(convert_to_czk(42.0, "ZZD4"), 42.0);
        assert_eq!(convert_from_czk(42.0, "ZZD4"), 42.0);
    }

    #[test]
    fn test_convert_from_czk_unknown_currency_is_recorded() {
        let _ = convert_from_czk(10.0, "ZZE5");
        assert!(get_missing_currencies().contains(&"ZZE5".to_string()));
    }

    #[test]
    fn test_get_exchange_rate_unknown_currency_is_recorded() {
        let rate = get_exchange_rate("ZZF6");
        assert_eq!(rate, 1.0);
        assert!(get_missing_currencies().contains(&"ZZF6".to_string()));
    }

    #[test]
    fn test_get_exchange_rate_lowercase_known_currency_not_recorded() {
        update_exchange_rates([("ZZG7".to_string(), 4.0)].into_iter().collect());
        let rate = get_exchange_rate("zzg7");
        assert!((rate - 4.0).abs() < 0.001);
        assert!(!get_missing_currencies().contains(&"ZZG7".to_string()));
    }

    #[test]
    fn test_missing_currency_recorded_uppercase_for_lowercase_input() {
        let _ = convert_to_czk(1.0, "zzh8");
        let missing = get_missing_currencies();
        assert!(missing.contains(&"ZZH8".to_string()));
        assert!(!missing.contains(&"zzh8".to_string()));
    }

    #[test]
    fn test_convert_to_czk_with_rates_fully_unknown_is_recorded() {
        let rates: HashMap<String, f64> = HashMap::new();
        let result = convert_to_czk_with_rates(7.0, "ZZJ9", &rates);
        assert_eq!(result, 7.0);
        assert!(get_missing_currencies().contains(&"ZZJ9".to_string()));
    }

    #[test]
    fn test_update_exchange_rates_clears_resolved_missing_currency() {
        // Record a fake currency as missing via the 1.0 fallback path
        let _ = convert_to_czk(10.0, "ZZK0");
        assert!(get_missing_currencies().contains(&"ZZK0".to_string()));

        // A rate arrives (e.g. ECB fetch) — the currency is no longer missing
        update_exchange_rates([("ZZK0".to_string(), 3.0)].into_iter().collect());
        assert!(!get_missing_currencies().contains(&"ZZK0".to_string()));

        // Conversion now uses the real rate
        let result = convert_to_czk(10.0, "ZZK0");
        assert!((result - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_get_missing_currencies_is_sorted() {
        let _ = convert_to_czk(1.0, "ZZZB");
        let _ = convert_to_czk(1.0, "ZZZA");
        let missing = get_missing_currencies();
        let mut sorted = missing.clone();
        sorted.sort();
        assert_eq!(missing, sorted);
    }

    #[test]
    fn test_breakdown_from_json_parses_valid_map() {
        let bd = breakdown_from_json("{\"USD\":100.5,\"CZK\":200}");
        assert_eq!(bd.len(), 2);
        assert!((bd["USD"] - 100.5).abs() < 0.001);
        assert!((bd["CZK"] - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_breakdown_from_json_invalid_or_empty_yields_empty_map() {
        assert!(breakdown_from_json("{}").is_empty());
        assert!(breakdown_from_json("not json").is_empty());
        assert!(breakdown_from_json("").is_empty());
        assert!(breakdown_from_json("[1,2]").is_empty());
    }

    #[test]
    fn test_breakdown_from_json_skips_non_numeric_entries() {
        let bd = breakdown_from_json("{\"USD\":\"bad\",\"EUR\":10}");
        assert_eq!(bd.len(), 1);
        assert!((bd["EUR"] - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_breakdown_czk_at_rates_sums_all_buckets() {
        let bd: HashMap<String, f64> = [("USD".to_string(), 100.0), ("CZK".to_string(), 500.0)]
            .into_iter()
            .collect();
        let rates: HashMap<String, f64> = [("CZK".to_string(), 1.0), ("USD".to_string(), 20.0)]
            .into_iter()
            .collect();
        let czk = breakdown_czk_at_rates(&bd, &rates).expect("all rates present");
        assert!((czk - 2500.0).abs() < 0.001);
    }

    #[test]
    fn test_breakdown_czk_at_rates_missing_or_unusable_rate_yields_none() {
        let bd: HashMap<String, f64> = [("GBP".to_string(), 10.0)].into_iter().collect();
        let rates: HashMap<String, f64> = [("CZK".to_string(), 1.0)].into_iter().collect();
        assert!(breakdown_czk_at_rates(&bd, &rates).is_none());

        let zero_rate: HashMap<String, f64> = [("GBP".to_string(), 0.0)].into_iter().collect();
        assert!(breakdown_czk_at_rates(&bd, &zero_rate).is_none());
    }

    #[test]
    fn test_breakdown_covers_total_within_tolerance() {
        assert!(breakdown_covers_total(2000.0, 2100.0)); // ~5% off
        assert!(!breakdown_covers_total(2000.0, 10000.0)); // fragment
        assert!(!breakdown_covers_total(0.0, 10000.0)); // nothing derived
    }

    #[test]
    fn test_breakdown_covers_total_zero_total_requires_zero_sum() {
        assert!(breakdown_covers_total(0.0, 0.0));
        assert!(!breakdown_covers_total(500.0, 0.0));
    }

    #[test]
    fn test_needs_breakdown_repair_policy() {
        let rates: HashMap<String, f64> = [("CZK".to_string(), 1.0), ("USD".to_string(), 20.0)]
            .into_iter()
            .collect();

        // Empty breakdown with a non-zero total needs repair; with a zero total it does not.
        assert!(needs_breakdown_repair("{}", 1000.0, Some(&rates)));
        assert!(!needs_breakdown_repair("{}", 0.0, Some(&rates)));

        // Consistent breakdown is kept.
        assert!(!needs_breakdown_repair(
            "{\"USD\":100}",
            2000.0,
            Some(&rates)
        ));

        // Fragment breakdown (covers 20% of the total) needs repair.
        assert!(needs_breakdown_repair(
            "{\"USD\":100}",
            10000.0,
            Some(&rates)
        ));

        // Unverifiable breakdowns are kept: no day rates, or a bucket currency
        // missing from the day map.
        assert!(!needs_breakdown_repair("{\"USD\":100}", 10000.0, None));
        assert!(!needs_breakdown_repair(
            "{\"GBP\":100}",
            10000.0,
            Some(&rates)
        ));
    }

    #[test]
    fn test_resolve_rates_for_day_from_range_exact_then_walk_back() {
        let day = 86400 * 100;
        let mut by_day: HashMap<i64, HashMap<String, f64>> = HashMap::new();
        by_day.insert(day, [("USD".to_string(), 20.0)].into_iter().collect());
        by_day.insert(
            day - 3 * 86400,
            [("USD".to_string(), 21.0)].into_iter().collect(),
        );

        // Exact day wins
        let exact = resolve_rates_for_day_from_range(&by_day, day).expect("exact");
        assert!((exact["USD"] - 20.0).abs() < 0.001);

        // Missing day walks back to the closest earlier snapshot (weekends/holidays)
        let walked = resolve_rates_for_day_from_range(&by_day, day - 86400).expect("walk back");
        assert!((walked["USD"] - 21.0).abs() < 0.001);

        // Beyond the 10-day walk-back window there is no match
        assert!(resolve_rates_for_day_from_range(&by_day, day + 20 * 86400).is_none());
    }
    // ---- ECB reference date and the CZK quote ----

    /// The ECB daily feed as published; `time` is the reference date.
    fn ecb_xml(time: &str, czk: Option<&str>) -> String {
        let czk_row = czk
            .map(|r| format!("<Cube currency='CZK' rate='{r}'/>"))
            .unwrap_or_default();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<gesmes:Envelope xmlns:gesmes=\"http://www.gesmes.org/xml/2002-08-01\" xmlns=\"http://www.ecb.int/vocabulary/2002-08-01/eurofxref\">
  <gesmes:subject>Reference rates</gesmes:subject>
  <Cube>
    <Cube time='{time}'>
      <Cube currency='USD' rate='1.2500'/>
      {czk_row}
      <Cube currency='GBP' rate='0.8000'/>
    </Cube>
  </Cube>
</gesmes:Envelope>"
        )
    }

    // 2026-10-02 is a Friday; the feed served on Saturday still says Friday.
    const FRIDAY: i64 = 1_790_899_200;
    const SATURDAY: i64 = FRIDAY + 86400;

    #[test]
    fn test_parse_ecb_xml_reads_the_reference_date_and_rates() {
        let ecb = parse_ecb_xml(&ecb_xml("2026-10-02", Some("25.000"))).expect("parses");
        assert_eq!(ecb.reference_day, FRIDAY);
        assert_eq!(ecb.rates["CZK"], 1.0);
        assert!((ecb.rates["EUR"] - 25.0).abs() < 1e-9);
        assert!((ecb.rates["USD"] - 20.0).abs() < 1e-9); // 25 CZK/EUR / 1.25 USD/EUR
        assert!((ecb.rates["GBP"] - 31.25).abs() < 1e-9);
    }

    #[test]
    fn test_parse_ecb_xml_without_a_czk_quote_is_an_error_not_25() {
        let err = parse_ecb_xml(&ecb_xml("2026-10-02", None)).unwrap_err();
        assert!(err.to_string().contains("CZK"), "{err}");
    }

    #[test]
    fn test_parse_ecb_xml_without_a_reference_date_is_an_error() {
        let xml = ecb_xml("2026-10-02", Some("25.0")).replace("time='2026-10-02'", "");
        let err = parse_ecb_xml(&xml).unwrap_err();
        assert!(err.to_string().contains("reference date"), "{err}");
        let bad = ecb_xml("not-a-date", Some("25.0"));
        assert!(parse_ecb_xml(&bad).is_err());
    }

    #[test]
    fn test_history_is_stored_under_the_ecb_reference_day() {
        let conn = setup_history_db();
        let ecb = parse_ecb_xml(&ecb_xml("2026-10-02", Some("25.000"))).unwrap();
        // The app fetched on Saturday, but the rates are Friday's.
        save_rates_to_history_db(&conn, &ecb.rates, ecb.reference_day).unwrap();

        let days: Vec<i64> = conn
            .prepare("SELECT DISTINCT date FROM exchange_rate_history")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(days, vec![FRIDAY], "no row under the fetch day (Saturday)");

        // A Saturday lookup walks back to Friday's rates; a later Frankfurter
        // INSERT OR IGNORE for Friday keeps the ECB row.
        let rates = get_rates_for_date(&conn, SATURDAY);
        assert!((rates["EUR"] - 25.0).abs() < 1e-9);
        conn.execute(
            "INSERT OR IGNORE INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 99.0)",
            [FRIDAY],
        )
        .unwrap();
        assert!((get_rates_for_date(&conn, FRIDAY)["EUR"] - 25.0).abs() < 1e-9);
    }

    #[test]
    fn test_history_save_normalises_the_day_to_midnight() {
        let conn = setup_history_db();
        let rates: HashMap<String, f64> = [("EUR".to_string(), 25.0)].into_iter().collect();
        save_rates_to_history_db(&conn, &rates, FRIDAY + 3600).unwrap();
        let day: i64 = conn
            .query_row("SELECT date FROM exchange_rate_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(day, FRIDAY);
    }
}
