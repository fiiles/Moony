//! Stock CSV import commands (design:
//! `docs/specs/2026-10-04-stock-csv-import-wizard-design.md`).
//!
//! Thin by design: validate, read the file, delegate to
//! `services::stock_import`, then run the side effects every change to stock
//! transactions needs (today's snapshot, the background history rebuild, the
//! `recalculation-complete` event). The frontend holds a file path and a
//! configuration, as in the bank CSV wizard.

use std::collections::BTreeSet;

use tauri::{AppHandle, Emitter, State};

use crate::commands::portfolio;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::services::history_recalc::HistoryRecalc;
use crate::services::stock_import::{
    batches, formats, import, inspect, instruments, parse, preview, SavedStockImportFormat,
    StockCsvInspectOptions, StockCsvInspection, StockImportBatch, StockImportConfig,
    StockImportPreview, StockImportResult, StockImportUndoResult, StockInstrumentQuery,
    StockInstrumentResolution,
};

/// Read a CSV file's bytes (decoding happens in the service).
fn read_csv_file(file_path: &str) -> Result<Vec<u8>> {
    std::fs::read(file_path).map_err(|e| AppError::Internal(format!("Cannot open file: {}", e)))
}

/// Last path segment, for the batch record and the inspection.
fn file_name_of(file_path: &str) -> String {
    std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.csv")
        .to_string()
}

/// What follows every committed change to stock transactions: today's
/// snapshot, one history rebuild per ticker from `earliest_day` (runs in the
/// background and coalesces with pending work) and the event the UI listens
/// for. The write already succeeded, so a failed snapshot is only logged.
async fn after_stock_change(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    tickers: impl IntoIterator<Item = String>,
    earliest_day: Option<i64>,
) {
    if let Err(e) = portfolio::update_todays_snapshot(db).await {
        log::warn!("[STOCK-IMPORT] Could not update today's snapshot: {e}");
    }

    if let Some(day) = earliest_day {
        let unique: BTreeSet<String> = tickers.into_iter().collect();
        if !unique.is_empty() {
            let jobs = unique.into_iter().map(|ticker| (ticker, day)).collect();
            portfolio::schedule_stock_history_rebuild(app, db, recalc, jobs);
        }
    }

    app.emit("recalculation-complete", ()).ok();
}

/// Inspect a file: encoding, delimiter, header row, the source its headers
/// belong to and a suggested configuration. `options` override the detection.
#[tauri::command]
pub async fn inspect_stock_csv(
    db: State<'_, Database>,
    file_path: String,
    options: StockCsvInspectOptions,
) -> Result<StockCsvInspection> {
    let bytes = read_csv_file(&file_path)?;
    // A damaged list of saved formats must not make every file unreadable.
    let saved = db.with_conn(formats::list_formats).unwrap_or_else(|e| {
        log::warn!("[STOCK-IMPORT] Could not load the saved formats: {e}");
        Vec::new()
    });
    inspect::inspect(&bytes, &file_name_of(&file_path), &options, &saved)
}

/// Look the instruments up on Yahoo Finance. Network only: no database, so
/// the lock is never held while waiting for the answers.
#[tauri::command]
pub async fn resolve_stock_instruments(
    queries: Vec<StockInstrumentQuery>,
) -> Result<Vec<StockInstrumentResolution>> {
    Ok(instruments::resolve_instruments(queries).await)
}

/// Dry run: the instruments, every row's status and the counts; writes nothing.
#[tauri::command]
pub async fn preview_stock_csv_import(
    db: State<'_, Database>,
    file_path: String,
    config: StockImportConfig,
) -> Result<StockImportPreview> {
    config.validate()?;
    let bytes = read_csv_file(&file_path)?;
    let parsed = parse::parse_file(&bytes, &config)?;
    db.with_conn(|conn| preview::preview(conn, &parsed, &config))
}

/// Import the rows the preview would import, atomically, as one batch.
#[tauri::command]
pub async fn import_stock_csv(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    file_path: String,
    config: StockImportConfig,
) -> Result<StockImportResult> {
    config.validate()?;
    let bytes = read_csv_file(&file_path)?;
    let parsed = parse::parse_file(&bytes, &config)?;
    let file_name = file_name_of(&file_path);
    let result = db.with_conn_mut(|conn| import::import(conn, &parsed, &config, &file_name))?;

    if result.imported > 0 {
        let tickers = result
            .new_positions
            .iter()
            .chain(result.updated_positions.iter())
            .cloned();
        after_stock_change(&app, &db, &recalc, tickers, result.earliest_day).await;
    }
    Ok(result)
}

/// Recorded imports, newest first.
#[tauri::command]
pub async fn list_stock_import_batches(db: State<'_, Database>) -> Result<Vec<StockImportBatch>> {
    db.with_conn(batches::list_batches)
}

/// Delete a batch's transactions (and the positions left without any).
#[tauri::command]
pub async fn undo_stock_import_batch(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    batch_id: String,
) -> Result<StockImportUndoResult> {
    let result = db.with_conn_mut(|conn| batches::undo_batch(conn, &batch_id))?;
    after_stock_change(
        &app,
        &db,
        &recalc,
        result.tickers.iter().cloned(),
        result.earliest_day,
    )
    .await;
    Ok(result)
}

/// Custom mappings remembered per header layout, newest first.
#[tauri::command]
pub async fn list_stock_import_formats(
    db: State<'_, Database>,
) -> Result<Vec<SavedStockImportFormat>> {
    db.with_conn(formats::list_formats)
}

/// Remember a mapping: files with the same headers are recognised next time.
#[tauri::command]
pub async fn save_stock_import_format(
    db: State<'_, Database>,
    name: String,
    headers: Vec<String>,
    config: StockImportConfig,
) -> Result<SavedStockImportFormat> {
    config.validate()?;
    db.with_conn(|conn| formats::save_format(conn, &name, &headers, &config))
}

#[tauri::command]
pub async fn delete_stock_import_format(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| formats::delete_format(conn, &id))
}
