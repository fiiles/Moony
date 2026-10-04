//! Background rebuilds of stock ticker history.
//!
//! A write command (add / edit / delete / import a stock transaction) used to
//! finish the ticker's history rebuild before returning: a Yahoo round trip
//! (hundreds of milliseconds, up to the 15 s timeout when offline) plus the
//! database work, so the dialog hung and the UI looked frozen. The command now
//! writes its rows, queues the affected ticker here and returns.
//!
//! * **Coalescing.** The queue holds one entry per ticker — the earliest
//!   affected date — so deleting five rows of a ticker (or importing a file
//!   that touches it five times) costs one rebuild per ticker, not five. Work
//!   queued while a batch is running is picked up by the same worker right
//!   after it.
//! * **Short lock holds.** Quotes are fetched without the database lock; the
//!   planned rows are written, and the portfolio aggregate re-derived, in
//!   chunks of [`CHUNK_DAYS`] days, one transaction and one lock acquisition
//!   per chunk, yielding in between so reads are served throughout.
//! * **Events.** `history-recalculation` carries `{ ticker, status }` with
//!   status `running`, `done` or `failed` (see [`HistoryRecalcEvent`]); the
//!   header badge listens to it. Once a batch has finished the established
//!   `recalculation-complete` event also fires so the trend charts refetch.
//!
//! The pure parts (queue, chunking) are unit-tested; the quote fetch is the
//! only network step and sits behind [`rebuild_stock_ticker`].

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use specta::Type;

use crate::db::Database;
use crate::error::Result;
use crate::services::portfolio_history::{
    aggregate_ticker_history, AssetClassKind, SECONDS_PER_DAY,
};
use crate::services::price_api::{self, HistoricalPrice};
use crate::services::ticker_history::{
    apply_stock_ticker_plan, plan_stock_ticker_history, RebuildStats,
};

/// Event carrying a ticker's rebuild status to the frontend.
pub const EVENT_STATUS: &str = "history-recalculation";
/// Existing event the trend charts refetch on; fired after a finished batch.
pub const EVENT_COMPLETE: &str = "recalculation-complete";
/// Days written (or aggregated) per transaction / lock acquisition.
pub const CHUNK_DAYS: i64 = 60;

/// Where a ticker's rebuild stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub enum RecalcStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "failed")]
    Failed,
}

/// Payload of the `history-recalculation` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct HistoryRecalcEvent {
    pub ticker: String,
    pub status: RecalcStatus,
}

/// What the worker tells the outside world; the command layer turns these
/// into Tauri events (keeps this module free of the Tauri runtime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Status(HistoryRecalcEvent),
    BatchComplete,
}

pub type Notifier = Arc<dyn Fn(Notice) + Send + Sync>;

/// Picks the quote that values a day (the commands layer passes its
/// `find_closest_price`, so this module follows whatever pricing rule that
/// function implements).
pub type PriceRule = fn(&[HistoricalPrice], i64) -> Option<&HistoricalPrice>;

#[derive(Default)]
struct Queue {
    /// ticker -> earliest affected date (unix seconds)
    pending: BTreeMap<String, i64>,
    worker_active: bool,
}

/// Queue of pending ticker rebuilds, shared as Tauri managed state.
#[derive(Clone, Default)]
pub struct HistoryRecalc {
    queue: Arc<Mutex<Queue>>,
}

impl HistoryRecalc {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queue `jobs`, keeping the earliest date per ticker. Returns true when
    /// no worker is draining the queue, i.e. the caller must start one.
    fn enqueue(&self, jobs: impl IntoIterator<Item = (String, i64)>) -> bool {
        let mut queue = self.lock();
        for (ticker, from) in jobs {
            queue
                .pending
                .entry(ticker)
                .and_modify(|earliest| *earliest = (*earliest).min(from))
                .or_insert(from);
        }
        if queue.worker_active {
            false
        } else {
            queue.worker_active = true;
            true
        }
    }

