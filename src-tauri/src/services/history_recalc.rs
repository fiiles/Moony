//! The one background queue of history rebuilds.
//!
//! A write command (a transaction, a balance record, a valuation, a loan, …) used to finish
//! its history work before returning: a Yahoo round trip (hundreds of
//! milliseconds, up to the 15 s timeout when offline) plus the database work, so the dialog
//! hung and the UI looked frozen. The command now writes its rows, queues what changed
//! here and returns (spec `docs/specs/2026-10-13-history-from-dated-facts-design.md`, A5).
//!
//! * **Jobs.** [`Job::StockTicker`] and [`Job::CryptoTicker`] rebuild one ticker's value
//!   history from a day (quotes from Yahoo Finance); [`Job::Days`] re-derives the
//!   portfolio days from the dated facts (`services/history_days.rs`). Every batch ends
//!   with a days rebuild from the earliest date of the whole batch, so a ticker job needs
//!   no separate `Days` job: the aggregate follows its ticker.
//! * **Coalescing.** The queue holds one entry per job key — the earliest affected date —
//!   so deleting five rows of a ticker (or importing a file that touches it five times)
//!   costs one rebuild per ticker, not five. Work queued while a batch is running is
//!   picked up by the same worker right after it.
//! * **Short lock holds.** Quotes are fetched without the database lock; the planned rows
//!   are written, and the portfolio days re-derived, in chunks of [`CHUNK_DAYS`] days, one
//!   transaction and one lock acquisition per chunk, yielding in between so reads are
//!   served throughout.
//! * **Events.** `history-recalculation` carries `{ ticker, status }` with status
//!   `running`, `done` or `failed` (see [`HistoryRecalcEvent`]); the header badge listens
//!   to it. Once a batch has finished the established `recalculation-complete` event also
//!   fires so the trend charts refetch.
//! * **Splits.** A stock rebuild records the splits its fetch reports
//!   (`stock_splits::record_detected`). A new one found by a rebuild that started after the
//!   ticker's first transaction queues the ticker again from that day ([`Job::StockTicker`]),
//!   so the closes before the window are split-adjusted too (spec D18).
//! * **Completion.** A caller that needs to know how the batch ended (the one-time rebuild
//!   stores its version flag then) passes a [`Completion`] and gets a [`BatchOutcome`]: whether
//!   the portfolio days were re-derived, and which tickers failed. A ticker the provider keeps
//!   rejecting is reported `failed` and logged but does not fail the batch; the caller decides
//!   what a failure costs (the one-time rebuild only needs the days, see
//!   `history_plan::rebuild_version_earned`). Such a batch fetches quotes strictly: a provider
//!   failure leaves the ticker's rows alone and reports it `failed`, instead of passing as "no
//!   data" (for a crypto ticker that would write the last known price over the whole range).
//!
//! The pure parts (queue, chunking) are unit-tested; the quote fetches are the only network
//! steps and sit behind [`rebuild_stock_ticker`] and [`rebuild_crypto_ticker`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::OptionalExtension;
use serde::Serialize;
use specta::Type;

use crate::db::Database;
use crate::error::Result;
use crate::services::crypto_symbols::{self, Resolved};
use crate::services::history_days::{plan_rebuild, prune_before_first_fact, rebuild_chunk};
use crate::services::portfolio_history::SECONDS_PER_DAY;
use crate::services::price_api::{self, HistoricalPrice};
use crate::services::pricing::AssetKind;
use crate::services::stock_splits::{self, StockSplitSource};
use crate::services::ticker_history::{
    apply_ticker_plan, plan_ticker_history, DayPlan, RebuildStats,
};

/// One days rebuild at a time: the queue worker and the durable MCP worker both re-derive
/// days, and a rebuild plans from the facts it read at its start. Taking the gate before
/// planning means a later rebuild always sees every write an earlier one raced with.
static DAYS_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

/// How a batch ended, as the completion of its jobs sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    /// The portfolio days were re-derived (a failed days rebuild fails every ticker too).
    pub days_ok: bool,
    /// Tickers whose quotes could not be rebuilt: logged and reported as `failed`, retried
    /// by a later write or the startup gap fill.
    pub failed_tickers: Vec<String>,
}

impl BatchOutcome {
    /// The outcome of a completion that has nothing historical to wait for.
    pub fn nothing_to_do() -> Self {
        Self {
            days_ok: true,
            failed_tickers: Vec::new(),
        }
    }

    /// The days were re-derived and no ticker failed.
    pub fn is_clean(&self) -> bool {
        self.days_ok && self.failed_tickers.is_empty()
    }
}

/// Called once with the outcome of the batch that carried it.
pub type Completion = Box<dyn FnOnce(&BatchOutcome) + Send>;

/// One piece of history work. Every date is the earliest affected day (unix seconds);
/// the worker floors it to the day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// Rebuild a stock ticker's value history from the day.
    StockTicker(String, i64),
    /// Rebuild a crypto ticker's value history from the day (the Yahoo symbol is read from
    /// `crypto_investments`, and resolved when missing, when the job runs).
    CryptoTicker(String, i64),
    /// Re-derive the portfolio days from the dated facts from the day. A day before the
    /// first fact is clamped to it (`history_days::plan_rebuild`), so `Days(0)` is "all".
    Days(i64),
}

impl Job {
    /// Stock ticker jobs for `(ticker, from)` pairs (what `rebuild_jobs` and the services
    /// that report their earliest dates per ticker return).
    pub fn stocks(pairs: impl IntoIterator<Item = (String, i64)>) -> Vec<Job> {
        pairs
            .into_iter()
            .map(|(ticker, from)| Job::StockTicker(ticker, from))
            .collect()
    }

    /// The earliest affected day of the job.
    pub fn from_day(&self) -> i64 {
        match self {
            Job::StockTicker(_, from) | Job::CryptoTicker(_, from) | Job::Days(from) => *from,
        }
    }

    fn key(&self) -> Key {
        match self {
            Job::StockTicker(ticker, _) => Key::Stock(ticker.clone()),
            Job::CryptoTicker(ticker, _) => Key::Crypto(ticker.clone()),
            Job::Days(_) => Key::Days,
        }
    }
}

/// What the queue coalesces on: a ticker of a class, or the portfolio days.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Stock(String),
    Crypto(String),
    Days,
}

/// Everything queued when a worker asked for work, by kind.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Batch {
    /// ticker -> earliest affected day
    pub stocks: BTreeMap<String, i64>,
    pub crypto: BTreeMap<String, i64>,
    /// earliest affected day of an explicit `Days` job
    pub days: Option<i64>,
}

impl Batch {
    /// The earliest day over every job of the batch: where the days rebuild starts.
    pub fn earliest(&self) -> Option<i64> {
        self.stocks
            .values()
            .chain(self.crypto.values())
            .copied()
            .chain(self.days)
            .min()
    }

    /// Every ticker the batch rebuilds, stocks first.
    pub fn tickers(&self) -> Vec<String> {
        self.stocks
            .keys()
            .chain(self.crypto.keys())
            .cloned()
            .collect()
    }

    fn from_pending(pending: BTreeMap<Key, i64>) -> Self {
        let mut batch = Batch::default();
        for (key, from) in pending {
            match key {
                Key::Stock(ticker) => {
                    batch.stocks.insert(ticker, from);
                }
                Key::Crypto(ticker) => {
                    batch.crypto.insert(ticker, from);
                }
                Key::Days => batch.days = Some(from),
            }
        }
        batch
    }
}

#[derive(Default)]
struct Queue {
    /// job key -> earliest affected date (unix seconds)
    pending: BTreeMap<Key, i64>,
    /// Completions of the work in `pending`: they go with the next batch.
    completions: Vec<Completion>,
    worker_active: bool,
}

impl Queue {
    /// Add `jobs`, keeping the earliest date per key.
    fn add(&mut self, jobs: impl IntoIterator<Item = Job>) {
        for job in jobs {
            let from = job.from_day();
            self.pending
                .entry(job.key())
                .and_modify(|earliest| *earliest = (*earliest).min(from))
                .or_insert(from);
        }
    }
}

