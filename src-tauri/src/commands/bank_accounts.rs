//! Bank account Tauri commands for managing accounts, transactions, and categories

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    BankAccount, BankAccountWithInstitution, BankTransaction, InsertBankAccount,
    InsertBankTransaction, InsertSavingsAccountZone, Institution, SavingsAccountZone,
    TransactionFilters, TransactionQueryResult,
};
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

// ============================================================================
// Bank Account Commands
// ============================================================================

use crate::commands::categorization::CategorizationState;
use crate::commands::portfolio;
use crate::services::bank_accounts as bank_service;
use crate::services::categorization::own_accounts;
use crate::services::csv_import;
use crate::services::csv_presets;

/// Keep the categorization engine's own-account IBANs in step with the
/// accounts table: they used to be read only at unlock, so an account
/// added afterwards was never recognised as "my own account". A failure here
/// must not undo the account change itself, so it is only logged.
fn refresh_own_ibans(db: &Database, engine: &CategorizationState) {
    if let Err(e) = db.with_conn(|conn| own_accounts::refresh_own_ibans(conn, &engine.0)) {
        log::warn!("Could not refresh own-account IBANs: {e}");
    }
}

/// Get all bank accounts with optional institution data
#[tauri::command]
pub async fn get_all_bank_accounts(
    db: State<'_, Database>,
) -> Result<Vec<BankAccountWithInstitution>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT 
                ba.id, ba.name, ba.account_type, ba.iban, ba.bban, ba.currency, ba.balance,
                ba.institution_id, ba.external_account_id, ba.data_source, ba.last_synced_at,
                ba.interest_rate, ba.has_zone_designation, ba.termination_date,
                ba.created_at, ba.updated_at,
                i.id, i.name, i.bic, i.country, i.logo_url, i.created_at,
                ba.exclude_from_balance, ba.interest_rate_valid_until
            FROM bank_accounts ba
            LEFT JOIN institutions i ON ba.institution_id = i.id
            ORDER BY ba.name ASC",
        )?;

        let accounts: Vec<BankAccountWithInstitution> = stmt
            .query_map([], |row| {
                let account = BankAccount {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    account_type: row.get(2)?,
                    iban: row.get(3)?,
                    bban: row.get(4)?,
                    currency: row.get(5)?,
                    balance: row.get(6)?,
                    institution_id: row.get(7)?,
                    external_account_id: row.get(8)?,
                    data_source: row.get(9)?,
                    last_synced_at: row.get(10)?,
                    interest_rate: row.get(11)?,
                    has_zone_designation: row.get::<_, i32>(12)? != 0,
                    termination_date: row.get(13)?,
                    interest_rate_valid_until: row.get(23)?,
                    exclude_from_balance: row.get::<_, i32>(22)? != 0,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                };

                let institution: Option<Institution> =
                    if row.get::<_, Option<String>>(16)?.is_some() {
                        Some(Institution {
                            id: row.get(16)?,
                            name: row.get(17)?,
                            bic: row.get(18)?,
                            country: row.get(19)?,
                            logo_url: row.get(20)?,
                            created_at: row.get(21)?,
                        })
                    } else {
                        None
                    };

                Ok(BankAccountWithInstitution {
                    account,
                    institution,
                    effective_interest_rate: None,
                    projected_earnings: None,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(accounts)
    })
}

/// Get a single bank account by ID
#[tauri::command]
pub async fn get_bank_account(
    db: State<'_, Database>,
    id: String,
) -> Result<Option<BankAccountWithInstitution>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT 
                ba.id, ba.name, ba.account_type, ba.iban, ba.bban, ba.currency, ba.balance,
                ba.institution_id, ba.external_account_id, ba.data_source, ba.last_synced_at,
                ba.interest_rate, ba.has_zone_designation, ba.termination_date,
                ba.created_at, ba.updated_at,
                i.id, i.name, i.bic, i.country, i.logo_url, i.created_at,
                ba.exclude_from_balance, ba.interest_rate_valid_until
            FROM bank_accounts ba
            LEFT JOIN institutions i ON ba.institution_id = i.id
            WHERE ba.id = ?1",
        )?;

        let result = stmt.query_row([&id], |row| {
            let account = BankAccount {
                id: row.get(0)?,
                name: row.get(1)?,
                account_type: row.get(2)?,
                iban: row.get(3)?,
                bban: row.get(4)?,
                currency: row.get(5)?,
                balance: row.get(6)?,
                institution_id: row.get(7)?,
                external_account_id: row.get(8)?,
                data_source: row.get(9)?,
                last_synced_at: row.get(10)?,
                interest_rate: row.get(11)?,
                has_zone_designation: row.get::<_, i32>(12)? != 0,
                termination_date: row.get(13)?,
                interest_rate_valid_until: row.get(23)?,
                exclude_from_balance: row.get::<_, i32>(22)? != 0,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            };

            let institution: Option<Institution> = if row.get::<_, Option<String>>(16)?.is_some() {
                Some(Institution {
                    id: row.get(16)?,
                    name: row.get(17)?,
                    bic: row.get(18)?,
                    country: row.get(19)?,
                    logo_url: row.get(20)?,
                    created_at: row.get(21)?,
                })
            } else {
                None
            };

            Ok(BankAccountWithInstitution {
                account,
                institution,
                effective_interest_rate: None,
                projected_earnings: None,
            })
        });

        match result {
            Ok(account) => Ok(Some(account)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    })
}

