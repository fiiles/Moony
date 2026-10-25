//! Bank account Tauri commands for managing accounts, transactions, and categories

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    BankAccount, BankAccountWithInstitution, BankTransaction, InsertBankAccount,
    InsertBankTransaction, InsertSavingsAccountZone, Institution, SavingsAccountZone,
    TransactionFilters, TransactionQueryResult,
};
use rusqlite::params;
use tauri::{AppHandle, State};
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
use crate::services::history_plan;
use crate::services::history_recalc::HistoryRecalc;

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
                ba.exclude_from_balance, ba.interest_rate_valid_until, ba.balance_as_of
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
                    balance_as_of: row.get(24)?,
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
                ba.exclude_from_balance, ba.interest_rate_valid_until, ba.balance_as_of
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
                balance_as_of: row.get(24)?,
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
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    engine: State<'_, CategorizationState>,
    data: InsertBankAccount,
) -> Result<BankAccount> {
    data.validate()?;
    // 1. Delegate to service layer (validates at the trust boundary itself,
    //    since the MCP write tool calls it directly too)
    let result = db.with_conn(|conn| bank_service::create_account(conn, &data))?;
    refresh_own_ibans(&db, &engine);

    // 2. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    // 3. A new account can change every day (its opening record, or a balance carried on all)
    portfolio::schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);

    Ok(result)
}

/// Update an existing bank account
#[tauri::command]
pub async fn update_bank_account(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    engine: State<'_, CategorizationState>,
    id: String,
    data: InsertBankAccount,
) -> Result<BankAccount> {
    // 1. Validate inputs at the trust boundary
    data.validate()?;

    // 2. Delegate to service layer
    let (before, result) = db.with_conn(|conn| {
        let before = bank_service::get_account_by_id(conn, &id)?;
        let updated = bank_service::update_account(conn, &id, &data)?;
        Ok((before, updated))
    })?;
    refresh_own_ibans(&db, &engine);

    // 3. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    // 4. Leaving or entering the balance, or a currency change, moves every day
    if let Some(job) = history_plan::account_updated(&before, &result) {
        portfolio::schedule_history_rebuild(&app, &db, &recalc, vec![job]);
    }

    Ok(result)
}

/// Delete a bank account and all its transactions
#[tauri::command]
pub async fn delete_bank_account(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    engine: State<'_, CategorizationState>,
    id: String,
) -> Result<()> {
    // 1. Delegate to service layer
    db.with_conn(|conn| bank_service::delete_account(conn, &id))?;
    refresh_own_ibans(&db, &engine);

    // 2. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    // 3. The account leaves the savings of every day it was counted on
    portfolio::schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);

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
    db.with_conn(|conn| {
        crate::services::bank_history::get_transactions(
            conn,
            &account_id,
            &filters.unwrap_or_default(),
        )
    })
}

/// Create a new transaction
#[tauri::command]
pub async fn create_bank_transaction(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    data: InsertBankTransaction,
) -> Result<BankTransaction> {
    data.validate()?;
    let created = db.with_conn(|conn| bank_service::create_transaction(conn, &data))?;
    // A back-dated entry may predate the FX timeseries; the backfill is a no-op when its
    // currency is the reference or already covered.
    portfolio::spawn_fx_backfill(app, db.inner().clone());
    Ok(created)
}

/// Set explicit financial meaning; null returns to automatic derivation.
#[tauri::command]
pub async fn update_transaction_flow(
    db: State<'_, Database>,
    transaction_id: String,
    flow_kind: Option<crate::models::FlowKind>,
) -> Result<()> {
    db.with_conn(|conn| bank_service::set_transaction_flow(conn, &transaction_id, flow_kind))
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
/// rows after the header; `country` (the profile country) picks the legacy
/// encoding of a file that is not UTF-8.
#[tauri::command]
pub async fn parse_csv_file(
    file_path: String,
    delimiter: Option<String>,
    skip_rows: Option<usize>,
    header_row: Option<usize>,
    preset_id: Option<String>,
    encoding: Option<String>,
    country: Option<String>,
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
            country: country.as_deref(),
        },
    )
}

/// Infer the columns of a CSV no preset describes from their content (the phone's guided
/// recognition, mobile spec §8.6 rung 3). `delimiter`, `header_row` and `encoding` are the
/// ones `parse_csv_file` reported for the file (detected again when absent); `country` picks
/// the legacy encoding as there. The first 200 data rows are read; nothing is written.
#[tauri::command]
pub async fn infer_csv_columns(
    file_path: String,
    delimiter: Option<String>,
    header_row: Option<usize>,
    encoding: Option<String>,
    country: Option<String>,
) -> Result<csv_import::InferredColumns> {
    let bytes = read_csv_file(&file_path)?;
    Ok(csv_import::infer_csv(
        &bytes,
        &csv_import::InferOptions {
            delimiter: delimiter.and_then(|s| s.chars().next()),
            header_row,
            encoding: encoding.as_deref(),
            country: country.as_deref(),
        },
    ))
}