/// Queue of pending history rebuilds, shared as Tauri managed state.
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

    /// Queue `jobs`, keeping the earliest date per key. Returns true when no worker is
    /// draining the queue, i.e. the caller must start one.
    fn enqueue(&self, jobs: impl IntoIterator<Item = Job>, completion: Option<Completion>) -> bool {
        let mut queue = self.lock();
        queue.add(jobs);
        queue.completions.extend(completion);
        if queue.worker_active {
            false
        } else {
            queue.worker_active = true;
            true
        }
    }

    /// Queue follow-up work found by the running batch (a new split, spec D18), keeping the
    /// earliest date per key. Only the worker calls this: it takes the jobs with its next batch,
    /// so no other worker is started.
    fn requeue(&self, jobs: impl IntoIterator<Item = Job>) {
        self.lock().add(jobs);
    }

    /// Everything queued so far with the completions that wait for it, or `None` — and the
    /// worker is marked idle in the same step, so a job queued right after starts a new
    /// worker.
    fn take_batch(&self) -> Option<(Batch, Vec<Completion>)> {
        let mut queue = self.lock();
        if queue.pending.is_empty() {
            queue.worker_active = false;
            None
        } else {
            let pending = std::mem::take(&mut queue.pending);
            let completions = std::mem::take(&mut queue.completions);
            Some((Batch::from_pending(pending), completions))
        }
    }

    /// Is a rebuild of the ticker (either class) queued behind the running batch?
    fn is_pending_ticker(&self, ticker: &str) -> bool {
        let queue = self.lock();
        queue.pending.contains_key(&Key::Stock(ticker.to_string()))
            || queue.pending.contains_key(&Key::Crypto(ticker.to_string()))
    }
}

/// Held by a worker for as long as it drains the queue. A batch that panics unwinds past the
/// worker without it ever marking itself idle, and `worker_active` would stay true: every later
/// rebuild would queue behind a worker that no longer exists and never run. The guard marks the
/// queue idle again in that case, so the next job starts a new worker. A worker that finished
/// normally already went idle through [`HistoryRecalc::take_batch`] (and a new worker may own the
/// queue by now), so it calls [`WorkerGuard::finish`] and the drop leaves the queue alone.
struct WorkerGuard<'a> {
    state: &'a HistoryRecalc,
    finished: bool,
}

impl<'a> WorkerGuard<'a> {
    fn new(state: &'a HistoryRecalc) -> Self {
        Self {
            state,
            finished: false,
        }
    }

    fn finish(&mut self) {
        self.finished = true;
    }
}

impl Drop for WorkerGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            log::warn!("[RECALC] The rebuild worker stopped unexpectedly; the queue is open again");
            self.state.lock().worker_active = false;
        }
    }
}

fn floor_day(ts: i64) -> i64 {
    ts.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY
}

fn today_start() -> i64 {
    floor_day(chrono::Utc::now().timestamp())
}

/// Queue the jobs for a background rebuild and make sure a worker is running. Jobs dated
/// today or later are dropped: today's row belongs to the live snapshot, there is no
/// history to repair. `completion`, when given, runs with the outcome of the batch that
/// carries it — at once, clean, when nothing historical is left to do.
pub fn schedule(
    state: &HistoryRecalc,
    db: &Database,
    notify: Notifier,
    price_rule: PriceRule,
    jobs: Vec<Job>,
    completion: Option<Completion>,
) {
    let today = today_start();
    let historical: Vec<Job> = jobs
        .into_iter()
        .filter(|job| floor_day(job.from_day()) < today)
        .collect();
    if historical.is_empty() {
        if let Some(completion) = completion {
            completion(&BatchOutcome::nothing_to_do());
        }
        return;
    }
    if !state.enqueue(historical, completion) {
        return;
    }

    let state = state.clone();
    let db = db.clone();
    tauri::async_runtime::spawn(async move {
        let mut guard = WorkerGuard::new(&state);
        while let Some((batch, completions)) = state.take_batch() {
            // Any dated write (a stock import, a backdated balance in a foreign currency, …)
            // may predate the FX timeseries: extend it first so history, positions and cost
            // basis convert at real rates. Without missing ranges this makes no request.
            crate::services::fx_backfill::ensure_fx_coverage_best_effort(&db).await;
            let outcome = run_batch(
                &state,
                &db,
                &notify,
                price_rule,
                &batch,
                !completions.is_empty(),
            )
            .await;
            for completion in completions {
                completion(&outcome);
            }
        }
        guard.finish();
    });
}

