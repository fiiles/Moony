//! Recorded imports and their undo.
//!
//! Every stock import writes one `stock_import_batches` row and stamps its
//! transactions with the batch id (`import_batch_id`, migration 003). Undoing
//! the batch deletes those transactions, deletes the positions left without
//! any transaction (their tags go with them), recalculates the others and
//! deletes the batch row, all in one SQL transaction. A transaction edited
//! after the import still carries the batch id, so it is removed too; one the
//! user deleted is simply gone.

use std::collections::BTreeSet;

use rusqlite::OptionalExtension;

use super::types::{StockImportBatch, StockImportUndoResult};
use crate::error::{AppError, Result};
use crate::services::investments::recalculate_investment_metrics;

/// Recorded imports, newest first (the order they were written in on a tie).
/// `remaining_count` is how many of a batch's transactions still exist.
pub fn list_batches(conn: &rusqlite::Connection) -> Result<Vec<StockImportBatch>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.file_name, b.source, b.trade_count, b.created_at,
                (SELECT COUNT(*) FROM investment_transactions t WHERE t.import_batch_id = b.id)
         FROM stock_import_batches b
         ORDER BY b.created_at DESC, b.rowid DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(StockImportBatch {
            id: row.get(0)?,
            file_name: row.get(1)?,
            source: row.get(2)?,
            trade_count: row.get::<_, i64>(3)?.max(0) as usize,
            remaining_count: row.get::<_, i64>(5)?.max(0) as usize,
            created_at: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Delete a batch's transactions (and positions left without any) atomically.
///
/// Returns every ticker the batch touched and the earliest day it wrote, for
/// the history rebuild, and the tickers of the positions that were deleted.
pub fn undo_batch(
    conn: &mut rusqlite::Connection,
    batch_id: &str,
) -> Result<StockImportUndoResult> {
    let tx = conn.transaction()?;
    let known: Option<String> = tx
        .query_row(
            "SELECT id FROM stock_import_batches WHERE id = ?1",
            [batch_id],
            |row| row.get(0),
        )
        .optional()?;
    if known.is_none() {
        return Err(AppError::NotFound("Import batch not found".into()));
    }

    // (position id, ticker) of every position the batch wrote to, and the
    // earliest day it wrote.
    let (positions, earliest_day) = {
        let mut stmt = tx.prepare(
            "SELECT investment_id, ticker, transaction_date
             FROM investment_transactions WHERE import_batch_id = ?1",
        )?;
        let rows = stmt.query_map([batch_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut positions: BTreeSet<(String, String)> = BTreeSet::new();
        let mut earliest: Option<i64> = None;
        for row in rows {
            let (position, ticker, day) = row?;
            positions.insert((ticker, position));
            earliest = Some(earliest.map_or(day, |e| e.min(day)));
        }
        (positions, earliest)
    };

    let removed = tx.execute(
        "DELETE FROM investment_transactions WHERE import_batch_id = ?1",
        [batch_id],
    )?;

    let mut tickers: Vec<String> = Vec::new();
    let mut removed_positions: Vec<String> = Vec::new();
    for (ticker, position) in positions {
        let remaining: i64 = tx.query_row(
            "SELECT COUNT(*) FROM investment_transactions WHERE investment_id = ?1",
            [&position],
            |row| row.get(0),
        )?;
        if remaining == 0 {
            tx.execute("DELETE FROM stock_investments WHERE id = ?1", [&position])?;
            removed_positions.push(ticker.clone());
        } else {
            recalculate_investment_metrics(&tx, &position)?;
        }
        tickers.push(ticker);
    }

    tx.execute("DELETE FROM stock_import_batches WHERE id = ?1", [batch_id])?;
    tx.commit()?;

    Ok(StockImportUndoResult {
        removed,
        removed_positions,
        tickers,
        earliest_day,
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::import::import;
    use super::super::simulate::test_db::*;
    use super::super::types::{ParsedTrade, StockImportResult};
    use super::*;

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn quantity(conn: &Connection, ticker: &str) -> Option<String> {
        conn.query_row(
            "SELECT quantity FROM stock_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .ok()
    }

    fn run_import(
        conn: &mut Connection,
        trades: Vec<ParsedTrade>,
        file: &str,
    ) -> StockImportResult {
        import(conn, &parsed(trades), &config("custom"), file).expect("import")
    }

    fn insert_batch(conn: &Connection, id: &str, created_at: i64, trade_count: i64) {
        conn.execute(
            "INSERT INTO stock_import_batches (id, file_name, source, trade_count, created_at)
             VALUES (?1, ?2, 'xtb', ?3, ?4)",
            rusqlite::params![id, format!("{id}.csv"), trade_count, created_at],
        )
        .unwrap();
    }

    fn put_in_batch(conn: &Connection, batch: &str, kind: &str, qty: &str, ticker: &str, day: i64) {
        let id = add_stored(conn, ticker, kind, qty, "10", day, None);
        conn.execute(
            "UPDATE investment_transactions SET import_batch_id = ?1 WHERE id = ?2",
            [batch, &id],
        )
        .unwrap();
    }

    // ---- list_batches --------------------------------------------------------

    #[test]
    fn no_imports_no_batches() {
        let conn = db();
        assert!(list_batches(&conn).unwrap().is_empty());
    }

    #[test]
    fn batches_are_listed_newest_first_with_what_is_still_there() {
        let conn = db();
        insert_batch(&conn, "old", 1_700_000_000, 3);
        insert_batch(&conn, "new", 1_700_100_000, 1);
        insert_batch(&conn, "tie-a", 1_700_050_000, 2);
        insert_batch(&conn, "tie-b", 1_700_050_000, 2); // same second: inserted later, listed first
        put_in_batch(&conn, "old", "buy", "1", "AAPL", day(0));
        put_in_batch(&conn, "old", "buy", "2", "AAPL", day(1)); // the third was deleted since
        put_in_batch(&conn, "new", "buy", "1", "MSFT", day(2));

        let batches = list_batches(&conn).unwrap();

        let ids: Vec<&str> = batches.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["new", "tie-b", "tie-a", "old"]);
        let old = batches.iter().find(|b| b.id == "old").unwrap();
        assert_eq!(old.file_name, "old.csv");
        assert_eq!(old.source, "xtb");
        assert_eq!(old.trade_count, 3);
        assert_eq!(old.remaining_count, 2);
        assert_eq!(old.created_at, 1_700_000_000);
        assert_eq!(
            batches
                .iter()
                .find(|b| b.id == "new")
                .unwrap()
                .remaining_count,
            1
        );
        assert_eq!(
            batches
                .iter()
                .find(|b| b.id == "tie-a")
                .unwrap()
                .remaining_count,
            0
        );
    }

    #[test]
    fn a_real_import_shows_up_as_a_batch() {
        let mut conn = db();
        let result = run_import(
            &mut conn,
            vec![
                buy(2, day(0), "AAPL", 1.0, 1.0),
                buy(3, day(1), "AAPL", 1.0, 2.0),
            ],
            "my file.csv",
        );
        let batches = list_batches(&conn).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(Some(batches[0].id.clone()), result.batch_id);
        assert_eq!(batches[0].file_name, "my file.csv");
        assert_eq!(batches[0].source, "custom");
        assert_eq!((batches[0].trade_count, batches[0].remaining_count), (2, 2));
    }

    // ---- undo_batch ----------------------------------------------------------

    #[test]
    fn undo_removes_the_batch_and_the_positions_it_created() {
        let mut conn = db();
        let result = run_import(
            &mut conn,
            vec![
                buy(2, day(1), "AAPL", 10.0, 100.0),
                sell(3, day(3), "AAPL", 4.0, 110.0),
                buy(4, day(2), "MSFT", 2.0, 300.0),
            ],
            "f.csv",
        );
        let batch_id = result.batch_id.unwrap();
        conn.execute(
            "INSERT INTO stock_investment_tags (investment_id, tag_id)
             SELECT id, 'tag-1' FROM stock_investments WHERE ticker = 'AAPL'",
            [],
        )
        .unwrap();
        assert_eq!(count(&conn, "stock_investment_tags"), 1);
        assert_eq!(quantity(&conn, "AAPL").as_deref(), Some("6"));

        let undone = undo_batch(&mut conn, &batch_id).unwrap();

        assert_eq!(undone.removed, 3);
        assert_eq!(undone.removed_positions, vec!["AAPL", "MSFT"]);
        assert_eq!(undone.tickers, vec!["AAPL", "MSFT"]);
        assert_eq!(undone.earliest_day, Some(day(1)));
        assert_eq!(count(&conn, "investment_transactions"), 0);
        assert_eq!(count(&conn, "stock_investments"), 0);
        assert_eq!(
            count(&conn, "stock_investment_tags"),
            0,
            "tags go with the position"
        );
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn undo_keeps_positions_that_have_other_transactions_and_recalculates_them() {
        let mut conn = db();
        // MSFT existed before the import: 5 held.
        add_stored(&conn, "MSFT", "buy", "5", "300", day(-10), None);
        let result = run_import(
            &mut conn,
            vec![
                buy(2, day(0), "MSFT", 3.0, 310.0),
                buy(3, day(0), "AAPL", 1.0, 100.0),
            ],
            "f.csv",
        );
        // The user added a hand-made buy to the position the import created.
        add_stored(&conn, "AAPL", "buy", "2", "105", day(1), None);

        let undone = undo_batch(&mut conn, &result.batch_id.unwrap()).unwrap();

        assert_eq!(undone.removed, 2);
        assert!(undone.removed_positions.is_empty());
        assert_eq!(undone.tickers, vec!["AAPL", "MSFT"]);
        assert_eq!(
            quantity(&conn, "MSFT").as_deref(),
            Some("5"),
            "back to what it was"
        );
        assert_eq!(
            quantity(&conn, "AAPL").as_deref(),
            Some("2"),
            "only the hand-made buy is left"
        );
        assert_eq!(count(&conn, "investment_transactions"), 2);
    }

    #[test]
    fn a_transaction_edited_after_the_import_still_belongs_to_the_batch() {
        let mut conn = db();
        let result = run_import(
            &mut conn,
            vec![buy(2, day(0), "AAPL", 10.0, 100.0)],
            "f.csv",
        );
        // What update_investment_transaction does: it never touches the batch column.
        conn.execute(
            "UPDATE investment_transactions SET price_per_unit = '99.9', quantity = '11'",
            [],
        )
        .unwrap();

        let undone = undo_batch(&mut conn, &result.batch_id.unwrap()).unwrap();

        assert_eq!(undone.removed, 1);
        assert_eq!(count(&conn, "investment_transactions"), 0);
    }

    #[test]
    fn undo_touches_only_its_own_batch() {
        let mut conn = db();
        let first = run_import(
            &mut conn,
            vec![buy(2, day(0), "AAPL", 10.0, 100.0)],
            "a.csv",
        );
        let second = run_import(&mut conn, vec![buy(2, day(1), "AAPL", 5.0, 110.0)], "b.csv");

        undo_batch(&mut conn, &first.batch_id.unwrap()).unwrap();

        assert_eq!(count(&conn, "stock_import_batches"), 1);
        let remaining: Vec<String> = list_batches(&conn)
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(remaining, vec![second.batch_id.unwrap()]);
        assert_eq!(quantity(&conn, "AAPL").as_deref(), Some("5"));
        assert_eq!(count(&conn, "investment_transactions"), 1);
    }

    #[test]
    fn undoing_a_batch_whose_transactions_are_gone_just_removes_the_record() {
        let mut conn = db();
        insert_batch(&conn, "empty", 1_700_000_000, 2);

        let undone = undo_batch(&mut conn, "empty").unwrap();

        assert_eq!(undone.removed, 0);
        assert!(undone.removed_positions.is_empty() && undone.tickers.is_empty());
        assert_eq!(undone.earliest_day, None);
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn an_unknown_batch_is_not_found() {
        let mut conn = db();
        let err = undo_batch(&mut conn, "nope").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn a_failing_undo_changes_nothing() {
        let mut conn = db();
        let result = run_import(
            &mut conn,
            vec![buy(2, day(0), "AAPL", 10.0, 100.0)],
            "f.csv",
        );
        conn.execute_batch(
            "CREATE TRIGGER keep_positions BEFORE DELETE ON stock_investments
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();

        let err = undo_batch(&mut conn, &result.batch_id.unwrap()).unwrap_err();

        assert!(err.to_string().contains("refused"), "{err}");
        assert_eq!(count(&conn, "investment_transactions"), 1, "rolled back");
        assert_eq!(count(&conn, "stock_import_batches"), 1);
        assert_eq!(quantity(&conn, "AAPL").as_deref(), Some("10"));
    }
}
