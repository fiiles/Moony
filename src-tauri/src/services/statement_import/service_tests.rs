use super::*;
use crate::services::statement_import::test_support::setup_statement_db;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/statements/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn db() -> rusqlite::Connection {
    let conn = setup_statement_db();
    conn.execute_batch(
        "INSERT INTO bank_accounts (id, name, currency, iban) VALUES
           ('acc', 'Main', 'CZK', 'CZ6508000000192000145399'),
           ('other', 'Savings', 'CZK', 'CZ5508000000001234567899');",
    )
    .unwrap();
    conn
}

fn options() -> StatementImportOptions {
    StatementImportOptions {
        statement_index: 0,
        date_order: None,
        skip_duplicates: true,
        import_anyway_lines: vec![],
        category_overrides: vec![],
        source_category_map: vec![],
        preview_offset: 0,
        preview_limit: 200,
    }
}

#[test]
fn csv_is_not_a_statement() {
    let conn = db();
    assert!(inspect(&conn, "acc", b"Date,Amount\n2026-01-01,5\n", None)
        .unwrap()
        .is_none());
}

#[test]
fn inspect_camt_summary() {
    let conn = db();
    let insp = inspect(&conn, "acc", &fixture("camt053/basic.xml"), None)
        .unwrap()
        .unwrap();
    assert_eq!(insp.format, StatementFormat::Camt053);
    let s = &insp.statements[0];
    assert_eq!(
        (s.entry_count, s.balance_check, s.account_match),
        (2, BalanceCheck::AddsUp, AccountMatch::Selected)
    );
    let other = inspect(&conn, "other", &fixture("camt053/basic.xml"), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            other.statements[0].account_match,
            other.statements[0].matched_account_id.as_deref()
        ),
        (AccountMatch::Other, Some("acc"))
    );
}

