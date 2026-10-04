//! MCP tools: exchange_rates. Query body moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use crate::error::Result;
use rusqlite::Connection;
use serde_json::Value;

use super::money::load_main_currency;

pub fn exchange_rates_list(conn: &Connection) -> Result<Value> {
    let mut stmt =
        conn.prepare("SELECT currency, rate, fetched_at FROM exchange_rates ORDER BY currency")?;
    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "currency": row.get::<_, String>(0)?,
                "rate": row.get::<_, f64>(1)?,
                "fetchedAt": row.get::<_, i64>(2)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({
        "baseCurrency": "CZK",
        "mainCurrency": load_main_currency(conn)?,
        "rates": rows,
        "note": "Rates are Moony's internal CZK pivot: multiply an amount by its currency's rate to get CZK. Other tools already report amounts in mainCurrency."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db(main_currency: Option<&str>) -> Connection {
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
                ('USD', 20.0, 1700000000), ('EUR', 25.0, 1700000000);
            "#,
        )
        .expect("schema + seed");
        if let Some(c) = main_currency {
            conn.execute("INSERT INTO user_profile (currency) VALUES (?1)", [c])
                .expect("profile");
        }
        conn
    }

    #[test]
    fn rates_stay_raw_against_the_czk_pivot_and_name_the_main_currency() {
        let conn = setup_db(Some("EUR"));

        let result = exchange_rates_list(&conn).expect("rates");

        assert_eq!(result["baseCurrency"], "CZK");
        assert_eq!(result["mainCurrency"], "EUR");
        // Ordered by currency, rates untouched (X -> CZK).
        assert_eq!(result["rates"][0]["currency"], "EUR");
        assert_eq!(result["rates"][0]["rate"], 25.0);
        assert_eq!(result["rates"][1]["currency"], "USD");
        assert_eq!(result["rates"][1]["rate"], 20.0);
        assert_eq!(result["rates"][1]["fetchedAt"], 1_700_000_000);
    }

    #[test]
    fn note_explains_the_pivot_and_points_to_main_currency() {
        let conn = setup_db(Some("EUR"));

        let result = exchange_rates_list(&conn).expect("rates");

        assert_eq!(
            result["note"],
            "Rates are Moony's internal CZK pivot: multiply an amount by its currency's rate \
             to get CZK. Other tools already report amounts in mainCurrency."
        );
    }

    #[test]
    fn main_currency_is_czk_without_a_profile() {
        let conn = setup_db(None);

        let result = exchange_rates_list(&conn).expect("rates");

        assert_eq!(result["mainCurrency"], "CZK");
    }
}
