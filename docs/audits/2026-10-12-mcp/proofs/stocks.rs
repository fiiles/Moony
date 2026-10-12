// Append this complete sibling module to services/mcp/investments.rs in scratch copy.
#[cfg(test)]
mod audit_securities_proofs {
    use super::*;
    use crate::services::cost_basis::{replayed_quantity, TxRow};
    use rusqlite::Connection;
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(r#"
            CREATE TABLE stock_investments (id TEXT PRIMARY KEY, ticker TEXT UNIQUE NOT NULL, company_name TEXT NOT NULL, quantity TEXT NOT NULL, currency TEXT NOT NULL);
            CREATE TABLE stock_import_batches (id TEXT PRIMARY KEY, file_name TEXT NOT NULL, source TEXT NOT NULL, trade_count INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch()));
            CREATE TABLE investment_transactions (id TEXT PRIMARY KEY, investment_id TEXT NOT NULL, type TEXT NOT NULL, ticker TEXT NOT NULL, company_name TEXT NOT NULL, quantity TEXT NOT NULL, price_per_unit TEXT NOT NULL, currency TEXT NOT NULL, transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL, import_batch_id TEXT, external_id TEXT);
        "#).unwrap();
        conn
    }
    fn row(kind: &str, quantity: &str, price: &str, date: i64) -> StockTransactionRow {
        StockTransactionRow { ticker: "AAPL".into(), company_name: None, tx_type: kind.into(), quantity: quantity.into(), price_per_unit: price.into(), currency: "USD".into(), transaction_date: date }
    }
    fn write(conn: &mut Connection, rows: Vec<StockTransactionRow>) -> BulkWriteReport {
        stock_transactions_create(conn, &StockTransactionsCreateArgs { transactions: rows }).unwrap()
    }
    #[test]
    fn audit_stock_import_accepts_oversell_and_stored_quantity_disagrees_with_replay() {
        let mut conn = db();
        let report = write(&mut conn, vec![row("buy", "1", "100", 1700000000), row("sell", "2", "110", 1700086400), row("buy", "3", "120", 1700172800)]);
        assert_eq!(report.created, 3);
        assert!(report.errors.is_empty());
        let quantity: String = conn.query_row("SELECT quantity FROM stock_investments", [], |r| r.get(0)).unwrap();
        assert_eq!(quantity, "2", "stored quantity is net of the impossible sale");
        let txs = vec![
            TxRow { tx_type: "buy".into(), quantity: 1.0, price_per_unit: 100.0, currency: "USD".into(), transaction_date: 1700000000 },
            TxRow { tx_type: "sell".into(), quantity: 2.0, price_per_unit: 110.0, currency: "USD".into(), transaction_date: 1700086400 },
            TxRow { tx_type: "buy".into(), quantity: 3.0, price_per_unit: 120.0, currency: "USD".into(), transaction_date: 1700172800 },
        ];
        assert_eq!(replayed_quantity(&txs), 3.0, "positions reads clamp each sale and value three units");
    }
    #[test]
    fn audit_stock_mcp_write_creates_no_undo_batch() {
        let mut conn = db();
        assert_eq!(write(&mut conn, vec![row("buy", "1", "100", 1700000000)]).created, 1);
        let batches: i64 = conn.query_row("SELECT COUNT(*) FROM stock_import_batches", [], |r| r.get(0)).unwrap();
        let owned: i64 = conn.query_row("SELECT COUNT(*) FROM investment_transactions WHERE import_batch_id IS NOT NULL", [], |r| r.get(0)).unwrap();
        assert_eq!((batches, owned), (0, 0));
    }
    #[test]
    fn audit_stock_numeric_spelling_bypasses_retry_dedup() {
        let mut conn = db();
        assert_eq!(write(&mut conn, vec![row("buy", "10", "100", 1700000000)]).created, 1);
        let report = write(&mut conn, vec![row("buy", "10.0", "100.00", 1700000000)]);
        assert_eq!(report.created, 1);
        assert!(report.skipped_duplicates.is_empty());
        let quantity: String = conn.query_row("SELECT quantity FROM stock_investments", [], |r| r.get(0)).unwrap();
        assert_eq!(quantity, "20");
    }
    #[test]
    fn audit_stock_currency_mismatch_is_hidden_by_dedup() {
        let mut conn = db();
        assert_eq!(write(&mut conn, vec![row("buy", "10", "100", 1700000000)]).created, 1);
        let mut mismatched = row("buy", "10", "100", 1700000000);
        mismatched.currency = "EUR".into();
        let report = write(&mut conn, vec![mismatched, row("buy", "1", "120", 1700086400)]);
        assert_eq!(report.created, 1, "valid second row commits despite first-row currency error");
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert!(report.errors.is_empty());
    }
    #[test]
    fn audit_stock_nan_quantity_passes_validation_and_persists() {
        let mut conn = db();
        let report = write(&mut conn, vec![row("buy", "NaN", "100", 1700000000)]);
        assert_eq!(report.created, 1);
        assert!(report.errors.is_empty());
        let quantity: String = conn.query_row("SELECT quantity FROM stock_investments", [], |r| r.get(0)).unwrap();
        assert_eq!(quantity, "NaN");
    }
    #[test]
    fn audit_stock_distinct_identical_fills_are_collapsed_design_limitation() {
        let mut conn = db();
        let report = write(&mut conn, vec![row("buy", "1", "100", 1700000000), row("buy", "1", "100", 1700000000)]);
        assert_eq!(report.created, 1);
        assert_eq!(report.skipped_duplicates.len(), 1);
    }
}
