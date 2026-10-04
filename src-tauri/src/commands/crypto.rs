//! Crypto commands

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    CryptoInvestment, CryptoTransaction, EnrichedCryptoInvestment, InsertCryptoInvestment,
    InsertCryptoTransaction,
};
use crate::services::crypto_investments as crypto_service;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Get all crypto investments with current prices
#[tauri::command]
pub async fn get_all_crypto(db: State<'_, Database>) -> Result<Vec<EnrichedCryptoInvestment>> {
    db.with_conn(|conn| {
        use crate::services::pricing;

        // Average price derives from transactions at their day's rates —
        // crypto positions may mix transaction currencies.
        let avg_map = crate::services::cost_basis::cost_basis_for_all(
            conn,
            crate::services::cost_basis::TxTable::Crypto,
        )?;
        let mut stmt = conn.prepare(
            "SELECT id, ticker, coingecko_id, name, quantity, currency FROM crypto_investments",
        )?;

        let investments: Vec<(CryptoInvestment, String)> = stmt
            .query_map([], |row| {
                Ok((
                    CryptoInvestment {
                        id: row.get(0)?,
                        ticker: row.get(1)?,
                        coingecko_id: row.get(2)?,
                        name: row.get(3)?,
                        quantity: row.get(4)?,
                        average_price: String::new(),
                    },
                    row.get::<_, Option<String>>(5)?
                        .unwrap_or_else(|| "USD".to_string()),
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(mut inv, ccy)| {
                inv.average_price = avg_map
                    .get(&inv.id)
                    .map(|p| p.average_price_native().to_string())
                    .unwrap_or_else(|| "0".to_string());
                (inv, ccy)
            })
            .collect();

        let mut enriched = Vec::new();
        for (inv, avg_price_currency) in investments {
            // Determine active price using unified resolver
            let resolved = pricing::resolve_crypto_price(conn, &inv.ticker);
            let (original_price, currency, current_price, fetched_at, is_manual) = match resolved {
                Some(rp) => (
                    rp.original_price,
                    rp.currency,
                    rp.price_czk.to_string(),
                    rp.fetched_at,
                    rp.is_manual,
                ),
                None => (
                    "0".to_string(),
                    "USD".to_string(),
                    "0".to_string(),
                    None,
                    false,
                ),
            };

            enriched.push(EnrichedCryptoInvestment {
                id: inv.id,
                ticker: inv.ticker,
                coingecko_id: inv.coingecko_id,
                name: inv.name,
                quantity: inv.quantity,
                average_price: inv.average_price,
                average_price_currency: avg_price_currency,
                current_price,
                original_price,
                currency,
                fetched_at,
                is_manual_price: is_manual,
            });
        }

        Ok(enriched)
    })
}

/// Create crypto investment
#[tauri::command]
pub async fn create_crypto(
    db: State<'_, Database>,
    app: AppHandle,
    data: InsertCryptoInvestment,
    initial_transaction: Option<InsertCryptoTransaction>,
) -> Result<CryptoInvestment> {
    // 1. Validate inputs at the trust boundary
    data.validate()?;
    if let Some(ref tx) = initial_transaction {
        tx.validate()?;
    }

    let ticker = data.ticker.to_uppercase();
    let coingecko_id = data.coingecko_id.clone();
    let initial_tx_date = initial_transaction.as_ref().map(|t| t.transaction_date);

    // 2. Delegate to service layer
    let inv = db.with_conn(|conn| {
        crypto_service::create_crypto_with_transaction(
            conn,
            &ticker,
            Some(&data.coingecko_id),
            &data.name,
            initial_transaction.as_ref(),
        )
    })?;

    // 3. Handle side effects (portfolio updates)
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // 4. Trigger per-ticker historical recalculation if there was an initial transaction
    if let Some(date) = initial_tx_date {
        crate::commands::portfolio::trigger_historical_recalculation_for_crypto_ticker(
            &db,
            date,
            &ticker,
            &coingecko_id,
        )
        .await
        .ok();

        app.emit("recalculation-complete", ()).ok();
    }

    Ok(inv)
}

/// Delete crypto investment
#[tauri::command]
pub async fn delete_crypto(db: State<'_, Database>, app: AppHandle, id: String) -> Result<()> {
    // Get earliest transaction date and crypto info before deleting (for historical recalc)
    let crypto_info: Option<(i64, String, String)> = db.with_conn(|conn| {
        // Get ticker and coingecko_id from the investment
        let info: Option<(String, String)> = conn
            .query_row(
                "SELECT ticker, coingecko_id FROM crypto_investments WHERE id = ?1",
                [&id],
                |row| Ok((row.get(0)?, row.get::<_, Option<String>>(1)?.unwrap_or_default())),
            )
            .ok();

        if let Some((ticker, coingecko_id)) = info {
            // Get earliest transaction date
            let earliest: Option<i64> = conn
                .query_row(
                    "SELECT MIN(transaction_date) FROM crypto_transactions WHERE investment_id = ?1",
                    [&id],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            if let Some(date) = earliest {
                return Ok(Some((date, ticker, coingecko_id)));
            }
        }
        Ok(None)
    })?;

    db.with_conn(|conn| {
        // Delete transactions first to avoid FK constraint violation
        conn.execute(
            "DELETE FROM crypto_transactions WHERE investment_id = ?1",
            [&id],
        )?;

        let changes = conn.execute("DELETE FROM crypto_investments WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Crypto investment not found".into()));
        }
        Ok(())
    })?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // Trigger per-ticker historical recalculation if there were transactions
    if let Some((tx_date, ticker, coingecko_id)) = crypto_info {
        if !coingecko_id.is_empty() {
            crate::commands::portfolio::trigger_historical_recalculation_for_crypto_ticker(
                &db,
                tx_date,
                &ticker,
                &coingecko_id,
            )
            .await
            .ok();
        }

        app.emit("recalculation-complete", ()).ok();
    }

    Ok(())
}

/// Get transactions for crypto
#[tauri::command]
pub async fn get_crypto_transactions(
    db: State<'_, Database>,
    investment_id: String,
) -> Result<Vec<CryptoTransaction>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM crypto_transactions WHERE investment_id = ?1
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
        )?;

        let txs = stmt
            .query_map([&investment_id], |row| {
                Ok(CryptoTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    name: row.get(4)?,
                    quantity: row.get(5)?,
                    price_per_unit: row.get(6)?,
                    currency: row.get(7)?,
                    transaction_date: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(txs)
    })
}

/// Get all crypto transactions across all investments
#[tauri::command]
pub async fn get_all_crypto_transactions(
    db: State<'_, Database>,
) -> Result<Vec<CryptoTransaction>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM crypto_transactions
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
        )?;

        let txs = stmt
            .query_map([], |row| {
                Ok(CryptoTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    name: row.get(4)?,
                    quantity: row.get(5)?,
                    price_per_unit: row.get(6)?,
                    currency: row.get(7)?,
                    transaction_date: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(txs)
    })
}

