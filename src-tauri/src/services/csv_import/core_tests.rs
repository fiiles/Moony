use super::test_support::setup_import_db;
use super::*;
use crate::models::InsertBankTransaction;

const DAY: i64 = 86_400;

fn db() -> rusqlite::Connection {
    let conn = setup_import_db();
    conn.execute_batch(
        "INSERT INTO bank_accounts (id, name, currency) VALUES ('acc', 'Main', 'CZK');
         INSERT INTO transaction_categories (id, name) VALUES ('cat_food', 'Groceries'), ('cat_fun', 'Fun');",
    )
    .unwrap();
    conn
}

fn candidate(
    line: usize,
    day: i64,
    amount: &str,
    credit: bool,
    source_category: Option<&str>,
) -> ParsedRow {
    let insert = InsertBankTransaction {
        bank_account_id: "acc".into(),
        transaction_id: None,
        tx_type: if credit { "credit" } else { "debit" }.into(),
        flow_kind: None,
        amount: amount.into(),
        currency: Some("CZK".into()),
        description: Some(format!("row {line}")),
        counterparty_name: None,
        counterparty_iban: None,
        counterparty_bank_code: None,
        counterparty_country: None,
        booking_date: day * DAY,
        value_date: None,
        category_id: None,
        status: Some("booked".into()),
    };
    ParsedRow {
        line,
        booking_date: Some(day * DAY),
        signed_amount: Some(if credit {
            amount.to_string()
        } else {
            format!("-{amount}")
        }),
        currency: Some("CZK".into()),
        description: insert.description.clone(),
        counterparty: None,
        balance: None,
        source_category: source_category.map(Into::into),
        state: RowState::Candidate(Box::new(insert)),
    }
}

fn parsed(rows: Vec<ParsedRow>) -> ParsedImport {
    ParsedImport {
        total_rows: rows.len(),
        rows,
        period: None,
        declared_balance: None,
    }
}