    /// Everything queued so far, or `None` — and the worker is marked idle in
    /// the same step, so a job queued right after starts a new worker.
    fn take_batch(&self) -> Option<BTreeMap<String, i64>> {
        let mut queue = self.lock();
        if queue.pending.is_empty() {
            queue.worker_active = false;
            None
        } else {
            Some(std::mem::take(&mut queue.pending))
        }
    }

    fn is_pending(&self, ticker: &str) -> bool {
        self.lock().pending.contains_key(ticker)
    }
}

fn floor_day(ts: i64) -> i64 {
    (ts / SECONDS_PER_DAY) * SECONDS_PER_DAY
}

fn today_start() -> i64 {
    floor_day(chrono::Utc::now().timestamp())
}

/// Queue the tickers for a background rebuild from their dates and make sure a
/// worker is running. Dates that are not before today are dropped: today's row
/// belongs to the live snapshot, there is no history to repair.
pub fn schedule(
    state: &HistoryRecalc,
    db: &Database,
    notify: Notifier,
    price_rule: PriceRule,
    jobs: Vec<(String, i64)>,
) {
    let today = today_start();
    let historical: Vec<(String, i64)> = jobs
        .into_iter()
        .filter(|(_, from)| floor_day(*from) < today)
        .collect();
    if historical.is_empty() || !state.enqueue(historical) {
        return;
    }

    let state = state.clone();
    let db = db.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(batch) = state.take_batch() {
            run_batch(&state, &db, &notify, price_rule, batch).await;
        }
    });
}

async fn run_batch(
    state: &HistoryRecalc,
    db: &Database,
    notify: &Notifier,
    price_rule: PriceRule,
    batch: BTreeMap<String, i64>,
) {
    let until = today_start();
    log::debug!("[RECALC] Background rebuild of {} ticker(s)", batch.len());
    for ticker in batch.keys() {
        notify(Notice::Status(HistoryRecalcEvent {
            ticker: ticker.clone(),
            status: RecalcStatus::Running,
        }));
    }

    let earliest = batch.values().copied().min().unwrap_or(until);
    let mut failed: Vec<String> = Vec::new();
    for (ticker, from) in &batch {
        if let Err(e) = rebuild_stock_ticker(db, ticker, *from, until, price_rule).await {
            log::warn!("[RECALC] Rebuilding a ticker's history failed: {e}");
            failed.push(ticker.clone());
        }
    }

    // Held tickers without any history in the range would be missing from the
    // aggregate: give them one too (what the synchronous path always did).
    match tickers_without_history(db, earliest, until, &batch) {
        Ok(missing) => {
            for ticker in missing {
                if let Err(e) = rebuild_stock_ticker(db, &ticker, earliest, until, price_rule).await
                {
                    log::warn!("[RECALC] Backfilling a ticker's history failed: {e}");
                }
            }
        }
        Err(e) => log::warn!("[RECALC] Could not look for tickers without history: {e}"),
    }

    let aggregated = aggregate_stocks(db, earliest, until).await;
    if let Err(e) = &aggregated {
        log::warn!("[RECALC] Re-deriving the portfolio history failed: {e}");
    }

    for ticker in batch.keys() {
        // Queued again while this batch ran: the next batch reports it.
        if state.is_pending(ticker) {
            continue;
        }
        let ok = aggregated.is_ok() && !failed.contains(ticker);
        notify(Notice::Status(HistoryRecalcEvent {
            ticker: ticker.clone(),
            status: if ok {
                RecalcStatus::Done
            } else {
                RecalcStatus::Failed
            },
        }));
    }
    notify(Notice::BatchComplete);
}

/// Where a rebuild of each ticker starts: the date of its earliest transaction. A ticker without
/// transactions (a watchlist ticker) has no history to rebuild and is left out. The order of
/// `tickers` is kept.
pub fn rebuild_jobs(conn: &rusqlite::Connection, tickers: &[String]) -> Result<Vec<(String, i64)>> {
    let mut earliest = conn
        .prepare("SELECT MIN(transaction_date) FROM investment_transactions WHERE ticker = ?1")?;
    let mut jobs = Vec::new();
    for ticker in tickers {
        // MIN over no rows is one row holding NULL.
        let from: Option<i64> = earliest.query_row([ticker], |row| row.get(0))?;
        if let Some(from) = from {
            jobs.push((ticker.clone(), from));
        }
    }
    Ok(jobs)
}

