//! Money context for the MCP tools (ADR 0001, ADR 0006).
//!
//! Aggregates are computed in CZK (the internal base currency) and converted
//! into the user's main currency once, at serialisation time. A `MoneyContext`
//! is loaded once per tool call so every amount in one response uses the same
//! rates and the same main currency.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

/// The user's main currency (`user_profile.currency`, trimmed and uppercased),
/// CZK when there is no profile or its currency is blank. The MCP write tools
/// use this to default a missing currency; the shared services keep their own
/// CZK default for the UI path (ADR 0007).
pub fn load_main_currency(conn: &Connection) -> Result<String> {
    let stored: Option<String> = conn
        .query_row("SELECT currency FROM user_profile LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(stored
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "CZK".to_string()))
}

/// A currency argument of an MCP write tool: the given code, or the user's
/// main currency when the argument is missing. Applied in the MCP layer only;
/// the shared services keep their own CZK default for the UI path (ADR 0007).
pub fn currency_or_main(conn: &Connection, currency: Option<&str>) -> Result<String> {
    match currency {
        Some(code) => Ok(code.to_string()),
        None => load_main_currency(conn),
    }
}

/// Exchange-rate snapshots (day -> X -> CZK per unit) for the days a history
/// response covers, loaded with one query for the whole span (see
/// `MoneyContext::day_rates_for`). The span is padded
/// by the 10-day walk-back that `currency::resolve_rates_for_day_from_range`
/// applies, so a row on a weekend or holiday still finds the closest earlier
/// snapshot.
fn load_day_rates(
    conn: &Connection,
    first_day: i64,
    last_day: i64,
) -> HashMap<i64, HashMap<String, f64>> {
    crate::services::currency::get_rates_for_date_range(
        conn,
        first_day - 10 * SECONDS_PER_DAY,
        last_day,
    )
}

const SECONDS_PER_DAY: i64 = 86_400;

pub struct MoneyContext {
    main: String,
    /// X -> CZK per unit, from the `exchange_rates` table (CZK = 1.0).
    rates: HashMap<String, f64>,
}

impl MoneyContext {
    /// Reads the stored exchange rates and the user's main currency
    /// (`user_profile.currency`, falling back to CZK when there is no profile).
    pub fn load(conn: &Connection) -> Result<MoneyContext> {
        let mut rates: HashMap<String, f64> = {
            let mut stmt = conn.prepare("SELECT currency, rate FROM exchange_rates")?;
            let result = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
            result
        };
        rates.insert("CZK".to_string(), 1.0);

        let main = load_main_currency(conn)?;

        Ok(MoneyContext { main, rates })
    }

    /// The user's main currency (3-letter uppercase code).
    pub fn main_currency(&self) -> &str {
        &self.main
    }

    /// Converts an amount in `currency` to CZK. A currency missing from the
    /// table falls back to the in-memory rate, then to the documented 1.0 last
    /// resort (see `currency::convert_to_czk_with_rates`).
    pub fn to_czk(&self, amount: f64, currency: &str) -> f64 {
        crate::services::currency::convert_to_czk_with_rates(amount, currency, &self.rates)
    }

    /// Converts a CZK amount into the main currency. A main currency without a
    /// usable stored rate uses the same last resort as the app
    /// (`currency::convert_from_czk`).
    pub fn czk_to_main(&self, czk: f64) -> f64 {
        if self.main == "CZK" {
            return czk;
        }
        match self.rates.get(&self.main) {
            Some(rate) if *rate > 0.0 => czk / rate,
            _ => crate::services::currency::convert_from_czk(czk, &self.main),
        }
    }

    /// Loads the rate snapshots needed to convert history rows recorded on
    /// `days`. Empty when nothing needs them (a CZK main currency converts
    /// nothing, and no rows means no days).
    pub fn day_rates_for(
        &self,
        conn: &Connection,
        days: &[i64],
    ) -> HashMap<i64, HashMap<String, f64>> {
        if self.main == "CZK" {
            return HashMap::new();
        }
        match (days.iter().min(), days.iter().max()) {
            (Some(first), Some(last)) => load_day_rates(conn, *first, *last),
            _ => HashMap::new(),
        }
    }