#[test]
fn import_then_reimport_is_idempotent() {
    let mut conn = db();
    let first = import(
        &mut conn,
        "acc",
        "basic.xml",
        &fixture("camt053/basic.xml"),
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!(first.imported_count, 2);
    let balance = first.statement_balance.unwrap();
    assert_eq!((balance.amount.as_str(), balance.line), ("1250.5", 2));
    let again = import(
        &mut conn,
        "acc",
        "basic.xml",
        &fixture("camt053/basic.xml"),
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!(
        (again.imported_count, again.skipped_duplicates.len()),
        (0, 2)
    );
}

#[test]
fn pending_is_promoted_by_its_booked_successor() {
    let mut conn = db();
    let text = String::from_utf8(fixture("camt053/batch-and-pending.xml")).unwrap();
    import(
        &mut conn,
        "acc",
        "a.xml",
        text.as_bytes(),
        None,
        &options(),
        None,
    )
    .unwrap();
    let booked = text.replacen("<Cd>PDNG</Cd>", "<Cd>BOOK</Cd>", 1);
    let result = import(
        &mut conn,
        "acc",
        "b.xml",
        booked.as_bytes(),
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!(result.promoted_count, 1);
}

#[test]
fn empty_statement_previews_without_error() {
    // Review Focus 4
    let conn = db();
    let text = String::from_utf8(fixture("camt053/basic.xml")).unwrap();
    let start = text.find("<Ntry>").unwrap();
    let end = text.rfind("</Ntry>").unwrap() + "</Ntry>".len();
    let quiet = format!("{}{}", &text[..start], &text[end..]).replace("1250.50", "1000.00");
    let insp = inspect(&conn, "acc", quiet.as_bytes(), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            insp.statements[0].entry_count,
            insp.statements[0].balance_check
        ),
        (0, BalanceCheck::AddsUp)
    );
    let preview = preview(&conn, "acc", quiet.as_bytes(), None, &options(), None).unwrap();
    assert_eq!((preview.total_rows, preview.ok_count), (0, 0));
}

#[test]
fn options_are_validated() {
    let conn = db();
    let bad = StatementImportOptions {
        statement_index: 3,
        ..options()
    };
    assert!(preview(
        &conn,
        "acc",
        &fixture("camt053/basic.xml"),
        None,
        &bad,
        None
    )
    .unwrap_err()
    .to_string()
    .contains("validation.statementIndexInvalid"));
    let order = StatementImportOptions {
        date_order: Some(DateOrder::Dmy),
        ..options()
    };
    assert!(preview(
        &conn,
        "acc",
        &fixture("camt053/basic.xml"),
        None,
        &order,
        None
    )
    .unwrap_err()
    .to_string()
    .contains("validation.statementDateOrderInvalid"));
}

#[test]
fn not_yet_supported_formats_say_so() {
    let conn = db();
    let err = inspect(&conn, "acc", b"!Type:Bank\nD1/2/26\nT5\n^\n", None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("validation.statementFormatNotYetSupported"));
}

#[test]
fn skipped_entries_are_counted_apart_from_importable_ones() {
    let conn = db();
    let insp = inspect(
        &conn,
        "acc",
        &fixture("camt053/batch-and-pending.xml"),
        None,
    )
    .unwrap()
    .unwrap();
    let s = &insp.statements[0];
    // The batch (2), the pending entry and the booked one; the INFO entry is only shown.
    assert_eq!((s.entry_count, s.skipped_count), (4, 1));
    assert_eq!(s.balance_check, BalanceCheck::AddsUp);
}

#[test]
fn default_statement_is_the_one_of_the_selected_account() {
    let conn = db();
    let text = String::from_utf8(fixture("camt053/basic.xml")).unwrap();
    let start = text.find("<Stmt>").unwrap();
    let end = text.rfind("</Stmt>").unwrap() + "</Stmt>".len();
    let second = text[start..end].replace("CZ6508000000192000145399", "CZ5508000000001234567899");
    let two = format!("{}{}{}", &text[..end], second, &text[end..]);
    let pick = |account: &str| {
        let insp = inspect(&conn, account, two.as_bytes(), None)
            .unwrap()
            .unwrap();
        assert_eq!(insp.statements.len(), 2);
        insp.default_statement_index
    };
    assert_eq!((pick("acc"), pick("other")), (0, 1));
    assert_eq!(
        pick("acc-1"),
        0,
        "no match falls back to the first statement"
    );
}

#[test]
fn source_categories_count_importable_entries_most_frequent_first() {
    let entry = |category: Option<&str>, skip: Option<&'static str>| Entry {
        source_category: category.map(str::to_string),
        skip_reason: skip,
        ..Entry::default()
    };
    let statement = Statement {
        entries: vec![
            entry(Some("Food"), None),
            entry(Some("Rent"), None),
            entry(Some("Food"), None),
            entry(Some("Fun"), None),
            entry(Some("Fun"), None),
            entry(Some(""), None),
            entry(None, None),
            entry(Some("Rent"), Some("statementImport.notBooked")),
            entry(Some("Rent"), Some("statementImport.notBooked")),
        ],
        ..Statement::default()
    };
    let counted: Vec<(String, usize)> = source_categories(&statement)
        .into_iter()
        .map(|c| (c.name, c.count))
        .collect();
    assert_eq!(
        counted,
        vec![
            ("Food".to_string(), 2),
            ("Fun".to_string(), 2),
            ("Rent".to_string(), 1)
        ]
    );
}

#[test]
fn a_statement_over_the_size_cap_is_refused() {
    let conn = db();
    let mut big = fixture("camt053/basic.xml");
    big.resize(MAX_FILE_BYTES as usize + 1, b' ');
    let err = inspect(&conn, "acc", &big, None).unwrap_err();
    assert_eq!(err.keyed().unwrap().0, "validation.statementTooLarge");
}

#[test]
fn a_file_that_is_not_a_statement_is_not_recognized_by_preview_and_import() {
    let mut conn = db();
    let csv = b"Date,Amount\n2026-01-01,5\n";
    let key = |e: AppError| e.keyed().unwrap().0;
    assert_eq!(
        key(preview(&conn, "acc", csv, None, &options(), None).unwrap_err()),
        "validation.statementNotRecognized"
    );
    assert_eq!(
        key(import(&mut conn, "acc", "a.csv", csv, None, &options(), None).unwrap_err()),
        "validation.statementNotRecognized"
    );
}