/// Create crypto transaction
#[tauri::command]
pub async fn create_crypto_transaction(
    db: State<'_, Database>,
    app: AppHandle,
    investment_id: String,
    data: InsertCryptoTransaction,
) -> Result<CryptoTransaction> {
    // 1. Validate inputs at the trust boundary
    data.validate()?;

    // Get coingecko_id for per-ticker recalculation
    let coingecko_id: Option<String> = db.with_conn(|conn| {
        Ok(conn
            .query_row(
                "SELECT coingecko_id FROM crypto_investments WHERE id = ?1",
                [&investment_id],
                |row| row.get(0),
            )
            .ok()
            .flatten())
    })?;

    // 2. Delegate to service layer
    let result = db
        .with_conn(|conn| crypto_service::add_transaction_to_crypto(conn, &investment_id, &data))?;

    // 3. Handle side effects (portfolio updates)
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // 4. Trigger per-ticker historical recalculation if transaction is retrospective
    if let Some(cg_id) = coingecko_id {
        crate::commands::portfolio::trigger_historical_recalculation_for_crypto_ticker(
            &db,
            result.transaction_date,
            &result.ticker,
            &cg_id,
        )
        .await
        .ok();
    }

    app.emit("recalculation-complete", ()).ok();

    Ok(result)
}

