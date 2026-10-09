//! MCP tools: exchange_rates (ADR 0006, ADR 0011, ADR 0012): the app's current quote snapshot.

use crate::error::Result;
use crate::services::fx;
use rusqlite::Connection;
use serde_json::Value;

use super::money::load_main_currency;

/// The current quote snapshot against its explicit base: `rates` are units of each currency per
/// 1 unit of `baseCurrency` (the base itself at 1), source quotes and derived (pegged) ones
/// alike, plus every supported currency code (a code without a rate is supported but not
/// quoted right now).
pub fn exchange_rates_list(conn: &Connection) -> Result<Value> {
    let quotes = fx::current();
    let rates: Vec<Value> = quotes
        .codes()
        .into_iter()
        .filter_map(|code| {
            let units = quotes.units(&code)?;
            Some(serde_json::json!({
                "currency": code,
                "rate": units,
                "derived": fx::derived(&code).is_some(),
            }))
        })
        .collect();
    Ok(serde_json::json!({
        "baseCurrency": quotes.reference(),
        "mainCurrency": load_main_currency(conn)?,
        "fetchedAt": fx::current_fetched_at(),
        "rates": rates,
        "supportedCurrencies": fx::supported_codes(),
        "note": format!(
            "Each rate is units of the currency per 1 {base}: divide an amount in a currency by \
             its rate to get {base}, and multiply a {base} amount by the rate to get the \
             currency. Rates marked derived are fixed pegs to another currency. Other tools \
             already report amounts in mainCurrency.",
            base = quotes.reference()
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db(main_currency: Option<&str>) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );",
        )
        .expect("schema");
        if let Some(c) = main_currency {
            conn.execute("INSERT INTO user_profile (currency) VALUES (?1)", [c])
                .expect("profile");
        }
        // The current snapshot is the source (C-A): EUR and USD quoted, the rest unquoted.
        fx::set_current_for_test(&[("USD", 20.0), ("EUR", 25.0)]);
        conn
    }

    fn rate_of(result: &Value, code: &str) -> Option<f64> {
        result["rates"]
            .as_array()?
            .iter()
            .find(|row| row["currency"] == code)?["rate"]
            .as_f64()
    }

    #[test]
    fn rates_are_units_per_the_explicit_base_including_derived_ones() {
        let conn = setup_db(Some("EUR"));
        let quotes = fx::current();

        let result = exchange_rates_list(&conn).expect("rates");

        // Whatever the storage reference is, it is named and quoted at 1.
        assert_eq!(result["baseCurrency"], quotes.reference());
        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(rate_of(&result, quotes.reference()), Some(1.0));
        // Every row is units per base, exactly the snapshot's quote.
        for code in ["USD", "EUR", "AED", "BGN"] {
            assert_eq!(rate_of(&result, code), quotes.units(code), "{code}");
        }
        // 20 CZK / 25 CZK per unit means 0.05 USD per CZK; AED is pegged to USD.
        let usd = rate_of(&result, "USD").expect("usd");
        let aed = rate_of(&result, "AED").expect("aed");
        assert!((aed - usd * 3.6725).abs() < 1e-9);
        let derived: Vec<&Value> = result["rates"]
            .as_array()
            .expect("rates")
            .iter()
            .filter(|row| row["derived"] == true)
            .collect();
        assert!(derived.iter().any(|row| row["currency"] == "AED"));
        assert!(derived.iter().all(|row| row["currency"] != "USD"));
        // Unquoted source currencies are not invented.
        assert_eq!(rate_of(&result, "JPY"), None);
    }

    #[test]
    fn supported_currencies_list_every_supported_code() {
        let conn = setup_db(Some("EUR"));

        let result = exchange_rates_list(&conn).expect("rates");

        let supported = result["supportedCurrencies"].as_array().expect("list");
        assert_eq!(supported.len(), 36);
        assert!(supported.iter().any(|code| code == "JPY"));
        assert!(supported.iter().any(|code| code == "AED"));
    }

    #[test]
    fn note_names_the_base_and_does_not_describe_a_czk_pivot() {
        let conn = setup_db(Some("EUR"));
        let base = fx::current().reference().to_string();

        let result = exchange_rates_list(&conn).expect("rates");

        let note = result["note"].as_str().expect("note");
        assert!(note.contains(&format!("per 1 {base}")), "{note}");
        assert!(!note.contains("pivot"), "{note}");
    }

    #[test]
    fn fetched_at_comes_from_the_snapshot_and_is_null_when_unknown() {
        let conn = setup_db(Some("EUR"));
        assert!(exchange_rates_list(&conn).expect("rates")["fetchedAt"].is_null());

        let mut quotes = fx::Quotes::new(fx::current().reference());
        quotes.insert("EUR", 0.04);
        fx::set_current_with_revision(quotes, 1_700_000_000);

        assert_eq!(
            exchange_rates_list(&conn).expect("rates")["fetchedAt"],
            1_700_000_000
        );
    }

    #[test]
    fn main_currency_is_czk_without_a_profile() {
        let conn = setup_db(None);

        let result = exchange_rates_list(&conn).expect("rates");

        assert_eq!(result["mainCurrency"], "CZK");
    }
}
