//! Currency conversion service
//!
//! Handles exchange rate caching and currency conversion
//! Base currency is CZK

use crate::services::fx::{self, Quotes};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::RwLock;

lazy_static! {
    /// Timestamp when exchange rates were last fetched (Unix timestamp in seconds)
    static ref EXCHANGE_RATES_FETCHED_AT: RwLock<Option<i64>> = RwLock::new(None);
}

/// Phase-1 shim: converts to CZK with the current snapshot. A currency without a quote is
/// excluded (0.0) and recorded as missing; it is never valued 1:1.
pub fn convert_to_czk(amount: f64, currency: &str) -> f64 {
    match fx::current().convert(amount, currency, "CZK") {
        Some(converted) => converted,
        None => {
            fx::record_missing(currency);
            0.0
        }
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

/// Phase-1 shim: CZK per 1 unit of every currency the current snapshot resolves
/// (derived codes included). Contains only `CZK` until rates are loaded.
pub fn get_all_rates() -> HashMap<String, f64> {
    fx::current().to_czk_per_unit()
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
    let mut latest_fetched_at: Option<i64> = None;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    for row in rows.flatten() {
        let (currency, rate, fetched_at) = row;
        if !rate.is_finite() || rate <= 0.0 {
            log::warn!("[CURRENCY] ignoring unusable stored rate for {currency}");
            continue;
        }
        rates.insert(currency, rate);
        latest_fetched_at = latest_fetched_at.max(Some(fetched_at));
    }

    if !rates.is_empty() {
        log::info!(
            "[CURRENCY] Loaded {} exchange rates from database",
            rates.len()
        );
        fx::set_current(Quotes::from_czk_per_unit(&rates));

        if let Some(ts) = latest_fetched_at {
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
    fx::set_current(Quotes::from_czk_per_unit(&ecb.rates));

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

/// Phase-1 shim: CZK-per-unit rates (derived codes included) of the closest day on or before
/// `date` that has stored quotes, looking back at most `fx::MAX_LOOKBACK_DAYS`. Empty when no
/// such day exists; current rates are never substituted.
pub fn get_rates_for_date(conn: &rusqlite::Connection, date: i64) -> HashMap<String, f64> {
    let day = date.div_euclid(fx::DAY) * fx::DAY;
    let Ok(history) = fx::History::load(conn, day, day) else {
        return HashMap::new();
    };
    (0..=fx::MAX_LOOKBACK_DAYS)
        .find_map(|back| {
            history
                .quotes_on(day - back * fx::DAY)
                .filter(|quotes| !quotes.is_empty())
        })
        .map(Quotes::to_czk_per_unit)
        .unwrap_or_default()
}

/// Phase-1 shim: CZK-per-unit rates for every stored day in `[start_date, end_date]`
/// (midnight-normalised keys; days without usable quotes are omitted).
pub fn get_rates_for_date_range(
    conn: &rusqlite::Connection,
    start_date: i64,
    end_date: i64,
) -> HashMap<i64, HashMap<String, f64>> {
    let start = start_date.div_euclid(fx::DAY) * fx::DAY;
    let end = end_date.div_euclid(fx::DAY) * fx::DAY;
    let Ok(history) = fx::History::load(conn, start, end) else {
        return HashMap::new();
    };
    history
        .days()
        .filter(|(day, quotes)| **day >= start && !quotes.is_empty())
        .map(|(day, quotes)| (*day, quotes.to_czk_per_unit()))
        .collect()
}

/// Phase-1 shim: converts to CZK using a prefetched CZK-per-unit map (e.g. one day's snapshot
/// from `get_rates_for_date`). A currency absent from the map (or with an unusable rate) is
/// excluded (0.0) and recorded as missing; neither current rates nor 1:1 are substituted.
pub fn convert_to_czk_with_rates(amount: f64, currency: &str, rates: &HashMap<String, f64>) -> f64 {
    let currency = currency.trim().to_uppercase();
    if currency == "CZK" {
        return amount;
    }
    match rates
        .get(&currency)
        .copied()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
    {
        Some(rate) => amount * rate,
        None => {
            fx::record_missing(&currency);
            0.0
        }
    }
}

/// Phase-1 shim: converts to CZK at the rates of `day_ts` (closest earlier stored day, at most
/// ten days back). Without quotes for that currency the amount is excluded (0.0) and recorded
/// as missing.
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
    let day = day.div_euclid(fx::DAY) * fx::DAY;
    (0..=fx::MAX_LOOKBACK_DAYS).find_map(|offset| rates_by_day.get(&(day - offset * fx::DAY)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn set_store(czk_per_unit: &[(&str, f64)]) {
        fx::set_current(Quotes::from_czk_per_unit(
            &czk_per_unit
                .iter()
                .map(|(code, rate)| (code.to_string(), *rate))
                .collect(),
        ));
    }

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
    fn unknown_currency_is_excluded_and_reported_not_one_to_one() {
        let _fx = fx::test_lock();
        fx::set_current(Quotes::from_czk_per_unit(&HashMap::from([(
            "EUR".to_string(),
            25.0,
        )])));
        assert_eq!(convert_to_czk(100.0, "KWD"), 0.0);
        assert!(fx::missing_now().contains(&"KWD".to_string()));
        assert_eq!(convert_to_czk(2.0, "EUR"), 50.0);
    }

    #[test]
    fn store_starts_without_invented_rates() {
        let _fx = fx::test_lock();
        fx::set_current(Quotes::new("CZK"));
        assert_eq!(convert_to_czk(1.0, "EUR"), 0.0);
        assert!(!get_all_rates().contains_key("EUR"));
    }

    #[test]
    fn rates_loaded_from_db_are_validated() {
        let _fx = fx::test_lock();
        let conn = Connection::open_in_memory().expect("db");
        conn.execute_batch("CREATE TABLE exchange_rates (currency TEXT PRIMARY KEY, rate REAL NOT NULL, fetched_at INTEGER NOT NULL);
            INSERT INTO exchange_rates VALUES ('EUR', 25.0, 1), ('USD', 0.0, 1), ('GBP', -3.0, 1);").expect("schema");
        load_rates_from_db(&conn).expect("load");
        let rates = get_all_rates();
        assert_eq!(rates.get("EUR"), Some(&25.0));
        assert!(!rates.contains_key("USD") && !rates.contains_key("GBP"));
    }

    #[test]
    fn set_current_resets_the_missing_set() {
        let _fx = fx::test_lock();
        set_store(&[("EUR", 25.0)]);
        assert_eq!(convert_to_czk(1.0, "ZZA"), 0.0);
        assert!(fx::missing_now().contains(&"ZZA".to_string()));
        set_store(&[("EUR", 25.0)]);
        assert!(fx::missing_now().is_empty());
    }

    #[test]
    fn missing_currency_is_recorded_uppercase_and_sorted_without_duplicates() {
        let _fx = fx::test_lock();
        set_store(&[("EUR", 25.0)]);
        let _ = convert_to_czk(1.0, "zzb");
        let _ = convert_to_czk(1.0, "ZZB");
        let _ = convert_to_czk(1.0, "ZZA");
        assert_eq!(
            fx::missing_now(),
            vec!["ZZA".to_string(), "ZZB".to_string()]
        );
    }

    #[test]
    fn czk_is_never_recorded_as_missing_and_known_currencies_convert() {
        let _fx = fx::test_lock();
        set_store(&[("EUR", 25.0)]);
        assert_eq!(convert_to_czk(10.0, "czk"), 10.0);
        assert!((convert_to_czk(10.0, "eur") - 250.0).abs() < 1e-9);
        assert!(fx::missing_now().is_empty());
    }

    #[test]
    fn derived_currencies_resolve_through_their_anchor() {
        let _fx = fx::test_lock();
        set_store(&[("USD", 20.0)]);
        assert!((convert_to_czk(3.6725, "AED") - 20.0).abs() < 1e-9);
        assert!(get_all_rates().contains_key("AED"));
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
        // Derived pegs are part of the day's rates.
        assert!((loaded["AED"] - 23.0 / 3.6725).abs() < 1e-6);
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
    fn get_rates_for_date_is_empty_without_history_never_current_rates() {
        let _fx = fx::test_lock();
        let conn = setup_history_db(); // empty table
        set_store(&[("EUR", 27.0)]);
        assert!(get_rates_for_date(&conn, 1_700_000_000).is_empty());
    }

    #[test]
    fn get_rates_for_date_gives_up_after_ten_days() {
        let conn = setup_history_db();
        let day: i64 = 1_700_000_000 / 86400 * 86400;
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1, 'EUR', 25.0)",
            [day - 11 * 86400],
        )
        .expect("insert");
        assert!(get_rates_for_date(&conn, day).is_empty());
        assert!(!get_rates_for_date(&conn, day - 86400).is_empty());
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
    fn get_rates_for_date_range_only_returns_days_inside_the_range() {
        let conn = setup_history_db();
        let base: i64 = 1_700_000_000 / 86400 * 86400;
        conn.execute_batch(&format!(
            "INSERT INTO exchange_rate_history VALUES ({}, 'EUR', 25.0);
             INSERT INTO exchange_rate_history VALUES ({}, 'EUR', 25.5);",
            base - 86400,
            base,
        ))
        .expect("insert");
        let result = get_rates_for_date_range(&conn, base, base);
        assert_eq!(result.keys().copied().collect::<Vec<_>>(), vec![base]);
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
    fn convert_to_czk_at_without_history_is_excluded_and_reported_not_current() {
        let _fx = fx::test_lock();
        let conn = setup_history_db(); // empty table, no history at all
        set_store(&[("NOK", 21.0)]);

        let result = convert_to_czk_at(&conn, 5.0, "NOK", 1_700_000_000);
        assert_eq!(result, 0.0);
        assert!(fx::missing_now().contains(&"NOK".to_string()));
    }

    #[test]
    fn test_convert_to_czk_at_czk_identity() {
        let conn = setup_history_db();
        let result = convert_to_czk_at(&conn, 500.0, "CZK", 1_700_000_000);
        assert_eq!(result, 500.0);
    }

    #[test]
    fn convert_to_czk_at_currency_missing_from_the_days_map_is_excluded_not_current() {
        let _fx = fx::test_lock();
        let conn = setup_history_db();
        let day: i64 = 1_700_000_000 / 86400 * 86400;
        // The day has history, but only for EUR — NOK is absent from that day's map.
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 24.0)",
            [day],
        )
        .expect("insert");
        set_store(&[("NOK", 21.0)]);

        assert_eq!(convert_to_czk_at(&conn, 5.0, "NOK", day), 0.0);
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
    fn convert_to_czk_with_rates_missing_currency_is_excluded_and_reported() {
        let _fx = fx::test_lock();
        set_store(&[("NOK", 21.0)]);
        let rates: HashMap<String, f64> = [("EUR".to_string(), 24.0)].into_iter().collect();

        let result = convert_to_czk_with_rates(5.0, "NOK", &rates);
        assert_eq!(result, 0.0);
        assert!(fx::missing_now().contains(&"NOK".to_string()));
    }

    #[test]
    fn test_convert_to_czk_with_rates_czk_identity() {
        let rates: HashMap<String, f64> = HashMap::new();
        let result = convert_to_czk_with_rates(100.0, "CZK", &rates);
        assert_eq!(result, 100.0);
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
