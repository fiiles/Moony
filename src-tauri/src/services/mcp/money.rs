//! Money context for the MCP tools (ADR 0001, ADR 0006).
//!
//! Aggregates are computed in CZK (the internal base currency) and converted
//! into the user's main currency once, at serialisation time. A `MoneyContext`
//! is loaded once per tool call so every amount in one response uses the same
//! rates and the same main currency.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

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

        let stored: Option<String> = conn
            .query_row("SELECT currency FROM user_profile LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        let main = stored
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "CZK".to_string());

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
}