    /// Converts a CZK amount into the main currency at the rates of `day`
    /// (ADR 0001: historical values use the rates of their day). `day_rates`
    /// comes from `load_day_rates`; the day resolves to the exact snapshot or
    /// the closest earlier one. A day without a snapshot, or a snapshot that
    /// lacks a usable main-currency rate, falls back to the current rates
    /// (`czk_to_main`).
    pub fn czk_to_main_on(
        &self,
        czk: f64,
        day: i64,
        day_rates: &HashMap<i64, HashMap<String, f64>>,
    ) -> f64 {
        if self.main == "CZK" {
            return czk;
        }
        let day_rate = crate::services::currency::resolve_rates_for_day_from_range(day_rates, day)
            .and_then(|rates| rates.get(&self.main))
            .copied()
            .filter(|rate| *rate > 0.0);
        match day_rate {
            Some(rate) => czk / rate,
            None => self.czk_to_main(czk),
        }
    }

    /// Converts a stored CZK money cell (TEXT in production) into the main
    /// currency at the rates of `day`, as a two-decimal string like the other
    /// money fields. A CZK main currency returns the stored value untouched;
    /// NULL stays NULL; an unparseable cell counts as 0 (the convention of the
    /// other MCP readers).
    pub fn stored_czk_to_main_on(
        &self,
        cell: rusqlite::types::Value,
        day: i64,
        day_rates: &HashMap<i64, HashMap<String, f64>>,
    ) -> serde_json::Value {
        if self.main == "CZK" {
            return super::sql_to_json(cell);
        }
        let czk = match cell {
            rusqlite::types::Value::Null | rusqlite::types::Value::Blob(_) => {
                return serde_json::Value::Null
            }
            rusqlite::types::Value::Integer(i) => i as f64,
            rusqlite::types::Value::Real(f) => f,
            rusqlite::types::Value::Text(s) => s.trim().parse::<f64>().unwrap_or(0.0),
        };
        serde_json::json!(format!("{:.2}", self.czk_to_main_on(czk, day, day_rates)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db(profile_currency: Option<&str>) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE exchange_rates (
                currency TEXT PRIMARY KEY,
                rate REAL NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES
                ('EUR', 25.0, 0), ('USD', 20.0, 0), ('ZMN1', 0.0, 0);
            "#,
        )
        .expect("schema");
        if let Some(c) = profile_currency {
            conn.execute("INSERT INTO user_profile (currency) VALUES (?1)", [c])
                .expect("profile");
        }
        conn
    }

    #[test]
    fn eur_main_currency_converts_czk_by_the_stored_rate() {
        let ctx = MoneyContext::load(&setup_db(Some("EUR"))).expect("load");
        assert_eq!(ctx.main_currency(), "EUR");
        assert_eq!(ctx.czk_to_main(2500.0), 100.0);
    }

    #[test]
    fn missing_profile_row_means_czk_and_identity() {
        let ctx = MoneyContext::load(&setup_db(None)).expect("load");
        assert_eq!(ctx.main_currency(), "CZK");
        assert_eq!(ctx.czk_to_main(2500.0), 2500.0);
    }

    #[test]
    fn empty_profile_currency_falls_back_to_czk() {
        let ctx = MoneyContext::load(&setup_db(Some("  "))).expect("load");
        assert_eq!(ctx.main_currency(), "CZK");
        assert_eq!(ctx.czk_to_main(10.0), 10.0);
    }

    #[test]
    fn profile_currency_is_trimmed_and_uppercased() {
        let ctx = MoneyContext::load(&setup_db(Some(" eur "))).expect("load");
        assert_eq!(ctx.main_currency(), "EUR");
        assert_eq!(ctx.czk_to_main(2500.0), 100.0);
    }

    #[test]
    fn to_czk_uses_the_table_rates() {
        let ctx = MoneyContext::load(&setup_db(Some("EUR"))).expect("load");
        assert_eq!(ctx.to_czk(10.0, "USD"), 200.0);
        assert_eq!(ctx.to_czk(10.0, "EUR"), 250.0);
        assert_eq!(ctx.to_czk(10.0, "CZK"), 10.0);
    }

    #[test]
    fn main_without_a_table_rate_uses_the_in_memory_rate() {
        // JPY is not in the test table; the built-in in-memory rate (0.15) applies,
        // exactly as `currency::convert_from_czk` would.
        let ctx = MoneyContext::load(&setup_db(Some("JPY"))).expect("load");
        let expected = crate::services::currency::convert_from_czk(1500.0, "JPY");
        assert_eq!(ctx.czk_to_main(1500.0), expected);
    }

    #[test]
    fn main_with_a_non_positive_rate_does_not_divide_by_zero() {
        // A stored rate of 0 is unusable; fall back to the app-wide last resort
        // (an unknown currency is 1 CZK, so the amount passes through).
        let ctx = MoneyContext::load(&setup_db(Some("ZMN1"))).expect("load");
        assert_eq!(ctx.czk_to_main(1500.0), 1500.0);
    }

    // ---- day-rate conversion (history rows, ADR 0001) ----

    const DAY: i64 = 86_400;

    fn add_history(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            rusqlite::params![day, currency, rate],
        )
        .expect("history row");
    }

    fn setup_history_db(profile_currency: Option<&str>) -> Connection {
        let conn = setup_db(profile_currency);
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );",
        )
        .expect("history schema");
        conn
    }

    #[test]
    fn day_rates_convert_each_day_at_its_own_rate() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        add_history(&conn, 101 * DAY, "EUR", 26.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, 100 * DAY, 101 * DAY);

        assert_eq!(ctx.czk_to_main_on(2400.0, 100 * DAY, &rates), 100.0);
        assert_eq!(ctx.czk_to_main_on(2600.0, 101 * DAY, &rates), 100.0);
        // The current rate (25) would give a different answer for both days.
        assert_eq!(ctx.czk_to_main(2400.0), 96.0);
    }

    #[test]
    fn a_day_without_a_snapshot_walks_back_to_the_closest_earlier_day() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        // The row sits three days after the snapshot; the range query has to
        // reach back that far.
        let rates = load_day_rates(&conn, 103 * DAY, 103 * DAY);

        assert_eq!(ctx.czk_to_main_on(2400.0, 103 * DAY, &rates), 100.0);
    }

    #[test]
    fn a_day_with_no_history_at_all_uses_the_current_rate() {
        let conn = setup_history_db(Some("EUR"));
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, 100 * DAY, 100 * DAY);

        assert_eq!(ctx.czk_to_main_on(2500.0, 100 * DAY, &rates), 100.0);
    }

    #[test]
    fn a_snapshot_without_the_main_currency_uses_the_current_rate() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "USD", 21.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, 100 * DAY, 100 * DAY);

        assert_eq!(ctx.czk_to_main_on(2500.0, 100 * DAY, &rates), 100.0);
    }

    #[test]
    fn czk_main_currency_ignores_day_rates() {
        let conn = setup_history_db(Some("CZK"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, 100 * DAY, 100 * DAY);

        assert_eq!(ctx.czk_to_main_on(2400.0, 100 * DAY, &rates), 2400.0);
    }

    #[test]
    fn stored_czk_text_is_converted_to_a_two_decimal_string() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, 100 * DAY, 100 * DAY);
        let text = |s: &str| rusqlite::types::Value::Text(s.to_string());

        assert_eq!(
            ctx.stored_czk_to_main_on(text("2500"), 100 * DAY, &rates),
            serde_json::json!("104.17")
        );
        assert_eq!(
            ctx.stored_czk_to_main_on(rusqlite::types::Value::Null, 100 * DAY, &rates),
            serde_json::Value::Null
        );
    }

    #[test]
    fn stored_czk_text_stays_untouched_for_a_czk_main_currency() {
        let conn = setup_history_db(Some("CZK"));
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, 100 * DAY, 100 * DAY);

        assert_eq!(
            ctx.stored_czk_to_main_on(
                rusqlite::types::Value::Text("1234.5678".to_string()),
                100 * DAY,
                &rates
            ),
            serde_json::json!("1234.5678")
        );
    }

    #[test]
    fn currency_or_main_keeps_a_given_code_and_fills_a_missing_one() {
        let conn = setup_db(Some("EUR"));

        assert_eq!(currency_or_main(&conn, Some("USD")).expect("given"), "USD");
        assert_eq!(currency_or_main(&conn, None).expect("missing"), "EUR");
    }

    #[test]
    fn load_main_currency_reads_the_profile_or_falls_back_to_czk() {
        assert_eq!(
            load_main_currency(&setup_db(Some(" eur "))).expect("main"),
            "EUR"
        );
        assert_eq!(load_main_currency(&setup_db(None)).expect("main"), "CZK");
    }
}
