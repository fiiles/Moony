//! Other assets business logic. Extracted from commands/other_assets.rs so
//! the Tauri command and the MCP write tool share one validated path
//! (ADR 0007).

use crate::error::{AppError, Result};
use crate::models::{
    InsertOtherAsset, InsertOtherAssetTransaction, OtherAsset, OtherAssetTransaction,
};
use crate::services::valuations::{self, ValuationKind};
use rusqlite::OptionalExtension;
use uuid::Uuid;

/// Recalculate native totals, converting each purchase at its own UTC day.
/// Missing historical FX is an error: callers keep the write and totals atomic.
pub fn recalculate_asset_totals(conn: &rusqlite::Connection, asset_id: &str) -> Result<()> {
    use crate::models::validation::finite_number;
    use crate::services::fx;
    let exact_quantity = crate::services::investments::exact_held_quantity(
        conn,
        "other_asset_transactions",
        asset_id,
    )?;
    let (currency, market_price): (String, String) = conn.query_row(
        "SELECT currency, market_price FROM other_assets WHERE id = ?1",
        [asset_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let market_price = finite_number(
        if market_price.is_empty() {
            "0"
        } else {
            &market_price
        },
        "validation.invalidPrice",
    )?;
    let mut stmt = conn.prepare(
        "SELECT type, quantity, price_per_unit, currency, transaction_date FROM other_asset_transactions WHERE asset_id = ?1",
    )?;
    let rows = stmt
        .query_map([asset_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let foreign_days: Vec<i64> = rows
        .iter()
        .filter(|r| r.0 == "buy" && !r.3.eq_ignore_ascii_case(&currency))
        .map(|r| r.4)
        .collect();
    let history = match (foreign_days.iter().min(), foreign_days.iter().max()) {
        (Some(start), Some(end)) => fx::History::load(conn, *start, *end)?,
        _ => fx::History::default(),
    };
    let mut total_quantity = 0.0;
    let mut total_cost = 0.0;
    let mut total_buy_quantity = 0.0;
    for (tx_type, qty, price, tx_currency, day) in rows {
        let qty = finite_number(&qty, "validation.invalidQuantity")?;
        let price = finite_number(&price, "validation.invalidPrice")?;
        if tx_type == "buy" {
            let price = history
                .convert_on(price, &tx_currency, &currency, day)
                .ok_or_else(|| AppError::Validation("validation.exchangeRateUnavailable".into()))?;
            total_quantity += qty;
            total_buy_quantity += qty;
            total_cost += qty * price;
        } else if tx_type == "sell" {
            total_quantity -= qty;
        }
        if !total_quantity.is_finite() || !total_buy_quantity.is_finite() || !total_cost.is_finite()
        {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }
    }
    let average_purchase_price = if total_buy_quantity > 0.0 {
        total_cost / total_buy_quantity
    } else {
        0.0
    };
    if !average_purchase_price.is_finite()
        || !(total_quantity * average_purchase_price).is_finite()
        || !(total_quantity * market_price).is_finite()
    {
        return Err(AppError::Validation("validation.invalidAmount".into()));
    }
    conn.execute(
        "UPDATE other_assets SET quantity = ?1, average_purchase_price = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![exact_quantity, average_purchase_price.to_string(), chrono::Utc::now().timestamp(), asset_id],
    )?;
    Ok(())
}

/// Create a new other asset, optionally with an initial buy/sell
/// transaction. SINGLE SOURCE OF TRUTH for other-asset creation. A priced
/// asset also gets the first row of its valuation log (price per unit, dated
/// at the creation day), so the first revaluation does not overwrite the only
/// record of the original estimate.
///
/// An initial transaction is authoritative for the initial quantity and cost.
pub fn create_asset(
    conn: &rusqlite::Connection,
    data: &InsertOtherAsset,
    initial_transaction: Option<&InsertOtherAssetTransaction>,
) -> Result<OtherAsset> {
    data.validate()?;
    if let Some(tx) = initial_transaction {
        tx.validate()?;
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let market_price = data.market_price.clone().unwrap_or_else(|| "0".to_string());
    let currency = data
        .currency
        .as_deref()
        .unwrap_or(crate::constants::STORAGE_DEFAULT_CURRENCY)
        .to_ascii_uppercase();

    // The asset, its first estimate and its initial transaction are saved together or not at all
    let db_tx = conn.unchecked_transaction()?;
    db_tx.execute(
        "INSERT INTO other_assets
         (id, name, quantity, market_price, currency, average_purchase_price, yield_type, yield_value, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        rusqlite::params![
            id,
            data.name,
            data.quantity.clone().unwrap_or_else(|| "0".to_string()),
            market_price,
            currency,
            data.average_purchase_price
                .clone()
                .unwrap_or_else(|| "0".to_string()),
            data.yield_type.as_deref().unwrap_or("none").to_lowercase(),
            data.yield_value,
            now,
        ],
    )?;

    valuations::record_initial_valuation(
        &db_tx,
        ValuationKind::OtherAsset,
        &id,
        &market_price,
        &currency,
        now,
    )?;

    if let Some(tx) = initial_transaction {
        let tx_id = Uuid::new_v4().to_string();
        // Normalize enum-like tx_type to lowercase so the stored value matches
        // both recalculate_asset_totals's exact "buy"/"sell" comparisons and
        // the dedup queries (mirrors services/bank_accounts.rs::create_transaction).
        let tx_type = tx.tx_type.to_lowercase();
        db_tx.execute(
            "INSERT INTO other_asset_transactions
             (id, asset_id, type, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                tx_id,
                id,
                tx_type,
                crate::services::dedup::canonical_amount(&tx.quantity),
                crate::services::dedup::canonical_amount(&tx.price_per_unit),
                tx.currency.to_ascii_uppercase(),
                crate::models::validation::utc_day(tx.transaction_date, "validation.invalidDate")?,
                now
            ],
        )?;
        recalculate_asset_totals(&db_tx, &id)?;
    }
    db_tx.commit()?;

    get_asset(conn, &id)
}

/// The columns `read_asset` reads, today's price in its own currency
/// (`valuations::OTHER_ASSET_PRICES`) instead of the stored one.
fn asset_select() -> String {
    format!(
        "SELECT a.id, a.name, a.quantity, p.price, p.currency, a.currency,
                a.average_purchase_price, a.yield_type, a.yield_value, a.created_at, a.updated_at
         FROM other_assets a JOIN ({prices}) p ON p.asset_id = a.id",
        prices = valuations::OTHER_ASSET_PRICES
    )
}

fn read_asset(row: &rusqlite::Row) -> rusqlite::Result<OtherAsset> {
    Ok(OtherAsset {
        id: row.get(0)?,
        name: row.get(1)?,
        quantity: row.get(2)?,
        market_price: row.get(3)?,
        market_price_currency: row.get(4)?,
        currency: row.get(5)?,
        average_purchase_price: row.get(6)?,
        yield_type: row.get(7)?,
        yield_value: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

/// Every other asset by name.
pub fn list_assets(conn: &rusqlite::Connection) -> Result<Vec<OtherAsset>> {
    let mut stmt = conn.prepare(&format!("{} ORDER BY a.name", asset_select()))?;
    let assets = stmt
        .query_map([], read_asset)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(assets)
}

/// One other asset.
pub fn get_asset(conn: &rusqlite::Connection, id: &str) -> Result<OtherAsset> {
    conn.query_row(
        &format!("{} WHERE a.id = ?1", asset_select()),
        [id],
        read_asset,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Other asset not found".into()))
}

/// Create a transaction against an existing other asset and recalculate its
/// totals. SINGLE SOURCE OF TRUTH for other-asset-transaction creation.
///
/// Hardening beyond the original command (spec D2/D10): verifies the asset
/// exists before inserting. The pre-extraction command inserted an orphan
/// transaction row for a bad `asset_id` with no error; the MCP path needs
/// the check and the UI benefits too.
pub fn create_asset_transaction(
    conn: &rusqlite::Connection,
    asset_id: &str,
    data: &InsertOtherAssetTransaction,
) -> Result<OtherAssetTransaction> {
    create_asset_transaction_impl(conn, asset_id, data, true)
}

/// The transactional MCP batch validates and recalculates only its final merged
/// history, so an incoming buy may fund a backdated sale regardless of row order.
pub(crate) fn create_asset_transaction_in_batch(
    conn: &rusqlite::Connection,
    asset_id: &str,
    data: &InsertOtherAssetTransaction,
) -> Result<OtherAssetTransaction> {
    create_asset_transaction_impl(conn, asset_id, data, false)
}

fn create_asset_transaction_impl(
    conn: &rusqlite::Connection,
    asset_id: &str,
    data: &InsertOtherAssetTransaction,
    recalculate: bool,
) -> Result<OtherAssetTransaction> {
    data.validate()?;

    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM other_assets WHERE id = ?1",
            [asset_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AppError::NotFound(format!(
            "Other asset not found: {}",
            asset_id
        )));
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Normalize enum-like tx_type to lowercase so the stored value matches
    // both recalculate_asset_totals's exact "buy"/"sell" comparisons and the
    // dedup queries, which compare a lowercased tx_type against the stored
    // value (mirrors services/bank_accounts.rs::create_transaction).
    let tx_type = data.tx_type.to_lowercase();

    let transaction_date =
        crate::models::validation::utc_day(data.transaction_date, "validation.invalidDate")?;
    let quantity = crate::services::dedup::canonical_amount(&data.quantity);
    let price_per_unit = crate::services::dedup::canonical_amount(&data.price_per_unit);
    let currency = data.currency.to_ascii_uppercase();
    // Savepoints compose with the outer MCP batch transaction and also protect
    // standalone UI writes when historical FX or aggregate arithmetic fails.
    conn.execute_batch("SAVEPOINT other_asset_write")?;
    let write_result = (|| -> Result<()> {
        conn.execute(
            "INSERT INTO other_asset_transactions
             (id, asset_id, type, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id,
                asset_id,
                tx_type,
                quantity,
                price_per_unit,
                currency,
                transaction_date,
                now
            ],
        )?;
        if recalculate {
            recalculate_asset_totals(conn, asset_id)?;
        }
        conn.execute_batch("RELEASE other_asset_write")?;
        Ok(())
    })();
    if let Err(error) = write_result {
        conn.execute_batch("ROLLBACK TO other_asset_write; RELEASE other_asset_write")?;
        return Err(error);
    }

    Ok(OtherAssetTransaction {
        id,
        asset_id: asset_id.to_string(),
        tx_type,
        quantity,
        price_per_unit,
        currency,
        transaction_date,
        created_at: now,
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
            CREATE TABLE other_assets (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL DEFAULT '0',
                market_price TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'CZK',
                average_purchase_price TEXT NOT NULL DEFAULT '0',
                yield_type TEXT NOT NULL DEFAULT 'none',
                yield_value TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE other_asset_transactions (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL REFERENCES other_assets(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE other_asset_valuations (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL REFERENCES other_assets(id) ON DELETE CASCADE,
                value TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                valued_at INTEGER NOT NULL,
                note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn valid_asset() -> InsertOtherAsset {
        InsertOtherAsset {
            name: "Gold coins".into(),
            quantity: None,
            market_price: None,
            currency: None,
            average_purchase_price: None,
            yield_type: None,
            yield_value: None,
        }
    }

    fn valid_tx() -> InsertOtherAssetTransaction {
        InsertOtherAssetTransaction {
            asset_id: None,
            tx_type: "buy".into(),
            quantity: "2".into(),
            price_per_unit: "5000".into(),
            currency: "CZK".into(),
            transaction_date: 1_700_000_000,
        }
    }

    #[test]
    fn create_asset_applies_defaults() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        assert_eq!(asset.quantity, "0");
        assert_eq!(asset.currency, "CZK");
        assert_eq!(asset.yield_type, "none");
        assert_eq!(asset.average_purchase_price, "0");
    }

    /// `(value, currency, valued_at)` of every valuation of an asset, oldest first.
    fn valuation_rows(conn: &Connection, asset_id: &str) -> Vec<(String, String, i64)> {
        let mut stmt = conn
            .prepare(
                "SELECT value, currency, valued_at FROM other_asset_valuations
                 WHERE asset_id = ?1 ORDER BY valued_at, created_at",
            )
            .unwrap();
        let rows = stmt
            .query_map([asset_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn create_asset_writes_the_first_estimate() {
        let conn = setup_test_db();
        let mut data = valid_asset();
        data.market_price = Some("62000".into());
        data.currency = Some("EUR".into());

        let before = crate::services::loan_amortization::today_utc_day();
        let asset = create_asset(&conn, &data, None).unwrap();
        let after = crate::services::loan_amortization::today_utc_day();

        let rows = valuation_rows(&conn, &asset.id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "62000");
        assert_eq!(rows[0].1, "EUR");
        assert!(
            rows[0].2 == before || rows[0].2 == after,
            "dated at the UTC day of the creation"
        );
    }

    /// A valuation in another currency than the asset's: the listed price is the newest
    /// valuation in its own currency, while the asset's currency still prices the average.
    #[test]
    fn listed_price_is_the_newest_valuation_in_its_currency() {
        let conn = setup_test_db();
        let mut data = valid_asset();
        data.market_price = Some("62000".into());
        data.currency = Some("CZK".into());
        let gold = create_asset(&conn, &data, None).unwrap();
        assert_eq!(gold.market_price, "62000");
        assert_eq!(gold.market_price_currency, "CZK");
        valuations::add_valuation(
            &conn,
            ValuationKind::OtherAsset,
            &crate::models::InsertAssetValuation {
                asset_id: gold.id.clone(),
                value: "2500".into(),
                currency: Some("USD".into()),
                valued_at: gold.created_at + 86_400,
                note: None,
            },
        )
        .unwrap();
        data.name = "Unpriced".into();
        data.market_price = None;
        data.currency = Some("EUR".into());
        let unpriced = create_asset(&conn, &data, None).unwrap();

        let listed = list_assets(&conn).unwrap();
        let prices: Vec<(&str, &str, &str, &str)> = listed
            .iter()
            .map(|a| {
                (
                    a.name.as_str(),
                    a.market_price.as_str(),
                    a.market_price_currency.as_str(),
                    a.currency.as_str(),
                )
            })
            .collect();
        assert_eq!(
            prices,
            [
                ("Gold coins", "2500", "USD", "CZK"),
                ("Unpriced", "0", "EUR", "EUR")
            ]
        );
        let fetched = get_asset(&conn, &gold.id).unwrap();
        assert_eq!(
            (
                fetched.market_price.as_str(),
                fetched.market_price_currency.as_str()
            ),
            ("2500", "USD")
        );
        assert_eq!(get_asset(&conn, &unpriced.id).unwrap().market_price, "0");
        assert!(matches!(
            get_asset(&conn, "missing"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn create_asset_without_a_price_has_no_estimate_yet() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        assert!(valuation_rows(&conn, &asset.id).is_empty());

        let mut blank = valid_asset();
        blank.name = "Stamps".into();
        blank.market_price = Some("".into());
        let asset = create_asset(&conn, &blank, None).unwrap();
        assert!(valuation_rows(&conn, &asset.id).is_empty());
    }

    /// The asset, its first estimate and its initial transaction are saved
    /// together or not at all: a failed write must not leave an asset behind
    /// that a retry duplicates.
    #[test]
    fn create_asset_saves_nothing_when_a_write_fails() {
        let conn = setup_test_db();
        conn.execute_batch(
            "CREATE TRIGGER fail_valuation_insert BEFORE INSERT ON other_asset_valuations
             BEGIN SELECT RAISE(ABORT, 'valuation write failed'); END;",
        )
        .expect("trigger");
        let mut data = valid_asset();
        data.market_price = Some("62000".into());

        assert!(create_asset(&conn, &data, None).is_err());
        let assets: i64 = conn
            .query_row("SELECT COUNT(*) FROM other_assets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(assets, 0);
    }

    #[test]
    fn create_asset_rejects_invalid() {
        let conn = setup_test_db();
        let mut data = valid_asset();
        data.name = "".into();
        assert!(create_asset(&conn, &data, None).is_err());
    }

    #[test]
    fn create_asset_with_initial_transaction_recalculates() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), Some(&valid_tx())).unwrap();

        // Transaction row exists...
        let tx_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM other_asset_transactions WHERE asset_id = ?1",
                [&asset.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tx_count, 1);

        // Totals come from the recorded purchase.
        assert_eq!(asset.quantity, "2");
        assert_eq!(asset.average_purchase_price, "5000");
    }

    #[test]
    fn create_asset_transaction_recalculates_totals() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();

        let tx = create_asset_transaction(&conn, &asset.id, &valid_tx()).unwrap();
        assert_eq!(tx.asset_id, asset.id);

        let (quantity, average_purchase_price): (String, String) = conn
            .query_row(
                "SELECT quantity, average_purchase_price FROM other_assets WHERE id = ?1",
                [&asset.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(quantity, "2");
        assert_eq!(average_purchase_price, "5000");
    }

    #[test]
    fn create_asset_transaction_rejects_unknown_asset() {
        let conn = setup_test_db();
        let result = create_asset_transaction(&conn, "ghost-asset", &valid_tx());
        assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
    }

    #[test]
    fn create_asset_transaction_rejects_invalid_data() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        let mut data = valid_tx();
        data.quantity = "0".into();
        assert!(create_asset_transaction(&conn, &asset.id, &data).is_err());
    }

    #[test]
    fn create_asset_transaction_normalizes_type_to_lowercase() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        let mut data = valid_tx();
        data.tx_type = "BUY".into();

        let tx = create_asset_transaction(&conn, &asset.id, &data).unwrap();
        assert_eq!(
            tx.tx_type, "buy",
            "returned struct must carry the normalized value"
        );

        let stored_type: String = conn
            .query_row(
                "SELECT type FROM other_asset_transactions WHERE id = ?1",
                [&tx.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_type, "buy");

        // Mixed-case must still be visible to recalculate_asset_totals's
        // exact "buy" comparison.
        let quantity: String = conn
            .query_row(
                "SELECT quantity FROM other_assets WHERE id = ?1",
                [&asset.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quantity, "2");
    }

    #[test]
    fn create_asset_normalizes_initial_transaction_type_to_lowercase() {
        let conn = setup_test_db();
        let mut tx = valid_tx();
        tx.tx_type = "Buy".into();

        let asset = create_asset(&conn, &valid_asset(), Some(&tx)).unwrap();

        let stored_type: String = conn
            .query_row(
                "SELECT type FROM other_asset_transactions WHERE asset_id = ?1",
                [&asset.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_type, "buy");
    }
    #[test]
    fn audit_assets_initial_transaction_derives_totals() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), Some(&valid_tx())).unwrap();
        assert_eq!(asset.quantity, "2");
        assert_eq!(asset.average_purchase_price, "5000");
    }

    #[test]
    fn audit_assets_mixed_currency_costs_use_transaction_day_rates() {
        let conn = setup_test_db();
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (date INTEGER, currency TEXT, rate REAL);",
        )
        .unwrap();
        let mut asset_data = valid_asset();
        asset_data.currency = Some("EUR".into());
        let mut euro_buy = valid_tx();
        euro_buy.quantity = "1".into();
        euro_buy.price_per_unit = "10".into();
        euro_buy.currency = "EUR".into();
        let asset = create_asset(&conn, &asset_data, Some(&euro_buy)).unwrap();
        let mut crown_buy = euro_buy.clone();
        crown_buy.currency = "CZK".into();
        crown_buy.price_per_unit = "100".into();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1, 'CZK', 25)",
            [crown_buy.transaction_date.div_euclid(86_400) * 86_400],
        )
        .unwrap();
        create_asset_transaction(&conn, &asset.id, &crown_buy).unwrap();
        let avg: String = conn
            .query_row(
                "SELECT average_purchase_price FROM other_assets WHERE id=?1",
                [&asset.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(avg, "7");
    }

    #[test]
    fn audit_assets_missing_fx_rolls_back_the_transaction_and_totals() {
        let conn = setup_test_db();
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (date INTEGER, currency TEXT, rate REAL);",
        )
        .unwrap();
        let asset = create_asset(&conn, &valid_asset(), Some(&valid_tx())).unwrap();
        let mut foreign_buy = valid_tx();
        foreign_buy.currency = "EUR".into();
        assert!(create_asset_transaction(&conn, &asset.id, &foreign_buy).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM other_asset_transactions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        let qty: String = conn
            .query_row(
                "SELECT quantity FROM other_assets WHERE id=?1",
                [&asset.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(qty, "2");
    }

    #[test]
    fn audit_assets_overflow_rolls_back_initial_asset() {
        let conn = setup_test_db();
        let mut buy = valid_tx();
        buy.quantity = "1e308".into();
        buy.price_per_unit = "1e308".into();
        assert!(create_asset(&conn, &valid_asset(), Some(&buy)).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM other_assets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn audit_assets_transaction_dates_are_bounded_and_canonical() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        let tx = create_asset_transaction(&conn, &asset.id, &valid_tx()).unwrap();
        assert_eq!(tx.transaction_date, 1_699_920_000);
        let mut bad = valid_tx();
        bad.transaction_date = i64::MAX;
        assert!(create_asset_transaction(&conn, &asset.id, &bad).is_err());
    }

    #[test]
    fn audit_assets_missing_fx_in_nested_write_leaves_outer_transaction_usable() {
        let conn = setup_test_db();
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (date INTEGER, currency TEXT, rate REAL);",
        )
        .unwrap();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        let outer = conn.unchecked_transaction().unwrap();
        let mut foreign = valid_tx();
        foreign.currency = "EUR".into();
        assert!(create_asset_transaction(&outer, &asset.id, &foreign).is_err());
        create_asset_transaction(&outer, &asset.id, &valid_tx()).unwrap();
        outer.commit().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM other_asset_transactions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn audit_assets_transaction_rejects_overflowing_market_value() {
        let conn = setup_test_db();
        let mut data = valid_asset();
        data.market_price = Some("1e308".into());
        let asset = create_asset(&conn, &data, None).unwrap();
        let mut buy = valid_tx();
        buy.quantity = "2".into();
        buy.price_per_unit = "1".into();
        assert!(create_asset_transaction(&conn, &asset.id, &buy).is_err());
        let totals: (String, i64) = conn.query_row("SELECT quantity, (SELECT COUNT(*) FROM other_asset_transactions) FROM other_assets WHERE id=?1", [&asset.id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(totals, ("0".into(), 0));
    }
    #[test]
    fn audit_assets_initial_sale_is_rejected_atomically() {
        let conn = setup_test_db();
        let mut sale = valid_tx();
        sale.tx_type = "sell".into();
        assert!(create_asset(&conn, &valid_asset(), Some(&sale)).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM other_assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn audit_assets_manual_backdated_sale_preserves_future_sales() {
        let conn = setup_test_db();
        let mut buy = valid_tx();
        buy.quantity = "1".into();
        let asset = create_asset(&conn, &valid_asset(), Some(&buy)).unwrap();
        let mut sale = buy.clone();
        sale.tx_type = "sell".into();
        sale.transaction_date += 2 * 86400;
        create_asset_transaction(&conn, &asset.id, &sale).unwrap();
        sale.transaction_date -= 86400;
        assert!(create_asset_transaction(&conn, &asset.id, &sale).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM other_asset_transactions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }
    #[test]
    fn audit_assets_exact_liquidation_leaves_zero_quantity() {
        let conn = setup_test_db();
        let mut buy = valid_tx();
        buy.quantity = "0.1".into();
        let asset = create_asset(&conn, &valid_asset(), Some(&buy)).unwrap();
        buy.quantity = "0.2".into();
        create_asset_transaction(&conn, &asset.id, &buy).unwrap();
        let mut sale = buy.clone();
        sale.tx_type = "sell".into();
        sale.quantity = "0.3".into();
        let written = create_asset_transaction(&conn, &asset.id, &sale);
        assert!(written.is_ok());
        let quantity: String = conn
            .query_row(
                "SELECT quantity FROM other_assets WHERE id=?1",
                [&asset.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quantity, "0");
    }
}