/// Create a new bank account
#[tauri::command]
pub async fn create_bank_account(
    db: State<'_, Database>,
    engine: State<'_, CategorizationState>,
    data: InsertBankAccount,
) -> Result<BankAccount> {
    // 1. Delegate to service layer (validates at the trust boundary itself,
    //    since the MCP write tool calls it directly too)
    let result = db.with_conn(|conn| bank_service::create_account(conn, &data))?;
    refresh_own_ibans(&db, &engine);

    // 2. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    Ok(result)
}

/// Update an existing bank account
#[tauri::command]
pub async fn update_bank_account(
    db: State<'_, Database>,
    engine: State<'_, CategorizationState>,
    id: String,
    data: InsertBankAccount,
) -> Result<BankAccount> {
    // 1. Validate inputs at the trust boundary
    data.validate()?;

    // 2. Delegate to service layer
    let result = db.with_conn(|conn| bank_service::update_account(conn, &id, &data))?;
    refresh_own_ibans(&db, &engine);

    // 3. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    Ok(result)
}

/// Delete a bank account and all its transactions
#[tauri::command]
pub async fn delete_bank_account(
    db: State<'_, Database>,
    engine: State<'_, CategorizationState>,
    id: String,
) -> Result<()> {
    // 1. Delegate to service layer
    db.with_conn(|conn| bank_service::delete_account(conn, &id))?;
    refresh_own_ibans(&db, &engine);

    // 2. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    Ok(())
}

// ============================================================================
// Institution Commands
// ============================================================================

/// Get all institutions
#[tauri::command]
pub async fn get_all_institutions(db: State<'_, Database>) -> Result<Vec<Institution>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, bic, country, logo_url, created_at 
             FROM institutions 
             ORDER BY name ASC",
        )?;

        let institutions: Vec<Institution> = stmt
            .query_map([], |row| {
                Ok(Institution {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    bic: row.get(2)?,
                    country: row.get(3)?,
                    logo_url: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(institutions)
    })
}

/// Create a new institution
#[tauri::command]
pub async fn create_institution(db: State<'_, Database>, name: String) -> Result<Institution> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO institutions (id, name, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, name, now],
        )?;

        Ok(Institution {
            id,
            name,
            bic: None,
            country: None,
            logo_url: None,
            created_at: now,
        })
    })
}

// ============================================================================
// Transaction Commands
// ============================================================================