// Learned CSV layouts (mobile spec §8.6 rung 4): the layouts a person confirmed once, kept in
// `app_config` by `csv_import::learned` and recognised next month by their header signature.

/// The learned layouts, the one used last first.
#[tauri::command]
pub async fn list_learned_csv_formats(
    db: State<'_, Database>,
) -> Result<Vec<csv_import::LearnedCsvFormat>> {
    db.with_conn(csv_import::learned::list_formats)
}

/// The format learned for a file with these header cells (as `parse_csv_file` reports them) and
/// this delimiter; `None` when its layout was never confirmed.
#[tauri::command]
pub async fn find_learned_csv_format(
    db: State<'_, Database>,
    headers: Vec<String>,
    delimiter: String,
) -> Result<Option<csv_import::LearnedCsvFormat>> {
    db.with_conn(|conn| csv_import::learned::find_format(conn, &headers, &delimiter))
}

/// Remember a confirmed layout; one layout has one format, saving it again replaces it.
#[tauri::command]
pub async fn save_learned_csv_format(
    db: State<'_, Database>,
    format: csv_import::InsertLearnedCsvFormat,
) -> Result<csv_import::LearnedCsvFormat> {
    format.validate()?;
    db.with_conn(|conn| csv_import::learned::save_format(conn, &format))
}

#[tauri::command]
pub async fn rename_learned_csv_format(
    db: State<'_, Database>,
    id: String,
    name: String,
) -> Result<csv_import::LearnedCsvFormat> {
    db.with_conn(|conn| csv_import::learned::rename_format(conn, &id, &name))
}

#[tauri::command]
pub async fn delete_learned_csv_format(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| csv_import::learned::delete_format(conn, &id))
}

/// An import succeeded with this format: it is the most recently used one now.
#[tauri::command]
pub async fn mark_learned_csv_format_used(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| csv_import::learned::mark_used(conn, &id))
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
    app: tauri::AppHandle,
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
    let result = db.with_conn_mut(|conn| {
        csv_import::import_transactions(
            conn,
            &account_id,
            &file_name,
            &decoded.text,
            &config,
            Some(&engine),
        )
    })?;
    // Imported statements may predate the FX timeseries (foreign-currency
    // accounts): extend it so reports convert those rows at historical rates.
    if result.imported_count > 0 {
        portfolio::spawn_fx_backfill(app, db.inner().clone());
    }
    Ok(result)
}

/// Represents a CSV import batch for the frontend
#[tauri::command]
pub async fn get_import_batches(
    db: State<'_, Database>,
    account_id: String,
) -> Result<Vec<csv_import::types::CsvImportBatch>> {
    db.with_conn(|conn| csv_import::import_batches(conn, &account_id))
}
#[tauri::command]
pub async fn delete_import_batch(db: State<'_, Database>, batch_id: String) -> Result<usize> {
    db.with_conn(|conn| csv_import::delete_import_batch(conn, &batch_id))
}
/// Dated balances of an account, newest first.
#[tauri::command]
pub async fn get_balance_records(
    db: State<'_, Database>,
    account_id: String,
) -> Result<Vec<crate::models::bank_accounts::BankBalanceRecord>> {
    db.with_conn(|conn| bank_service::list_balance_records(conn, &account_id))
}

/// Record a dated balance (by hand or from a statement); returns the refreshed account.
#[tauri::command]
pub async fn record_bank_account_balance(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    account_id: String,
    data: crate::models::bank_accounts::InsertBalanceRecord,
) -> Result<BankAccount> {
    data.validate()?;
    let account = db.with_conn(|conn| bank_service::record_balance(conn, &account_id, &data))?;
    // The record holds from its day on, and it ends a balance carried on every earlier day (a
    // statement balance from a CSV import comes this way): every day is derived again
    portfolio::schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);
    Ok(account)
}

/// Delete one balance record; returns the account with the re-mirrored balance.
#[tauri::command]
pub async fn delete_balance_record(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    record_id: String,
) -> Result<BankAccount> {
    let account = db.with_conn(|conn| bank_service::delete_balance_record(conn, &record_id))?;
    portfolio::schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);
    Ok(account)
}
