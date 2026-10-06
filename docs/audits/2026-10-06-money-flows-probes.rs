//! Characterization probes for the pre-release audit. These assert CURRENT
//! behavior, including defects; passing does not mean that behavior is correct.
//! Temporarily include from services/mod.rs with #[cfg(test)] and
//! #[path = "../../../docs/audits/2026-10-06-money-flows-probes.rs"].
use crate::services::categorization::{CategorizationEngine, TransactionInput};
use crate::services::{budgeting, cashflow_actuals, csv_import};
use csv_import::test_support::setup_import_db;
use csv_import::CsvImportConfig;
use rusqlite::Connection;

fn config() -> CsvImportConfig {
    CsvImportConfig {
        delimiter: ",".into(),
        date_column: "Date".into(),
        date_format: "%Y-%m-%d".into(),
        amount_column: Some("Amount".into()),
        description_columns: Some(vec!["Description".into()]),
        currency_column: Some("Currency".into()),
        counterparty_column: Some("Payee".into()),
        decimal_separator: Some(".".into()),
        ..Default::default()
    }
}

fn report_db() -> Connection {
    let conn = setup_import_db();
    conn.execute_batch("ALTER TABLE transaction_categories ADD COLUMN icon TEXT;
        ALTER TABLE transaction_categories ADD COLUMN color TEXT;
        INSERT INTO transaction_categories (id,name) VALUES ('cat_income','Income'), ('cat_dining','Dining'), ('custom_income','Salary');
        CREATE TABLE budget_goals (id TEXT PRIMARY KEY, category_id TEXT, timeframe TEXT, amount TEXT, currency TEXT, created_at INTEGER, updated_at INTEGER);
        CREATE TABLE exchange_rate_history (date INTEGER, currency TEXT, rate REAL);").unwrap();
    conn
}

fn row(conn: &Connection, id: &str, amount: &str, direction: &str, category: &str, status: &str) {
    conn.execute("INSERT INTO bank_transactions (id,bank_account_id,booking_date,amount,currency,tx_type,category_id,status)
        VALUES (?1,'acc-1',1791244800,?2,'CZK',?3,?4,?5)", rusqlite::params![id,amount,direction,category,status]).unwrap();
}

#[test]
fn audit_delete_only_removes_rows_owned_by_that_batch() {
    let mut conn = setup_import_db();
    let csv = "Date,Amount,Description,Currency,Payee\n2026-10-06,-100,Coffee,CZK,Shop\n";
    let first =
        csv_import::import_transactions(&mut conn, "acc-1", "first.csv", csv, &config(), None)
            .unwrap();
    let second =
        csv_import::import_transactions(&mut conn, "acc-1", "second.csv", csv, &config(), None)
            .unwrap();
    assert_eq!(
        (
            first.imported_count,
            second.imported_count,
            second.skipped_duplicates.len()
        ),
        (1, 0, 1)
    );
    let batch: String = conn
        .query_row(
            "SELECT id FROM csv_import_batches WHERE file_name='second.csv'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "DELETE FROM bank_transactions WHERE import_batch_id=?1",
        [&batch],
    )
    .unwrap();
    conn.execute("DELETE FROM csv_import_batches WHERE id=?1", [&batch])
        .unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM bank_transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let batch: String = conn
        .query_row(
            "SELECT id FROM csv_import_batches WHERE file_name='first.csv'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "DELETE FROM bank_transactions WHERE import_batch_id=?1",
        [&batch],
    )
    .unwrap();
    conn.execute("DELETE FROM csv_import_batches WHERE id=?1", [&batch])
        .unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM bank_transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn audit_dedup_discards_different_currency_and_different_counterparty() {
    for csv in [
        "Date,Amount,Description,Currency,Payee\n2026-10-06,-100,Payment,EUR,Shop\n2026-10-06,-100,Payment,USD,Shop\n",
        "Date,Amount,Description,Currency,Payee\n2026-10-06,-100,Payment,CZK,Shop A\n2026-10-06,-100,Payment,CZK,Shop B\n",
    ] {
        let mut conn = setup_import_db();
        let result = csv_import::import_transactions(&mut conn,"acc-1","file.csv",csv,&config(),None).unwrap();
        assert_eq!((result.imported_count,result.skipped_duplicates.len()),(1,1));
    }
}

#[test]
fn audit_refund_in_other_category_disagrees_between_reports() {
    let conn = report_db();
    row(&conn, "expense", "1000", "debit", "cat_dining", "booked");
    row(&conn, "refund", "500", "credit", "cat-1", "booked");
    let bounds = cashflow_actuals::range_bounds("ytd", 1791244800).unwrap();
    let budget = budgeting::get_report(&conn, bounds.start, bounds.end, "yearly").unwrap();
    let actual = cashflow_actuals::get_actuals(&conn, "ytd", 1791244800).unwrap();
    assert_eq!(budget.total_expenses, "1000.00");
    assert_eq!(actual.total_expenses, "500.00");
}

#[test]
fn audit_income_debit_is_ignored_and_custom_income_is_not_income() {
    let conn = report_db();
    row(&conn, "salary", "1000", "credit", "cat_income", "booked");
    row(
        &conn,
        "salary-reversal",
        "100",
        "debit",
        "cat_income",
        "booked",
    );
    row(
        &conn,
        "custom-salary",
        "2000",
        "credit",
        "custom_income",
        "booked",
    );
    let actual = cashflow_actuals::get_actuals(&conn, "ytd", 1791244800).unwrap();
    assert_eq!(actual.total_income, "1000.00");
    assert_eq!(
        actual
            .expense_categories
            .iter()
            .find(|c| c.key == "custom_income")
            .unwrap()
            .amount,
        "-2000.00"
    );
}

#[test]
fn audit_pending_transactions_are_included_in_actuals_and_budgets() {
    let conn = report_db();
    row(&conn, "booked", "100", "debit", "cat_dining", "booked");
    row(&conn, "pending", "100", "debit", "cat_dining", "pending");
    let actual = cashflow_actuals::get_actuals(&conn, "ytd", 1791244800).unwrap();
    let budget =
        budgeting::get_report(&conn, actual.range_start, actual.range_end, "yearly").unwrap();
    assert_eq!(actual.total_expenses, "200.00");
    assert_eq!(budget.total_expenses, "200.00");
}

#[test]
fn audit_learned_payee_overrides_own_iban_and_domestic_number_does_not_match() {
    let engine = CategorizationEngine::new_empty();
    engine.set_own_ibans(vec!["CZ6508000000192000145399".into()]);
    let mut tx = TransactionInput {
        id: "tx".into(),
        description: Some("Transfer".into()),
        counterparty: Some("My Name".into()),
        counterparty_iban: Some("CZ6508000000192000145399".into()),
        amount: 100.0,
        is_credit: false,
        bank_account_id: Some("acc-1".into()),
    };
    assert_eq!(
        engine.categorize(&tx).category_id(),
        Some("cat_internal_transfers")
    );
    engine.learn_from_user_simple("My Name", "cat_dining");
    assert_eq!(engine.categorize(&tx).category_id(), Some("cat_dining"));
    engine.forget_payee_simple("My Name");
    tx.counterparty_iban = Some("19-2000145399/0800".into());
    assert_eq!(engine.categorize(&tx).category_id(), None);
}

#[test]
fn audit_balance_has_no_currency_or_as_of_date_in_result() {
    let mut conn = setup_import_db();
    let mut cfg = config();
    cfg.balance_column = Some("Balance".into());
    let result = csv_import::import_transactions(
        &mut conn,
        "acc-1",
        "old.csv",
        "Date,Amount,Description,Currency,Payee,Balance\n2024-01-01,10,Payment,EUR,Shop,100\n",
        &cfg,
        None,
    )
    .unwrap();
    assert_eq!(result.last_balance, Some("100".into()));
    let currency: String = conn
        .query_row(
            "SELECT currency FROM bank_accounts WHERE id='acc-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(currency, "CZK");
}

#[test]
fn audit_finite_amount_validation_is_missing_at_wire_boundary() {
    let mut data = crate::models::InsertBankTransaction {
        bank_account_id: "acc-1".into(),
        transaction_id: None,
        tx_type: "debit".into(),
        amount: "NaN".into(),
        currency: Some("CZK".into()),
        description: None,
        counterparty_name: None,
        counterparty_iban: None,
        booking_date: 1791244800,
        value_date: None,
        category_id: None,
        status: None,
    };
    assert!(data.validate().is_ok());
    data.amount = "inf".into();
    assert!(data.validate().is_ok());
}