/// One batch (A5): stock tickers, crypto tickers, then the portfolio days from the
/// batch's earliest date. Every failure is logged; a failed ticker reports `failed`, and
/// a failed days rebuild fails every ticker of the batch. Returns how the batch ended: a
/// failed ticker does not stop the days rebuild. `strict` makes a provider failure an error
/// (the ticker keeps its rows and reports `failed`) instead of "no quotes".
async fn run_batch(
    state: &HistoryRecalc,
    db: &Database,
    notify: &Notifier,
    price_rule: PriceRule,
    batch: &Batch,
    strict: bool,
) -> BatchOutcome {
    let until = today_start();
    let earliest = batch.earliest().unwrap_or(until);
    log::debug!(
        "[RECALC] Background rebuild of {} stock and {} crypto ticker(s), days from {}",
        batch.stocks.len(),
        batch.crypto.len(),
        earliest
    );
    let tickers = batch.tickers();
    for ticker in &tickers {
        notify(Notice::Status(HistoryRecalcEvent {
            ticker: ticker.clone(),
            status: RecalcStatus::Running,
        }));
    }

    let mut failed: BTreeSet<String> = BTreeSet::new();
    // Full rebuilds asked for by a new split (spec D18): queued for the next batch.
    let mut follow_ups: Vec<Job> = Vec::new();
    for (ticker, from) in &batch.stocks {
        match rebuild_stock_ticker_with_policy(db, ticker, *from, until, price_rule, strict).await {
            Ok(stats) => follow_ups.extend(
                stats
                    .full_rebuild_from
                    .map(|first| Job::StockTicker(ticker.clone(), first)),
            ),
            Err(e) => {
                log::warn!("[RECALC] Rebuilding a stock ticker's history failed: {e}");
                failed.insert(ticker.clone());
            }
        }
    }
    follow_ups.extend(
        rebuild_unseeded_tickers(
            db,
            AssetKind::Stock,
            (earliest, until),
            &batch.stocks,
            price_rule,
            strict,
        )
        .await,
    );

    for (ticker, from) in &batch.crypto {
        if let Err(e) =
            rebuild_crypto_ticker_with_policy(db, ticker, *from, until, price_rule, strict).await
        {
            log::warn!("[RECALC] Rebuilding a crypto ticker's history failed: {e}");
            failed.insert(ticker.clone());
        }
    }
    rebuild_unseeded_tickers(
        db,
        AssetKind::Crypto,
        (earliest, until),
        &batch.crypto,
        price_rule,
        strict,
    )
    .await;

    let days = rebuild_days_async(db, earliest, until).await;
    if let Err(e) = &days {
        log::warn!("[RECALC] Re-deriving the portfolio history failed: {e}");
    }
    // Before the statuses: a ticker with a follow-up stays `running` until it has run.
    state.requeue(follow_ups);

    for ticker in &tickers {
        // Queued again while this batch ran: the next batch reports it.
        if state.is_pending_ticker(ticker) {
            continue;
        }
        let ok = days.is_ok() && !failed.contains(ticker);
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
    BatchOutcome {
        days_ok: days.is_ok(),
        failed_tickers: failed.into_iter().collect(),
    }
}

/// Held tickers (outside `queued`) without any history row in `range` would be missing from
/// the portfolio days: give them one too. Best effort — a failure is logged and the ticker
/// is tried again by the next batch that finds it still without history. Returns the full
/// rebuilds a new split asks for (spec D18).
async fn rebuild_unseeded_tickers(
    db: &Database,
    kind: AssetKind,
    range: (i64, i64),
    queued: &BTreeMap<String, i64>,
    price_rule: PriceRule,
    strict: bool,
) -> Vec<Job> {
    let until = range.1;
    let missing = match unseeded_ticker_jobs(db, kind, range, queued) {
        Ok(missing) => missing,
        Err(e) => {
            log::warn!("[RECALC] Could not look for tickers without history: {e}");
            return Vec::new();
        }
    };
    let mut follow_ups = Vec::new();
    for (ticker, from) in missing {
        let rebuilt = match kind {
            AssetKind::Stock => {
                rebuild_stock_ticker_with_policy(db, &ticker, from, until, price_rule, strict).await
            }
            AssetKind::Crypto => {
                rebuild_crypto_ticker_with_policy(db, &ticker, from, until, price_rule, strict)
                    .await
            }
        };
        match rebuilt {
            Ok(stats) => follow_ups.extend(
                stats
                    .full_rebuild_from
                    .map(|first| Job::StockTicker(ticker, first)),
            ),
            Err(e) => log::warn!("[RECALC] Backfilling a ticker's history failed: {e}"),
        }
    }
    follow_ups
}

/// The tickers [`rebuild_unseeded_tickers`] fetches quotes for, each with the day its rebuild
/// starts: the later of the batch's earliest day and its first transaction (a days job dated 0,
/// "from the first fact", must not ask a provider for quotes back to 1970). Only a ticker that
/// can be missing from the portfolio days qualifies (see [`tickers_without_history`]); the
/// rest costs no provider call.
fn unseeded_ticker_jobs(
    db: &Database,
    kind: AssetKind,
    range: (i64, i64),
    queued: &BTreeMap<String, i64>,
) -> Result<Vec<(String, i64)>> {
    let (earliest, until) = range;
    let missing = tickers_without_history(db, kind, earliest, until, queued)?;
    let firsts = db.with_conn(|conn| match kind {
        AssetKind::Stock => rebuild_jobs(conn, &missing),
        AssetKind::Crypto => crypto_rebuild_jobs(conn, &missing),
    })?;
    Ok(firsts
        .into_iter()
        .map(|(ticker, first_transaction)| (ticker, earliest.max(first_transaction)))
        .collect())
}

/// Re-derive the portfolio days of `[from_day, until_day)` from the dated facts (A4): the
/// plan and the pruning of rows before the first fact in one short read, then the days in
/// [`CHUNK_DAYS`]-day chunks, one lock acquisition each, yielding in between. Idempotent;
/// `until_day` is today's start (today's row is never touched).
pub async fn rebuild_days_async(
    db: &Database,
    from_day: i64,
    until_day: i64,
) -> Result<RebuildStats> {
    let _gate = DAYS_GATE.lock().await;
    let (mut plan, deleted) = db.with_conn(|conn| {
        let plan = plan_rebuild(conn, from_day, until_day)?;
        let deleted = prune_before_first_fact(conn, &plan)?;
        Ok((plan, deleted))
    })?;
    let mut stats = RebuildStats {
        upserted: 0,
        deleted,
        full_rebuild_from: None,
    };
    for (chunk_start, chunk_end) in plan.chunks() {
        let chunk = db.with_conn(|conn| rebuild_chunk(conn, &mut plan, chunk_start, chunk_end))?;
        stats.upserted += chunk.upserted;
        tokio::task::yield_now().await;
    }
    log::debug!(
        "[RECALC] Re-derived the portfolio days: {} rows written, {} removed",
        stats.upserted,
        stats.deleted
    );
    Ok(stats)
}

/// First transaction day of each of `tickers` in `table`. A ticker without transactions (a
/// watchlist ticker, a deleted holding) has no history to rebuild and is left out. The
/// order of `tickers` is kept.
fn first_transaction_days(
    conn: &rusqlite::Connection,
    table: &str,
    tickers: &[String],
) -> Result<Vec<(String, i64)>> {
    let mut earliest = conn.prepare(&format!(
        "SELECT MIN(transaction_date) FROM {table} WHERE ticker = ?1"
    ))?;
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

/// Where a rebuild of each stock ticker starts: the date of its earliest transaction. A ticker
/// without transactions (a watchlist ticker) has no history to rebuild and is left out. The
/// order of `tickers` is kept.
pub fn rebuild_jobs(conn: &rusqlite::Connection, tickers: &[String]) -> Result<Vec<(String, i64)>> {
    first_transaction_days(conn, "investment_transactions", tickers)
}

/// [`rebuild_jobs`] for crypto tickers.
pub fn crypto_rebuild_jobs(
    conn: &rusqlite::Connection,
    tickers: &[String],
) -> Result<Vec<(String, i64)>> {
    first_transaction_days(conn, "crypto_transactions", tickers)
}

/// A job per ticker of `table` with transactions, from its first transaction day.
fn first_transaction_jobs(
    conn: &rusqlite::Connection,
    table: &str,
    make: fn(String, i64) -> Job,
) -> Result<Vec<Job>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT ticker, MIN(transaction_date) FROM {table} GROUP BY ticker ORDER BY ticker"
    ))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .map(|(ticker, from)| make(ticker, from))
        .collect())
}

/// Every crypto ticker with transactions from its first transaction day.
pub fn crypto_ticker_jobs(conn: &rusqlite::Connection) -> Result<Vec<Job>> {
    first_transaction_jobs(conn, "crypto_transactions", Job::CryptoTicker)
}

/// Every ticker with transactions, stocks then crypto, each from its first transaction
/// day: what the one-time rebuild and "recalculate everything" walk.
pub fn all_ticker_jobs(conn: &rusqlite::Connection) -> Result<Vec<Job>> {
    let mut jobs = first_transaction_jobs(conn, "investment_transactions", Job::StockTicker)?;
    jobs.extend(crypto_ticker_jobs(conn)?);
    Ok(jobs)
}

/// Held tickers of `kind` (outside `skip`) that have no history row in `from..=until`. A ticker
/// is held in the window when its transactions leave a position at the start of the window, or
/// when it trades inside it (a buy opens one, a sell closes one that was held). A holding that
/// was sold out before the window has no days in it to value: it needs no rows and must never
/// cost a provider call.
fn tickers_without_history(
    db: &Database,
    kind: AssetKind,
    from: i64,
    until: i64,
    skip: &BTreeMap<String, i64>,
) -> Result<Vec<String>> {
    let (investments, transactions, history) = match kind {
        AssetKind::Stock => (
            "stock_investments",
            "investment_transactions",
            "stock_value_history",
        ),
        AssetKind::Crypto => (
            "crypto_investments",
            "crypto_transactions",
            "crypto_value_history",
        ),
    };
    let from_day = floor_day(from);
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT DISTINCT i.ticker FROM {investments} i
             WHERE i.ticker NOT IN (
                 SELECT ticker FROM {history}
                 WHERE recorded_at >= ?1 AND recorded_at <= ?2
             )
             AND (
                 COALESCE((
                     SELECT SUM(CASE WHEN t.type = 'buy' THEN CAST(t.quantity AS REAL)
                                     ELSE -CAST(t.quantity AS REAL) END)
                     FROM {transactions} t
                     WHERE t.ticker = i.ticker AND t.transaction_date < ?1
                 ), 0) > {HELD_EPSILON}
                 OR EXISTS (
                     SELECT 1 FROM {transactions} t
                     WHERE t.ticker = i.ticker AND t.transaction_date >= ?1
                       AND t.transaction_date < ?2
                 )
             )"
        ))?;
        let rows = stmt.query_map([from_day, until], |row| row.get::<_, String>(0))?;
        let mut tickers = Vec::new();
        for row in rows {
            let ticker = row?;
            if !skip.contains_key(&ticker) {
                tickers.push(ticker);
            }
        }
        Ok(tickers)
    })
}

/// Below this a quantity left by buys and sells is float residue, not a position.
const HELD_EPSILON: f64 = 0.000_001;

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
    rebuild_stock_ticker_with_policy(db, ticker, from_ts, until_day, price_rule, false).await
}

/// Durable imports must retain their retry intent when the history provider fails.
pub async fn rebuild_stock_ticker_strict(
    db: &Database,
    ticker: &str,
    from_ts: i64,
    until_day: i64,
    price_rule: PriceRule,
) -> Result<RebuildStats> {
    rebuild_stock_ticker_with_policy(db, ticker, from_ts, until_day, price_rule, true).await
}