/// Held tickers (outside `batch`) that have no history row in `from..=until`.
fn tickers_without_history(
    db: &Database,
    from: i64,
    until: i64,
    batch: &BTreeMap<String, i64>,
) -> Result<Vec<String>> {
    let from_day = floor_day(from);
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT ticker FROM stock_investments
             WHERE ticker NOT IN (
                 SELECT ticker FROM stock_value_history
                 WHERE recorded_at >= ?1 AND recorded_at <= ?2
             )",
        )?;
        let rows = stmt.query_map([from_day, until], |row| row.get::<_, String>(0))?;
        let mut tickers = Vec::new();
        for row in rows {
            let ticker = row?;
            if !batch.contains_key(&ticker) {
                tickers.push(ticker);
            }
        }
        Ok(tickers)
    })
}

/// Rebuild one ticker's history rows from `from_ts` through `until_day`:
/// quotes are fetched without the database lock, the plan is computed in one
/// short read, and the rows are written in [`CHUNK_DAYS`]-day chunks. A ticker
/// without any transactions (a deleted investment) skips the fetch — its plan
/// only removes rows.
pub async fn rebuild_stock_ticker(
    db: &Database,
    ticker: &str,
    from_ts: i64,
    until_day: i64,
    price_rule: PriceRule,
) -> Result<RebuildStats> {
    let from_day = floor_day(from_ts);
    if from_day > until_day {
        return Ok(RebuildStats::default());
    }

    let has_transactions = db.with_conn(|conn| {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM investment_transactions WHERE ticker = ?1)",
            [ticker],
            |row| row.get::<_, bool>(0),
        )?)
    })?;
    let quotes = if has_transactions {
        price_api::get_historical_stock_prices_yahoo(&[ticker.to_string()], from_day, until_day)
            .await?
    } else {
        Default::default()
    };
    let prices: &[HistoricalPrice] = quotes.get(ticker).map(Vec::as_slice).unwrap_or(&[]);

    let plan = db.with_conn(|conn| {
        plan_stock_ticker_history(conn, ticker, from_day, until_day, |day| {
            price_rule(prices, day).map(|quote| (quote.price, quote.currency.clone()))
        })
    })?;

    let mut stats = RebuildStats::default();
    for chunk in plan.chunks(CHUNK_DAYS as usize) {
        let chunk_stats = db.with_conn(|conn| apply_stock_ticker_plan(conn, ticker, chunk))?;
        stats.upserted += chunk_stats.upserted;
        stats.deleted += chunk_stats.deleted;
        tokio::task::yield_now().await;
    }
    log::debug!(
        "[RECALC] Rebuilt a ticker's history: {} rows written, {} removed",
        stats.upserted,
        stats.deleted
    );
    Ok(stats)
}

