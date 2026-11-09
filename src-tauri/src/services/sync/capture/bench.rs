//! What change capture costs a write (spec §18, R19; plan's call P20): the developer tools'
//! measurement behind `sync_debug_capture_overhead`. The same inserts run on two fresh migrated
//! in-memory profiles, the second with capture on; nothing touches the open profile. The
//! statement-import measurement of the convergence harness stays a test.

use std::time::{Duration, Instant};

use rusqlite::{params, Connection};

use super::super::hlc::DeviceId;
use super::super::state;
use crate::error::Result;

/// The time `rows` inserts took without capture and with it, and how many rows capture queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overhead {
    pub without: Duration,
    pub with: Duration,
    /// Rows the captured run added to `sync_outbox`: the account and every transaction.
    pub queued: u64,
}

/// One bank account and `rows` bank transactions, in one transaction, on a fresh profile without
/// capture and on one with capture on.
pub fn insert_overhead(rows: usize) -> Result<Overhead> {
    let plain = migrated()?;
    let without = timed_inserts(&plain, rows)?;

    let captured = migrated()?;
    state::enable(&captured, DeviceId::random())?;
    let before = outbox_rows(&captured)?;
    let with = timed_inserts(&captured, rows)?;
    let queued = outbox_rows(&captured)? - before;

    Ok(Overhead {
        without,
        with,
        queued,
    })
}

fn migrated() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    crate::db::run_migrations(&conn)?;
    Ok(conn)
}

/// The account and the transactions, with fixed values, timed from `BEGIN` to `COMMIT`.
fn timed_inserts(conn: &Connection, rows: usize) -> Result<Duration> {
    let started = Instant::now();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO bank_accounts (id, name, currency, balance) \
           VALUES ('bench-account', 'Bench', 'CZK', '0')",
        [],
    )?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO bank_transactions \
               (id, bank_account_id, tx_type, amount, currency, booking_date, description) \
             VALUES (?1, 'bench-account', 'debit', '-125.50', 'CZK', ?2, 'Bench payment')",
        )?;
        for i in 0..rows {
            insert.execute(params![
                format!("bench-tx-{i:06}"),
                1_767_225_600 + i as i64
            ])?;
        }
    }
    tx.commit()?;
    Ok(started.elapsed())
}

fn outbox_rows(conn: &Connection) -> Result<u64> {
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))?;
    Ok(u64::try_from(rows).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_overhead_measures_both_runs() {
        let o = insert_overhead(50).unwrap();
        assert!(o.without > Duration::ZERO && o.with > Duration::ZERO);
        // 50 transactions and the account.
        assert_eq!(o.queued, 51);
    }
}
