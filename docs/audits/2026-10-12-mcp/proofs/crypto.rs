// Append this complete sibling module to services/mcp/crypto.rs in scratch copy.
#[cfg(test)]
mod audit_securities_proofs {
    use super::*;
    use rusqlite::Connection;
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(r#"
            CREATE TABLE crypto_investments (id TEXT PRIMARY KEY, ticker TEXT UNIQUE NOT NULL, coingecko_id TEXT NOT NULL, name TEXT NOT NULL, quantity TEXT NOT NULL, currency TEXT NOT NULL DEFAULT 'USD');
            CREATE TABLE crypto_transactions (id TEXT PRIMARY KEY, investment_id TEXT NOT NULL, type TEXT NOT NULL, ticker TEXT NOT NULL, name TEXT NOT NULL, quantity TEXT NOT NULL, price_per_unit TEXT NOT NULL, currency TEXT NOT NULL, transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL);
        "#).unwrap();
        conn
    }
    fn row(kind: &str, quantity: &str, price: &str, currency: &str, date: i64, cg: Option<&str>) -> CryptoTransactionRow {
        CryptoTransactionRow { ticker: "BTC".into(), name: None, coingecko_id: cg.map(str::to_owned), tx_type: kind.into(), quantity: quantity.into(), price_per_unit: price.into(), currency: currency.into(), transaction_date: date }
    }
    fn write(conn: &mut Connection, rows: Vec<CryptoTransactionRow>) -> BulkWriteReport {
        crypto_transactions_create(conn, &CryptoTransactionsCreateArgs { transactions: rows }).unwrap()
    }
    #[test]
    fn audit_crypto_import_accepts_oversell() {
        let mut conn = db();
        let report = write(&mut conn, vec![row("buy", "1", "100", "USD", 1700000000, Some("bitcoin")), row("sell", "2", "110", "USD", 1700086400, None), row("buy", "3", "120", "USD", 1700172800, None)]);
        assert_eq!(report.created, 3);
        assert!(report.errors.is_empty());
        let quantity: String = conn.query_row("SELECT quantity FROM crypto_investments", [], |r| r.get(0)).unwrap();
        assert_eq!(quantity, "2");
    }
    #[test]
    fn audit_crypto_numeric_spelling_bypasses_retry_dedup() {
        let mut conn = db();
        assert_eq!(write(&mut conn, vec![row("buy", "10", "100", "USD", 1700000000, Some("bitcoin"))]).created, 1);
        let report = write(&mut conn, vec![row("buy", "10.0", "100.00", "USD", 1700000000, None)]);
        assert_eq!(report.created, 1);
        assert!(report.skipped_duplicates.is_empty());
    }
    #[test]
    fn audit_crypto_nonfinite_fields_pass_validation_and_persist() {
        for (quantity, price) in [("NaN", "100"), ("inf", "100"), ("1", "NaN"), ("1", "inf")] {
            let mut conn = db();
            let report = write(&mut conn, vec![row("buy", quantity, price, "USD", 1700000000, Some("bitcoin"))]);
            assert_eq!(report.created, 1, "quantity={quantity}, price={price}");
            assert!(report.errors.is_empty());
            let stored: (String, String) = conn.query_row("SELECT quantity, price_per_unit FROM crypto_transactions", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            assert_eq!(stored, (quantity.into(), price.into()));
        }
    }
    #[test]
    fn audit_crypto_manual_multicurrency_buy_succeeds_but_mcp_rejects_same_input() {
        let mut conn = db();
        assert_eq!(write(&mut conn, vec![row("buy", "1", "100", "USD", 1700000000, Some("bitcoin"))]).created, 1);
        let id: String = conn.query_row("SELECT id FROM crypto_investments", [], |r| r.get(0)).unwrap();
        let input = crate::models::InsertCryptoTransaction { investment_id: None, tx_type: "buy".into(), ticker: "BTC".into(), name: "Bitcoin".into(), quantity: "2".into(), price_per_unit: "90".into(), currency: "EUR".into(), transaction_date: 1700086400 };
        input.validate().unwrap();
        crypto_service::add_transaction_to_crypto(&conn, &id, &input).unwrap();
        let report = write(&mut conn, vec![row("buy", "3", "95", "EUR", 1700172800, None)]);
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("Currency mismatch"));
    }
    #[test]
    fn audit_crypto_event_coin_id_can_disagree_with_persisted_identity() {
        let mut conn = db();
        let args = CryptoTransactionsCreateArgs { transactions: vec![
            row("buy", "1", "100", "USD", 1700172800, Some("bitcoin")),
            row("buy", "2", "90", "USD", 1700000000, Some("bitcoin-cash")),
        ] };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2);
        assert!(report.errors.is_empty());
        let persisted: String = conn.query_row("SELECT coingecko_id FROM crypto_investments", [], |r| r.get(0)).unwrap();
        assert_eq!(persisted, "bitcoin-cash", "writer processes earliest row first");
        assert_eq!(distinct_tickers(&args), vec![("BTC".into(), Some("bitcoin".into()))], "history event follows original array order");
    }
    #[test]
    fn audit_crypto_existing_identity_is_ignored_when_building_history_event() {
        let mut conn = db();
        assert_eq!(write(&mut conn, vec![row("buy", "1", "100", "USD", 1700000000, Some("bitcoin"))]).created, 1);
        let args = CryptoTransactionsCreateArgs { transactions: vec![row("buy", "2", "90", "USD", 1700086400, Some("bitcoin-cash"))] };
        assert_eq!(crypto_transactions_create(&mut conn, &args).unwrap().created, 1);
        let persisted: String = conn.query_row("SELECT coingecko_id FROM crypto_investments", [], |r| r.get(0)).unwrap();
        assert_eq!(persisted, "bitcoin");
        assert_eq!(distinct_tickers(&args), vec![("BTC".into(), Some("bitcoin-cash".into()))]);
    }
}