/// Day ranges `[start, end)` of at most `step` seconds covering
/// `[from_day, until_day)`.
fn chunk_ranges(from_day: i64, until_day: i64, step: i64) -> Vec<(i64, i64)> {
    let mut ranges = Vec::new();
    let mut start = from_day;
    while start < until_day {
        let end = (start + step).min(until_day);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// Re-derive the investments class of the snapshot rows in `[start, end)`
/// from the per-ticker history, in one transaction.
pub fn aggregate_stock_chunk(conn: &rusqlite::Connection, start: i64, end: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    aggregate_ticker_history(&tx, AssetClassKind::Investments, start, end)?;
    tx.commit()?;
    Ok(())
}

/// Re-derive the investments class of every snapshot day from `from_ts` up to
/// (not including) `until_day`, [`CHUNK_DAYS`] days per lock acquisition.
pub async fn aggregate_stocks(db: &Database, from_ts: i64, until_day: i64) -> Result<()> {
    for (start, end) in chunk_ranges(floor_day(from_ts), until_day, CHUNK_DAYS * SECONDS_PER_DAY) {
        db.with_conn(|conn| aggregate_stock_chunk(conn, start, end))?;
        tokio::task::yield_now().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECONDS_PER_DAY;

    fn jobs(entries: &[(&str, i64)]) -> Vec<(String, i64)> {
        entries.iter().map(|(t, d)| (t.to_string(), *d)).collect()
    }

    #[test]
    fn five_deletes_of_one_ticker_coalesce_into_one_rebuild_from_the_earliest_date() {
        let queue = HistoryRecalc::new();
        // The first enqueue starts the worker, the rest ride along.
        assert!(queue.enqueue(jobs(&[("AAPL", 500 * DAY)])));
        for date in [300 * DAY, 400 * DAY, 100 * DAY, 450 * DAY] {
            assert!(!queue.enqueue(jobs(&[("AAPL", date)])));
        }

        let batch = queue.take_batch().expect("one batch");
        assert_eq!(batch.len(), 1, "one rebuild for the ticker");
        assert_eq!(batch["AAPL"], 100 * DAY, "from the earliest affected date");
        assert!(queue.take_batch().is_none(), "nothing else is queued");
    }

    #[test]
    fn each_ticker_gets_its_own_entry() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue(jobs(&[("AAPL", 10 * DAY), ("MSFT", 20 * DAY)])));
        assert!(!queue.enqueue(jobs(&[("MSFT", 5 * DAY), ("CEZ.PR", 30 * DAY)])));

        let batch = queue.take_batch().expect("batch");
        let entries: Vec<(&str, i64)> = batch.iter().map(|(t, d)| (t.as_str(), *d)).collect();
        assert_eq!(
            entries,
            vec![("AAPL", 10 * DAY), ("CEZ.PR", 30 * DAY), ("MSFT", 5 * DAY)]
        );
    }

    #[test]
    fn work_queued_while_a_batch_runs_goes_to_the_same_worker() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue(jobs(&[("AAPL", DAY)])));
        let first = queue.take_batch().expect("first batch");
        assert_eq!(first.len(), 1);

        // Mid-batch: the worker is still active, so no second worker starts…
        assert!(!queue.enqueue(jobs(&[("AAPL", 2 * DAY)])));
        assert!(queue.is_pending("AAPL"));
        // …and it finds the new job when it asks again.
        let second = queue.take_batch().expect("second batch");
        assert_eq!(second["AAPL"], 2 * DAY);
        assert!(!queue.is_pending("AAPL"));
    }

    #[test]
    fn a_job_after_the_worker_went_idle_starts_a_new_worker() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue(jobs(&[("AAPL", DAY)])));
        assert!(queue.take_batch().is_some());
        assert!(queue.take_batch().is_none(), "worker marks itself idle");

        assert!(
            queue.enqueue(jobs(&[("AAPL", DAY)])),
            "a new worker is needed"
        );
    }

    fn transactions_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, transaction_date INTEGER NOT NULL
            );",
        )
        .expect("schema");
        conn
    }

    fn add_transaction(conn: &rusqlite::Connection, id: &str, ticker: &str, date: i64) {
        conn.execute(
            "INSERT INTO investment_transactions (id, ticker, transaction_date) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, ticker, date],
        )
        .expect("transaction");
    }

    fn tickers(names: &[&str]) -> Vec<String> {
        names.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn a_rebuild_starts_at_the_earliest_transaction_of_each_ticker() {
        let conn = transactions_db();
        add_transaction(&conn, "t1", "CSPX.L", 300 * DAY);
        add_transaction(&conn, "t2", "CSPX.L", 100 * DAY);
        add_transaction(&conn, "t3", "BARC.L", 200 * DAY);
        add_transaction(&conn, "t4", "AAPL", 50 * DAY);

        let jobs = rebuild_jobs(&conn, &tickers(&["CSPX.L", "BARC.L"])).expect("jobs");
        assert_eq!(
            jobs,
            vec![
                ("CSPX.L".to_string(), 100 * DAY),
                ("BARC.L".to_string(), 200 * DAY)
            ],
            "AAPL was not asked for"
        );
    }

    #[test]
    fn a_ticker_without_transactions_has_nothing_to_rebuild() {
        let conn = transactions_db();
        add_transaction(&conn, "t1", "CSPX.L", 100 * DAY);

        // A watchlist ticker, and one asked for twice.
        let jobs = rebuild_jobs(&conn, &tickers(&["WATCH.L", "CSPX.L", "WATCH.L"])).expect("jobs");
        assert_eq!(jobs, vec![("CSPX.L".to_string(), 100 * DAY)]);
        assert!(rebuild_jobs(&conn, &[]).expect("none").is_empty());
    }

    #[test]
    fn chunk_ranges_cover_the_span_without_gaps_or_overlap() {
        assert_eq!(chunk_ranges(0, 0, 60 * DAY), vec![]);
        assert_eq!(
            chunk_ranges(10 * DAY, 20 * DAY, 60 * DAY),
            vec![(10 * DAY, 20 * DAY)]
        );

        let ranges = chunk_ranges(0, 150 * DAY, 60 * DAY);
        assert_eq!(
            ranges,
            vec![(0, 60 * DAY), (60 * DAY, 120 * DAY), (120 * DAY, 150 * DAY)]
        );
    }

    #[test]
    fn status_event_serializes_to_the_documented_contract() {
        let json = serde_json::to_value(HistoryRecalcEvent {
            ticker: "AAPL".to_string(),
            status: RecalcStatus::Running,
        })
        .expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({ "ticker": "AAPL", "status": "running" })
        );
        for (status, text) in [
            (RecalcStatus::Done, "done"),
            (RecalcStatus::Failed, "failed"),
        ] {
            assert_eq!(serde_json::to_value(status).expect("serialize"), text);
        }
    }

    fn no_quote(_: &[HistoricalPrice], _: i64) -> Option<&HistoricalPrice> {
        None
    }

    /// The whole worker step on a real (SQLCipher, migrated) database, for a ticker that
    /// no longer has transactions — the one case that needs no network: its stale history
    /// rows are removed in chunks, the portfolio aggregate is re-derived without it, and
    /// the events arrive in the documented order.
    #[tokio::test]
    async fn a_deleted_ticker_is_cleaned_up_and_reported_running_then_done() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Database::new();
        let key = crate::services::crypto::master_key_to_hex(&[3u8; 32]);
        db.create_with_key(dir.path().join("moony.db"), &key)
            .expect("create db");

        let today = today_start();
        let first = today - 3 * DAY;
        db.with_conn(|conn| {
            for offset in 0..3 {
                let day = first + offset * DAY;
                conn.execute(
                    "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                     VALUES (?1, 'GONE', ?2, '500', '2', '25', 'USD')",
                    rusqlite::params![format!("h{offset}"), day],
                )?;
                conn.execute(
                    "INSERT INTO portfolio_metrics_history
                     (id, total_savings, total_loans_principal, total_investments, total_bonds,
                      total_real_estate_personal, total_real_estate_investment, recorded_at,
                      investments_by_currency, source)
                     VALUES (?1, '0', '0', '500', '0', '0', '0', ?2, '{\"USD\":50}', 'backfill')",
                    rusqlite::params![format!("p{offset}"), day],
                )?;
            }
            Ok(())
        })
        .expect("seed");

        let notices: Arc<Mutex<Vec<Notice>>> = Arc::default();
        let sink = notices.clone();
        let notify: Notifier = Arc::new(move |notice| sink.lock().expect("lock").push(notice));
        let state = HistoryRecalc::new();
        let batch = BTreeMap::from([("GONE".to_string(), first)]);

        run_batch(&state, &db, &notify, no_quote, batch).await;

        let status = |status| {
            Notice::Status(HistoryRecalcEvent {
                ticker: "GONE".to_string(),
                status,
            })
        };
        assert_eq!(
            *notices.lock().expect("lock"),
            vec![
                status(RecalcStatus::Running),
                status(RecalcStatus::Done),
                Notice::BatchComplete
            ]
        );
        db.with_conn(|conn| {
            let rows: i64 = conn.query_row(
                "SELECT COUNT(*) FROM stock_value_history WHERE ticker = 'GONE'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(rows, 0, "the deleted ticker's history is gone");
            let breakdowns: Vec<String> = conn
                .prepare("SELECT investments_by_currency FROM portfolio_metrics_history")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(
                breakdowns,
                vec!["{}"; 3],
                "the aggregate no longer counts it"
            );
            Ok(())
        })
        .expect("verify");
    }

    #[test]
    fn chunked_aggregation_equals_one_pass() {
        // The portfolio snapshot rows derived chunk by chunk (what the worker
        // does) are the same as deriving them in one call.
        use rusqlite::{params, Connection};
        let first_day = 20_000 * DAY;
        let build = || {
            let conn = Connection::open_in_memory().expect("db");
            conn.execute_batch(
                r#"
                CREATE TABLE investment_transactions (id TEXT PRIMARY KEY, type TEXT NOT NULL, ticker TEXT NOT NULL, quantity TEXT NOT NULL, transaction_date INTEGER NOT NULL);
                CREATE TABLE stock_value_history (id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL, quantity TEXT NOT NULL, price TEXT NOT NULL, currency TEXT NOT NULL, UNIQUE(ticker, recorded_at));
                CREATE TABLE exchange_rate_history (date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL, PRIMARY KEY (date, currency));
                CREATE TABLE portfolio_metrics_history (
                    id TEXT PRIMARY KEY, total_savings TEXT NOT NULL DEFAULT '0', total_loans_principal TEXT NOT NULL DEFAULT '0',
                    total_investments TEXT NOT NULL DEFAULT '0', total_crypto TEXT NOT NULL DEFAULT '0', total_bonds TEXT NOT NULL DEFAULT '0',
                    total_real_estate_personal TEXT NOT NULL DEFAULT '0', total_real_estate_investment TEXT NOT NULL DEFAULT '0',
                    total_other_assets TEXT NOT NULL DEFAULT '0', recorded_at INTEGER NOT NULL,
                    investments_by_currency TEXT NOT NULL DEFAULT '{}', crypto_by_currency TEXT NOT NULL DEFAULT '{}',
                    savings_by_currency TEXT NOT NULL DEFAULT '{}', bonds_by_currency TEXT NOT NULL DEFAULT '{}',
                    real_estate_by_currency TEXT NOT NULL DEFAULT '{}', loans_by_currency TEXT NOT NULL DEFAULT '{}',
                    other_assets_by_currency TEXT NOT NULL DEFAULT '{}', source TEXT NOT NULL DEFAULT 'backfill'
                );
                INSERT INTO exchange_rate_history VALUES (19990 * 86400, 'USD', 20.0);
                INSERT INTO investment_transactions VALUES ('t1', 'buy', 'AAPL', '3', 20010 * 86400);
                "#,
            )
            .expect("schema");
            for offset in 0..200 {
                let day = first_day + offset * DAY;
                conn.execute(
                    "INSERT INTO portfolio_metrics_history (id, recorded_at) VALUES (?1, ?2)",
                    params![format!("p{offset}"), day],
                )
                .expect("snapshot");
                conn.execute(
                    "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
                     VALUES (?1, 'AAPL', ?2, '3', ?3, 'USD')",
                    params![format!("h{offset}"), day, (100 + offset).to_string()],
                )
                .expect("history");
            }
            conn
        };
        let read = |conn: &Connection| -> Vec<(i64, String)> {
            let mut stmt = conn
                .prepare(
                    "SELECT recorded_at, investments_by_currency FROM portfolio_metrics_history ORDER BY recorded_at",
                )
                .expect("prepare");
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .expect("query")
                .map(|r| r.expect("row"))
                .collect()
        };

        let until = first_day + 200 * DAY;
        let whole = build();
        aggregate_stock_chunk(&whole, first_day, until).expect("whole");

        let chunked = build();
        for (start, end) in chunk_ranges(first_day, until, CHUNK_DAYS * DAY) {
            aggregate_stock_chunk(&chunked, start, end).expect("chunk");
        }

        let (a, b) = (read(&whole), read(&chunked));
        assert_eq!(a, b);
        assert!(
            a.iter().any(|(_, json)| json.contains("USD")),
            "the fixture holds a position part of the time"
        );
    }
}
