//! MCP tools: savings. Query body moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use crate::error::Result;
use rusqlite::Connection;
use serde_json::Value;

use super::sql_to_json;

pub fn savings_list(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT ba.id, ba.name, ba.account_type, ba.iban, ba.currency, ba.balance,
                ba.interest_rate, ba.has_zone_designation, ba.termination_date,
                ba.exclude_from_balance, ba.created_at, ba.updated_at,
                i.name
         FROM bank_accounts ba
         LEFT JOIN institutions i ON ba.institution_id = i.id
         ORDER BY ba.name",
    )?;
    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "accountType": row.get::<_, String>(2)?,
                "iban": sql_to_json(row.get::<_, rusqlite::types::Value>(3).unwrap_or(rusqlite::types::Value::Null)),
                "currency": row.get::<_, String>(4)?,
                "balance": row.get::<_, String>(5)?,
                "interestRate": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
                "hasZoneDesignation": row.get::<_, i32>(7)? != 0,
                "terminationDate": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
                "excludeFromBalance": row.get::<_, i32>(9)? != 0,
                "createdAt": row.get::<_, i64>(10)?,
                "updatedAt": row.get::<_, i64>(11)?,
                "institutionName": sql_to_json(row.get::<_, rusqlite::types::Value>(12).unwrap_or(rusqlite::types::Value::Null)),
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Value::Array(rows))
}
