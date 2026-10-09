//! Exchange rate storage: the ECB feed, the stored current rates and the daily rate history.
//! Every stored rate is units of a currency per 1 EUR ([`fx::REFERENCE`]), as the ECB publishes
//! it. Conversions go through `services::fx`.

use crate::services::fx::{self, Quotes};
use std::collections::HashMap;

/// Get the timestamp when exchange rates were last fetched (kept in the FX current store).
pub fn get_exchange_rates_fetched_at() -> Option<i64> {
    fx::current_fetched_at()
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
    let mut rates: Vec<(String, f64)> = Vec::new();
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
        rates.push((currency, rate));
        latest_fetched_at = latest_fetched_at.max(Some(fetched_at));
    }

    if !rates.is_empty() {
        log::info!(
            "[CURRENCY] Loaded {} exchange rates from database",
            rates.len()
        );
        let quotes = Quotes::from_stored(rates.iter().map(|(code, rate)| (code.as_str(), *rate)));
        match latest_fetched_at {
            Some(ts) => fx::set_current_with_revision(quotes, ts),
            None => fx::set_current(quotes),
        }
    } else {
        log::info!("[CURRENCY] No exchange rates in database, will fetch from ECB");
    }

    Ok(())
}

/// Save exchange rates (units per 1 EUR) to the database for offline use. Only stored quotes are
/// written: never the reference itself or a derived (pegged) code.
pub fn save_rates_to_db(
    conn: &rusqlite::Connection,
    rates: &HashMap<String, f64>,
) -> crate::error::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;

    for (currency, rate) in rates.iter().filter(|(code, _)| fx::is_stored_code(code)) {
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

    Ok(())
}

/// The ECB daily reference rates as published (units per 1 EUR) plus the day they were
/// published *for* (the `time` attribute of the feed). On weekends and
/// holidays the feed keeps serving the last business day, so the reference day
/// is not the day the app fetched it.
#[derive(Debug, Clone)]
pub struct EcbRates {
    /// Currency -> units per 1 EUR. EUR itself is the reference and has no entry.
    pub rates: HashMap<String, f64>,
    /// Midnight UTC of the ECB reference date.
    pub reference_day: i64,
}

/// Fetch exchange rates from the ECB feed (units per 1 EUR).
pub async fn fetch_ecb_rates() -> crate::error::Result<EcbRates> {
    let url = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";

    let response = crate::services::http::client().get(url).send().await?;
    let body = response.text().await?;
    let ecb = parse_ecb_xml(&body)?;

    // Update global rates together with their fetched-at revision
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;
    fx::set_current_with_revision(
        Quotes::from_stored(ecb.rates.iter().map(|(code, rate)| (code.as_str(), *rate))),
        now,
    );

    Ok(ecb)
}

/// Parse the ECB daily XML (`<Cube time='2026-10-02'>` with
/// `<Cube currency='USD' rate='1.0843'/>` rows, all quoted per 1 EUR) into the published rates
/// and the reference day. No currency is required: a feed without CZK still carries every other
/// quote.
///
/// Fails — instead of guessing — when the feed has no usable rate at all or no parseable
/// reference date.
pub fn parse_ecb_xml(body: &str) -> crate::error::Result<EcbRates> {
    use crate::error::AppError;

    let reference_day = extract_reference_day(body).ok_or_else(|| {
        AppError::ExternalApi("ECB feed has no usable reference date (time attribute)".into())
    })?;

    let rates: HashMap<String, f64> = extract_all_currencies(body)
        .into_iter()
        .filter(|currency| fx::is_stored_code(currency))
        .filter_map(|currency| {
            let rate = extract_rate(body, &currency).filter(|r| r.is_finite() && *r > 0.0)?;
            Some((currency, rate))
        })
        .collect();
    if rates.is_empty() {
        return Err(AppError::ExternalApi("ECB feed has no usable rates".into()));
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

/// Save rates (units per 1 EUR) into the exchange_rate_history table (one row per currency per
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

    // The reference is 1.0 by definition and derived codes are resolved at read time.
    for (currency, rate) in rates.iter().filter(|(code, _)| fx::is_stored_code(code)) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(date, currency) DO UPDATE SET rate = ?3",
            rusqlite::params![day, currency, rate],
        )?;
    }

    Ok(())
}

/// Maximum relative difference tolerated between a per-currency breakdown's CZK
/// equivalent and the CZK total stored alongside it. Rate drift between the two
/// sources is a few percent at most; a larger gap means the breakdown does not
/// cover the whole asset class (e.g. it was derived from incomplete per-ticker
/// history).
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

/// True when a breakdown's CZK sum is close enough to the stored CZK total for
/// the breakdown to represent the whole asset class (see the tolerance above).
pub fn breakdown_covers_total(czk_sum: f64, czk_total: f64) -> bool {
    if czk_total <= 0.0 {
        return czk_sum.abs() <= f64::EPSILON;
    }
    ((czk_sum - czk_total) / czk_total).abs() <= BREAKDOWN_CONSISTENCY_TOLERANCE
}

