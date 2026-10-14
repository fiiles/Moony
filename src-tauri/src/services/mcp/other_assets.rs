//! MCP tools: other assets. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::services::{dedup, other_assets as other_assets_service};

use super::money::currency_or_main;
use super::{BulkWriteReport, RowError, SkippedRow, MAX_BULK_ROWS};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OtherAssetsTransactionsArgs {
    #[serde(rename = "assetId")]
    #[schemars(description = "The asset ID to get transactions for")]
    pub asset_id: String,
}

/// Every asset by name, priced like today's net worth (`other_assets::list_assets`):
/// `marketPrice` is in `marketPriceCurrency`, the purchase average in `currency`.
pub fn other_assets_list(conn: &Connection) -> Result<Value> {
    let assets = other_assets_service::list_assets(conn)?;
    Ok(serde_json::to_value(assets)?)
}

pub fn other_assets_transactions(conn: &Connection, asset_id: &str) -> Result<Value> {
    let asset_id = asset_id.to_string();
    let mut stmt = conn.prepare(
        "SELECT id, asset_id, type, quantity, price_per_unit, currency,
                    transaction_date, created_at
             FROM other_asset_transactions WHERE asset_id = ?
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
    )?;
    let rows: Vec<Value> = stmt
        .query_map([&asset_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "assetId": row.get::<_, String>(1)?,
                "type": row.get::<_, String>(2)?,
                "quantity": row.get::<_, String>(3)?,
                "pricePerUnit": row.get::<_, String>(4)?,
                "currency": row.get::<_, String>(5)?,
                "transactionDate": row.get::<_, i64>(6)?,
                "createdAt": row.get::<_, i64>(7)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Value::Array(rows))
}

// ============================================================================
// Write tools (Task 10: single-create entity tool + bulk transaction import)
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OtherAssetTransactionRow {
    #[serde(rename = "type")]
    #[schemars(description = "buy or sell")]
    pub tx_type: String,
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    pub price_per_unit: String,
    pub currency: String,
    #[serde(rename = "transactionDate")]
    #[schemars(description = "Unix timestamp (seconds)")]
    pub transaction_date: i64,
    #[serde(default, rename = "importAnyway")]
    #[schemars(
        description = "Import an identical-looking transaction only after the user confirms it is a distinct trade"
    )]
    pub import_anyway: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OtherAssetCreateArgs {
    pub asset: crate::models::InsertOtherAsset,
    // Typed as the row shape (not the raw InsertOtherAssetTransaction model)
    // so the LLM-facing schema never leaks the silently-ignored `assetId`
    // field and keeps the per-field docs on OtherAssetTransactionRow.
    #[serde(rename = "initialTransaction")]
    pub initial_transaction: Option<OtherAssetTransactionRow>,
}

