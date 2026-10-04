//! MCP tools: real estate. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).
//! Post-port divergence: the detail JSON surfaces all selected columns (the REST
//! handler dropped purchase/rent/costs/notes even though its SELECT fetched them).

use rmcp::schemars;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, Result};

use super::sql_to_json;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RealEstateDetailArgs {
    #[schemars(description = "The real estate property ID")]
    pub id: String,
}

pub fn real_estate_list(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT id, name, address, type, purchase_price, purchase_price_currency,
                market_price, market_price_currency, monthly_rent, monthly_rent_currency,
                recurring_costs, notes, created_at, updated_at
         FROM real_estate ORDER BY name",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "name": row.get::<_, String>(1)?,
        "address": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
        "type": row.get::<_, String>(3)?,
        "purchasePrice": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
        "purchasePriceCurrency": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
        "marketPrice": row.get::<_, String>(6)?,
        "marketPriceCurrency": row.get::<_, String>(7)?,
        "monthlyRent": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
        "monthlyRentCurrency": sql_to_json(row.get::<_, rusqlite::types::Value>(9).unwrap_or(rusqlite::types::Value::Null)),
        "recurringCosts": sql_to_json(row.get::<_, rusqlite::types::Value>(10).unwrap_or(rusqlite::types::Value::Null)),
        "notes": sql_to_json(row.get::<_, rusqlite::types::Value>(11).unwrap_or(rusqlite::types::Value::Null)),
        "createdAt": row.get::<_, i64>(12)?,
        "updatedAt": row.get::<_, i64>(13)?,
    })))?.filter_map(|r| r.ok()).collect();
    Ok(Value::Array(rows))
}

pub fn real_estate_detail(conn: &Connection, id: &str) -> Result<Value> {
    let id = id.to_string();
    let prop = conn.query_row(
        "SELECT id, name, address, type, purchase_price, purchase_price_currency,
                market_price, market_price_currency, monthly_rent, monthly_rent_currency,
                recurring_costs, notes, created_at, updated_at
         FROM real_estate WHERE id = ?",
        [&id],
        |row| Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "address": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
            "type": row.get::<_, String>(3)?,
            "purchasePrice": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
            "purchasePriceCurrency": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
            "marketPrice": row.get::<_, String>(6)?,
            "marketPriceCurrency": row.get::<_, String>(7)?,
            "monthlyRent": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
            "monthlyRentCurrency": sql_to_json(row.get::<_, rusqlite::types::Value>(9).unwrap_or(rusqlite::types::Value::Null)),
            "recurringCosts": sql_to_json(row.get::<_, rusqlite::types::Value>(10).unwrap_or(rusqlite::types::Value::Null)),
            "notes": sql_to_json(row.get::<_, rusqlite::types::Value>(11).unwrap_or(rusqlite::types::Value::Null)),
            "createdAt": row.get::<_, i64>(12)?,
            "updatedAt": row.get::<_, i64>(13)?,
        })),
    );
    match prop {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Ok(serde_json::json!({ "error": format!("Property {} not found", id) }))
        }
        Err(e) => Err(e.into()),
        Ok(mut obj) => {
            let costs: Vec<Value> = {
                let mut s = conn.prepare(
                    "SELECT id, name, description, amount, currency, date, created_at
                     FROM real_estate_one_time_costs WHERE real_estate_id = ?
                     ORDER BY date DESC",
                )?;
                let result = s.query_map([&id], |row| Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "description": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
                    "amount": row.get::<_, String>(3)?,
                    "currency": row.get::<_, String>(4)?,
                    "date": row.get::<_, i64>(5)?,
                    "createdAt": row.get::<_, i64>(6)?,
                })))?.filter_map(|r| r.ok()).collect();
                result
            };
            obj["costs"] = Value::Array(costs);
            Ok(obj)
        }
    }
}

// ============================================================================
// Write tools (Task 10: single-create entity tool, dup check per spec D4).
// Photos are deliberately absent from this args struct (spec D5/§3) — the
// tool has its own shape rather than exposing InsertRealEstate directly.
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RealEstateCreateArgs {
    pub name: String,
    pub address: String,
    #[serde(rename = "type")]
    #[schemars(
        description = "Property type, e.g. apartment, house, land; \"personal\" usage is inferred elsewhere — pass the type shown in the app"
    )]
    pub property_type: String,
    #[serde(rename = "purchasePrice")]
    pub purchase_price: Option<String>,
    #[serde(rename = "purchasePriceCurrency")]
    pub purchase_price_currency: Option<String>,
    #[serde(rename = "marketPrice")]
    pub market_price: Option<String>,
    #[serde(rename = "marketPriceCurrency")]
    pub market_price_currency: Option<String>,
    #[serde(rename = "monthlyRent")]
    pub monthly_rent: Option<String>,
    #[serde(rename = "monthlyRentCurrency")]
    pub monthly_rent_currency: Option<String>,
    #[serde(rename = "recurringCosts")]
    pub recurring_costs: Option<Vec<crate::models::RecurringCost>>,
    pub notes: Option<String>,
}