fn category_of(conn: &rusqlite::Connection, line: i64) -> (Option<String>, Option<String>) {
    conn.query_row(
        "SELECT category_id, categorization_source FROM bank_transactions WHERE import_line = ?1",
        [line],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn source_category_map_categorizes_as_manual() {
    let mut conn = db();
    let rows = parsed(vec![candidate(
        1,
        20_000,
        "12.5",
        false,
        Some("Food:Groceries"),
    )]);
    let options = ImportOptions {
        source_category_map: vec![SourceCategoryMapping {
            source: "Food:Groceries".into(),
            category_id: "cat_food".into(),
        }],
        ..ImportOptions::default()
    };
    import_rows(&mut conn, "acc", "x.qif", &rows, &options, None).unwrap();
    assert_eq!(
        category_of(&conn, 1),
        (Some("cat_food".into()), Some("manual".into()))
    );
}

#[test]
fn line_override_beats_source_category_map() {
    let mut conn = db();
    let rows = parsed(vec![candidate(
        1,
        20_000,
        "12.5",
        false,
        Some("Food:Groceries"),
    )]);
    let options = ImportOptions {
        source_category_map: vec![SourceCategoryMapping {
            source: "Food:Groceries".into(),
            category_id: "cat_food".into(),
        }],
        category_overrides: vec![CsvCategoryOverride {
            line: 1,
            category_id: "cat_fun".into(),
        }],
        ..ImportOptions::default()
    };
    import_rows(&mut conn, "acc", "x.qif", &rows, &options, None).unwrap();
    assert_eq!(category_of(&conn, 1).0, Some("cat_fun".into()));
}

#[test]
fn mapping_to_a_deleted_category_falls_through() {
    let mut conn = db();
    let rows = parsed(vec![candidate(1, 20_000, "12.5", false, Some("Food"))]);
    let options = ImportOptions {
        source_category_map: vec![SourceCategoryMapping {
            source: "Food".into(),
            category_id: "gone".into(),
        }],
        ..ImportOptions::default()
    };
    import_rows(&mut conn, "acc", "x.qif", &rows, &options, None).unwrap();
    assert_eq!(category_of(&conn, 1), (None, None));
}

#[test]
fn declared_balance_and_period_win_over_rows() {
    let mut conn = db();
    let mut rows = parsed(vec![
        candidate(1, 20_005, "500.5", true, None),
        candidate(2, 20_020, "250", false, None),
    ]);
    rows.period = Some(CsvDateRange {
        from: 20_001 * DAY,
        to: 20_030 * DAY,
    });
    rows.declared_balance = Some(DeclaredBalance {
        amount: "1250.5".into(),
        currency: "CZK".into(),
        as_of: 20_030 * DAY,
    });
    let result = import_rows(
        &mut conn,
        "acc",
        "s.xml",
        &rows,
        &ImportOptions::default(),
        None,
    )
    .unwrap();
    let balance = result.statement_balance.expect("declared balance");
    assert_eq!(
        (balance.amount.as_str(), balance.as_of, balance.line),
        ("1250.5", 20_030 * DAY, 2)
    );
    assert_eq!(result.last_balance.as_deref(), Some("1250.5"));
    let (from, to): (i64, i64) = conn
        .query_row(
            "SELECT date_from, date_to FROM csv_import_batches",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((from, to), (20_001 * DAY, 20_030 * DAY));
}

#[test]
fn preview_rows_shows_the_source_category() {
    let conn = db();
    let rows = parsed(vec![candidate(1, 20_000, "1", false, Some("Food"))]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    assert_eq!(preview.rows[0].source_category.as_deref(), Some("Food"));
}

fn earlier_batch(conn: &rusqlite::Connection, from_day: i64, to_day: i64) {
    conn.execute(
        "INSERT INTO csv_import_batches (id, bank_account_id, file_name, date_from, date_to) VALUES ('b1', 'acc', 'june.csv', ?1, ?2)",
        [from_day * DAY, to_day * DAY],
    )
    .unwrap();
}

#[test]
fn overlap_counts_new_rows_inside_an_earlier_batch_excluding_its_last_day() {
    let conn = db();
    earlier_batch(&conn, 20_010, 20_020);
    let rows = parsed(vec![
        candidate(1, 20_012, "1", false, None), // inside
        candidate(2, 20_020, "2", false, None), // the batch's last day: not counted
        candidate(3, 20_025, "3", false, None), // after
    ]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    let overlap = preview.overlap.expect("overlap");
    assert_eq!(overlap.new_rows_in_covered_range, 1);
    assert_eq!(overlap.batches[0].file_name, "june.csv");
}

#[test]
fn no_overlap_without_new_rows_in_range() {
    let conn = db();
    earlier_batch(&conn, 20_010, 20_020);
    let rows = parsed(vec![candidate(1, 20_030, "1", false, None)]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    assert_eq!(preview.overlap, None);
}

#[test]
fn csv_preview_reports_overlap_too() {
    let conn = db();
    // 2026-06-12 = day 20_616
    earlier_batch(&conn, 20_610, 20_620);
    let config = CsvImportConfig {
        date_column: "Date".into(),
        date_format: "%Y-%m-%d".into(),
        amount_column: Some("Amount".into()),
        description_columns: Some(vec!["Description".into()]),
        ..CsvImportConfig::default()
    };
    let preview = preview_import(
        &conn,
        "acc",
        "Date,Amount,Description\n2026-06-12,-10,Coffee\n",
        &config,
        None,
    )
    .unwrap();
    assert_eq!(
        preview.overlap.map(|o| o.new_rows_in_covered_range),
        Some(1)
    );
}

fn batch_at(
    conn: &rusqlite::Connection,
    (id, file_name, imported_at): (&str, &str, i64),
    (from_day, to_day): (i64, i64),
) {
    conn.execute(
        "INSERT INTO csv_import_batches (id, bank_account_id, file_name, date_from, date_to, imported_at) VALUES (?1, 'acc', ?2, ?3, ?4, ?5)",
        rusqlite::params![id, file_name, from_day * DAY, to_day * DAY, imported_at],
    )
    .unwrap();
}

#[test]
fn overlap_covers_the_whole_file_not_only_the_preview_page() {
    let conn = db();
    earlier_batch(&conn, 20_010, 20_020);
    let rows = parsed(vec![
        candidate(1, 20_030, "1", false, None), // outside, and the only row on the page
        candidate(2, 20_012, "2", false, None),
        candidate(3, 20_013, "3", false, None),
    ]);
    let options = ImportOptions {
        preview_limit: 1,
        ..ImportOptions::default()
    };
    let preview = preview_rows(&conn, "acc", &rows, &options, None).unwrap();
    assert_eq!(preview.rows.len(), 1);
    assert_eq!(
        preview.overlap.map(|o| o.new_rows_in_covered_range),
        Some(2)
    );
}

#[test]
fn overlap_lists_up_to_three_covering_batches_newest_first() {
    let conn = db();
    batch_at(&conn, ("a", "oldest.csv", 100), (20_000, 20_030));
    batch_at(&conn, ("b", "second.csv", 200), (20_000, 20_030));
    batch_at(&conn, ("c", "third.csv", 300), (20_000, 20_030));
    batch_at(&conn, ("d", "newest.csv", 400), (20_000, 20_030));
    batch_at(&conn, ("e", "elsewhere.csv", 500), (20_100, 20_130)); // covers no new row
    let rows = parsed(vec![candidate(1, 20_012, "1", false, None)]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    let overlap = preview.overlap.expect("overlap");
    let names: Vec<&str> = overlap
        .batches
        .iter()
        .map(|b| b.file_name.as_str())
        .collect();
    assert_eq!(names, ["newest.csv", "third.csv", "second.csv"]);
    assert_eq!(overlap.new_rows_in_covered_range, 1);
}

#[test]
fn rows_that_are_duplicates_do_not_count_as_overlap() {
    let conn = db();
    earlier_batch(&conn, 20_010, 20_020);
    conn.execute(
        "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, description, booking_date, status)
         VALUES ('t1', 'acc', 'debit', '1', 'CZK', 'row 1', ?1, 'booked')",
        [20_012 * DAY],
    )
    .unwrap();
    let rows = parsed(vec![candidate(1, 20_012, "1", false, None)]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    assert_eq!(preview.duplicate_count, 1);
    assert_eq!(preview.overlap, None);
}