/// Get transactions for a bank account with optional filters
#[tauri::command]
pub async fn get_bank_transactions(
    db: State<'_, Database>,
    account_id: String,
    filters: Option<TransactionFilters>,
) -> Result<TransactionQueryResult> {
    let filters = filters.unwrap_or_default();
    let limit = filters.limit.unwrap_or(50);
    let offset = filters.offset.unwrap_or(0);

    db.with_conn(|conn| {
        // Build base query
        let mut sql = String::from(
            "SELECT id, bank_account_id, transaction_id, tx_type, amount, currency,
             description, counterparty_name, counterparty_iban, booking_date, value_date,
             category_id, merchant_category_code, remittance_info, suggested_category_id,
             status, data_source, created_at
             FROM bank_transactions WHERE bank_account_id = ?",
        );

        let mut count_sql =
            String::from("SELECT COUNT(*) FROM bank_transactions WHERE bank_account_id = ?");

        // Build dynamic filters
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(account_id.clone())];

        if let Some(date_from) = filters.date_from {
            sql.push_str(" AND booking_date >= ?");
            count_sql.push_str(" AND booking_date >= ?");
            params_vec.push(Box::new(date_from));
        }
        if let Some(date_to) = filters.date_to {
            sql.push_str(" AND booking_date <= ?");
            count_sql.push_str(" AND booking_date <= ?");
            params_vec.push(Box::new(date_to));
        }
        if let Some(ref category_id) = filters.category_id {
            sql.push_str(" AND category_id = ?");
            count_sql.push_str(" AND category_id = ?");
            params_vec.push(Box::new(category_id.clone()));
        }
        if let Some(ref tx_type) = filters.tx_type {
            sql.push_str(" AND tx_type = ?");
            count_sql.push_str(" AND tx_type = ?");
            params_vec.push(Box::new(tx_type.clone()));
        }
        if let Some(ref search) = filters.search {
            sql.push_str(" AND (description LIKE ? OR counterparty_name LIKE ?)");
            count_sql.push_str(" AND (description LIKE ? OR counterparty_name LIKE ?)");
            let pattern = format!("%{}%", search);
            params_vec.push(Box::new(pattern.clone()));
            params_vec.push(Box::new(pattern));
        }

        sql.push_str(" ORDER BY booking_date DESC");
        sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

        // Execute query
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let transactions: Vec<BankTransaction> = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(BankTransaction {
                    id: row.get(0)?,
                    bank_account_id: row.get(1)?,
                    transaction_id: row.get(2)?,
                    tx_type: row.get(3)?,
                    amount: row.get(4)?,
                    currency: row.get(5)?,
                    description: row.get(6)?,
                    counterparty_name: row.get(7)?,
                    counterparty_iban: row.get(8)?,
                    booking_date: row.get(9)?,
                    value_date: row.get(10)?,
                    category_id: row.get(11)?,
                    merchant_category_code: row.get(12)?,
                    remittance_info: row.get(13)?,
                    suggested_category_id: row.get(14)?,
                    status: row.get(15)?,
                    data_source: row.get(16)?,
                    created_at: row.get(17)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Get total count (reuse params without search duplicates)
        let count_params: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let total: i64 = conn.query_row(&count_sql, count_params.as_slice(), |row| row.get(0))?;

        Ok(TransactionQueryResult {
            transactions,
            total,
        })
    })
}

/// Create a new transaction
#[tauri::command]
pub async fn create_bank_transaction(
    db: State<'_, Database>,
    data: InsertBankTransaction,
) -> Result<BankTransaction> {
    db.with_conn(|conn| bank_service::create_transaction(conn, &data))
}

/// Delete a transaction
#[tauri::command]
pub async fn delete_bank_transaction(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute("DELETE FROM bank_transactions WHERE id = ?1", [&id])?;
        Ok(())
    })
}

/// Update a transaction's category
#[tauri::command]
pub async fn update_transaction_category(
    db: State<'_, Database>,
    transaction_id: String,
    category_id: Option<String>,
) -> Result<()> {
    db.with_conn(|conn| {
        // No provenance label: a manual assignment stores NULL, and clears any
        // label an import had stamped (spec D3).
        bank_service::set_transaction_category(conn, &transaction_id, category_id.as_deref(), None)
    })
}

// ============================================================================
// Interest-rate Zone Commands
// ============================================================================

/// Get the interest-rate zones (tiers) of a bank account
#[tauri::command]
pub async fn get_account_zones(
    db: State<'_, Database>,
    account_id: String,
) -> Result<Vec<SavingsAccountZone>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, bank_account_id, from_amount, to_amount, interest_rate, created_at
             FROM bank_account_zones WHERE bank_account_id = ?1 ORDER BY from_amount",
        )?;

        let zones = stmt
            .query_map([&account_id], |row| {
                Ok(SavingsAccountZone {
                    id: row.get(0)?,
                    savings_account_id: row.get(1)?,
                    from_amount: row.get(2)?,
                    to_amount: row.get(3)?,
                    interest_rate: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(zones)
    })
}

/// Create an interest-rate zone for a bank account
#[tauri::command]
pub async fn create_account_zone(
    db: State<'_, Database>,
    data: InsertSavingsAccountZone,
) -> Result<SavingsAccountZone> {
    // Validate inputs at the trust boundary
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO bank_account_zones (id, bank_account_id, from_amount, to_amount, interest_rate, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                data.savings_account_id,
                data.from_amount,
                data.to_amount,
                data.interest_rate,
                now,
            ],
        )?;

        Ok(SavingsAccountZone {
            id,
            savings_account_id: data.savings_account_id,
            from_amount: data.from_amount,
            to_amount: data.to_amount,
            interest_rate: data.interest_rate,
            created_at: now,
        })
    })
}

/// Delete an interest-rate zone
#[tauri::command]
pub async fn delete_account_zone(db: State<'_, Database>, zone_id: String) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute("DELETE FROM bank_account_zones WHERE id = ?1", [&zone_id])?;
        Ok(())
    })
}

// ============================================================================
// CSV Import Commands
// ============================================================================

/// Read a CSV file's bytes (decoding happens in the service).
fn read_csv_file(file_path: &str) -> Result<Vec<u8>> {
    std::fs::read(file_path).map_err(|e| AppError::Internal(format!("Cannot open file: {}", e)))
}

/// Get all bank CSV presets (JSON data under `resources/csv-presets/`)
#[tauri::command]
pub async fn get_csv_presets() -> Result<Vec<csv_presets::CsvPreset>> {
    Ok(csv_presets::all_presets().to_vec())
}