pub fn real_estate_create(conn: &Connection, args: &RealEstateCreateArgs) -> Result<Value> {
    let dup: Option<String> = conn
        .query_row(
            "SELECT id FROM real_estate WHERE LOWER(name) = LOWER(?1)",
            [&args.name],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = dup {
        return Err(AppError::Validation(format!(
            "A property with this name already exists (id: {})",
            id
        )));
    }
    let data = crate::models::InsertRealEstate {
        name: args.name.clone(),
        address: args.address.clone(),
        property_type: args.property_type.clone(),
        purchase_price: args.purchase_price.clone(),
        purchase_price_currency: args.purchase_price_currency.clone(),
        market_price: args.market_price.clone(),
        market_price_currency: args.market_price_currency.clone(),
        monthly_rent: args.monthly_rent.clone(),
        monthly_rent_currency: args.monthly_rent_currency.clone(),
        recurring_costs: args.recurring_costs.clone(),
        photos: None,
        notes: args.notes.clone(),
    };
    let id = crate::services::real_estate::create_property(conn, &data)?;
    // In-conn re-read using the module's existing real_estate_detail so the
    // returned shape matches what other read tools already return.
    real_estate_detail(conn, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                address TEXT NOT NULL,
                type TEXT NOT NULL,
                purchase_price TEXT NOT NULL DEFAULT '0',
                purchase_price_currency TEXT NOT NULL DEFAULT 'CZK',
                market_price TEXT NOT NULL DEFAULT '0',
                market_price_currency TEXT NOT NULL DEFAULT 'CZK',
                monthly_rent TEXT,
                monthly_rent_currency TEXT DEFAULT 'CZK',
                recurring_costs TEXT DEFAULT '[]',
                photos TEXT DEFAULT '[]',
                notes TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE real_estate_one_time_costs (
                id TEXT PRIMARY KEY,
                real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                description TEXT,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
        "#,
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO real_estate (id, name, address, type, purchase_price,
                purchase_price_currency, market_price, market_price_currency,
                monthly_rent, monthly_rent_currency, recurring_costs, notes,
                created_at, updated_at)
             VALUES ('re-1', 'Byt Praha', 'Dlouhá 1', 'apartment', '5000000',
                'CZK', '6500000', 'CZK', '25000', 'CZK', '[]', 'poznámka', 100, 200)",
            [],
        )
        .expect("insert property");
        conn
    }

    #[test]
    fn detail_returns_financial_fields_as_strings() {
        let conn = setup_test_db();
        let detail = real_estate_detail(&conn, "re-1").expect("detail");

        assert_eq!(detail["purchasePrice"], "5000000");
        assert_eq!(detail["purchasePriceCurrency"], "CZK");
        assert_eq!(detail["monthlyRent"], "25000");
        assert_eq!(detail["monthlyRentCurrency"], "CZK");
        assert_eq!(detail["recurringCosts"], "[]");
        assert_eq!(detail["notes"], "poznámka");
    }

    #[test]
    fn detail_keys_are_superset_of_list_row_keys() {
        let conn = setup_test_db();
        let list = real_estate_list(&conn).expect("list");
        let detail = real_estate_detail(&conn, "re-1").expect("detail");

        let list_row = list[0].as_object().expect("list row object");
        let detail_obj = detail.as_object().expect("detail object");
        for key in list_row.keys() {
            assert!(detail_obj.contains_key(key), "detail is missing key {key}");
        }
        assert!(detail_obj.contains_key("costs"));
    }

    fn create_args(name: &str) -> RealEstateCreateArgs {
        RealEstateCreateArgs {
            name: name.to_string(),
            address: "Nová 2".into(),
            property_type: "apartment".into(),
            purchase_price: Some("4000000".into()),
            purchase_price_currency: None,
            market_price: Some("4500000".into()),
            market_price_currency: None,
            monthly_rent: None,
            monthly_rent_currency: None,
            recurring_costs: None,
            notes: None,
        }
    }

    #[test]
    fn create_happy_path_returns_detail_shape() {
        let conn = setup_test_db();
        let value = real_estate_create(&conn, &create_args("Byt Brno")).unwrap();
        assert_eq!(value["name"], "Byt Brno");
        assert_eq!(value["purchasePrice"], "4000000");
        // Detail shape includes the costs array even for a brand-new property.
        assert!(value["costs"].as_array().unwrap().is_empty());
    }

    #[test]
    fn create_rejects_duplicate_name_case_insensitive_and_reports_existing_id() {
        let conn = setup_test_db();
        // Seeded property is 'Byt Praha' (id 're-1').
        let err = real_estate_create(&conn, &create_args("byt praha")).unwrap_err();
        match err {
            AppError::Validation(msg) => assert!(
                msg.contains("re-1"),
                "error should carry the existing property id: {msg}"
            ),
            other => panic!("expected Validation error, got {other:?}"),
        }
    }
}
