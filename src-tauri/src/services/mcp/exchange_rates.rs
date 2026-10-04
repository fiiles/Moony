//! MCP tools: exchange_rates. Query body moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use crate::error::Result;
use rusqlite::Connection;
use serde_json::Value;

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
        "rates": rows,
        "note": "Multiply amount in currency by rate to get CZK equivalent."
    }))
}