async fn rebuild_stock_ticker_with_policy(
    db: &Database,
    ticker: &str,
    from_ts: i64,
    until_day: i64,
    price_rule: PriceRule,
    strict: bool,
) -> Result<RebuildStats> {
    let from_day = floor_day(from_ts);
    if from_day > until_day {
        return Ok(RebuildStats::default());
    }

    let tickers = [ticker.to_string()];
    let first_day = db.with_conn(|conn| {
        Ok(rebuild_jobs(conn, &tickers)?
            .first()
            .map(|(_, first)| *first))
    })?;
    let fetched = if first_day.is_some() {
        price_api::get_historical_stock_series_yahoo(&tickers, from_day, until_day, strict).await?
    } else {
        Default::default()
    };
    let series = fetched.get(ticker);
    let prices: &[HistoricalPrice] = series.map_or(&[], |series| series.prices.as_slice());
    let new_splits = match series {
        Some(series) if !series.splits.is_empty() => record_new_splits(db, ticker, &series.splits),
        _ => 0,
    };

    let plan = db.with_conn(|conn| {
        plan_ticker_history(conn, AssetKind::Stock, ticker, from_day, until_day, |day| {
            price_rule(prices, day).map(|quote| (quote.price, quote.currency.clone()))
        })
    })?;
    let mut stats = apply_plan_in_chunks(db, AssetKind::Stock, ticker, &plan).await?;
    stats.full_rebuild_from = needs_full_rebuild(new_splits, from_day, first_day);
    Ok(stats)
}

/// Records the splits a history fetch reported (spec D5) and returns how many were new. A
/// failure is logged: the history itself was fetched, and a later fetch records them.
fn record_new_splits(db: &Database, ticker: &str, splits: &[stock_splits::DetectedSplit]) -> usize {
    db.with_conn(|conn| {
        stock_splits::record_detected(conn, ticker, splits, StockSplitSource::Yahoo)
    })
    .map(|inserted| inserted.len())
    .unwrap_or_else(|e| {
        log::warn!("[RECALC] Recording the splits of a history fetch failed: {e}");
        0
    })
}

/// Where the full rebuild a new split asks for starts (spec D18): the ticker's first
/// transaction day when the fetch recorded new splits but started after that day, so the closes
/// before it are still the unadjusted ones. `None` when nothing is new or the fetch already
/// covered the whole history.
fn needs_full_rebuild(new_rows: usize, from_day: i64, first_day: Option<i64>) -> Option<i64> {
    first_day.filter(|&first| new_rows > 0 && floor_day(first) < from_day)
}

/// Rebuild one crypto ticker's history rows from `from_ts` through `until_day`, like
/// [`rebuild_stock_ticker`] (spec D8). A ticker without transactions skips the fetch (a deleted
/// holding only loses its rows). Otherwise the holding's Yahoo symbol is read, and resolved when
/// missing (`crypto_symbols::ensure_symbol`); a holding without one is left alone, in strict mode
/// too, because a retry cannot fix it (the symbol picker can). With a symbol the daily USD closes
/// of the whole range come in one Yahoo request; a day without a quote keeps the last known
/// price (the price before the range, else today's quote), else no row.
pub async fn rebuild_crypto_ticker(
    db: &Database,
    ticker: &str,
    from_ts: i64,
    until_day: i64,
    price_rule: PriceRule,
) -> Result<RebuildStats> {
    rebuild_crypto_ticker_with_policy(db, ticker, from_ts, until_day, price_rule, false).await
}

/// Durable work must retain its retry intent when the history provider fails.
pub async fn rebuild_crypto_ticker_strict(
    db: &Database,
    ticker: &str,
    from_ts: i64,
    until_day: i64,
    price_rule: PriceRule,
) -> Result<RebuildStats> {
    rebuild_crypto_ticker_with_policy(db, ticker, from_ts, until_day, price_rule, true).await
}

async fn rebuild_crypto_ticker_with_policy(
    db: &Database,
    ticker: &str,
    from_ts: i64,
    until_day: i64,
    price_rule: PriceRule,
    strict: bool,
) -> Result<RebuildStats> {
    let mut from_day = floor_day(from_ts);
    if from_day > until_day {
        return Ok(RebuildStats::default());
    }

    let has_transactions = db.with_conn(|conn| {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM crypto_transactions WHERE ticker = ?1)",
            [ticker],
            |row| row.get::<_, bool>(0),
        )?)
    })?;

    let prices: Vec<HistoricalPrice> = if has_transactions {
        let symbol = match crypto_symbols::ensure_symbol(db, ticker).await? {
            Resolved::Already(symbol) => symbol,
            Resolved::Now(symbol) => {
                // The holding was on no Yahoo series until now: the whole history moves to
                // this coin's, so the range reaches back to its first transaction (spec D7).
                let first =
                    db.with_conn(|conn| crypto_rebuild_jobs(conn, &[ticker.to_string()]))?;
                from_day = extend_to_first_day(from_day, first.first().map(|(_, first)| *first));
                symbol
            }
            Resolved::None => {
                // Nothing to price the holding with: leave its history alone rather than
                // invent one from today's quote.
                log::debug!("[RECALC] Crypto holding without a Yahoo symbol skipped");
                return Ok(RebuildStats::default());
            }
        };
        let symbols = [symbol];
        let mut fetched = if strict {
            price_api::get_historical_stock_prices_yahoo_strict(&symbols, from_day, until_day)
                .await?
        } else {
            price_api::get_historical_stock_prices_yahoo(&symbols, from_day, until_day).await?
        };
        // The stored crypto price is always USD.
        usd_quotes_or_error(
            fetched.remove(&symbols[0]).unwrap_or_default(),
            strict,
            &symbols[0],
        )?
    } else {
        Vec::new()
    };

    let plan = db.with_conn(|conn| {
        // Seed for days the quotes do not cover: the last row before the range, else
        // today's quote, so a gap in the provider data does not drop the day.
        let mut last_known = last_known_crypto_price(conn, ticker, from_day)?;
        plan_ticker_history(
            conn,
            AssetKind::Crypto,
            ticker,
            from_day,
            until_day,
            |day| {
                if let Some(quote) = price_rule(&prices, day) {
                    last_known = Some((quote.price, quote.currency.clone()));
                }
                last_known.clone()
            },
        )
    })?;
    apply_plan_in_chunks(db, AssetKind::Crypto, ticker, &plan).await
}

/// The day a rebuild starts when the holding's symbol was resolved during it: the earlier of the
/// requested day and the floor of its first transaction (`None`: no transaction, unchanged).
fn extend_to_first_day(from_day: i64, first_transaction: Option<i64>) -> i64 {
    first_transaction.map_or(from_day, |first| from_day.min(floor_day(first)))
}

/// The USD quotes of a fetched crypto series. Quotes in another currency are dropped (the stored
/// crypto price is always USD); a strict rebuild whose fetch returned quotes but none in USD is
/// `AppError::ExternalApi("No USD quotes for <symbol>")` instead of planning the range from the
/// last known price, so a durable job keeps its retry intent. No quotes at all is an empty
/// series, as before.
fn usd_quotes_or_error(
    quotes: Vec<HistoricalPrice>,
    strict: bool,
    symbol: &str,
) -> Result<Vec<HistoricalPrice>> {
    let fetched = quotes.len();
    let usd: Vec<HistoricalPrice> = quotes
        .into_iter()
        .filter(|quote| quote.currency.eq_ignore_ascii_case("USD"))
        .collect();
    if strict && fetched > 0 && usd.is_empty() {
        return Err(crate::error::AppError::ExternalApi(format!(
            "No USD quotes for {symbol}"
        )));
    }
    Ok(usd)
}

