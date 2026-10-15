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
fn inspect_mt940_summary() {
    let conn = db();
    let insp = inspect(&conn, "acc", &fixture("mt940/basic.sta"), None)
        .unwrap()
        .unwrap();
    assert_eq!(insp.format, StatementFormat::Mt940);
    // the two pages of the file are chained into one statement
    assert_eq!(insp.statements.len(), 1);
    let s = &insp.statements[0];
    assert_eq!(
        (s.entry_count, s.balance_check, s.account_match),
        (3, BalanceCheck::AddsUp, AccountMatch::Selected)
    );
}

#[test]
fn mt940_imports_through_the_same_pipeline() {
    let mut conn = db();
    let first = import(
        &mut conn,
        "acc",
        "basic.sta",
        &fixture("mt940/basic.sta"),
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!(first.imported_count, 3);
    let balance = first.statement_balance.unwrap();
    assert_eq!(balance.amount, "1260.5");
    let again = import(
        &mut conn,
        "acc",
        "basic.sta",
        &fixture("mt940/basic.sta"),
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!(
        (again.imported_count, again.skipped_duplicates.len()),
        (0, 3)
    );
}

fn us_db() -> rusqlite::Connection {
    let conn = db();
    conn.execute_batch(
        "INSERT INTO bank_accounts (id, name, currency, bban) VALUES
           ('us', 'Checking', 'USD', '021000021123456789');",
    )
    .unwrap();
    conn
}

#[test]
fn inspect_ofx_matches_a_us_account_by_routing_and_account_number() {
    let conn = us_db();
    let insp = inspect(&conn, "us", &fixture("ofx/checking-sgml.ofx"), None)
        .unwrap()
        .unwrap();
    assert_eq!(insp.format, StatementFormat::Ofx);
    assert!(insp.warnings.is_empty());
    assert_eq!(insp.statements.len(), 1);
    let s = &insp.statements[0];
    // the correction is shown, not imported
    assert_eq!(
        (
            s.entry_count,
            s.skipped_count,
            s.balance_check,
            s.account_match
        ),
        (2, 1, BalanceCheck::Unavailable, AccountMatch::Selected)
    );
    assert_eq!(s.currency.as_deref(), Some("USD"));
    let other = inspect(&conn, "acc", &fixture("ofx/checking-sgml.ofx"), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            other.statements[0].account_match,
            other.statements[0].matched_account_id.as_deref()
        ),
        (AccountMatch::Other, Some("us"))
    );
}

#[test]
fn ofx_imports_through_the_same_pipeline() {
    let mut conn = us_db();
    let bytes = fixture("ofx/checking-sgml.ofx");
    let first = import(
        &mut conn,
        "us",
        "checking.ofx",
        &bytes,
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!((first.imported_count, first.skipped_filtered), (2, 1));
    assert_eq!(first.statement_balance.unwrap().amount, "1250.5");
    let again = import(
        &mut conn,
        "us",
        "checking.ofx",
        &bytes,
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
fn inspect_qfx_credit_card_is_a_statement_of_another_account() {
    let conn = db();
    let insp = inspect(&conn, "acc", &fixture("ofx/card-xml.qfx"), None)
        .unwrap()
        .unwrap();
    assert_eq!(insp.format, StatementFormat::Ofx);
    let s = &insp.statements[0];
    assert_eq!(
        s.account_identifier.as_deref(),
        Some("DE89370400440532013000")
    );
    assert_eq!(
        (s.entry_count, s.account_match, s.currency.as_deref()),
        (1, AccountMatch::NoMatch, Some("EUR"))
    );
    assert_eq!(
        s.closing_balance.as_ref().map(|b| b.amount.as_str()),
        Some("-12.3")
    );
}

#[test]
fn ofx_investment_transactions_warn_or_refuse() {
    let conn = us_db();
    let mixed = String::from_utf8(fixture("ofx/checking-sgml.ofx"))
        .unwrap()
        .replace(
            "</OFX>",
            "<INVSTMTMSGSRSV1><INVSTMTTRNRS><INVSTMTRS></INVSTMTRS></INVSTMTTRNRS></INVSTMTMSGSRSV1></OFX>",
        );
    let insp = inspect(&conn, "us", mixed.as_bytes(), None)
        .unwrap()
        .unwrap();
    assert_eq!(insp.warnings.len(), 1);
    assert_eq!(insp.warnings[0].key, "statementImport.investmentIgnored");
    assert_eq!(insp.statements.len(), 1);
    let err = inspect(&conn, "us", &fixture("ofx/investment-only.ofx"), None).unwrap_err();
    assert_eq!(err.keyed().unwrap().0, "validation.statementInvestmentOnly");
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
fn inspect_qif_summary() {
    let conn = db();
    let insp = inspect(&conn, "acc", &fixture("qif/quicken-us.qif"), None)
        .unwrap()
        .unwrap();
    assert_eq!(insp.format, StatementFormat::Qif);
    assert!(insp.warnings.is_empty());
    assert!(!insp.date_order_ambiguous);
    assert_eq!(insp.detected_date_order, Some(DateOrder::Mdy));
    assert_eq!(insp.statements.len(), 1);
    let s = &insp.statements[0];
    // QIF names no account number: the match is unknown, and no closing balance is declared
    assert_eq!(
        (s.entry_count, s.balance_check, s.account_match),
        (3, BalanceCheck::Unavailable, AccountMatch::Unknown)
    );
    assert_eq!(s.account_name.as_deref(), Some("Checking"));
    assert_eq!(
        (
            s.opening_balance.as_ref().map(|b| b.amount.as_str()),
            s.closing_balance.as_ref()
        ),
        (Some("1000"), None)
    );
    // the split transaction and the transfer name no category
    let categories: Vec<(&str, usize)> = s
        .source_categories
        .iter()
        .map(|c| (c.name.as_str(), c.count))
        .collect();
    assert_eq!(categories, vec![("Income:Salary", 1)]);
}

#[test]
fn qif_imports_through_the_same_pipeline_without_a_balance() {
    let mut conn = db();
    let bytes = fixture("qif/quicken-us.qif");
    let preview = preview(&conn, "acc", &bytes, None, &options(), None).unwrap();
    assert_eq!((preview.total_rows, preview.ok_count), (3, 3));
    let categories: Vec<Option<&str>> = preview
        .rows
        .iter()
        .map(|row| row.source_category.as_deref())
        .collect();
    assert_eq!(categories, vec![Some("Income:Salary"), None, None]);
    let first = import(
        &mut conn,
        "acc",
        "quicken.qif",
        &bytes,
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!(first.imported_count, 3);
    assert!(
        first.statement_balance.is_none(),
        "only a declared balance becomes a balance record"
    );
    // QIF rows have no bank id: the repeat is caught by the composite key
    let again = import(
        &mut conn,
        "acc",
        "quicken.qif",
        &bytes,
        None,
        &options(),
        None,
    )
    .unwrap();
    assert_eq!(
        (again.imported_count, again.skipped_duplicates.len()),
        (0, 3)
    );
}

#[test]
fn an_ambiguous_qif_asks_for_the_date_order_and_accepts_one() {
    let conn = db();
    let bytes = b"!Type:Bank\nD03/04/2026\nT5\n^\nD05/06/2026\nT6\n^\n";
    let insp = inspect(&conn, "acc", bytes, None).unwrap().unwrap();
    assert!(insp.date_order_ambiguous);
    assert_eq!(insp.detected_date_order, Some(DateOrder::Dmy));
    let range = |order: Option<DateOrder>| {
        let chosen = StatementImportOptions {
            date_order: order,
            ..options()
        };
        let preview = preview(&conn, "acc", bytes, None, &chosen, None).unwrap();
        assert_eq!(preview.total_rows, 2);
        preview.date_range.map(|r| (r.from, r.to))
    };
    let day = |m: u32, d: u32| model::utc_day(chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap());
    assert_eq!(range(Some(DateOrder::Dmy)), Some((day(4, 3), day(6, 5))));
    assert_eq!(range(Some(DateOrder::Mdy)), Some((day(3, 4), day(5, 6))));
    assert_eq!(
        range(None),
        range(Some(DateOrder::Dmy)),
        "the detected order"
    );
}

#[test]
fn qif_statement_index_is_stable_across_date_order_overrides() {
    let conn = db();
    let bytes = b"!Account\nNChecking\n^\n!Type:Bank\nD03/04/2026\nT-5\nPShop\n^\n\
                  !Account\nNSavings\n^\n!Type:Bank\nD05/06/2026\nT9\nPInterest\n^\n";
    let insp = inspect(&conn, "acc", bytes, None).unwrap().unwrap();
    assert!(insp.date_order_ambiguous);
    let names: Vec<_> = insp
        .statements
        .iter()
        .map(|s| s.account_name.as_deref())
        .collect();
    assert_eq!(names, vec![Some("Checking"), Some("Savings")]);
    for order in [DateOrder::Dmy, DateOrder::Mdy] {
        let with = |statement_index: usize| StatementImportOptions {
            statement_index,
            date_order: Some(order),
            ..options()
        };
        for (index, payee) in [(0, "Shop"), (1, "Interest")] {
            let preview = preview(&conn, "acc", bytes, None, &with(index), None).unwrap();
            assert_eq!(
                preview.rows[0].counterparty.as_deref(),
                Some(payee),
                "statement {index} under {order:?}"
            );
        }
        let beyond = preview(&conn, "acc", bytes, None, &with(2), None).unwrap_err();
        assert_eq!(
            beyond.keyed().unwrap().0,
            "validation.statementIndexInvalid"
        );
    }
}

#[test]
fn inspect_qif_with_two_accounts_and_investments() {
    let conn = db();
    let insp = inspect(&conn, "acc", &fixture("qif/two-accounts.qif"), None)
        .unwrap()
        .unwrap();
    assert_eq!(insp.statements.len(), 2);
    assert_eq!(
        insp.statements
            .iter()
            .map(|s| s.account_name.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("Checking"), Some("Savings")]
    );
    assert_eq!(insp.warnings.len(), 1);
    assert_eq!(insp.warnings[0].key, "statementImport.investmentIgnored");
    assert_eq!(insp.warnings[0].detail.as_deref(), Some("2"));
    assert!(!insp.date_order_ambiguous, "the 15th settles month-first");
}

#[test]
fn qif_with_only_investments_is_refused() {
    let conn = db();
    let err = inspect(&conn, "acc", b"!Type:Invst\nD1/2/2026\nNBuy\nT5\n^\n", None).unwrap_err();
    assert_eq!(err.keyed().unwrap().0, "validation.statementInvestmentOnly");
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