/// Get the CSV preset for a seeded institution (`inst_revolut`)
#[tauri::command]
pub async fn get_csv_preset_by_institution(
    institution_id: String,
) -> Result<Option<csv_presets::CsvPreset>> {
    Ok(csv_presets::get_preset_by_institution(&institution_id).cloned())
}

/// Inspect a CSV file: headers, header row, bank preset, suggested mappings,
/// date format and decimal separator. `delimiter`, `header_row`, `preset_id`
/// and `encoding` override the detection when given; `skip_rows` skips data
/// rows after the header.
#[tauri::command]
pub async fn parse_csv_file(
    file_path: String,
    delimiter: Option<String>,
    skip_rows: Option<usize>,
    header_row: Option<usize>,
    preset_id: Option<String>,
    encoding: Option<String>,
) -> Result<csv_import::CsvPreviewResult> {
    let bytes = read_csv_file(&file_path)?;
    csv_import::inspect_csv(
        &bytes,
        &csv_import::InspectOptions {
            delimiter: delimiter.and_then(|s| s.chars().next()),
            header_row,
            skip_rows: skip_rows.unwrap_or(0),
            preset_id: preset_id.as_deref(),
            encoding: encoding.as_deref(),
        },
    )
}

/// Dry-run an import: parsed rows with their status (ok / error / duplicate /
/// skipped), nothing is written.
#[tauri::command]
pub async fn preview_csv_import(
    db: State<'_, Database>,
    categorization: State<'_, crate::commands::categorization::CategorizationState>,
    account_id: String,
    file_path: String,
    config: csv_import::CsvImportConfig,
) -> Result<csv_import::CsvImportPreview> {
    let bytes = read_csv_file(&file_path)?;
    let decoded = csv_import::decode_for_import(&bytes, config.encoding.as_deref(), None);
    // The preview shows the category each row would get, so the same engine
    // the import uses resolves it here (redesign phase 2, backend C).
    let engine = categorization.0.clone();
    db.with_conn(|conn| {
        csv_import::preview_import(conn, &account_id, &decoded.text, &config, Some(&engine))
    })
}

#[tauri::command]
pub async fn import_csv_transactions(
    db: State<'_, Database>,
    categorization: State<'_, crate::commands::categorization::CategorizationState>,
    account_id: String,
    file_path: String,
    config: csv_import::CsvImportConfig,
) -> Result<csv_import::CsvImportResult> {
    let bytes = read_csv_file(&file_path)?;
    let decoded = csv_import::decode_for_import(&bytes, config.encoding.as_deref(), None);

    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.csv")
        .to_string();

    // Rules-first import (spec D2): the CSV path shares the write path with
    // the MCP bulk tool, so it gets the same deterministic categorization.
    let engine = categorization.0.clone();
    db.with_conn_mut(|conn| {
        csv_import::import_transactions(
            conn,
            &account_id,
            &file_name,
            &decoded.text,
            &config,
            Some(&engine),
        )
    })
}

/// Represents a CSV import batch for the frontend
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvImportBatch {
    pub id: String,
    pub bank_account_id: String,
    pub file_name: String,
    pub imported_count: i64,
    pub duplicate_count: i64,
    pub error_count: i64,
    pub imported_at: i64,
}

/// Get all import batches for a bank account
#[tauri::command]
pub async fn get_import_batches(
    db: State<'_, Database>,
    account_id: String,
) -> Result<Vec<CsvImportBatch>> {
    let batches = db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, bank_account_id, file_name, imported_count, duplicate_count, error_count, imported_at 
             FROM csv_import_batches 
             WHERE bank_account_id = ?1 
             ORDER BY imported_at DESC"
        )?;
        let batch_iter = stmt.query_map([&account_id], |row| {
            Ok(CsvImportBatch {
                id: row.get(0)?,
                bank_account_id: row.get(1)?,
                file_name: row.get(2)?,
                imported_count: row.get(3)?,
                duplicate_count: row.get(4)?,
                error_count: row.get(5)?,
                imported_at: row.get(6)?,
            })
        })?;
        let batches: Vec<CsvImportBatch> = batch_iter.filter_map(|r| r.ok()).collect();
        Ok(batches)
    })?;

    Ok(batches)
}

/// Delete an import batch and all its transactions
#[tauri::command]
pub async fn delete_import_batch(db: State<'_, Database>, batch_id: String) -> Result<()> {
    // Transactions are deleted via CASCADE, but let's be explicit
    db.with_conn(|conn| {
        // Delete transactions first
        conn.execute(
            "DELETE FROM bank_transactions WHERE import_batch_id = ?1",
            params![batch_id],
        )?;
        // Delete the batch record
        conn.execute(
            "DELETE FROM csv_import_batches WHERE id = ?1",
            params![batch_id],
        )?;
        Ok(())
    })?;

    Ok(())
}