/// `(price, currency)` of the ticker's newest history row before `from_day`, else of its
/// current quote; rows with an unreadable or non-positive price do not count.
fn last_known_crypto_price(
    conn: &rusqlite::Connection,
    ticker: &str,
    from_day: i64,
) -> Result<Option<(f64, String)>> {
    let usable = |row: Option<(String, String)>| {
        row.and_then(|(price, currency)| {
            let price = price.trim().parse::<f64>().ok()?;
            (price.is_finite() && price > 0.0).then_some((price, currency))
        })
    };
    let before: Option<(String, String)> = conn
        .query_row(
            "SELECT price, currency FROM crypto_value_history
             WHERE ticker = ?1 AND recorded_at < ?2 ORDER BY recorded_at DESC LIMIT 1",
            rusqlite::params![ticker, from_day],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some(known) = usable(before) {
        return Ok(Some(known));
    }
    let current: Option<(String, String)> = conn
        .query_row(
            "SELECT price, currency FROM crypto_prices WHERE symbol = ?1",
            [ticker],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(usable(current))
}

/// Write a ticker plan in [`CHUNK_DAYS`]-day chunks, one lock acquisition each, yielding in
/// between so reads are served throughout.
async fn apply_plan_in_chunks(
    db: &Database,
    kind: AssetKind,
    ticker: &str,
    plan: &[DayPlan],
) -> Result<RebuildStats> {
    let mut stats = RebuildStats::default();
    for chunk in plan.chunks(CHUNK_DAYS as usize) {
        let chunk_stats = db.with_conn(|conn| apply_ticker_plan(conn, kind, ticker, chunk))?;
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
pub(crate) fn chunk_ranges(from_day: i64, until_day: i64, step: i64) -> Vec<(i64, i64)> {
    let mut ranges = Vec::new();
    let mut start = from_day;
    while start < until_day {
        let end = (start + step).min(until_day);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECONDS_PER_DAY;

    fn stock(ticker: &str, from: i64) -> Job {
        Job::StockTicker(ticker.to_string(), from)
    }

    fn crypto(ticker: &str, from: i64) -> Job {
        Job::CryptoTicker(ticker.to_string(), from)
    }

    #[test]
    fn five_deletes_of_one_ticker_coalesce_into_one_rebuild_from_the_earliest_date() {
        let queue = HistoryRecalc::new();
        // The first enqueue starts the worker, the rest ride along.
        assert!(queue.enqueue([stock("AAPL", 500 * DAY)], None));
        for date in [300 * DAY, 400 * DAY, 100 * DAY, 450 * DAY] {
            assert!(!queue.enqueue([stock("AAPL", date)], None));
        }

        let (batch, _) = queue.take_batch().expect("one batch");
        assert_eq!(batch.stocks.len(), 1, "one rebuild for the ticker");
        assert_eq!(batch.stocks["AAPL"], 100 * DAY, "from the earliest date");
        assert!(batch.crypto.is_empty() && batch.days.is_none());
        assert!(queue.take_batch().is_none(), "nothing else is queued");
    }

    #[test]
    fn each_ticker_gets_its_own_entry() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue([stock("AAPL", 10 * DAY), stock("MSFT", 20 * DAY)], None));
        assert!(!queue.enqueue([stock("MSFT", 5 * DAY), stock("CEZ.PR", 30 * DAY)], None));

        let (batch, _) = queue.take_batch().expect("batch");
        let entries: Vec<(&str, i64)> =
            batch.stocks.iter().map(|(t, d)| (t.as_str(), *d)).collect();
        assert_eq!(
            entries,
            vec![("AAPL", 10 * DAY), ("CEZ.PR", 30 * DAY), ("MSFT", 5 * DAY)]
        );
    }

    #[test]
    fn the_job_kinds_coalesce_apart_each_to_its_earliest_date() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue(
            [
                stock("BTC", 50 * DAY),
                crypto("BTC", 80 * DAY),
                Job::Days(60 * DAY),
            ],
            None
        ));
        assert!(!queue.enqueue(
            [
                stock("BTC", 70 * DAY),
                crypto("BTC", 20 * DAY),
                crypto("ETH", 90 * DAY),
                Job::Days(30 * DAY),
                Job::Days(40 * DAY),
            ],
            None
        ));

        let (batch, _) = queue.take_batch().expect("batch");
        assert_eq!(
            batch.stocks,
            BTreeMap::from([("BTC".to_string(), 50 * DAY)])
        );
        assert_eq!(
            batch.crypto,
            BTreeMap::from([("BTC".to_string(), 20 * DAY), ("ETH".to_string(), 90 * DAY)]),
            "a crypto ticker is not the stock ticker of the same symbol"
        );
        assert_eq!(batch.days, Some(30 * DAY));
        assert_eq!(
            batch.earliest(),
            Some(20 * DAY),
            "the days rebuild starts here"
        );
        assert_eq!(batch.tickers(), vec!["BTC", "BTC", "ETH"]);
    }

    #[test]
    fn an_empty_batch_has_no_earliest_day() {
        assert_eq!(Batch::default().earliest(), None);
        let days_only = Batch {
            days: Some(7 * DAY),
            ..Batch::default()
        };
        assert_eq!(days_only.earliest(), Some(7 * DAY));
        assert!(days_only.tickers().is_empty());
    }

    #[test]
    fn work_queued_while_a_batch_runs_goes_to_the_same_worker() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue([stock("AAPL", DAY)], None));
        let (first, _) = queue.take_batch().expect("first batch");
        assert_eq!(first.stocks.len(), 1);

        // Mid-batch: the worker is still active, so no second worker starts…
        assert!(!queue.enqueue([stock("AAPL", 2 * DAY), crypto("BTC", 3 * DAY)], None));
        assert!(queue.is_pending_ticker("AAPL"));
        assert!(queue.is_pending_ticker("BTC"), "either class counts");
        // …and it finds the new jobs when it asks again.
        let (second, _) = queue.take_batch().expect("second batch");
        assert_eq!(second.stocks["AAPL"], 2 * DAY);
        assert_eq!(second.crypto["BTC"], 3 * DAY);
        assert!(!queue.is_pending_ticker("AAPL"));
    }

    #[test]
    fn a_follow_up_of_a_running_batch_goes_to_the_same_worker() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue([stock("AAPL", 40 * DAY)], None));
        queue.take_batch().expect("first batch");

        // The batch found a new split: the full rebuild goes behind it, earliest date kept.
        queue.requeue([stock("AAPL", 10 * DAY)]);
        queue.requeue([stock("AAPL", 20 * DAY)]);
        assert!(queue.is_pending_ticker("AAPL"));
        assert!(
            !queue.enqueue([], None),
            "the running worker still owns the queue"
        );
        let (second, _) = queue.take_batch().expect("the follow-up batch");
        assert_eq!(second.stocks["AAPL"], 10 * DAY);
        assert!(queue.take_batch().is_none());
    }

    #[test]
    fn a_worker_that_panics_leaves_the_queue_open_for_the_next_job() {
        let state = HistoryRecalc::new();
        assert!(state.enqueue([Job::Days(86_400)], None), "starts a worker");

        // The worker takes its batch, then the batch panics.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = WorkerGuard::new(&state);
            let _batch = state.take_batch().expect("a batch");
            panic!("a batch failed");
        }));
        assert!(outcome.is_err());

        assert!(
            state.enqueue([Job::Days(172_800)], None),
            "a later rebuild starts a new worker instead of waiting for the dead one"
        );
    }

    #[test]
    fn a_worker_that_finished_does_not_reopen_the_queue_for_a_second_worker() {
        let state = HistoryRecalc::new();
        assert!(state.enqueue([Job::Days(86_400)], None));
        let mut guard = WorkerGuard::new(&state);
        assert!(state.take_batch().is_some());
        assert!(state.take_batch().is_none(), "idle");

        // A job queued right after starts the next worker…
        assert!(state.enqueue([Job::Days(172_800)], None));
        // …which the first worker finishing must not push aside.
        guard.finish();
        drop(guard);
        assert!(
            !state.enqueue([Job::Days(259_200)], None),
            "the second worker is still the only one"
        );
    }

    #[test]
    fn a_job_after_the_worker_went_idle_starts_a_new_worker() {
        let queue = HistoryRecalc::new();
        assert!(queue.enqueue([Job::Days(DAY)], None));
        assert!(queue.take_batch().is_some());
        assert!(queue.take_batch().is_none(), "worker marks itself idle");

        assert!(
            queue.enqueue([Job::Days(DAY)], None),
            "a new worker is needed"
        );
    }

    #[test]
    fn a_completion_waits_for_the_batch_that_carries_its_jobs() {
        let queue = HistoryRecalc::new();
        let outcomes: Arc<Mutex<Vec<BatchOutcome>>> = Arc::default();
        let sink = outcomes.clone();
        let completion: Completion =
            Box::new(move |outcome| sink.lock().expect("lock").push(outcome.clone()));

        assert!(queue.enqueue([stock("AAPL", DAY)], Some(completion)));
        let (batch, completions) = queue.take_batch().expect("batch");
        assert_eq!(batch.stocks.len(), 1);
        assert_eq!(completions.len(), 1);
        assert!(outcomes.lock().expect("lock").is_empty(), "not called yet");
        let failed_ticker = BatchOutcome {
            days_ok: true,
            failed_tickers: vec!["AAPL".to_string()],
        };
        for completion in completions {
            completion(&failed_ticker);
        }
        assert_eq!(
            *outcomes.lock().expect("lock"),
            vec![failed_ticker],
            "the completion learns which tickers failed"
        );

        // A job without a completion leaves the next batch without one.
        assert!(!queue.enqueue([Job::Days(DAY)], None));
        let (_, completions) = queue.take_batch().expect("next batch");
        assert!(completions.is_empty());
    }

    #[test]
    fn jobs_dated_today_or_later_are_dropped_and_a_completion_runs_at_once() {
        let queue = HistoryRecalc::new();
        let outcomes: Arc<Mutex<Vec<BatchOutcome>>> = Arc::default();
        let sink = outcomes.clone();
        let completion: Completion =
            Box::new(move |outcome| sink.lock().expect("lock").push(outcome.clone()));
        let notify: Notifier = Arc::new(|_| {});
        let db = Database::new();

        schedule(
            &queue,
            &db,
            notify,
            no_quote,
            vec![Job::Days(today_start()), stock("AAPL", today_start() + DAY)],
            Some(completion),
        );

        assert_eq!(
            *outcomes.lock().expect("lock"),
            vec![BatchOutcome::nothing_to_do()]
        );
        assert!(queue.take_batch().is_none(), "nothing was queued");
    }

    fn transactions_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, transaction_date INTEGER NOT NULL
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, transaction_date INTEGER NOT NULL
            );",
        )
        .expect("schema");
        conn
    }

    fn add_transaction(
        conn: &rusqlite::Connection,
        table: &str,
        id: &str,
        ticker: &str,
        date: i64,
    ) {
        conn.execute(
            &format!("INSERT INTO {table} (id, ticker, transaction_date) VALUES (?1, ?2, ?3)"),
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
        add_transaction(&conn, "investment_transactions", "t1", "CSPX.L", 300 * DAY);
        add_transaction(&conn, "investment_transactions", "t2", "CSPX.L", 100 * DAY);
        add_transaction(&conn, "investment_transactions", "t3", "BARC.L", 200 * DAY);
        add_transaction(&conn, "investment_transactions", "t4", "AAPL", 50 * DAY);

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
    fn crypto_rebuild_jobs_start_at_each_coins_first_transaction() {
        let conn = transactions_db();
        add_transaction(&conn, "crypto_transactions", "a", "BTC", 500);
        add_transaction(&conn, "crypto_transactions", "b", "BTC", 200);
        add_transaction(&conn, "crypto_transactions", "c", "ETH", 900);
        assert_eq!(
            crypto_rebuild_jobs(&conn, &tickers(&["ETH", "BTC", "SOL"])).expect("jobs"),
            vec![("ETH".to_string(), 900), ("BTC".to_string(), 200)],
            "a coin without transactions has nothing to rebuild"
        );
    }

    #[test]
    fn every_ticker_with_transactions_is_a_job_from_its_first_transaction() {
        let conn = transactions_db();
        add_transaction(&conn, "investment_transactions", "t1", "MSFT", 300 * DAY);
        add_transaction(&conn, "investment_transactions", "t2", "AAPL", 100 * DAY);
        add_transaction(&conn, "investment_transactions", "t3", "AAPL", 50 * DAY);
        add_transaction(&conn, "crypto_transactions", "c1", "BTC", 70 * DAY);

        assert_eq!(
            all_ticker_jobs(&conn).expect("jobs"),
            vec![
                stock("AAPL", 50 * DAY),
                stock("MSFT", 300 * DAY),
                crypto("BTC", 70 * DAY),
            ]
        );
        assert!(all_ticker_jobs(&transactions_db())
            .expect("none")
            .is_empty());
    }

    #[test]
    fn a_ticker_without_transactions_has_nothing_to_rebuild() {
        let conn = transactions_db();
        add_transaction(&conn, "investment_transactions", "t1", "CSPX.L", 100 * DAY);

        // A watchlist ticker, and one asked for twice.
        let jobs = rebuild_jobs(&conn, &tickers(&["WATCH.L", "CSPX.L", "WATCH.L"])).expect("jobs");
        assert_eq!(jobs, vec![("CSPX.L".to_string(), 100 * DAY)]);
        assert!(rebuild_jobs(&conn, &[]).expect("none").is_empty());
    }

    #[test]
    fn a_new_split_found_by_a_partial_rebuild_asks_for_the_full_history() {
        let first = 10 * DAY;
        assert_eq!(needs_full_rebuild(1, 40 * DAY, Some(first)), Some(first));
        assert_eq!(needs_full_rebuild(2, first + DAY, Some(first)), Some(first));
        // Nothing new, or the fetch already started at the first transaction.
        assert_eq!(needs_full_rebuild(0, 40 * DAY, Some(first)), None);
        assert_eq!(needs_full_rebuild(1, first, Some(first)), None);
        assert_eq!(needs_full_rebuild(1, first - DAY, Some(first)), None);
        // A first transaction later on its own first day is still that day.
        assert_eq!(needs_full_rebuild(1, first, Some(first + 3_600)), None);
        // No transactions, nothing to rebuild.
        assert_eq!(needs_full_rebuild(1, 40 * DAY, None), None);
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

    // --- crypto rebuild -----------------------------------------------------------------

    fn crypto_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, type TEXT NOT NULL,
                quantity TEXT NOT NULL, transaction_date INTEGER NOT NULL
            );
            CREATE TABLE crypto_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL, price TEXT NOT NULL, currency TEXT NOT NULL,
                UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE crypto_prices (symbol TEXT PRIMARY KEY, price TEXT, currency TEXT);",
        )
        .expect("schema");
        conn
    }

    #[test]
    fn a_resolution_reaches_the_range_back_to_the_first_transaction() {
        // The first transaction is earlier than the requested day: the whole series moves.
        assert_eq!(
            extend_to_first_day(50 * DAY, Some(10 * DAY + 3_600)),
            10 * DAY
        );
        // A request that already starts earlier, or a holding without transactions, is unchanged.
        assert_eq!(extend_to_first_day(5 * DAY, Some(10 * DAY)), 5 * DAY);
        assert_eq!(extend_to_first_day(50 * DAY, None), 50 * DAY);
    }

    fn quote(timestamp: i64, currency: &str) -> HistoricalPrice {
        HistoricalPrice {
            timestamp,
            price: 1.0,
            currency: currency.to_string(),
        }
    }

    #[test]
    fn a_strict_rebuild_without_usd_quotes_fails_instead_of_carrying_the_last_price() {
        let eur_only = vec![quote(DAY, "EUR"), quote(2 * DAY, "EUR")];
        let err = usd_quotes_or_error(eur_only.clone(), true, "SUI20947-USD").unwrap_err();
        assert!(
            matches!(&err, crate::error::AppError::ExternalApi(m) if m == "No USD quotes for SUI20947-USD"),
            "{err:?}"
        );
        // Not strict: today's behaviour, no USD rows, the plan falls back to the last known price.
        assert!(usd_quotes_or_error(eur_only, false, "SUI20947-USD")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn usd_quotes_are_kept_and_an_empty_fetch_is_not_an_error_even_when_strict() {
        let mixed = vec![
            quote(DAY, "USD"),
            quote(2 * DAY, "EUR"),
            quote(3 * DAY, "usd"),
        ];
        let kept = usd_quotes_or_error(mixed, true, "BTC-USD").unwrap();
        assert_eq!(
            kept.iter().map(|q| q.timestamp).collect::<Vec<_>>(),
            vec![DAY, 3 * DAY]
        );
        // No quotes at all keeps today's meaning ("no data for the range").
        assert!(usd_quotes_or_error(Vec::new(), true, "BTC-USD")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_last_known_crypto_price_is_the_row_before_the_range_else_the_current_quote() {
        let conn = crypto_db();
        assert_eq!(
            last_known_crypto_price(&conn, "BTC", 10 * DAY).expect("none"),
            None
        );

        conn.execute(
            "INSERT INTO crypto_prices (symbol, price, currency) VALUES ('BTC', '30000', 'USD')",
            [],
        )
        .expect("current quote");
        assert_eq!(
            last_known_crypto_price(&conn, "BTC", 10 * DAY).expect("current"),
            Some((30_000.0, "USD".to_string()))
        );

        for (day, price) in [(4, "100"), (8, "120"), (10, "999")] {
            conn.execute(
                "INSERT INTO crypto_value_history (id, ticker, recorded_at, quantity, price, currency)
                 VALUES (?1, 'BTC', ?2, '1', ?3, 'EUR')",
                rusqlite::params![format!("h{day}"), day * DAY, price],
            )
            .expect("row");
        }
        assert_eq!(
            last_known_crypto_price(&conn, "BTC", 10 * DAY).expect("before the range"),
            Some((120.0, "EUR".to_string())),
            "the newest row before the range wins over the current quote; the row on the range's first day does not count"
        );

        conn.execute(
            "UPDATE crypto_value_history SET price = 'x' WHERE id = 'h8'",
            [],
        )
        .expect("corrupt");
        assert_eq!(
            last_known_crypto_price(&conn, "BTC", 10 * DAY).expect("unreadable"),
            Some((30_000.0, "USD".to_string())),
            "an unreadable price is no price"
        );
    }

    #[test]
    fn a_crypto_plan_carries_the_last_known_price_over_days_the_quotes_do_not_cover() {
        let conn = crypto_db();
        for (id, kind, qty, day) in [("a", "buy", "2", 1), ("b", "sell", "2", 5)] {
            conn.execute(
                "INSERT INTO crypto_transactions (id, ticker, type, quantity, transaction_date)
                 VALUES (?1, 'BTC', ?2, ?3, ?4)",
                rusqlite::params![id, kind, qty, day * DAY],
            )
            .expect("tx");
        }
        // A stale row of the sold-out day that the rebuild must remove.
        conn.execute(
            "INSERT INTO crypto_value_history (id, ticker, recorded_at, quantity, price, currency)
             VALUES ('old', 'BTC', ?1, '2', '1', 'USD')",
            [5 * DAY],
        )
        .expect("stale row");

        let quotes = vec![HistoricalPrice {
            timestamp: 2 * DAY + 3600,
            price: 50.0,
            currency: "USD".to_string(),
        }];
        let rule: PriceRule =
            |prices, day| crate::services::backfill::price_at_or_before(prices, day);
        let mut last_known: Option<(f64, String)> = Some((10.0, "USD".to_string()));
        let plan = plan_ticker_history(&conn, AssetKind::Crypto, "BTC", DAY, 6 * DAY, |day| {
            if let Some(quote) = rule(&quotes, day) {
                last_known = Some((quote.price, quote.currency.clone()));
            }
            last_known.clone()
        })
        .expect("plan");
        apply_ticker_plan(&conn, AssetKind::Crypto, "BTC", &plan).expect("apply");

        let rows: Vec<(i64, f64, f64)> = conn
            .prepare(
                "SELECT recorded_at / 86400, CAST(quantity AS REAL), CAST(price AS REAL)
                 FROM crypto_value_history ORDER BY recorded_at",
            )
            .expect("prepare")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        assert_eq!(
            rows,
            vec![
                (1, 2.0, 10.0), // before the first quote: the seed
                (2, 2.0, 50.0), // the first quote
                (3, 2.0, 50.0), // carried
                (4, 2.0, 50.0),
            ],
            "sold out from day 5: no row, the stale one is gone"
        );
    }

    // --- the worker on a real database ----------------------------------------------------

    fn open_test_db(seed: u8) -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Database::new();
        let key = crate::services::crypto::master_key_to_hex(&[seed; 32]);
        db.create_with_key(dir.path().join("moony.db"), &key)
            .expect("create db");
        (dir, db)
    }

    fn recording_notifier() -> (Notifier, Arc<Mutex<Vec<Notice>>>) {
        let notices: Arc<Mutex<Vec<Notice>>> = Arc::default();
        let sink = notices.clone();
        let notify: Notifier = Arc::new(move |notice| sink.lock().expect("lock").push(notice));
        (notify, notices)
    }

    fn status(ticker: &str, status: RecalcStatus) -> Notice {
        Notice::Status(HistoryRecalcEvent {
            ticker: ticker.to_string(),
            status,
        })
    }

    /// An account whose balance record opens the history `first` days ago.
    fn seed_balance_record(db: &Database, as_of: i64) {
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO bank_accounts (id, name, currency, balance, balance_as_of)
                 VALUES ('acc', 'Main', 'CZK', '100', ?1)",
                [as_of],
            )?;
            conn.execute(
                "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of)
                 VALUES ('rec', 'acc', '100', 'CZK', ?1)",
                [as_of],
            )?;
            Ok(())
        })
        .expect("seed account");
    }

    /// The whole worker step on a real (SQLCipher, migrated) database, for a ticker that
    /// no longer has transactions — the one case that needs no network: its stale history
    /// rows are removed in chunks, the portfolio days are re-derived without it, and the
    /// events arrive in the documented order.
    #[tokio::test]
    async fn a_deleted_ticker_is_cleaned_up_and_reported_running_then_done() {
        let (_dir, db) = open_test_db(3);
        let today = today_start();
        let first = today - 3 * DAY;
        seed_balance_record(&db, first);
        db.with_conn(|conn| {
            for offset in 0..3 {
                let day = first + offset * DAY;
                conn.execute(
                    "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
                     VALUES (?1, 'GONE', ?2, '2', '25', 'USD')",
                    rusqlite::params![format!("h{offset}"), day],
                )?;
                conn.execute(
                    "INSERT INTO portfolio_metrics_history (id, recorded_at, investments_by_currency, source)
                     VALUES (?1, ?2, '{\"USD\":50}', 'backfill')",
                    rusqlite::params![format!("p{offset}"), day],
                )?;
            }
            Ok(())
        })
        .expect("seed");

        let (notify, notices) = recording_notifier();
        let state = HistoryRecalc::new();
        let batch = Batch {
            stocks: BTreeMap::from([("GONE".to_string(), first)]),
            ..Batch::default()
        };

        assert!(run_batch(&state, &db, &notify, no_quote, &batch, false)
            .await
            .is_clean());

        assert_eq!(
            *notices.lock().expect("lock"),
            vec![
                status("GONE", RecalcStatus::Running),
                status("GONE", RecalcStatus::Done),
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
            let days: Vec<(i64, String, String)> = conn
                .prepare(
                    "SELECT recorded_at, investments_by_currency, savings_by_currency
                     FROM portfolio_metrics_history ORDER BY recorded_at",
                )?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(
                days.iter().map(|(day, _, _)| *day).collect::<Vec<_>>(),
                vec![first, first + DAY, first + 2 * DAY],
                "one row per day since the first fact, today's row is not touched"
            );
            for (_, investments, savings) in &days {
                assert_eq!(investments, "{}", "the days no longer count the ticker");
                assert!(
                    savings.contains("100"),
                    "the balance record holds: {savings}"
                );
            }
            Ok(())
        })
        .expect("verify");
    }

    #[tokio::test]
    async fn a_deleted_crypto_ticker_is_cleaned_up_without_a_network_call() {
        let (_dir, db) = open_test_db(4);
        let today = today_start();
        let first = today - 3 * DAY;
        seed_balance_record(&db, first);
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO crypto_value_history (id, ticker, recorded_at, quantity, price, currency)
                 VALUES ('h', 'GONE', ?1, '1', '9', 'USD')",
                [first],
            )?;
            Ok(())
        })
        .expect("seed");

        let (notify, notices) = recording_notifier();
        let batch = Batch {
            crypto: BTreeMap::from([("GONE".to_string(), first)]),
            ..Batch::default()
        };
        assert!(
            run_batch(&HistoryRecalc::new(), &db, &notify, no_quote, &batch, true)
                .await
                .is_clean(),
            "a holding without a symbol or transactions needs no provider, strict or not"
        );

        assert_eq!(
            *notices.lock().expect("lock"),
            vec![
                status("GONE", RecalcStatus::Running),
                status("GONE", RecalcStatus::Done),
                Notice::BatchComplete
            ]
        );
        let rows: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM crypto_value_history WHERE ticker = 'GONE'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .expect("count");
        assert_eq!(rows, 0);
    }

    /// A holding with its transactions `(type, day)`; the stored quantity is what they leave.
    fn seed_holding(db: &Database, crypto: bool, ticker: &str, trades: &[(&str, i64)]) {
        let transactions = if crypto {
            "crypto_transactions"
        } else {
            "investment_transactions"
        };
        let held: i64 = trades
            .iter()
            .map(|(kind, _)| if *kind == "buy" { 1 } else { -1 })
            .sum();
        db.with_conn(|conn| {
            if crypto {
                conn.execute(
                    "INSERT INTO crypto_investments (id, ticker, yahoo_symbol, name, quantity)
                     VALUES (?1, ?2, ?2 || '-USD', ?2, ?3)",
                    rusqlite::params![format!("inv-{ticker}"), ticker, held.to_string()],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO stock_investments (id, ticker, company_name, quantity)
                     VALUES (?1, ?2, ?2, ?3)",
                    rusqlite::params![format!("inv-{ticker}"), ticker, held.to_string()],
                )?;
            }
            for (index, (kind, day)) in trades.iter().enumerate() {
                let name_column = if crypto { "name" } else { "company_name" };
                conn.execute(
                    &format!(
                        "INSERT INTO {transactions}
                         (id, investment_id, type, ticker, {name_column}, quantity, price_per_unit,
                          currency, transaction_date)
                         VALUES (?1, ?2, ?3, ?4, ?4, '1', '10', 'USD', ?5)"
                    ),
                    rusqlite::params![
                        format!("tx-{ticker}-{index}"),
                        format!("inv-{ticker}"),
                        kind,
                        ticker,
                        day
                    ],
                )?;
            }
            Ok(())
        })
        .expect("seed holding");
    }

    fn seed_history_row(db: &Database, crypto: bool, ticker: &str, day: i64) {
        let table = if crypto {
            "crypto_value_history"
        } else {
            "stock_value_history"
        };
        db.with_conn(|conn| {
            conn.execute(
                &format!(
                    "INSERT INTO {table} (id, ticker, recorded_at, quantity, price, currency)
                     VALUES (?1, ?2, ?3, '1', '10', 'USD')"
                ),
                rusqlite::params![format!("h-{ticker}-{day}"), ticker, day],
            )?;
            Ok(())
        })
        .expect("seed history row");
    }

    /// The network-bound step looks up the tickers this returns (and nothing else): a holding
    /// that was sold out before the window needs no rows in it, so it must not cost a provider
    /// call on every unrelated batch.
    #[tokio::test]
    async fn only_tickers_held_in_the_window_are_looked_up() {
        for crypto in [false, true] {
            let kind = if crypto {
                AssetKind::Crypto
            } else {
                AssetKind::Stock
            };
            let (_dir, db) = open_test_db(if crypto { 11 } else { 10 });
            let today = today_start();
            let from = today - 10 * DAY;
            // held through the window, without a single row in it
            seed_holding(&db, crypto, "HELD", &[("buy", today - 100 * DAY)]);
            // sold out before the window, no trade inside it
            seed_holding(
                &db,
                crypto,
                "SOLD",
                &[("buy", today - 100 * DAY), ("sell", today - 50 * DAY)],
            );
            // sold inside the window: held on its first days
            seed_holding(
                &db,
                crypto,
                "SOLD_IN",
                &[("buy", today - 100 * DAY), ("sell", today - 5 * DAY)],
            );
            // bought inside the window
            seed_holding(&db, crypto, "NEW", &[("buy", today - 3 * DAY)]);
            // bought today: no past day holds it
            seed_holding(&db, crypto, "TODAY", &[("buy", today)]);
            // held and already covered
            seed_holding(&db, crypto, "COVERED", &[("buy", today - 100 * DAY)]);
            seed_history_row(&db, crypto, "COVERED", today - 4 * DAY);
            // held but queued on its own
            seed_holding(&db, crypto, "QUEUED", &[("buy", today - 100 * DAY)]);
            let queued = BTreeMap::from([("QUEUED".to_string(), from)]);

            let mut found = tickers_without_history(&db, kind, from, today, &queued)
                .expect("tickers without history");
            found.sort();
            assert_eq!(found, vec!["HELD", "NEW", "SOLD_IN"], "crypto: {crypto}");

            // the same through the plan of the days-only batch: each starts at the later of the
            // batch's earliest day and its first transaction
            let mut jobs = unseeded_ticker_jobs(&db, kind, (from, today), &queued).expect("jobs");
            jobs.sort();
            assert_eq!(
                jobs,
                vec![
                    ("HELD".to_string(), from),
                    ("NEW".to_string(), today - 3 * DAY),
                    ("SOLD_IN".to_string(), from),
                ],
                "crypto: {crypto}"
            );
        }
    }

    #[tokio::test]
    async fn a_days_only_batch_with_only_sold_out_holdings_looks_up_nothing() {
        let (_dir, db) = open_test_db(12);
        let today = today_start();
        for (crypto, ticker) in [(false, "OLD"), (true, "OLDCOIN")] {
            seed_holding(
                &db,
                crypto,
                ticker,
                &[("buy", today - 100 * DAY), ("sell", today - 50 * DAY)],
            );
        }
        let none = BTreeMap::new();
        for kind in [AssetKind::Stock, AssetKind::Crypto] {
            assert!(
                unseeded_ticker_jobs(&db, kind, (today - 10 * DAY, today), &none)
                    .expect("jobs")
                    .is_empty()
            );
        }
        // …and a start before the sale does hold them in the window
        assert_eq!(
            unseeded_ticker_jobs(&db, AssetKind::Stock, (today - 60 * DAY, today), &none)
                .expect("jobs"),
            vec![("OLD".to_string(), today - 60 * DAY)]
        );
    }

    #[tokio::test]
    async fn a_days_job_fills_a_fresh_profile_from_a_balance_record_a_year_back() {
        let (_dir, db) = open_test_db(5);
        let today = today_start();
        let year_ago = today - 365 * DAY;
        seed_balance_record(&db, year_ago);

        let (notify, notices) = recording_notifier();
        let batch = Batch {
            days: Some(year_ago),
            ..Batch::default()
        };
        assert!(
            run_batch(&HistoryRecalc::new(), &db, &notify, no_quote, &batch, false)
                .await
                .is_clean()
        );
        assert_eq!(
            *notices.lock().expect("lock"),
            vec![Notice::BatchComplete],
            "no ticker, no status events"
        );

        let (rows, first_row): (i64, i64) = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*), MIN(recorded_at) FROM portfolio_metrics_history",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .expect("rows");
        assert_eq!(rows, 365, "every day since the record, today excluded");
        assert_eq!(first_row, year_ago);
    }

    #[tokio::test]
    async fn the_days_rebuild_never_starts_before_the_first_fact_or_the_floor() {
        let (_dir, db) = open_test_db(6);
        let today = today_start();
        seed_balance_record(&db, today - 5 * DAY);

        let stats = rebuild_days_async(&db, 0, today).await.expect("rebuild");
        assert_eq!(stats.upserted, 5, "Days(0) is clamped to the first fact");
    }
}
