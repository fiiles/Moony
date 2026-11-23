//! The report page's remembered choices: cost method, holding test switch and its length.
//!
//! Stored in `app_config` under [`CONFIG_KEY`] as the wire JSON of
//! [`StockTransactionsReportSettings`]; the key syncs between devices. The period is not stored.

use rusqlite::{Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::models::stock_transactions_report::MAX_HOLDING_TEST_MONTHS;
use crate::models::{StockReportCostMethod, StockTransactionsReportSettings};

/// `app_config` key of the settings.
pub const CONFIG_KEY: &str = "stock_transactions_report";

fn defaults() -> StockTransactionsReportSettings {
    StockTransactionsReportSettings {
        cost_method: StockReportCostMethod::Fifo,
        holding_test_enabled: false,
        holding_test_months: 36,
    }
}

/// The stored settings, or the defaults when nothing is stored or the value is unreadable.
pub fn load(conn: &Connection) -> Result<StockTransactionsReportSettings> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [CONFIG_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(defaults());
    };
    Ok(serde_json::from_str(&raw).unwrap_or_else(|_| {
        log::warn!("[STOCK REPORT] Stored settings are unreadable, defaults used");
        defaults()
    }))
}

/// Stores the settings; the test length must be 1..=600 months.
pub fn save(conn: &Connection, settings: &StockTransactionsReportSettings) -> Result<()> {
    if !(1..=MAX_HOLDING_TEST_MONTHS).contains(&settings.holding_test_months) {
        return Err(AppError::Validation(
            "validation.holdingTestMonthsInvalid".into(),
        ));
    }
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![CONFIG_KEY, serde_json::to_string(settings)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT);")
            .expect("schema");
        conn
    }

    fn settings(
        method: StockReportCostMethod,
        enabled: bool,
        months: i64,
    ) -> StockTransactionsReportSettings {
        StockTransactionsReportSettings {
            cost_method: method,
            holding_test_enabled: enabled,
            holding_test_months: months,
        }
    }

    #[test]
    fn defaults_when_nothing_is_stored() {
        let loaded = load(&db()).unwrap();
        assert_eq!(loaded.cost_method, StockReportCostMethod::Fifo);
        assert!(!loaded.holding_test_enabled);
        assert_eq!(loaded.holding_test_months, 36);
    }

    #[test]
    fn save_then_load_round_trips() {
        let conn = db();
        save(&conn, &settings(StockReportCostMethod::Average, true, 18)).unwrap();
        let loaded = load(&conn).unwrap();
        assert_eq!(loaded.cost_method, StockReportCostMethod::Average);
        assert!(loaded.holding_test_enabled);
        assert_eq!(loaded.holding_test_months, 18);
        save(&conn, &settings(StockReportCostMethod::Fifo, false, 24)).unwrap();
        assert_eq!(load(&conn).unwrap().holding_test_months, 24);
    }

    #[test]
    fn an_unreadable_value_falls_back_to_defaults() {
        let conn = db();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, 'not json')",
            [CONFIG_KEY],
        )
        .unwrap();
        let loaded = load(&conn).unwrap();
        assert_eq!(loaded.cost_method, StockReportCostMethod::Fifo);
        assert_eq!(loaded.holding_test_months, 36);
    }

    #[test]
    fn months_out_of_range_are_refused() {
        let conn = db();
        for months in [0, 601] {
            match save(&conn, &settings(StockReportCostMethod::Fifo, true, months)) {
                Err(AppError::Validation(key)) => {
                    assert_eq!(key, "validation.holdingTestMonthsInvalid")
                }
                other => panic!("{months}: {other:?}"),
            }
        }
        assert_eq!(load(&conn).unwrap().holding_test_months, 36);
    }
}
