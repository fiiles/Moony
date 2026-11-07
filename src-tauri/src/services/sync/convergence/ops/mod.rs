//! Random operations for the convergence harness. Every operation goes through the real service
//! (and the `validate()` its command calls first), never raw SQL, on entities the device itself
//! holds, picked with the seeded rng. Names come from small pools, so devices collide on unique
//! keys and rename scopes. An operation the service rejects is skipped: that is part of the test.

mod assets;
mod bank;
mod planning;
mod stocks;

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use crate::error::Result;

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

type OpFn = fn(&Connection, &mut StdRng) -> (String, Outcome);

/// Every operation, by name.
fn catalog() -> Vec<(&'static str, OpFn)> {
    [bank::OPS, stocks::OPS, assets::OPS, planning::OPS].concat()
}

/// The names of every operation `random_op` may pick.
pub fn names() -> Vec<&'static str> {
    catalog().into_iter().map(|(name, _)| name).collect()
}

/// One random operation on `conn`.
pub fn random_op(conn: &Connection, rng: &mut StdRng) -> Step {
    let ops = catalog();
    let (name, op) = ops[rng.random_range(0..ops.len())];
    let (detail, outcome) = op(conn, rng);
    Step {
        name,
        detail,
        outcome,
    }
}

/// The operation `name`, on `conn`.
pub fn run_op(conn: &Connection, rng: &mut StdRng, name: &str) -> Step {
    let (name, op) = catalog()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no operation {name}"));
    let (detail, outcome) = op(conn, rng);
    Step {
        name,
        detail,
        outcome,
    }
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

fn run(detail: String, work: impl FnOnce() -> Result<()>) -> (String, Outcome) {
    let outcome = match work() {
        Ok(()) => Outcome::Done,
        Err(e) => Outcome::Rejected(e.to_string()),
    };
    (detail, outcome)
}

fn no_target() -> (String, Outcome) {
    (String::new(), Outcome::NoTarget)
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

/// The first column of every row `sql` selects, in its order.
fn ids(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect(sql);
    stmt.query_map([], |r| r.get(0))
        .expect(sql)
        .map(|r| r.expect("id"))
        .collect()
}

/// One row of `sql` (its first column), or `None` when it selects nothing. `sql` orders its rows
/// by `rowid` (the order this device created or merged them): ids are random UUIDs, so an order
/// by id would make one seed pick different rows on every run.
fn pick(conn: &Connection, rng: &mut StdRng, sql: &str) -> Option<String> {
    let ids = ids(conn, sql);
    (!ids.is_empty()).then(|| ids[rng.random_range(0..ids.len())].clone())
}

/// One row of `sql` as two text columns.
fn pick2(conn: &Connection, rng: &mut StdRng, sql: &str) -> Option<(String, String)> {
    let mut stmt = conn.prepare(sql).expect(sql);
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect(sql)
        .map(|r| r.expect("row"))
        .collect();
    (!rows.is_empty()).then(|| rows[rng.random_range(0..rows.len())].clone())
}

/// The wall clock, as the commands pass it to the services.
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