/// Repair policy for a stored `*_by_currency` column: only an empty breakdown with a
/// non-zero CZK total needs repair. A non-empty breakdown holds native amounts, which are
/// the source of truth — it is never rewritten, however far its converted value drifts
/// from the CZK cache total.
pub fn needs_breakdown_repair(json: &str, czk_total: f64) -> bool {
    breakdown_from_json(json).is_empty() && czk_total != 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn current_czk(amount: f64, currency: &str) -> Option<f64> {
        fx::current().convert(amount, currency, "CZK")
    }

    #[test]
    fn store_starts_without_invented_rates() {
        fx::set_current(Quotes::new(fx::REFERENCE));
        assert_eq!(current_czk(1.0, "EUR"), None);
        assert_eq!(current_czk(100.0, "czk"), Some(100.0));
    }

    #[test]
    fn rates_loaded_from_db_are_validated() {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute_batch("CREATE TABLE exchange_rates (currency TEXT PRIMARY KEY, rate REAL NOT NULL, fetched_at INTEGER NOT NULL);
            INSERT INTO exchange_rates VALUES ('CZK', 25.0, 1), ('USD', 0.0, 1), ('GBP', -3.0, 1);").expect("schema");
        load_rates_from_db(&conn).expect("load");
        assert!((current_czk(1.0, "EUR").expect("EUR loaded") - 25.0).abs() < 1e-9);
        assert_eq!(current_czk(1.0, "USD"), None);
        assert_eq!(current_czk(1.0, "GBP"), None);
        assert_eq!(fx::current_fetched_at(), Some(1));
        assert_eq!(fx::current().reference(), "EUR");
    }

    #[test]
    fn derived_currencies_resolve_through_their_anchor() {
        fx::set_current_for_test(&[("CZK", 25.0), ("USD", 1.25)]);
        assert!((current_czk(3.6725, "AED").expect("pegged") - 20.0).abs() < 1e-9);
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
    fn saved_history_converts_at_the_days_quotes_with_derived_pegs() {
        let conn = setup_history_db();
        let mut rates = HashMap::new();
        rates.insert("CZK".to_string(), 25.0);
        rates.insert("USD".to_string(), 25.0 / 23.0);
        rates.insert("AED".to_string(), 4.0); // derived: never stored
        rates.insert("EUR".to_string(), 2.0); // the reference: never stored
        let day = 1_700_006_400;
        save_rates_to_history_db(&conn, &rates, day).expect("save");
        let stored: i64 = conn
            .query_row("SELECT COUNT(*) FROM exchange_rate_history", [], |r| {
                r.get(0)
            })
            .expect("count");
        assert_eq!(stored, 2);

        let history = fx::History::load(&conn, day, day).expect("load");
        let eur = history.convert_on(1.0, "EUR", "CZK", day).expect("EUR");
        assert!((eur - 25.0).abs() < 1e-9);
        let aed = history
            .convert_on(3.6725, "AED", "CZK", day)
            .expect("AED via USD");
        assert!((aed - 23.0).abs() < 1e-6);
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
        // Empty breakdown with a non-zero total needs repair; with a zero total it does not.
        assert!(needs_breakdown_repair("{}", 1000.0));
        assert!(!needs_breakdown_repair("{}", 0.0));
        assert!(needs_breakdown_repair("not json", 1000.0));

        // A consistent native breakdown is kept.
        assert!(!needs_breakdown_repair("{\"USD\":100}", 2000.0));
    }

    #[test]
    fn native_breakdown_far_from_its_czk_total_is_not_rewritten() {
        // 100 USD at ~20 CZK covers 20 % of a 10 000 CZK total: far beyond the old 25 %
        // tolerance, still the native truth.
        assert!(!needs_breakdown_repair("{\"USD\":100}", 10000.0));
        // A bucket without any quote is kept as well.
        assert!(!needs_breakdown_repair("{\"GBP\":100}", 10000.0));
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
      <Cube currency='USD' rate='1.1700'/>
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
        // Units per 1 EUR, exactly as published.
        assert_eq!(ecb.rates["CZK"], 25.0);
        assert_eq!(ecb.rates["USD"], 1.17);
        assert_eq!(ecb.rates["GBP"], 0.8);
        assert_eq!(ecb.rates.len(), 3);
    }

    #[test]
    fn test_parse_ecb_xml_without_a_czk_quote_keeps_the_published_rates() {
        let ecb = parse_ecb_xml(&ecb_xml("2026-10-02", None)).expect("parses without CZK");
        assert!((ecb.rates["USD"] - 1.17).abs() < 1e-12);
        assert!((ecb.rates["GBP"] - 0.8).abs() < 1e-12);
        assert!(!ecb.rates.contains_key("CZK"));
        assert!(
            !ecb.rates.contains_key("EUR"),
            "the reference has no row of its own"
        );
    }

    #[test]
    fn test_parse_ecb_xml_without_any_rate_is_an_error() {
        let xml = "<Cube><Cube time='2026-10-02'></Cube></Cube>";
        let err = parse_ecb_xml(xml).unwrap_err();
        assert!(err.to_string().contains("no usable rates"), "{err}");
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
        let eur_on = |conn: &Connection, day: i64| {
            fx::History::load(conn, day, day)
                .unwrap()
                .convert_on(1.0, "EUR", "CZK", day)
                .unwrap()
        };
        assert!((eur_on(&conn, SATURDAY) - 25.0).abs() < 1e-9);
        conn.execute(
            "INSERT OR IGNORE INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'CZK', 99.0)",
            [FRIDAY],
        )
        .unwrap();
        assert!((eur_on(&conn, FRIDAY) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn test_history_save_normalises_the_day_to_midnight() {
        let conn = setup_history_db();
        let rates: HashMap<String, f64> = [("CZK".to_string(), 25.0)].into_iter().collect();
        save_rates_to_history_db(&conn, &rates, FRIDAY + 3600).unwrap();
        let day: i64 = conn
            .query_row("SELECT date FROM exchange_rate_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(day, FRIDAY);
    }
}
