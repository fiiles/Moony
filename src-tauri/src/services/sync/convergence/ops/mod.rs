//! Random operations for the convergence harness. Every operation goes through the real service
//! (and the `validate()` its command calls first), never raw SQL, on entities the device itself
//! holds. Names come from small pools, so devices collide on unique keys and rename scopes.
//!
//! An operation the service refuses (validation, not found, a unique or foreign key the service
//! leaves to the database) is skipped: that is part of the test. Any other error is the engine's
//! and comes back as `Err`, so the harness fails with the seed and the trace.
//!
//! Targets are picked by content, never by rowid or a random UUID: by the row's values, then by
//! its generation clock `'*'`, which the fake wall clock makes the same on every run. So a seed
//! picks the same rows every time.

mod assets;
mod attachments;
mod bank;
mod planning;
mod stocks;

use std::path::Path;

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use crate::error::{AppError, Result};

pub(in crate::services::sync::convergence) use attachments::queue_consistent;

/// What one operation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// The service (or its command's validation) refused it.
    Rejected(String),
    /// The device holds nothing the operation could act on.
    NoTarget,
}

/// One operation as the trace shows it.
#[derive(Debug, Clone)]
pub struct Step {
    pub name: &'static str,
    pub detail: String,
    pub outcome: Outcome,
}

/// What an operation returns: its detail and outcome, or the engine's error.
type Res = Result<(String, Outcome)>;
type OpFn = fn(&mut Connection, &mut StdRng, &Path) -> Res;

/// Every operation, by name.
fn catalog() -> Vec<(&'static str, OpFn)> {
    [
        bank::OPS,
        stocks::OPS,
        assets::OPS,
        planning::OPS,
        attachments::OPS,
    ]
    .concat()
}

/// The names of every operation.
pub fn names() -> Vec<&'static str> {
    catalog().into_iter().map(|(name, _)| name).collect()
}

/// The name of one operation, chosen by the schedule rng.
pub fn random_name(schedule: &mut StdRng) -> &'static str {
    let ops = catalog();
    ops[schedule.random_range(0..ops.len())].0
}

/// Runs the operation `name` on `conn`; its targets and arguments come from `rng`. `dir` is the
/// profile's data directory (deletes clean attachment files there).
pub fn run_op(conn: &mut Connection, rng: &mut StdRng, dir: &Path, name: &str) -> Result<Step> {
    let (name, op) = catalog()
        .into_iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| AppError::Internal(format!("no operation {name}")))?;
    let (detail, outcome) = op(conn, rng, dir)?;
    Ok(Step {
        name,
        detail,
        outcome,
    })
}

/// UTC midnight of 2026-01-01; every date an operation picks is a day of the 150 that follow.
const DAY0: i64 = 1_767_225_600;
const DAY: i64 = 86_400;
/// The first day of each month of 2026 up to October.
const MONTHS: [i64; 10] = [
    1_767_225_600,
    1_769_904_000,
    1_772_323_200,
    1_775_001_600,
    1_777_593_600,
    1_780_272_000,
    1_782_864_000,
    1_785_542_400,
    1_788_220_800,
    1_790_812_800,
];
const CURRENCIES: [&str; 3] = ["CZK", "EUR", "USD"];

/// Whether `error` is a refusal of the write rather than an engine failure.
fn is_refusal(error: &AppError) -> bool {
    match error {
        AppError::Validation(_) | AppError::NotFound(_) => true,
        AppError::Database(message) => {
            message.contains("UNIQUE constraint failed")
                || message.contains("FOREIGN KEY constraint failed")
        }
        _ => false,
    }
}

fn run(detail: String, work: impl FnOnce() -> Result<()>) -> Res {
    match work() {
        Ok(()) => Ok((detail, Outcome::Done)),
        Err(e) if is_refusal(&e) => Ok((detail, Outcome::Rejected(e.to_string()))),
        Err(e) => Err(AppError::Internal(format!("{detail}: {e}"))),
    }
}

fn no_target() -> Res {
    Ok((String::new(), Outcome::NoTarget))
}

fn day(rng: &mut StdRng) -> i64 {
    DAY0 + rng.random_range(0..150) * DAY
}

fn month(rng: &mut StdRng) -> i64 {
    MONTHS[rng.random_range(0..MONTHS.len())]
}

fn money(rng: &mut StdRng) -> String {
    format!(
        "{}.{:02}",
        rng.random_range(1..5_000),
        rng.random_range(0..100)
    )
}

fn one_of<T: Copy>(rng: &mut StdRng, items: &[T]) -> T {
    items[rng.random_range(0..items.len())]
}

fn currency(rng: &mut StdRng) -> &'static str {
    one_of(rng, &CURRENCIES)
}

/// `SELECT {select} FROM {table} t {filter}`, ordered by `content` (columns of the row, never its
/// id), then by the row's generation clock: an order every run of a seed repeats.
fn by_content(table: &str, select: &str, filter: &str, content: &str) -> String {
    let clock = |column: &str| {
        format!(
            "(SELECT c.{column} FROM sync_clock c WHERE c.tbl = '{table}' AND c.pk = t.id \
             AND c.col = '*')"
        )
    };
    format!(
        "SELECT {select} FROM {table} t {filter} ORDER BY {content}, {}, {}",
        clock("hlc"),
        clock("device")
    )
}

/// The first column of every row `sql` selects, in its order.
fn ids(conn: &Connection, sql: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// One row of `sql` (its first column), or `None` when it selects nothing.
fn pick(conn: &Connection, rng: &mut StdRng, sql: &str) -> Result<Option<String>> {
    let ids = ids(conn, sql)?;
    Ok((!ids.is_empty()).then(|| ids[rng.random_range(0..ids.len())].clone()))
}

/// One row of `sql` as two text columns.
fn pick2(conn: &Connection, rng: &mut StdRng, sql: &str) -> Result<Option<(String, String)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok((!rows.is_empty()).then(|| rows[rng.random_range(0..rows.len())].clone()))
}

/// The wall clock, as the commands pass it to the services.
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