/// Delete crypto transaction
#[tauri::command]
pub async fn delete_crypto_transaction(
    db: State<'_, Database>,
    app: AppHandle,
    tx_id: String,
) -> Result<()> {
    // Get transaction info before deleting (for historical recalc)
    let tx_info: Option<(i64, String, String)> = db.with_conn(|conn| {
        // Get transaction date and investment_id
        let info: Option<(i64, String)> = conn
            .query_row(
                "SELECT transaction_date, investment_id FROM crypto_transactions WHERE id = ?1",
                [&tx_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((date, inv_id)) = info {
            // Get ticker and coingecko_id from the investment
            let crypto_info: Option<(String, String)> = conn
                .query_row(
                    "SELECT ticker, coingecko_id FROM crypto_investments WHERE id = ?1",
                    [&inv_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        ))
                    },
                )
                .ok();

            if let Some((ticker, coingecko_id)) = crypto_info {
                return Ok(Some((date, ticker, coingecko_id)));
            }
        }
        Ok(None)
    })?;

    db.with_conn(|conn| {
        // Get investment_id before deleting
        let investment_id: String = conn.query_row(
            "SELECT investment_id FROM crypto_transactions WHERE id = ?1",
            [&tx_id],
            |row| row.get(0),
        )?;

        conn.execute("DELETE FROM crypto_transactions WHERE id = ?1", [&tx_id])?;

        // Check if any transactions remain
        let tx_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM crypto_transactions WHERE investment_id = ?1",
            [&investment_id],
            |row| row.get(0),
        )?;

        if tx_count == 0 {
            // No transactions left - delete the crypto investment entirely
            conn.execute(
                "DELETE FROM crypto_investments WHERE id = ?1",
                [&investment_id],
            )?;
        } else {
            // Recalculate crypto investment metrics
            crypto_service::recalculate_crypto_metrics(conn, &investment_id)?;
        }

        Ok(())
    })?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // Trigger per-ticker historical recalculation if transaction was historical
    if let Some((date, ticker, coingecko_id)) = tx_info {
        if !coingecko_id.is_empty() {
            crate::commands::portfolio::trigger_historical_recalculation_for_crypto_ticker(
                &db,
                date,
                &ticker,
                &coingecko_id,
            )
            .await
            .ok();
        }

        app.emit("recalculation-complete", ()).ok();
    }

    Ok(())
}

/// Update crypto price manually (creates or updates override)
#[tauri::command]
pub async fn update_crypto_price(
    db: State<'_, Database>,
    symbol: String,
    price: String,
    currency: String,
    _coingecko_id: Option<String>,
) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let symbol_upper = symbol.to_uppercase();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO crypto_price_overrides (id, symbol, price, currency, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(symbol) DO UPDATE SET price = ?3, currency = ?4, updated_at = ?5",
            rusqlite::params![id, symbol_upper, price, currency, now],
        )?;
        Ok(())
    })?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db).await?;

    Ok(())
}

/// Delete manual crypto price override (revert to API price)
#[tauri::command]
pub async fn delete_crypto_manual_price(db: State<'_, Database>, symbol: String) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM crypto_price_overrides WHERE symbol = ?1",
            [symbol.to_uppercase()],
        )?;
        Ok(())
    })?;

    Ok(())
}

/// Get value history for a specific crypto ticker
#[tauri::command]
pub async fn get_crypto_value_history(
    db: State<'_, Database>,
    ticker: String,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<crate::models::TickerValueHistory>> {
    db.with_conn(|conn| crypto_service::get_value_history(conn, &ticker, start_date, end_date))
}
