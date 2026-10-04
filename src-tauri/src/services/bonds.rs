//! Bonds business logic. Extracted from commands/bonds.rs so the Tauri
//! command and the MCP write tool share one validated path (ADR 0007).

use crate::error::Result;
use crate::models::{Bond, InsertBond};
use uuid::Uuid;

/// Create a new bond. SINGLE SOURCE OF TRUTH for bond creation.
pub fn create_bond(conn: &rusqlite::Connection, data: &InsertBond) -> Result<Bond> {
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let interest_rate = data
        .interest_rate
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let currency = data.currency.clone().unwrap_or_else(|| "CZK".to_string());
    let quantity = data.quantity.clone().unwrap_or_else(|| "1".to_string());

    conn.execute(
        "INSERT INTO bonds (id, name, isin, coupon_value, quantity, currency, interest_rate, maturity_date, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        rusqlite::params![id, data.name, data.isin.as_deref(), data.coupon_value, quantity, currency, interest_rate, data.maturity_date, now],
    )?;

    Ok(Bond {
        id,
        name: data.name.clone(),
        isin: data.isin.clone(),
        coupon_value: data.coupon_value.clone(),
        quantity,
        currency,
        interest_rate,
        maturity_date: data.maturity_date,
        created_at: now,
        updated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE bonds (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                isin TEXT,
                coupon_value TEXT NOT NULL,
                interest_rate TEXT NOT NULL DEFAULT '0',
                maturity_date INTEGER,
                currency TEXT NOT NULL DEFAULT 'CZK',
                quantity TEXT NOT NULL DEFAULT '1',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn valid_insert() -> InsertBond {
        InsertBond {
            name: "Test bond".into(),
            isin: None,
            coupon_value: "1000".into(),
            quantity: None,
            currency: None,
            interest_rate: None,
            maturity_date: None,
        }
    }

    #[test]
    fn create_bond_applies_defaults() {
        let conn = setup_test_db();
        let bond = create_bond(&conn, &valid_insert()).unwrap();
        assert_eq!(bond.quantity, "1");
        assert_eq!(bond.currency, "CZK");
        assert_eq!(bond.interest_rate, "0");
    }

    #[test]
    fn create_bond_rejects_invalid() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.name = "".into();
        assert!(create_bond(&conn, &data).is_err());
    }
}
