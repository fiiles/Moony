//! Stock split commands (spec `docs/specs/2026-10-24-stock-splits-design.md`): thin wrappers
//! over `services::stock_splits`. The write commands that move transactions refresh today's
//! snapshot, queue the ticker's history rebuild and emit `recalculation-complete`.

use tauri::{AppHandle, Emitter, State};

use crate::commands::portfolio;
use crate::db::Database;
use crate::error::Result;
use crate::services::history_recalc::{HistoryRecalc, Job};
use crate::services::stock_splits::{self, InsertStockSplit, StockSplit, StockSplitApplyResult};

/// What follows an apply, revert or manual record: the snapshot, then one rebuild of the whole
/// ticker history from its first transaction day (D10; skipped when it holds no transactions).
/// The write already succeeded, so a failed snapshot is ignored.
async fn after_split_change(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    result: &StockSplitApplyResult,
) {
    portfolio::update_todays_snapshot(db).await.ok();

    if let Some(day) = result.earliest_day {
        portfolio::schedule_history_rebuild(
            app,
            db,
            recalc,
            vec![Job::StockTicker(result.split.ticker.clone(), day)],
        );
    }

    app.emit("recalculation-complete", ()).ok();
}

/// The ticker's splits that still have transactions dated before them (D6).
#[tauri::command]
pub async fn list_stock_splits(db: State<'_, Database>, ticker: String) -> Result<Vec<StockSplit>> {
    let ticker = ticker.trim().to_uppercase();
    db.with_conn(|conn| stock_splits::list_for_ticker(conn, &ticker))
}

/// Apply a pending split to the stored transactions and side data (D10).
#[tauri::command]
pub async fn apply_stock_split(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    id: String,
) -> Result<StockSplitApplyResult> {
    let result = db.with_conn(|conn| stock_splits::apply(conn, &id))?;
    after_split_change(&app, &db, &recalc, &result).await;
    Ok(result)
}

/// Undo an applied split; the row goes back to `pending`.
#[tauri::command]
pub async fn revert_stock_split(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    id: String,
) -> Result<StockSplitApplyResult> {
    let result = db.with_conn(|conn| stock_splits::revert(conn, &id))?;
    after_split_change(&app, &db, &recalc, &result).await;
    Ok(result)
}

/// Dismiss a pending split: only its status changes, no transaction moves.
#[tauri::command]
pub async fn dismiss_stock_split(db: State<'_, Database>, id: String) -> Result<StockSplit> {
    db.with_conn(|conn| stock_splits::dismiss(conn, &id))
}

/// Record a split by hand and apply it at once (D11).
#[tauri::command]
pub async fn record_stock_split(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    mut data: InsertStockSplit,
) -> Result<StockSplitApplyResult> {
    // The service works on the stored uppercase ticker.
    data.ticker = data.ticker.trim().to_uppercase();
    data.validate()?;

    let result = db.with_conn(|conn| stock_splits::record_manual(conn, &data))?;
    after_split_change(&app, &db, &recalc, &result).await;
    Ok(result)
}

/// Delete a manual split that is not applied.
#[tauri::command]
pub async fn delete_stock_split(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| stock_splits::delete_manual(conn, &id))
}