pub fn other_asset_create(conn: &Connection, args: &OtherAssetCreateArgs) -> Result<Value> {
    let dup: Option<String> = conn
        .query_row(
            "SELECT id FROM other_assets WHERE LOWER(name) = LOWER(?1)",
            [&args.asset.name],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = dup {
        return Err(AppError::Validation(format!(
            "An asset with this name already exists (id: {})",
            id
        )));
    }
    let initial_transaction = args.initial_transaction.as_ref().map(row_to_insert);
    // The service defaults to CZK for the UI path (ADR 0007); an MCP client
    // that leaves the currency out means the user's own currency.
    let mut data = args.asset.clone();
    data.currency = Some(currency_or_main(conn, data.currency.as_deref())?);
    let asset = other_assets_service::create_asset(conn, &data, initial_transaction.as_ref())?;
    Ok(serde_json::to_value(asset)?)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OtherAssetTransactionsCreateArgs {
    #[serde(rename = "assetId")]
    #[schemars(description = "Existing asset id (see other_assets_list)")]
    pub asset_id: String,
    #[schemars(length(max = MAX_BULK_ROWS))]
    pub transactions: Vec<OtherAssetTransactionRow>,
}

fn row_to_insert(row: &OtherAssetTransactionRow) -> crate::models::InsertOtherAssetTransaction {
    crate::models::InsertOtherAssetTransaction {
        asset_id: None,
        tx_type: row.tx_type.clone(),
        quantity: row.quantity.clone(),
        price_per_unit: row.price_per_unit.clone(),
        currency: row.currency.clone(),
        transaction_date: row.transaction_date,
    }
}

/// Two-phase bulk import mirroring `bank_transactions_create` (services/mcp/bank_accounts.rs).
/// Phase 1 (no writes): the asset must exist (AppError::NotFound otherwise)
/// and every row must field-validate — any failure returns the full error
/// list with created == 0. Phase 2: one SQL transaction; duplicate rows
/// (dedup::find_duplicate_other_asset_transaction) are skipped and reported;
/// everything else is inserted via services::other_assets::create_asset_transaction; commit.
///
/// Note: `create_asset_transaction` recalculates the asset's aggregate
/// quantity/average price by rescanning ALL of its transactions on every
/// call, so this loop is O(N^2) over the batch size — accepted because import
/// batches are small (tens to hundreds of rows, one asset at a time).
pub fn other_asset_transactions_create(
    conn: &mut rusqlite::Connection,
    args: &OtherAssetTransactionsCreateArgs,
) -> Result<BulkWriteReport> {
    super::check_bulk_size(args.transactions.len())?;

    let asset_exists: Option<String> = conn
        .query_row(
            "SELECT id FROM other_assets WHERE id = ?1",
            [&args.asset_id],
            |row| row.get(0),
        )
        .optional()?;
    if asset_exists.is_none() {
        return Err(AppError::NotFound(format!(
            "Other asset not found: {}",
            args.asset_id
        )));
    }

    let mut errors: Vec<RowError> = Vec::new();
    for (i, row) in args.transactions.iter().enumerate() {
        if let Err(e) = row_to_insert(row).validate() {
            errors.push(RowError {
                index: i,
                message: super::error_text::render(&e),
            });
        }
    }
    if !errors.is_empty() {
        return Ok(BulkWriteReport {
            import_batch_id: None,
            promoted: 0,
            created: 0,
            skipped_duplicates: vec![],
            errors,
        });
    }

    // Dedup runs INSIDE the transaction, so it also catches duplicates within
    // the batch itself: after row 3 inserts, an identical row 7 matches it.
    let tx = conn.transaction()?;
    let mut created = 0usize;
    let mut last_created_index = 0;
    let mut skipped: Vec<SkippedRow> = Vec::new();
    for (i, row) in args.transactions.iter().enumerate() {
        if let Some(reason) = dedup::find_duplicate_other_asset_transaction(
            &tx,
            &args.asset_id,
            crate::models::validation::utc_day(row.transaction_date, "validation.invalidDate")?,
            &row.tx_type.to_lowercase(),
            &row.quantity,
            &row.price_per_unit,
            &row.currency,
        )? {
            if !row.import_anyway {
                skipped.push(SkippedRow::possible_duplicate(i, reason));
                continue;
            }
        }
        if let Err(error) = other_assets_service::create_asset_transaction_in_batch(
            &tx,
            &args.asset_id,
            &row_to_insert(row),
        ) {
            // The caller gets the offending input index, and no prior row or
            // duplicate classification is reported as committed after rollback.
            tx.rollback()?;
            return Ok(BulkWriteReport {
                import_batch_id: None,
                promoted: 0,
                created: 0,
                skipped_duplicates: vec![],
                errors: vec![RowError {
                    index: i,
                    message: super::error_text::render(&error),
                }],
            });
        }
        created += 1;
        last_created_index = i;
    }
    if created > 0 {
        if let Err(error) = other_assets_service::recalculate_asset_totals(&tx, &args.asset_id) {
            let index = crate::services::investments::oversold_day(
                &tx,
                "other_asset_transactions",
                &args.asset_id,
            )?
            .and_then(|day| {
                crate::services::investments::blamed_sell_row(
                    args.transactions.iter().enumerate().map(|(i, row)| {
                        (
                            i,
                            row.transaction_date.div_euclid(86_400) * 86_400,
                            row.tx_type.eq_ignore_ascii_case("sell"),
                        )
                    }),
                    day,
                )
            })
            .unwrap_or(last_created_index);
            tx.rollback()?;
            return Ok(BulkWriteReport {
                import_batch_id: None,
                promoted: 0,
                created: 0,
                skipped_duplicates: vec![],
                errors: vec![RowError {
                    index,
                    message: super::error_text::render(&error),
                }],
            });
        }
    }
    tx.commit()?;

    Ok(BulkWriteReport {
        import_batch_id: None,
        promoted: 0,
        created,
        skipped_duplicates: skipped,
        errors: vec![],
    })
}

/// Earliest transaction date in this batch, for the data-changed event payload.
/// A date before today means the import was retroactive, so the portfolio history
/// has to be recalculated (mirrors `investments::earliest_date`). Other assets have
/// no tickers, so this is the only scope the recalculation needs.
pub fn earliest_date(args: &OtherAssetTransactionsCreateArgs) -> Option<i64> {
    args.transactions
        .iter()
        .filter_map(|r| {
            crate::models::validation::utc_day(r.transaction_date, "validation.invalidDate").ok()
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InsertOtherAsset;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
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

    /// The list prices an asset like today's net worth: the newest valuation in its own
    /// currency, else the market price in the asset's currency.
    #[test]
    fn list_prices_an_asset_in_the_currency_of_its_newest_valuation() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO other_assets (id, name, quantity, market_price, currency)
             VALUES ('gold', 'Gold', '2', '2500', 'CZK'), ('car', 'Car', '1', '4000', 'EUR');
             INSERT INTO other_asset_valuations (id, asset_id, value, currency, valued_at, created_at)
             VALUES ('v1', 'gold', '62000', 'CZK', 1700006400, 1),
                    ('v2', 'gold', '2500', 'USD', 1700092800, 2);",
        )
        .unwrap();

        let listed = other_assets_list(&conn).unwrap();
        let listed = listed.as_array().unwrap();
        assert_eq!(listed[0]["name"], "Car");
        assert_eq!(listed[0]["marketPrice"], "4000");
        assert_eq!(listed[0]["marketPriceCurrency"], "EUR");
        assert_eq!(listed[1]["name"], "Gold");
        assert_eq!(listed[1]["marketPrice"], "2500");
        assert_eq!(listed[1]["marketPriceCurrency"], "USD");
        assert_eq!(
            listed[1]["currency"], "CZK",
            "the purchase average's currency"
        );
    }

    fn valid_asset(name: &str) -> InsertOtherAsset {
        InsertOtherAsset {
            name: name.to_string(),
            quantity: None,
            market_price: None,
            currency: None,
            average_purchase_price: None,
            yield_type: None,
            yield_value: None,
        }
    }

    fn seed_asset(conn: &Connection, name: &str) -> String {
        let value = other_asset_create(
            conn,
            &OtherAssetCreateArgs {
                asset: valid_asset(name),
                initial_transaction: None,
            },
        )
        .unwrap();
        value["id"].as_str().unwrap().to_string()
    }

    fn tx_row(tx_type: &str, quantity: &str, price: &str, date: i64) -> OtherAssetTransactionRow {
        OtherAssetTransactionRow {
            tx_type: tx_type.to_string(),
            quantity: quantity.to_string(),
            price_per_unit: price.to_string(),
            currency: "CZK".to_string(),
            transaction_date: date,
            import_anyway: false,
        }
    }

    fn asset_totals(conn: &Connection, asset_id: &str) -> (String, String) {
        conn.query_row(
            "SELECT quantity, average_purchase_price FROM other_assets WHERE id = ?1",
            [asset_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    fn tx_count(conn: &Connection, asset_id: &str) -> i64 {
        conn.query_row(
            "SELECT count(*) FROM other_asset_transactions WHERE asset_id = ?1",
            [asset_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    // ---- other_asset_create -------------------------------------------

    #[test]
    fn asset_create_happy_path() {
        let conn = setup_test_db();
        let value = other_asset_create(
            &conn,
            &OtherAssetCreateArgs {
                asset: valid_asset("Gold coins"),
                initial_transaction: None,
            },
        )
        .unwrap();
        assert_eq!(value["name"], "Gold coins");
        assert_eq!(value["currency"], "CZK");
        assert_eq!(value["yieldType"], "none");
    }

    #[test]
    fn asset_create_defaults_a_missing_currency_to_the_main_currency() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO user_profile (currency) VALUES ('EUR')", [])
            .unwrap();

        let value = other_asset_create(
            &conn,
            &OtherAssetCreateArgs {
                asset: valid_asset("Silver bars"),
                initial_transaction: None,
            },
        )
        .unwrap();

        assert_eq!(value["currency"], "EUR");
    }

    #[test]
    fn asset_create_keeps_an_explicit_currency() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO user_profile (currency) VALUES ('EUR')", [])
            .unwrap();
        let mut asset = valid_asset("Dollar art");
        asset.currency = Some("USD".into());

        let value = other_asset_create(
            &conn,
            &OtherAssetCreateArgs {
                asset,
                initial_transaction: None,
            },
        )
        .unwrap();

        assert_eq!(value["currency"], "USD");
    }

    #[test]
    fn asset_create_rejects_duplicate_name_case_insensitive_and_reports_existing_id() {
        let conn = setup_test_db();
        let existing_id = seed_asset(&conn, "Gold coins");

        let err = other_asset_create(
            &conn,
            &OtherAssetCreateArgs {
                asset: valid_asset("gold coins"),
                initial_transaction: None,
            },
        )
        .unwrap_err();
        match err {
            AppError::Validation(msg) => assert!(
                msg.contains(&existing_id),
                "error should carry the existing asset id: {msg}"
            ),
            other => panic!("expected Validation error, got {other:?}"),
        }
    }

    // ---- other_asset_transactions_create -------------------------------

    #[test]
    fn transactions_create_rejects_unknown_asset() {
        let mut conn = setup_test_db();
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: "no-such-asset".into(),
            transactions: vec![tx_row("buy", "1", "1000", 1_700_000_000)],
        };
        assert!(matches!(
            other_asset_transactions_create(&mut conn, &args).unwrap_err(),
            AppError::NotFound(_)
        ));
    }

    #[test]
    fn transactions_create_rejects_oversized_batch() {
        let mut conn = setup_test_db();
        let asset_id = seed_asset(&conn, "Gold coins");
        let oversized_count = MAX_BULK_ROWS + 1;
        let mut transactions = Vec::with_capacity(oversized_count);
        for i in 0..oversized_count {
            transactions.push(tx_row("buy", "1", "1000", 1_700_000_000 + i as i64));
        }
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions,
        };
        let err = other_asset_transactions_create(&mut conn, &args).unwrap_err();
        match err {
            AppError::Validation(msg) => {
                assert!(
                    msg.contains(&oversized_count.to_string()),
                    "message must state the actual count: {msg}"
                );
                assert!(
                    msg.contains(&MAX_BULK_ROWS.to_string()),
                    "message must state the cap: {msg}"
                );
            }
            other => panic!("expected Validation error, got {other:?}"),
        }
        assert_eq!(
            tx_count(&conn, &asset_id),
            0,
            "an oversized batch must write nothing"
        );
    }

    #[test]
    fn transactions_create_phase1_rejection_writes_nothing() {
        let mut conn = setup_test_db();
        let asset_id = seed_asset(&conn, "Gold coins");
        let mut bad = tx_row("banana", "1", "1000", 1_700_000_000); // invalid type
        bad.quantity = "1".into();
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![
                bad,
                tx_row("buy", "2", "5000", 1_700_000_100), // otherwise valid
            ],
        };
        let report = other_asset_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert!(report.skipped_duplicates.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 0);
        assert_eq!(
            tx_count(&conn, &asset_id),
            0,
            "good row must not be inserted either"
        );
    }

    #[test]
    fn transactions_create_happy_path_recalculates_totals() {
        let mut conn = setup_test_db();
        let asset_id = seed_asset(&conn, "Gold coins");
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![
                tx_row("buy", "2", "5000", 1_700_000_000),
                tx_row("buy", "1", "6000", 1_700_000_100),
            ],
        };
        let report = other_asset_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2);
        assert!(report.skipped_duplicates.is_empty());
        assert!(report.errors.is_empty());
        assert_eq!(tx_count(&conn, &asset_id), 2);

        let (quantity, average) = asset_totals(&conn, &asset_id);
        assert_eq!(quantity, "3");
        // (2*5000 + 1*6000) / 3 = 5333.33...
        let avg: f64 = average.parse().unwrap();
        assert!((avg - 5333.333333333333).abs() < 1e-6, "avg was {average}");
    }

    /// Carry-forward hardening note: lock the sell branch of
    /// `recalculate_asset_totals` via the bulk path — buy 2@5000 then sell
    /// 1@6000 should leave quantity at 1 while the average purchase price
    /// (computed only from buys) stays at 5000.
    #[test]
    fn transactions_create_buy_then_sell_locks_sell_branch_of_recalc() {
        let mut conn = setup_test_db();
        let asset_id = seed_asset(&conn, "Silver bars");
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![
                tx_row("buy", "2", "5000", 1_700_000_000),
                tx_row("sell", "1", "6000", 1_700_000_100),
            ],
        };
        let report = other_asset_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2, "errors: {:?}", report.errors);
        assert!(report.errors.is_empty());

        let (quantity, average) = asset_totals(&conn, &asset_id);
        assert_eq!(quantity, "1");
        assert_eq!(average, "5000");
    }

    #[test]
    fn transactions_create_skips_duplicates_including_intra_batch() {
        let mut conn = setup_test_db();
        let asset_id = seed_asset(&conn, "Gold coins");
        let first = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![tx_row("buy", "2", "5000", 1_700_000_000)],
        };
        assert_eq!(
            other_asset_transactions_create(&mut conn, &first)
                .unwrap()
                .created,
            1
        );

        let second = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![
                tx_row("buy", "2", "5000", 1_700_000_000), // dup of existing
                tx_row("buy", "1", "7000", 1_700_000_500),
                tx_row("buy", "1", "7000", 1_700_000_500), // intra-batch dup
            ],
        };
        let report = other_asset_transactions_create(&mut conn, &second).unwrap();
        assert_eq!(report.created, 1);
        assert!(report.errors.is_empty());
        assert_eq!(report.skipped_duplicates.len(), 2);
        assert_eq!(report.skipped_duplicates[0].index, 0);
        assert_eq!(report.skipped_duplicates[1].index, 2);
        assert_eq!(tx_count(&conn, &asset_id), 2);
    }

    /// Locking test for the CRITICAL fix: a mixed-case `type` must be
    /// normalized on insert so it's visible to `recalculate_asset_totals`'s
    /// exact "buy"/"sell" comparisons, and a re-import of the same row in
    /// canonical lowercase must be caught as a duplicate (mirrors the bank
    /// engine's `create_stores_lowercase_so_reimport_matches`).
    #[test]
    fn transactions_create_stores_lowercase_so_reimport_matches() {
        let mut conn = setup_test_db();
        let asset_id = seed_asset(&conn, "Gold coins");
        let first = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![tx_row("Buy", "2", "5000", 1_700_000_000)],
        };
        let report = other_asset_transactions_create(&mut conn, &first).unwrap();
        assert_eq!(report.created, 1, "errors: {:?}", report.errors);
        let stored_type: String = conn
            .query_row(
                "SELECT type FROM other_asset_transactions WHERE asset_id = ?1",
                [&asset_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            stored_type, "buy",
            "mixed-case input is normalized on insert"
        );

        // Totals must reflect the mixed-case buy (locks the recalc's exact
        // "buy" comparison against the now-normalized stored value).
        let (quantity, average) = asset_totals(&conn, &asset_id);
        assert_eq!(quantity, "2");
        assert_eq!(average, "5000");

        // Re-import of the same row in canonical lowercase must be caught.
        let second = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![tx_row("buy", "2", "5000", 1_700_000_000)],
        };
        let report = other_asset_transactions_create(&mut conn, &second).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(tx_count(&conn, &asset_id), 1);
    }

    #[test]
    fn transactions_create_rollback_leaves_no_partial_state() {
        // A batch whose SECOND row fails phase-1 validation must leave the
        // first row's would-be insert un-applied too (all-or-nothing).
        let mut conn = setup_test_db();
        let asset_id = seed_asset(&conn, "Gold coins");
        let mut bad = tx_row("sell", "0", "5000", 1_700_000_100); // quantity must be > 0
        bad.quantity = "0".into();
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: asset_id.clone(),
            transactions: vec![tx_row("buy", "2", "5000", 1_700_000_000), bad],
        };
        let report = other_asset_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 1);
        assert_eq!(tx_count(&conn, &asset_id), 0);
        let (quantity, _) = asset_totals(&conn, &asset_id);
        assert_eq!(quantity, "0");
    }

    // ---- earliest_date -------------------------------------------------

    #[test]
    fn earliest_date_picks_the_minimum() {
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: "asset1".into(),
            transactions: vec![
                tx_row("buy", "1", "1000", 500),
                tx_row("buy", "1", "1000", 100),
                tx_row("buy", "1", "1000", 300),
            ],
        };
        assert_eq!(earliest_date(&args), Some(0));
    }

    #[test]
    fn earliest_date_is_none_for_an_empty_batch() {
        let args = OtherAssetTransactionsCreateArgs {
            asset_id: "asset1".into(),
            transactions: vec![],
        };
        assert_eq!(earliest_date(&args), None);
    }
    #[test]
    fn audit_assets_possible_duplicates_can_be_imported_explicitly() {
        let mut conn = setup_test_db();
        let id = seed_asset(&conn, "Gold coins");
        let args: OtherAssetTransactionsCreateArgs = serde_json::from_value(serde_json::json!({
            "assetId": id,
            "transactions": [{"type":"buy", "quantity":"2", "pricePerUnit":"5000", "currency":"CZK", "transactionDate":1700000000}]
        })).unwrap();
        assert_eq!(
            other_asset_transactions_create(&mut conn, &args)
                .unwrap()
                .created,
            1
        );
        let duplicate = other_asset_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(duplicate.skipped_duplicates.len(), 1);
        assert!(duplicate.skipped_duplicates[0].possible_duplicate);
        let confirmed: OtherAssetTransactionsCreateArgs = serde_json::from_value(serde_json::json!({
            "assetId": id,
            "transactions": [{"type":"buy", "quantity":"2.00", "pricePerUnit":"5000.0", "currency":"CZK", "transactionDate":1700000000, "importAnyway": true}]
        })).unwrap();
        assert_eq!(
            other_asset_transactions_create(&mut conn, &confirmed)
                .unwrap()
                .created,
            1
        );
        assert_eq!(tx_count(&conn, &id), 2);
    }

    #[test]
    fn audit_assets_missing_fx_batch_returns_row_error_and_rolls_back() {
        let mut conn = setup_test_db();
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (date INTEGER, currency TEXT, rate REAL);",
        )
        .unwrap();
        let id = seed_asset(&conn, "Gold coins");
        let mut foreign = tx_row("buy", "1", "10", 1_700_000_000);
        foreign.currency = "EUR".into();
        let report = other_asset_transactions_create(
            &mut conn,
            &OtherAssetTransactionsCreateArgs {
                asset_id: id.clone(),
                transactions: vec![tx_row("buy", "2", "5000", 1_700_000_000), foreign],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 1);
        let message = &report.errors[0].message;
        assert!(message.contains("exchange rates for its date"), "{message}");
        assert!(!message.contains("validation."), "{message}");
        assert_eq!(tx_count(&conn, &id), 0);
        assert_eq!(asset_totals(&conn, &id), ("0".into(), "0".into()));
    }

    #[test]
    fn audit_assets_dedup_canonicalizes_numbers_days_and_keeps_distinct_currency() {
        let mut conn = setup_test_db();
        conn.execute_batch("CREATE TABLE exchange_rate_history (date INTEGER, currency TEXT, rate REAL); INSERT INTO exchange_rate_history VALUES (1699920000, 'CZK', 25);").unwrap();
        let id = seed_asset(&conn, "Gold coins");
        let first = OtherAssetTransactionsCreateArgs {
            asset_id: id.clone(),
            transactions: vec![tx_row("Buy", "2", "5000", 1_700_000_000)],
        };
        assert_eq!(
            other_asset_transactions_create(&mut conn, &first)
                .unwrap()
                .created,
            1
        );
        let mut foreign = tx_row("buy", "2", "5000", 1_700_000_001);
        foreign.currency = "EUR".into();
        let report = other_asset_transactions_create(
            &mut conn,
            &OtherAssetTransactionsCreateArgs {
                asset_id: id.clone(),
                transactions: vec![tx_row("buy", "2.00", "5e3", 1_700_000_001), foreign],
            },
        )
        .unwrap();
        assert_eq!(report.created, 1);
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(report.skipped_duplicates[0].index, 0);
        assert_eq!(tx_count(&conn, &id), 2);
    }
    #[test]
    fn audit_assets_bulk_oversell_rolls_back_and_validates_merged_backdates() {
        let mut conn = setup_test_db();
        let id = seed_asset(&conn, "Gold");
        let mut args = OtherAssetTransactionsCreateArgs {
            asset_id: id.clone(),
            transactions: vec![
                tx_row("buy", "1", "10", 1700000000),
                tx_row("sell", "2", "10", 1700086400),
            ],
        };
        let rejected = other_asset_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(rejected.created, 0);
        assert!(!rejected.errors.is_empty());
        assert_eq!(tx_count(&conn, &id), 0);
        // The error names the oversell even when a later-listed row was written last.
        args.transactions = vec![
            tx_row("sell", "2", "10", 1700086400),
            tx_row("buy", "1", "10", 1700000000),
        ];
        let rejected = other_asset_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(rejected.errors.len(), 1);
        assert_eq!(rejected.errors[0].index, 0);
        assert_eq!(tx_count(&conn, &id), 0);
        args.transactions = vec![
            tx_row("buy", "1", "10", 1700000000),
            tx_row("sell", "1", "10", 1700172800),
        ];
        assert_eq!(
            other_asset_transactions_create(&mut conn, &args)
                .unwrap()
                .created,
            2
        );
        // The new sale initially invalidates the stored later sale; its same-day
        // funding buy completes a valid merged history, regardless of input order.
        args.transactions = vec![
            tx_row("sell", "1", "10", 1700086400),
            tx_row("buy", "1", "10", 1700086400),
        ];
        assert_eq!(
            other_asset_transactions_create(&mut conn, &args)
                .unwrap()
                .created,
            2
        );
        assert_eq!(asset_totals(&conn, &id).0, "0");
        args.transactions = vec![tx_row("sell", "0.00000000000000000001", "10", 1700086400)];
        assert_eq!(
            other_asset_transactions_create(&mut conn, &args)
                .unwrap()
                .created,
            0
        );
        assert_eq!(tx_count(&conn, &id), 4);
    }
}
