//! Import engine tests: the 11 bank fixtures end to end, duplicate semantics,
//! preview/import agreement, and the behaviours the audit findings asked for.

use super::test_support::*;
use super::*;
use crate::error::AppError;
use crate::services::csv_presets::CsvRowFilter;

fn basic_config() -> CsvImportConfig {
    CsvImportConfig {
        delimiter: ";".into(),
        date_column: "Datum".into(),
        date_format: "%d.%m.%Y".into(),
        amount_column: Some("Částka".into()),
        description_columns: Some(vec!["Popis".into()]),
        ..Default::default()
    }
}

fn import(conn: &mut Connection, content: &str, config: &CsvImportConfig) -> CsvImportResult {
    import_transactions(conn, "acc-1", "test.csv", content, config, None).expect("import")
}

fn mapped(r: &CsvPreviewResult, role: &str) -> Option<String> {
    r.suggested_mappings.get(role).map(|(h, _)| h.clone())
}

/// Every row-level message is an i18n key, never English prose.
fn assert_i18n_key(m: &CsvRowMessage) {
    let valid = (m.key.starts_with("csvImport.") || m.key.starts_with("validation."))
        && m.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    assert!(valid, "row message is not an i18n key: {m:?}");
}

fn stored(conn: &Connection, sql: &str) -> Vec<(String, String, String, Option<String>)> {
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

// ======================================================================
// Fixtures: mapping detection + full import, one test per bank file
// ======================================================================

#[test]
fn generic_iso_imports_ten_rows_without_manual_mapping() {
    let inspection = inspect_fixture("generic-iso.csv");
    assert_eq!(inspection.delimiter, ",");
    assert_eq!(inspection.header_row, 0);
    assert_eq!(inspection.total_rows, 10);
    assert_eq!(inspection.date_format, "%Y-%m-%d");
    assert!(!inspection.date_format_ambiguous);
    assert_eq!(inspection.decimal_separator, ".");
    assert!(inspection.detected_preset_id.is_none());
    assert_eq!(mapped(&inspection, "date").as_deref(), Some("Date"));
    assert_eq!(mapped(&inspection, "amount").as_deref(), Some("Amount"));
    assert_eq!(
        mapped(&inspection, "description").as_deref(),
        Some("Description")
    );
    assert_eq!(
        mapped(&inspection, "counterparty").as_deref(),
        Some("Counterparty")
    );
    assert_eq!(mapped(&inspection, "currency").as_deref(), Some("Currency"));
    assert_eq!(
        mapped(&inspection, "counterparty_iban").as_deref(),
        Some("IBAN")
    );

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "generic-iso.csv");
    assert_eq!(result.imported_count, 10);
    assert_eq!(result.error_count, 0);
    assert!(result.skipped_duplicates.is_empty() && result.duplicates.is_empty());
    assert_eq!((result.skipped_zero, result.skipped_filtered), (0, 0));
    assert_eq!(tx_count(&conn), 10);
    assert_eq!(
        result.uncategorized_count, 10,
        "no engine, nothing categorized"
    );
    assert!(result.last_balance.is_none());

    // 2026-09-01 .. 2026-09-30 (midnight UTC)
    let range = result.date_range.expect("range");
    assert_eq!(range.from, 1_788_220_800);
    assert_eq!(range.to, 1_790_726_400);

    // Rows land with the right sign, currency and counterparty.
    let rows = stored(
        &conn,
        "SELECT tx_type, amount, currency, counterparty_name FROM bank_transactions
         WHERE description = 'Refund Amazon'",
    );
    assert_eq!(
        rows,
        vec![(
            "credit".into(),
            "29.99".into(),
            "EUR".into(),
            Some("Amazon EU".into())
        )]
    );

    // DE IBANs → the Germany pack is suggested (not enabled by default).
    assert_eq!(result.suggested_rule_pack.as_deref(), Some("de"));
}

#[test]
fn generic_iso_overlap_skips_the_two_duplicates() {
    let mut conn = setup_import_db();
    import_fixture_auto(&mut conn, "generic-iso.csv");

    let result = import_fixture_auto(&mut conn, "generic-iso-overlap.csv");
    assert_eq!(
        result.skipped_duplicates.len(),
        2,
        "Restaurant + Interest overlap"
    );
    assert_eq!(result.imported_count, 2, "Salary October + Rent are new");
    assert_eq!(result.error_count, 0);
    assert!(result.duplicates.is_empty());
    assert_eq!(tx_count(&conn), 12);
    assert_eq!(
        result
            .skipped_duplicates
            .iter()
            .map(|m| m.line)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert!(result
        .skipped_duplicates
        .iter()
        .all(|m| m.key == "csvImport.duplicate"));

    // The batch history counts the skipped ones, so imported + duplicates adds up.
    let (imported, duplicates, errors): (i64, i64, i64) = conn
        .query_row(
            "SELECT imported_count, duplicate_count, error_count FROM csv_import_batches
             ORDER BY rowid DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((imported, duplicates, errors), (2, 2, 0));
}

#[test]
fn overlap_with_import_anyway_lines_or_skip_off_imports_all_four() {
    let mut conn = setup_import_db();
    import_fixture_auto(&mut conn, "generic-iso.csv");
    let bytes = fixture_bytes("generic-iso-overlap.csv");
    let text = decode_for_import(&bytes, None, None).text;
    let mut config = config_from_inspection(&inspect_fixture("generic-iso-overlap.csv"));

    // Import only line 2 anyway: it goes in and is reported as a flagged duplicate.
    config.import_anyway_lines = vec![2];
    let result = import(&mut conn, &text, &config);
    assert_eq!(result.imported_count, 3);
    assert_eq!(result.duplicates.len(), 1);
    assert_eq!(result.duplicates[0].line, 2);
    assert_eq!(result.duplicate_count, 1);
    assert_eq!(result.skipped_duplicates.len(), 1);
    assert_eq!(result.skipped_duplicates[0].line, 3);

    // skipDuplicates=false: everything goes in, hits are flagged.
    let mut conn = setup_import_db();
    import_fixture_auto(&mut conn, "generic-iso.csv");
    config.import_anyway_lines.clear();
    config.skip_duplicates = false;
    let result = import(&mut conn, &text, &config);
    assert_eq!(result.imported_count, 4);
    assert_eq!(result.duplicate_count, 2);
    assert!(result.skipped_duplicates.is_empty());
}

#[test]
fn identical_rows_with_distinct_bank_ids_are_two_payments() {
    // with a bank-side id column the id is the only key — two 99 CZK
    // coffees on one day are both imported; without ids the second is a
    // suspect the user can import anyway.
    let mut config = basic_config();
    config.transaction_id_column = Some("ID".into());
    let with_ids = "ID;Datum;Částka;Popis\nA1;15.01.2026;-99;coffee\nA2;15.01.2026;-99;coffee\n";
    let mut conn = setup_import_db();
    let result = import(&mut conn, with_ids, &config);
    assert_eq!(result.imported_count, 2);
    assert!(result.skipped_duplicates.is_empty());

    // The dry run agrees with the import (on an empty account).
    let fresh = setup_import_db();
    let preview = preview_import(&fresh, "acc-1", with_ids, &config, None).unwrap();
    assert_eq!(preview.duplicate_count, 0);

    let mut conn = setup_import_db();
    let mut no_ids = basic_config();
    no_ids.transaction_id_column = None;
    let without = "Datum;Částka;Popis\n15.01.2026;-99;coffee\n15.01.2026;-99;coffee\n";
    let result = import(&mut conn, without, &no_ids);
    assert_eq!(result.imported_count, 1);
    assert_eq!(result.skipped_duplicates.len(), 1);
}

#[test]
fn reimporting_the_same_file_skips_everything() {
    let mut conn = setup_import_db();
    let first = import_fixture_auto(&mut conn, "generic-iso.csv");
    assert_eq!(first.imported_count, 10);
    let second = import_fixture_auto(&mut conn, "generic-iso.csv");
    assert_eq!(second.imported_count, 0);
    assert_eq!(second.skipped_duplicates.len(), 10);
    assert!(second.date_range.is_none(), "nothing imported, no range");
    assert_eq!(tx_count(&conn), 10);
}

#[test]
fn edge_cases_buckets_add_up() {
    let inspection = inspect_fixture("edge-cases.csv");
    assert_eq!(inspection.total_rows, 14);
    assert_eq!(inspection.date_format, "%Y-%m-%d");
    assert_eq!(inspection.decimal_separator, ".");

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "edge-cases.csv");

    assert_eq!(result.imported_count, 8);
    assert_eq!(
        result.skipped_duplicates.len(),
        1,
        "the repeated first row, once"
    );
    assert_eq!(result.skipped_duplicates[0].line, 3);
    assert_eq!(result.skipped_zero, 1, "0.00 is skipped, not an error");
    assert_eq!(result.error_count, 4);
    // Every data row is in exactly one bucket.
    assert_eq!(
        result.imported_count
            + result.skipped_duplicates.len()
            + result.skipped_zero
            + result.skipped_filtered
            + result.error_count,
        inspection.total_rows
    );

    // Errors: empty amount (line 7), impossible date (line 8), 2-digit year (line 12),
    // currency "XYZ" that is not an ISO 4217 code (line 13).
    let errors: Vec<(usize, &str, Option<&str>)> = result
        .errors
        .iter()
        .map(|m| (m.line, m.key.as_str(), m.detail.as_deref()))
        .collect();
    assert_eq!(
        errors,
        vec![
            (7, "validation.amountRequired", None),
            (8, "csvImport.dateUnparseable", Some("31-02-2026")),
            (12, "csvImport.dateUnparseable", Some("26/09/06")),
            (13, "validation.currencyUnknown", Some("XYZ")),
        ]
    );
    result.errors.iter().for_each(assert_i18n_key);

    // "1,234.56" is 1234.56, "(12.34)" is a 12.34 debit, "EUR " is trimmed,
    // "-7.00 " is a 7 debit.
    let rows = stored(
        &conn,
        "SELECT description, tx_type, amount, currency FROM bank_transactions
         WHERE description IN ('Thousands separator','Parentheses negative','Trailing spaces')
         ORDER BY description",
    );
    assert_eq!(
        rows,
        vec![
            (
                "Parentheses negative".into(),
                "debit".into(),
                "12.34".into(),
                Some("EUR".into())
            ),
            (
                "Thousands separator".into(),
                "credit".into(),
                "1234.56".into(),
                Some("EUR".into())
            ),
            (
                "Trailing spaces".into(),
                "debit".into(),
                "7".into(),
                Some("EUR".into())
            ),
        ]
    );
    // The huge amount keeps its digits; the multi-line description arrives whole.
    let huge: String = conn
        .query_row(
            "SELECT amount FROM bank_transactions WHERE description = 'Huge amount'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(huge, "123456789012.99");
    let multi: i64 = conn
        .query_row(
            "SELECT count(*) FROM bank_transactions WHERE description = 'Multi' || char(10) || 'line'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(multi, 1);
}

#[test]
fn edge_cases_line_numbers_follow_physical_lines() {
    // The quoted multi-line cell starts on line 15 and ends on 16; rows after a
    // multi-line cell must keep counting physical lines.
    let bytes = fixture_bytes("edge-cases.csv");
    let text = decode_for_import(&bytes, None, None).text;
    let config = config_from_inspection(&inspect_fixture("edge-cases.csv"));
    let parsed = parse_rows(&text, "acc-1", &config).unwrap();
    let lines: Vec<usize> = parsed.rows.iter().map(|r| r.line).collect();
    assert_eq!(lines, (2..=13).chain([14, 15]).collect::<Vec<_>>());
}

#[test]
fn fio_preamble_is_skipped_and_ids_feed_the_dedup_rule() {
    let inspection = inspect_fixture("fio.csv");
    assert_eq!(inspection.header_row, 10, "9 preamble lines + a blank one");
    assert_eq!(inspection.headers[0], "ID operace");
    assert_eq!(inspection.total_rows, 3);
    assert_eq!(inspection.detected_preset_id.as_deref(), Some("fio"));
    assert_eq!(mapped(&inspection, "date").as_deref(), Some("Datum"));
    assert_eq!(mapped(&inspection, "amount").as_deref(), Some("Objem"));
    assert_eq!(mapped(&inspection, "currency").as_deref(), Some("Měna"));
    assert_eq!(
        mapped(&inspection, "counterparty").as_deref(),
        Some("Název protiúčtu")
    );
    assert_eq!(
        mapped(&inspection, "transactionId").as_deref(),
        Some("ID operace")
    );
    assert_eq!(inspection.decimal_separator, ",");
    assert_eq!(inspection.date_format, "%d.%m.%Y");
    assert!(
        !inspection.date_format_ambiguous,
        "the preset knows the order"
    );
    assert_eq!(
        inspection.suggested_description_columns,
        vec![
            "Zpráva pro příjemce",
            "Uživatelská identifikace",
            "Komentář"
        ]
    );

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "fio.csv");
    assert_eq!(result.imported_count, 3);
    assert_eq!(result.error_count, 0);
    // Header on line 11, so the first data row is line 12.
    let parsed = parse_rows(
        &decode_for_import(&fixture_bytes("fio.csv"), None, None).text,
        "acc-1",
        &config_from_inspection(&inspection),
    )
    .unwrap();
    assert_eq!(parsed.rows.first().map(|r| r.line), Some(12));

    let ids = stored(
        &conn,
        "SELECT transaction_id, amount, tx_type, currency FROM bank_transactions ORDER BY booking_date",
    );
    assert_eq!(
        ids[0],
        (
            "26000000001".into(),
            "25000".into(),
            "credit".into(),
            Some("CZK".into())
        )
    );
    assert_eq!(ids[1].0, "26000000002");

    // Re-import: the bank-side id rule catches every row, with its own key.
    let again = import_fixture_auto(&mut conn, "fio.csv");
    assert_eq!(again.imported_count, 0);
    assert_eq!(again.skipped_duplicates.len(), 3);
    assert!(again
        .skipped_duplicates
        .iter()
        .all(|m| m.key == "csvImport.duplicateById"));
}

#[test]
fn forcing_an_id_duplicate_in_does_not_violate_the_unique_id() {
    let mut conn = setup_import_db();
    import_fixture_auto(&mut conn, "fio.csv");
    let text = decode_for_import(&fixture_bytes("fio.csv"), None, None).text;
    let mut config = config_from_inspection(&inspect_fixture("fio.csv"));
    config.import_anyway_lines = vec![12];

    let result = import(&mut conn, &text, &config);
    assert_eq!(result.imported_count, 1);
    assert_eq!(result.duplicates.len(), 1);
    assert_eq!(result.duplicates[0].key, "csvImport.duplicateById");
    assert_eq!(result.skipped_duplicates.len(), 2);
    assert_eq!(tx_count(&conn), 4);
    let without_id: i64 = conn
        .query_row(
            "SELECT count(*) FROM bank_transactions WHERE transaction_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        without_id, 1,
        "the forced copy is stored without the bank-side id"
    );
}

#[test]
fn ceska_sporitelna_maps_and_imports() {
    let inspection = inspect_fixture("ceska-sporitelna.csv");
    assert_eq!(inspection.header_row, 0);
    assert_eq!(inspection.delimiter, ";");
    assert_eq!(
        inspection.detected_preset_id.as_deref(),
        Some("ceska-sporitelna")
    );
    assert_eq!(
        mapped(&inspection, "date").as_deref(),
        Some("Datum zaúčtování")
    );
    assert_eq!(mapped(&inspection, "amount").as_deref(), Some("Částka"));
    assert_eq!(
        mapped(&inspection, "description").as_deref(),
        Some("Zpráva pro příjemce")
    );
    assert_eq!(
        mapped(&inspection, "counterparty").as_deref(),
        Some("Název protiúčtu")
    );
    assert_eq!(mapped(&inspection, "currency").as_deref(), Some("Měna"));
    assert_eq!(
        mapped(&inspection, "counterparty_iban").as_deref(),
        Some("Číslo protiúčtu")
    );
    assert_eq!(inspection.decimal_separator, ",");

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "ceska-sporitelna.csv");
    assert_eq!(result.imported_count, 4);
    assert_eq!(result.error_count, 0);
    let rows = stored(
        &conn,
        "SELECT tx_type, amount, currency, counterparty_name FROM bank_transactions
         WHERE counterparty_name = 'ZAMĚSTNAVATEL S.R.O.'",
    );
    assert_eq!(
        rows,
        vec![(
            "credit".into(),
            "45000".into(),
            "CZK".into(),
            Some("ZAMĚSTNAVATEL S.R.O.".into())
        )]
    );
}

#[test]
fn sparkasse_maps_german_headers_and_two_digit_years() {
    let inspection = inspect_fixture("de-sparkasse.csv");
    assert_eq!(inspection.detected_preset_id.as_deref(), Some("sparkasse"));
    assert_eq!(mapped(&inspection, "date").as_deref(), Some("Buchungstag"));
    assert_eq!(
        mapped(&inspection, "valueDate").as_deref(),
        Some("Valutadatum")
    );
    assert_eq!(mapped(&inspection, "amount").as_deref(), Some("Betrag"));
    assert_eq!(
        mapped(&inspection, "description").as_deref(),
        Some("Verwendungszweck")
    );
    assert_eq!(
        mapped(&inspection, "counterparty").as_deref(),
        Some("Beguenstigter/Zahlungspflichtiger")
    );
    assert_eq!(mapped(&inspection, "currency").as_deref(), Some("Waehrung"));
    assert_eq!(
        mapped(&inspection, "counterparty_iban").as_deref(),
        Some("Kontonummer/IBAN")
    );
    assert_eq!(inspection.date_format, "%d.%m.%y");
    assert_eq!(inspection.decimal_separator, ",");

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "de-sparkasse.csv");
    assert_eq!(result.imported_count, 3);
    assert_eq!(result.error_count, 0);
    let rows = stored(
        &conn,
        "SELECT tx_type, amount, currency, description FROM bank_transactions ORDER BY booking_date",
    );
    assert_eq!(
        rows[0],
        (
            "credit".into(),
            "2750".into(),
            "EUR".into(),
            Some("Gehalt September | LOHN/GEHALT".into())
        )
    );
    assert_eq!(rows[1].1, "980");
    assert_eq!(result.suggested_rule_pack.as_deref(), Some("de"));
}

#[test]
fn n26_maps_amount_in_brackets_and_partner_columns() {
    let inspection = inspect_fixture("n26.csv");
    assert_eq!(inspection.detected_preset_id.as_deref(), Some("n26"));
    assert_eq!(mapped(&inspection, "date").as_deref(), Some("Booking Date"));
    assert_eq!(
        mapped(&inspection, "amount").as_deref(),
        Some("Amount (EUR)")
    );
    assert_eq!(
        mapped(&inspection, "counterparty").as_deref(),
        Some("Partner Name")
    );
    assert_eq!(
        mapped(&inspection, "counterparty_iban").as_deref(),
        Some("Partner Iban")
    );
    assert_eq!(
        mapped(&inspection, "description").as_deref(),
        Some("Payment Reference")
    );
    assert!(
        mapped(&inspection, "currency").is_none(),
        "booked in the account currency"
    );

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "n26.csv");
    assert_eq!(result.imported_count, 4);
    assert_eq!(result.error_count, 0);
    // No currency column: the account's own currency applies (acc-1 is CZK here).
    let rows = stored(
        &conn,
        "SELECT tx_type, amount, currency, description FROM bank_transactions ORDER BY booking_date",
    );
    assert_eq!(rows[0].2, "CZK");
    // No reference text: the partner name becomes the description (and keeps
    // two same-day, same-amount purchases at different shops apart).
    assert_eq!(rows[0].3.as_deref(), Some("Lidl"));
    assert_eq!(rows[1].3.as_deref(), Some("Dinner | Max Mustermann"));
}

#[test]
fn revolut_pending_rows_are_filtered_and_balance_is_the_last_completed() {
    let inspection = inspect_fixture("revolut.csv");
    assert_eq!(inspection.detected_preset_id.as_deref(), Some("revolut"));
    assert_eq!(
        mapped(&inspection, "date").as_deref(),
        Some("Completed Date")
    );
    assert_eq!(mapped(&inspection, "amount").as_deref(), Some("Amount"));
    assert_eq!(
        mapped(&inspection, "description").as_deref(),
        Some("Description")
    );
    assert_eq!(mapped(&inspection, "currency").as_deref(), Some("Currency"));
    assert_eq!(mapped(&inspection, "balance").as_deref(), Some("Balance"));
    assert_eq!(
        inspection.date_format, "%Y-%m-%d",
        "date part of the preset's format"
    );
    assert_eq!(
        inspection.suggested_row_filter,
        Some(CsvRowFilter {
            column: "State".into(),
            equals: "COMPLETED".into()
        })
    );

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "revolut.csv");
    assert_eq!(result.imported_count, 6);
    assert_eq!(result.skipped_filtered, 1, "the PENDING row");
    assert_eq!(
        result.error_count, 0,
        "its empty Completed Date is not an error"
    );
    assert_eq!(result.last_balance.as_deref(), Some("1050.36"));
    let gbp: i64 = conn
        .query_row(
            "SELECT count(*) FROM bank_transactions WHERE currency = 'GBP'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(gbp, 6);

    // Without the filter the pending row would be a date error — which is why
    // the inspection hands the preset's filter to the import.
    let text = decode_for_import(&fixture_bytes("revolut.csv"), None, None).text;
    let mut config = config_from_inspection(&inspection);
    config.row_filter = None;
    let mut conn = setup_import_db();
    let unfiltered = import(&mut conn, &text, &config);
    assert_eq!(unfiltered.error_count, 1);
    assert_eq!(unfiltered.errors[0].key, "validation.dateRequired");
}

#[test]
fn wise_maps_transaction_id_and_payee() {
    let inspection = inspect_fixture("wise.csv");
    assert_eq!(inspection.detected_preset_id.as_deref(), Some("wise"));
    assert_eq!(
        mapped(&inspection, "transactionId").as_deref(),
        Some("TransferWise ID")
    );
    assert_eq!(
        mapped(&inspection, "counterparty").as_deref(),
        Some("Payee Name")
    );
    assert_eq!(
        mapped(&inspection, "balance").as_deref(),
        Some("Running Balance")
    );
    assert_eq!(inspection.date_format, "%d-%m-%Y");
    assert!(
        !inspection.date_format_ambiguous,
        "preset order confirmed by the samples"
    );

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "wise.csv");
    assert_eq!(result.imported_count, 3);
    assert_eq!(result.error_count, 0);
    assert_eq!(result.last_balance.as_deref(), Some("1870.01"));
    let ids: i64 = conn
        .query_row(
            "SELECT count(*) FROM bank_transactions WHERE transaction_id LIKE '%-%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ids, 3);
}

#[test]
fn uk_debit_credit_pair_becomes_one_signed_amount() {
    let inspection = inspect_fixture("uk-debit-credit.csv");
    assert_eq!(
        inspection.detected_preset_id.as_deref(),
        Some("uk-debit-credit")
    );
    assert_eq!(mapped(&inspection, "debit").as_deref(), Some("Debit"));
    assert_eq!(mapped(&inspection, "credit").as_deref(), Some("Credit"));
    assert!(mapped(&inspection, "amount").is_none());
    assert_eq!(inspection.date_format, "%d/%m/%Y");
    assert!(!inspection.date_format_ambiguous);

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "uk-debit-credit.csv");
    assert_eq!(result.imported_count, 4);
    assert_eq!(result.error_count, 0);
    assert_eq!(result.last_balance.as_deref(), Some("2796.39"));
    let rows = stored(
        &conn,
        "SELECT tx_type, amount, description, NULL FROM bank_transactions ORDER BY booking_date",
    );
    assert_eq!(
        rows[0],
        (
            "credit".into(),
            "2850".into(),
            "SALARY ACME LTD".into(),
            None
        )
    );
    assert_eq!(
        rows[1],
        ("debit".into(), "78.5".into(), "DD BRITISH GAS".into(), None)
    );
    // DD/MM/YYYY consistently: 15/09 is September 15th, 01/09 September 1st.
    let first: i64 = conn
        .query_row("SELECT min(booking_date) FROM bank_transactions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(first, 1_788_220_800, "2026-09-01");
}

#[test]
fn chase_month_first_dates_import_consistently() {
    let inspection = inspect_fixture("us-chase.csv");
    assert_eq!(inspection.detected_preset_id.as_deref(), Some("chase"));
    assert_eq!(mapped(&inspection, "date").as_deref(), Some("Posting Date"));
    assert_eq!(mapped(&inspection, "amount").as_deref(), Some("Amount"));
    assert_eq!(
        mapped(&inspection, "description").as_deref(),
        Some("Description")
    );
    assert_eq!(mapped(&inspection, "balance").as_deref(), Some("Balance"));
    assert_eq!(inspection.date_format, "%m/%d/%Y");

    let mut conn = setup_import_db();
    let result = import_fixture_auto(&mut conn, "us-chase.csv");
    assert_eq!(result.imported_count, 4);
    assert_eq!(result.error_count, 0);
    assert_eq!(result.last_balance.as_deref(), Some("5773.23"));
    // 09/02 is September 2nd, 09/13 September 13th — never February/March.
    let dates: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT booking_date FROM bank_transactions ORDER BY booking_date")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(
        dates,
        vec![1_788_307_200, 1_788_566_400, 1_789_171_200, 1_789_257_600],
        "Sep 2, Sep 5, Sep 12, Sep 13 2026"
    );
}

#[test]
fn month_first_file_with_a_day_first_config_errors_instead_of_guessing() {
    // the old per-row fallback read 09/02 as 9 Feb and 09/13 as
    // Sep 13 in the same file. With an explicit DD/MM format the MM/DD-only
    // dates are errors, not silently re-interpreted.
    let mut conn = setup_import_db();
    let text = decode_for_import(&fixture_bytes("us-chase.csv"), None, None).text;
    let mut config = config_from_inspection(&inspect_fixture("us-chase.csv"));
    config.date_format = "%d/%m/%Y".into();
    let result = import(&mut conn, &text, &config);
    assert_eq!(
        result.imported_count, 3,
        "09/02, 09/05, 09/12 are valid DD/MM dates"
    );
    assert_eq!(result.error_count, 1, "09/13 has no month 13");
    assert_eq!(result.errors[0].key, "csvImport.dateUnparseable");
    assert_eq!(result.errors[0].detail.as_deref(), Some("09/13/2026"));
    // ... and they were all read the same way: day first.
    let months: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT cast(strftime('%m', booking_date, 'unixepoch') as integer) FROM bank_transactions")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert!(months.iter().all(|m| [2, 5, 12].contains(m)), "{months:?}");
}

// ======================================================================
// Preview
// ======================================================================

#[test]
fn preview_matches_the_import_and_writes_nothing() {
    for fixture in [
        "generic-iso.csv",
        "edge-cases.csv",
        "fio.csv",
        "revolut.csv",
        "uk-debit-credit.csv",
    ] {
        let mut conn = setup_import_db();
        // Overlap: part of every file is already in the account.
        if fixture == "generic-iso.csv" {
            import_fixture_auto(&mut conn, "generic-iso-overlap.csv");
        }
        let text = decode_for_import(&fixture_bytes(fixture), None, None).text;
        let config = config_from_inspection(&inspect_fixture(fixture));

        let before = tx_count(&conn);
        let preview = preview_import(&conn, "acc-1", &text, &config, None).unwrap();
        assert_eq!(tx_count(&conn), before, "{fixture}: preview must not write");
        let batches: i64 = conn
            .query_row("SELECT count(*) FROM csv_import_batches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            batches,
            i64::from(fixture == "generic-iso.csv"),
            "{fixture}: no batch"
        );

        let result = import(&mut conn, &text, &config);
        assert_eq!(preview.ok_count, result.imported_count, "{fixture}: ok");
        assert_eq!(preview.error_count, result.error_count, "{fixture}: errors");
        assert_eq!(
            preview.duplicate_count,
            result.skipped_duplicates.len(),
            "{fixture}: dups"
        );
        assert_eq!(
            preview.skipped_count,
            result.skipped_zero + result.skipped_filtered,
            "{fixture}: skipped"
        );
        assert_eq!(preview.date_range, result.date_range, "{fixture}: range");
        assert_eq!(
            preview.ok_count
                + preview.error_count
                + preview.duplicate_count
                + preview.skipped_count,
            preview.total_rows
        );
        for row in &preview.rows {
            if let Some(m) = &row.message {
                assert_i18n_key(m);
            }
        }
    }
}

#[test]
fn preview_rows_carry_parsed_values_and_statuses() {
    let mut conn = setup_import_db();
    import_fixture_auto(&mut conn, "generic-iso.csv");
    let text = decode_for_import(&fixture_bytes("generic-iso-overlap.csv"), None, None).text;
    let config = config_from_inspection(&inspect_fixture("generic-iso-overlap.csv"));

    let preview = preview_import(&conn, "acc-1", &text, &config, None).unwrap();
    let statuses: Vec<CsvRowStatus> = preview.rows.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        vec![
            CsvRowStatus::Duplicate,
            CsvRowStatus::Duplicate,
            CsvRowStatus::Ok,
            CsvRowStatus::Ok
        ]
    );
    let first = &preview.rows[0];
    assert_eq!(first.line, 2);
    assert_eq!(first.amount.as_deref(), Some("-45.8"));
    assert_eq!(first.currency.as_deref(), Some("EUR"));
    assert_eq!(first.description.as_deref(), Some("Restaurant"));
    assert_eq!(first.counterparty.as_deref(), Some("Trattoria Roma"));
    assert_eq!(first.booking_date, Some(1_790_294_400), "2026-09-25");
    assert_eq!(
        first.message.as_ref().map(|m| m.key.as_str()),
        Some("csvImport.duplicate")
    );
    assert_eq!(preview.rows[2].amount.as_deref(), Some("3200"));
    // Range of what would be written: the two new rows only.
    let range = preview.date_range.unwrap();
    assert_eq!((range.from, range.to), (1_790_812_800, 1_790_899_200));

    // Marking a duplicate "import anyway" widens the range accordingly.
    let mut forced = config.clone();
    forced.import_anyway_lines = vec![2];
    let preview = preview_import(&conn, "acc-1", &text, &forced, None).unwrap();
    assert_eq!(preview.date_range.unwrap().from, 1_790_294_400);
}

#[test]
fn preview_resolves_categories_with_the_engine_and_overrides() {
    use crate::services::categorization::{CategorizationRule, RuleType};

    let conn = setup_import_db();
    conn.execute(
        "INSERT INTO transaction_categories (id, name) VALUES ('cat-2', 'Dining')",
        [],
    )
    .unwrap();
    let engine = CategorizationEngine::new(vec![CategorizationRule::new(
        "r1".into(),
        "Albert".into(),
        RuleType::Contains,
        "albert".into(),
        "cat-1".into(),
    )]);
    let content = "Datum;Částka;Popis\n15.01.2024;-250,50;Albert Hypermarket\n16.01.2024;-99,00;Unknown merchant\n17.01.2024;-120,00;Pizzeria\n";

    // Rules only: the Albert row is categorized, the others stay open.
    let preview = preview_import(&conn, "acc-1", content, &basic_config(), Some(&engine)).unwrap();
    assert_eq!(preview.categorized_count, 1);
    assert_eq!(preview.rows[0].category_id.as_deref(), Some("cat-1"));
    assert_eq!(preview.rows[0].category_source.as_deref(), Some("rule"));
    assert!(preview.rows[1].category_id.is_none());

    // Overrides win, and one onto a deleted category is ignored.
    let mut config = basic_config();
    config.category_overrides = vec![
        CsvCategoryOverride {
            line: 4,
            category_id: "cat-2".into(),
        },
        CsvCategoryOverride {
            line: 3,
            category_id: "cat-gone".into(),
        },
    ];
    let preview = preview_import(&conn, "acc-1", content, &config, Some(&engine)).unwrap();
    assert_eq!(preview.categorized_count, 2);
    assert_eq!(preview.rows[2].category_id.as_deref(), Some("cat-2"));
    assert_eq!(preview.rows[2].category_source.as_deref(), Some("manual"));
    assert!(preview.rows[1].category_id.is_none());

    // Without an engine only the overrides apply.
    let preview = preview_import(&conn, "acc-1", content, &config, None).unwrap();
    assert_eq!(preview.categorized_count, 1);
    assert!(preview.rows[0].category_id.is_none());
}

#[test]
fn import_applies_category_overrides_before_the_rules() {
    use crate::services::categorization::{CategorizationRule, RuleType};

    let mut conn = setup_import_db();
    conn.execute(
        "INSERT INTO transaction_categories (id, name) VALUES ('cat-2', 'Dining')",
        [],
    )
    .unwrap();
    let engine = CategorizationEngine::new(vec![CategorizationRule::new(
        "r1".into(),
        "Albert".into(),
        RuleType::Contains,
        "albert".into(),
        "cat-1".into(),
    )]);
    let content = "Datum;Částka;Popis\n15.01.2024;-250,50;Albert Hypermarket\n16.01.2024;-99,00;Unknown merchant\n";
    let mut config = basic_config();
    config.category_overrides = vec![
        // The user disagrees with the rule on line 2 and categorizes line 3 by hand.
        CsvCategoryOverride {
            line: 2,
            category_id: "cat-2".into(),
        },
        CsvCategoryOverride {
            line: 3,
            category_id: "cat-2".into(),
        },
    ];
    let result = import_transactions(
        &mut conn,
        "acc-1",
        "test.csv",
        content,
        &config,
        Some(&engine),
    )
    .unwrap();
    assert_eq!(result.imported_count, 2);
    assert_eq!(result.uncategorized_count, 0);
    let rows: Vec<(String, Option<String>, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT description, category_id, categorization_source FROM bank_transactions
                 ORDER BY booking_date",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(rows[0].1.as_deref(), Some("cat-2"));
    assert_eq!(rows[0].2.as_deref(), Some("manual"));
    assert_eq!(rows[1].1.as_deref(), Some("cat-2"));
}

#[test]
fn preview_flags_in_file_duplicates_like_the_import() {
    let conn = setup_import_db();
    let content =
        "Datum;Částka;Popis\n15.01.2024;-99;coffee\n15.01.2024;-99;coffee\n16.01.2024;-99;coffee\n";
    let preview = preview_import(&conn, "acc-1", content, &basic_config(), None).unwrap();
    let statuses: Vec<CsvRowStatus> = preview.rows.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        vec![CsvRowStatus::Ok, CsvRowStatus::Duplicate, CsvRowStatus::Ok]
    );
}

#[test]
fn preview_lists_the_first_200_rows_but_counts_all() {
    let conn = setup_import_db();
    let mut content = String::from("Datum;Částka;Popis\n");
    for i in 0..250 {
        content.push_str(&format!("15.01.2024;-{};row {}\n", i + 1, i));
    }
    let preview = preview_import(&conn, "acc-1", &content, &basic_config(), None).unwrap();
    assert_eq!(preview.rows.len(), PREVIEW_ROWS);
    assert_eq!(preview.total_rows, 250);
    assert_eq!(preview.ok_count, 250);
}

// ======================================================================
// Amounts, columns and config in the import
// ======================================================================

#[test]
fn thousands_separator_is_decided_per_column() {
    // US column: "1,234" is 1234 because the other cells use '.' as decimal.
    let mut conn = setup_import_db();
    let content = "Date,Description,Amount\n2026-09-01,a,\"1,234\"\n2026-09-02,b,56.78\n";
    let config = CsvImportConfig {
        delimiter: ",".into(),
        date_column: "Date".into(),
        date_format: "%Y-%m-%d".into(),
        amount_column: Some("Amount".into()),
        description_columns: Some(vec!["Description".into()]),
        ..Default::default()
    };
    import(&mut conn, content, &config);
    let amounts: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT amount FROM bank_transactions ORDER BY booking_date")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(amounts, vec!["1234", "56.78"]);

    // European column: the same cell text is 1.234 when the others use ','.
    let mut conn = setup_import_db();
    let content = "Date,Description,Amount\n2026-09-01,a,\"1,234\"\n2026-09-02,b,\"56,78\"\n";
    import(&mut conn, content, &config);
    let amounts: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT amount FROM bank_transactions ORDER BY booking_date")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(amounts, vec!["1.234", "56.78"]);

    // An explicit separator overrides the detection; a cell that contradicts it
    // is an error, not a ×100 surprise.
    let mut conn = setup_import_db();
    let content = "Date,Description,Amount\n2026-09-01,a,12.50\n2026-09-02,b,\"12,50\"\n";
    let mut explicit = config.clone();
    explicit.decimal_separator = Some(".".into());
    let result = import(&mut conn, content, &explicit);
    assert_eq!((result.imported_count, result.error_count), (1, 1));
    assert_eq!(result.errors[0].key, "csvImport.amountUnparseable");
    assert_eq!(result.errors[0].detail.as_deref(), Some("12,50"));
}

#[test]
fn row_errors_zero_amounts_and_skips_do_not_abort_the_import() {
    let mut conn = setup_import_db();
    // Line 2: unparseable date. Line 3: zero amount. Line 4: garbage amount.
    // Line 5: valid. Line 6: blank (ignored). Line 7: bad currency.
    let content = "Datum;Částka;Popis;Měna\ngarbage;-10;bad date;CZK\n15.01.2024;0;zero;CZK\n15.01.2024;abc;bad amount;CZK\n16.01.2024;-20;ok;CZK\n\n17.01.2024;-5;bad ccy;C1\n";
    let mut config = basic_config();
    config.currency_column = Some("Měna".into());
    let result = import(&mut conn, content, &config);

    assert_eq!(result.imported_count, 1);
    assert_eq!(result.skipped_zero, 1);
    assert_eq!(result.error_count, 3);
    let errors: Vec<(usize, &str, Option<&str>)> = result
        .errors
        .iter()
        .map(|m| (m.line, m.key.as_str(), m.detail.as_deref()))
        .collect();
    assert_eq!(
        errors,
        vec![
            (2, "csvImport.dateUnparseable", Some("garbage")),
            (4, "csvImport.amountUnparseable", Some("abc")),
            (7, "validation.currencyInvalid", None),
        ]
    );
    result.errors.iter().for_each(assert_i18n_key);
    assert_eq!(tx_count(&conn), 1);

    let (imported, dups, errs): (i64, i64, i64) = conn
        .query_row(
            "SELECT imported_count, duplicate_count, error_count FROM csv_import_batches",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((imported, dups, errs), (1, 0, 3));
}

#[test]
fn debit_credit_pair_edge_cases() {
    let config = CsvImportConfig {
        delimiter: ",".into(),
        date_column: "Date".into(),
        date_format: "%Y-%m-%d".into(),
        debit_column: Some("Debit".into()),
        credit_column: Some("Credit".into()),
        description_columns: Some(vec!["Description".into()]),
        ..Default::default()
    };
    let mut conn = setup_import_db();
    let content = "Date,Description,Debit,Credit\n\
        2026-09-01,debit as negative,-12.50,\n\
        2026-09-02,credit only,,100.00\n\
        2026-09-03,both empty,,\n\
        2026-09-04,both zero,0.00,0.00\n\
        2026-09-05,both filled,10.10,10.40\n\
        2026-09-06,debit garbage,x,\n";
    let result = import(&mut conn, content, &config);
    assert_eq!(result.imported_count, 3);
    assert_eq!(result.skipped_zero, 1);
    assert_eq!(result.error_count, 2);
    assert_eq!(result.errors[0].key, "validation.amountRequired");
    assert_eq!(result.errors[1].key, "csvImport.amountUnparseable");
    let rows = stored(
        &conn,
        "SELECT description, tx_type, amount, NULL FROM bank_transactions ORDER BY booking_date",
    );
    assert_eq!(
        rows[0],
        (
            "debit as negative".into(),
            "debit".into(),
            "12.5".into(),
            None
        )
    );
    assert_eq!(
        rows[1],
        ("credit only".into(), "credit".into(), "100".into(), None)
    );
    assert_eq!(
        rows[2],
        ("both filled".into(), "credit".into(), "0.3".into(), None),
        "no float noise"
    );

    // An amount column wins over a pair; without either it is a validation error.
    let mut both = config.clone();
    both.amount_column = Some("Debit".into());
    assert!(both.validate().is_ok());
    let mut neither = config;
    neither.debit_column = None;
    assert!(matches!(
        neither.validate(),
        Err(AppError::Validation(k)) if k == "validation.amountColumnRequired"
    ));
}

#[test]
fn row_filter_equals_is_case_and_space_insensitive_and_needs_its_column() {
    let content = "Date,Description,Amount,State\n2026-09-01,a,-1,completed \n2026-09-02,b,-2,PENDING\n2026-09-03,c,-3,\n";
    let mut config = CsvImportConfig {
        delimiter: ",".into(),
        date_column: "Date".into(),
        date_format: "%Y-%m-%d".into(),
        amount_column: Some("Amount".into()),
        description_columns: Some(vec!["Description".into()]),
        row_filter: Some(CsvRowFilter {
            column: "state".into(),
            equals: " COMPLETED".into(),
        }),
        ..Default::default()
    };
    let mut conn = setup_import_db();
    let result = import(&mut conn, content, &config);
    assert_eq!((result.imported_count, result.skipped_filtered), (1, 2));

    config.row_filter = Some(CsvRowFilter {
        column: "Nope".into(),
        equals: "x".into(),
    });
    let err = import_transactions(&mut conn, "acc-1", "t.csv", content, &config, None).unwrap_err();
    assert!(matches!(err, AppError::Validation(k) if k == "validation.csvColumnsNotFound"));
}

#[test]
fn auto_date_format_and_manual_header_row_and_skip_rows() {
    // "auto" detects the format from the column; a manual header row and
    // skipped rows address the file's physical lines.
    let content = "Report;generated;by bank\nTotal;rows;3\nDatum;Částka;Popis\nskip me;1;x\n15.01.2024;-5;a\n25.01.2024;-6;b\n";
    let mut config = basic_config();
    config.date_format = "auto".into();
    config.header_row = Some(2);
    config.skip_rows = 1;
    let mut conn = setup_import_db();
    let result = import(&mut conn, content, &config);
    assert_eq!(result.imported_count, 2);
    assert_eq!(result.error_count, 0);

    // The same file, header auto-detected, no skip: line 4 is a date error.
    config.header_row = None;
    config.skip_rows = 0;
    let mut conn = setup_import_db();
    let result = import(&mut conn, content, &config);
    assert_eq!(result.imported_count, 2);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].line, 4);
}

#[test]
fn column_names_resolve_case_and_diacritics_insensitively() {
    let mut conn = setup_import_db();
    let content = "DATUM;CASTKA;POPIS\n15.01.2024;-5;a\n";
    let result = import(&mut conn, content, &basic_config());
    assert_eq!(result.imported_count, 1);
}

#[test]
fn legacy_payload_keeps_working() {
    // The pre-v2 dialog sends neither the new fields nor a header row.
    let config: CsvImportConfig = serde_json::from_str(
        r#"{"delimiter":";","skipRows":0,"dateColumn":"Datum","dateFormat":"%d.%m.%Y",
        "amountColumn":"Částka","descriptionColumns":["Popis"],"counterpartyColumn":null,
        "counterpartyIbanColumn":null,"currencyColumn":null}"#,
    )
    .unwrap();
    let mut conn = setup_import_db();
    let content = "Datum;Částka;Popis\n15.01.2024;-250,50;groceries\n16.01.2024;1 000,00;salary\n";
    let result = import(&mut conn, content, &config);
    assert_eq!(result.imported_count, 2);
}

// ======================================================================
// Write path behaviour kept from before v2
// ======================================================================

#[test]
fn import_writes_through_service_with_batch_metadata() {
    let mut conn = setup_import_db();
    let content = "Datum;Částka;Popis\n15.01.2024;-250,50;groceries\n16.01.2024;1 000,00;salary\n";
    let result = import(&mut conn, content, &basic_config());

    assert_eq!(result.imported_count, 2);
    assert_eq!(result.duplicate_count, 0);
    assert_eq!(result.error_count, 0);
    assert!(result.errors.is_empty() && result.duplicates.is_empty());

    // Rows carry the import origin and NO fabricated bank-side id.
    let (tx_type, amount, currency, source, batch_id, ext_id): (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT tx_type, amount, currency, data_source, import_batch_id, transaction_id
             FROM bank_transactions WHERE description = 'groceries'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(tx_type, "debit");
    assert_eq!(amount, "250.5");
    assert_eq!(currency, "CZK");
    assert_eq!(source, "csv_import");
    assert!(batch_id.is_some(), "rows must be linked to the batch");
    assert!(
        ext_id.is_none(),
        "no random UUID in the bank-side id column"
    );

    let (imported, duplicates, errs): (i64, i64, i64) = conn
        .query_row(
            "SELECT imported_count, duplicate_count, error_count FROM csv_import_batches WHERE id = ?1",
            [batch_id.unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((imported, duplicates, errs), (2, 0, 0));

    // Without an engine nothing is auto-categorized.
    let (category, provenance): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT category_id, categorization_source FROM bank_transactions WHERE description = 'groceries'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((category, provenance), (None, None));
}

#[test]
fn import_auto_applies_rules_with_provenance_and_counts_uncategorized() {
    use crate::services::categorization::{CategorizationRule, RuleType};

    let mut conn = setup_import_db();
    let engine = CategorizationEngine::new(vec![CategorizationRule::new(
        "r1".into(),
        "Albert".into(),
        RuleType::Contains,
        "albert".into(),
        "cat-1".into(),
    )]);
    let content = "Datum;Částka;Popis\n15.01.2024;-250,50;Albert Hypermarket\n16.01.2024;-99,00;Unknown merchant\n";
    let result = import_transactions(
        &mut conn,
        "acc-1",
        "test.csv",
        content,
        &basic_config(),
        Some(&engine),
    )
    .unwrap();
    assert_eq!(result.imported_count, 2);
    assert_eq!(
        result.uncategorized_count, 1,
        "only the unknown merchant is left"
    );

    let rows: Vec<(String, Option<String>, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT description, category_id, categorization_source FROM bank_transactions
                 ORDER BY booking_date",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(
        rows,
        vec![
            (
                "Albert Hypermarket".to_string(),
                Some("cat-1".to_string()),
                Some("rule".to_string())
            ),
            ("Unknown merchant".to_string(), None, None),
        ],
        "the UI CSV import inherits rules-first categorization (spec D2)"
    );
}

#[test]
fn identical_rows_in_one_file_import_once_unless_forced() {
    // Two genuinely separate 99 CZK coffees look identical under the
    // composite rule; the second is skipped by default and can be forced in.
    let content = "Datum;Částka;Popis\n15.01.2024;-99;coffee\n15.01.2024;-99;coffee\n";
    let mut conn = setup_import_db();
    let result = import(&mut conn, content, &basic_config());
    assert_eq!(result.imported_count, 1);
    assert_eq!(result.skipped_duplicates.len(), 1);
    assert_eq!(result.skipped_duplicates[0].line, 3);
    assert_eq!(tx_count(&conn), 1);

    let mut forced = basic_config();
    forced.import_anyway_lines = vec![3];
    let mut conn = setup_import_db();
    let result = import(&mut conn, content, &forced);
    assert_eq!(result.imported_count, 2);
    assert_eq!(result.duplicates.len(), 1);
    assert_eq!(tx_count(&conn), 2);
}

#[test]
fn import_falls_back_to_account_currency_and_uses_the_currency_column() {
    let content = "Datum;Částka;Popis\n15.01.2024;-10;lunch\n";
    let mut conn = setup_import_db();
    import_transactions(
        &mut conn,
        "acc-eur",
        "t.csv",
        content,
        &basic_config(),
        None,
    )
    .unwrap();
    let currency: String = conn
        .query_row("SELECT currency FROM bank_transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(currency, "EUR", "no currency column → account currency");

    let mut config = basic_config();
    config.currency_column = Some("Měna".into());
    let content = "Datum;Částka;Popis;Měna\n15.01.2024;-10;lunch;usd\n";
    let mut conn = setup_import_db();
    import(&mut conn, content, &config);
    let currency: String = conn
        .query_row("SELECT currency FROM bank_transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(currency, "USD", "currency is trimmed and upper-cased");
}

#[test]
fn unknown_currency_codes_are_row_errors_while_empty_cells_use_the_account_currency() {
    // "XYZ" used to import unchecked while a malformed code failed.
    let mut config = basic_config();
    config.currency_column = Some("Měna".into());
    // Line 2: not an ISO code. Line 3: valid, lower-case and padded. Line 4: empty
    // cell → account currency. Line 5: a fund code, not a currency. Line 6: malformed.
    let content = "Datum;Částka;Popis;Měna\n15.01.2024;-10;unknown;XYZ\n16.01.2024;-20;padded;  eur \n17.01.2024;-30;empty;\n18.01.2024;-40;metal;XAU\n19.01.2024;-5;malformed;C1\n";

    let mut conn = setup_import_db();
    let result = import(&mut conn, content, &config);
    assert_eq!((result.imported_count, result.error_count), (2, 3));
    let errors: Vec<(usize, &str, Option<&str>)> = result
        .errors
        .iter()
        .map(|m| (m.line, m.key.as_str(), m.detail.as_deref()))
        .collect();
    assert_eq!(
        errors,
        vec![
            (2, "validation.currencyUnknown", Some("XYZ")),
            (5, "validation.currencyUnknown", Some("XAU")),
            (6, "validation.currencyInvalid", None),
        ]
    );
    result.errors.iter().for_each(assert_i18n_key);
    let rows = stored(
        &conn,
        "SELECT description, currency, amount, NULL FROM bank_transactions ORDER BY booking_date",
    );
    assert_eq!(
        rows.iter()
            .map(|r| (r.0.as_str(), r.1.as_str()))
            .collect::<Vec<_>>(),
        vec![("padded", "EUR"), ("empty", "CZK")],
        "valid codes are normalised, an empty cell falls back to the account currency"
    );

    // The preview reports the same rows without writing anything.
    let conn = setup_import_db();
    let preview = preview_import(&conn, "acc-1", content, &config, None).unwrap();
    let statuses: Vec<CsvRowStatus> = preview.rows.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        vec![
            CsvRowStatus::Error,
            CsvRowStatus::Ok,
            CsvRowStatus::Ok,
            CsvRowStatus::Error,
            CsvRowStatus::Error
        ]
    );
    assert_eq!(preview.rows[0].currency.as_deref(), Some("XYZ"));
    assert_eq!(tx_count(&conn), 0);
}

#[test]
fn rejected_config_and_unknown_account_write_nothing() {
    let content = "Datum;Částka;Popis\n15.01.2024;-10;lunch\n";
    let mut conn = setup_import_db();

    let mut config = basic_config();
    config.date_column = "Nonexistent".into();
    let err = import_transactions(&mut conn, "acc-1", "t.csv", content, &config, None).unwrap_err();
    assert!(matches!(err, AppError::Validation(k) if k == "validation.csvColumnsNotFound"));

    let mut config = basic_config();
    config.amount_column = None;
    let err = import_transactions(&mut conn, "acc-1", "t.csv", content, &config, None).unwrap_err();
    assert!(matches!(err, AppError::Validation(k) if k == "validation.amountColumnRequired"));

    let err = import_transactions(&mut conn, "ghost", "t.csv", content, &basic_config(), None)
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));

    let err =
        import_transactions(&mut conn, "acc-1", "t.csv", "", &basic_config(), None).unwrap_err();
    assert!(matches!(err, AppError::Validation(k) if k == "validation.csvEmptyFile"));

    assert_eq!(tx_count(&conn), 0);
    let batches: i64 = conn
        .query_row("SELECT count(*) FROM csv_import_batches", [], |r| r.get(0))
        .unwrap();
    assert_eq!(batches, 0, "no batch record for a rejected import");
}

// ======================================================================
// Post-import hints
// ======================================================================

#[test]
fn last_balance_follows_the_latest_date_in_either_file_order() {
    fn balance_of(content: &str) -> Option<String> {
        let config = CsvImportConfig {
            delimiter: ",".into(),
            date_column: "Date".into(),
            date_format: "%Y-%m-%d".into(),
            amount_column: Some("Amount".into()),
            balance_column: Some("Balance".into()),
            ..Default::default()
        };
        let mut conn = setup_import_db();
        import(&mut conn, content, &config).last_balance
    }
    // Oldest first (Revolut style): the last row is the latest.
    assert_eq!(
        balance_of("Date,Amount,Balance\n2026-09-01,-1,99\n2026-09-02,-1,98\n").as_deref(),
        Some("98")
    );
    // Newest first (Chase style): the first row is the latest.
    assert_eq!(
        balance_of("Date,Amount,Balance\n2026-09-02,-1,98.5\n2026-09-01,-1,99.5\n").as_deref(),
        Some("98.5")
    );
    // Same day, newest-first file: the first row of that day came last.
    assert_eq!(
        balance_of("Date,Amount,Balance\n2026-09-02,-1,10\n2026-09-02,-2,11\n2026-09-01,-1,12\n")
            .as_deref(),
        Some("10")
    );
    // Same day, oldest-first file: the last row of that day.
    assert_eq!(
        balance_of("Date,Amount,Balance\n2026-09-01,-1,12\n2026-09-02,-1,11\n2026-09-02,-2,10\n")
            .as_deref(),
        Some("10")
    );
    // Negative balances keep their sign; no balance cell → None.
    assert_eq!(
        balance_of("Date,Amount,Balance\n2026-09-01,-1,-5.25\n").as_deref(),
        Some("-5.25")
    );
    assert_eq!(balance_of("Date,Amount,Balance\n2026-09-01,-1,\n"), None);
}

#[test]
fn suggested_rule_pack_needs_a_disabled_existing_pack() {
    let conn = setup_import_db();
    let ibans = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let de = ibans(&[
        "DE89 3704 0044 0532 0130 00",
        "de02120300000000202051",
        "CZ6508000000192000145399",
    ]);

    // Most frequent country wins; Germany's pack exists and is off by default.
    assert_eq!(suggest_rule_pack(&conn, &de).as_deref(), Some("de"));

    // Enabled already → no hint.
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES ('rule_packs_enabled', '[\"global\",\"de\"]')",
        [],
    )
    .unwrap();
    assert_eq!(suggest_rule_pack(&conn, &de), None);

    // Account numbers that are not IBANs, no IBANs, or a country without a pack.
    assert_eq!(
        suggest_rule_pack(&conn, &ibans(&["123456789/0800", "", "x"])),
        None
    );
    assert_eq!(suggest_rule_pack(&conn, &[]), None);
    assert_eq!(
        suggest_rule_pack(&conn, &ibans(&["XX89370400440532013000"])),
        None
    );
    // Ties resolve alphabetically.
    let tie = ibans(&[
        "PL61109010140000071219812874",
        "FR1420041010050500013M02606",
    ]);
    assert_eq!(suggest_rule_pack(&conn, &tie).as_deref(), Some("fr"));
}

#[test]
fn iban_country_recognises_ibans_only() {
    assert_eq!(
        iban_country("DE89 3704 0044 0532 0130 00").as_deref(),
        Some("DE")
    );
    assert_eq!(
        iban_country("cz6508000000192000145399").as_deref(),
        Some("CZ")
    );
    assert_eq!(iban_country("123456789/0800"), None);
    assert_eq!(iban_country("US12345678"), None, "too short for an IBAN");
    assert_eq!(iban_country(""), None);
}

#[test]
fn duplicate_hits_are_classified_by_rule() {
    // dedup reports the id rule with "bank-side id" in its reason; the CSV
    // layer relies on it to pick the i18n key. If dedup's wording changes this
    // test fails instead of silently relabelling every id duplicate.
    let conn = setup_import_db();
    conn.execute(
        "INSERT INTO bank_transactions (id, bank_account_id, transaction_id, tx_type, amount, currency, booking_date)
         VALUES ('t1', 'acc-1', 'X1', 'debit', '5', 'CZK', 1700000000)",
        [],
    )
    .unwrap();
    let mut insert = InsertBankTransaction {
        bank_account_id: "acc-1".into(),
        transaction_id: Some("X1".into()),
        tx_type: "debit".into(),
        amount: "9".into(),
        currency: None,
        description: None,
        counterparty_name: None,
        counterparty_iban: None,
        booking_date: 1700086400,
        value_date: None,
        category_id: None,
        status: None,
    };
    let checker = DuplicateChecker::default();
    assert_eq!(
        checker.check(&conn, "acc-1", &insert).unwrap(),
        Some(DuplicateKind::BankId)
    );
    insert.transaction_id = None;
    insert.booking_date = 1700000000;
    insert.amount = "5".into();
    assert_eq!(
        checker.check(&conn, "acc-1", &insert).unwrap(),
        Some(DuplicateKind::Composite)
    );
    insert.amount = "6".into();
    assert_eq!(checker.check(&conn, "acc-1", &insert).unwrap(), None);
}

// ======================================================================
// Inspection details
// ======================================================================

#[test]
fn inspection_honours_manual_overrides() {
    let bytes = fixture_bytes("fio.csv");
    // Wrong header row on purpose: the user's choice wins over detection.
    let r = inspect_csv(
        &bytes,
        &InspectOptions {
            header_row: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.header_row, 0);
    assert_eq!(r.headers, vec!["accountId", "2900000001"]);
    assert!(r.detected_preset_id.is_none());

    // skip_rows skips data rows after the header.
    let r = inspect_csv(
        &fixture_bytes("generic-iso.csv"),
        &InspectOptions {
            skip_rows: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.total_rows, 7);
    assert_eq!(r.sample_rows.len(), 5);
    assert_eq!(r.sample_rows[0][1], "Netflix");

    // An explicit delimiter is used as given.
    let r = inspect_csv(
        &fixture_bytes("generic-iso.csv"),
        &InspectOptions {
            delimiter: Some(';'),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.delimiter, ";");
    assert_eq!(r.headers.len(), 1);

    // An empty file is a validation error with an i18n key.
    let err = inspect_csv(b"", &InspectOptions::default()).unwrap_err();
    assert!(matches!(err, AppError::Validation(k) if k == "validation.csvEmptyFile"));
}

#[test]
fn a_user_picked_preset_overrides_detection() {
    // generic-iso has no preset of its own; picking "Revolut" cannot apply (its
    // columns are missing), so nothing is claimed.
    let r = inspect_csv(
        &fixture_bytes("generic-iso.csv"),
        &InspectOptions {
            preset_id: Some("revolut"),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.detected_preset_id.is_none());
    // Picking the right one works, and an unknown id falls back to detection.
    let r = inspect_csv(
        &fixture_bytes("revolut.csv"),
        &InspectOptions {
            preset_id: Some("revolut"),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.detected_preset_id.as_deref(), Some("revolut"));
    let r = inspect_csv(
        &fixture_bytes("revolut.csv"),
        &InspectOptions {
            preset_id: Some("no-such-bank"),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.detected_preset_id.as_deref(), Some("revolut"));
}

#[test]
fn ambiguous_dates_are_reported_without_a_preset() {
    let content = "Date,Description,Amount\n01/02/2026,a,1\n03/04/2026,b,2\n";
    let r = inspect_csv(content.as_bytes(), &InspectOptions::default()).unwrap();
    assert_eq!(r.date_format, "%d/%m/%Y");
    assert!(r.date_format_ambiguous);
}

#[test]
fn two_digit_year_dates_are_flagged_unless_a_value_decides_the_order() {
    // "24-01-15" is 2015-01-24 or 2024-01-15 — the preview must say
    // the order is a guess; a value above 31 / 12 settles it.
    let flagged = "Date,Description,Amount\n24-01-15,a,1\n25-02-16,b,2\n";
    let r = inspect_csv(flagged.as_bytes(), &InspectOptions::default()).unwrap();
    assert_eq!(r.date_format, "%d-%m-%y");
    assert!(r.date_format_ambiguous);

    let year_first = "Date,Description,Amount\n99-12-31,a,1\n98-11-30,b,2\n";
    let r = inspect_csv(year_first.as_bytes(), &InspectOptions::default()).unwrap();
    assert_eq!(r.date_format, "%y-%m-%d");
    assert!(!r.date_format_ambiguous);

    // The confirmed format imports with the intended meaning.
    let mut conn = setup_import_db();
    let mut config = basic_config();
    config.delimiter = ",".into();
    config.date_column = "Date".into();
    config.date_format = "%y-%m-%d".into();
    config.amount_column = Some("Amount".into());
    config.description_columns = Some(vec!["Description".into()]);
    let result = import(&mut conn, flagged, &config);
    assert_eq!(result.imported_count, 2);
    let first: i64 = conn
        .query_row("SELECT MIN(booking_date) FROM bank_transactions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(first, 1_705_276_800, "2024-01-15, not 2015-01-24");
}

#[test]
fn a_repeating_id_column_is_not_suggested_as_transaction_id() {
    let content = "Transaction ID,Date,Amount,Description\nA,2026-09-01,-1,x\nA,2026-09-02,-2,y\n";
    let r = inspect_csv(content.as_bytes(), &InspectOptions::default()).unwrap();
    assert!(mapped(&r, "transactionId").is_none());
    let unique = "Transaction ID,Date,Amount,Description\nA,2026-09-01,-1,x\nB,2026-09-02,-2,y\n";
    let r = inspect_csv(unique.as_bytes(), &InspectOptions::default()).unwrap();
    assert_eq!(
        mapped(&r, "transactionId").as_deref(),
        Some("Transaction ID")
    );
}

#[test]
fn windows_1250_file_is_decoded_with_the_czech_preset_encoding() {
    // ČS exports in Windows-1250 are not valid UTF-8; the preset's encoding
    // applies and the diacritics survive.
    let text = "Datum zaúčtování;Částka;Měna;Název protiúčtu;Číslo protiúčtu;Zpráva pro příjemce;Variabilní symbol;Typ transakce\n01.09.2026;-89,00;CZK;Příjemce Žluťoučký;;Nákup;;Platba kartou\n";
    let (bytes, _, _) = encoding_rs::WINDOWS_1250.encode(text);
    assert!(std::str::from_utf8(&bytes).is_err());
    let r = inspect_csv(&bytes, &InspectOptions::default()).unwrap();
    assert_eq!(r.encoding, "windows-1250");
    assert_eq!(r.detected_preset_id.as_deref(), Some("ceska-sporitelna"));
    assert_eq!(r.sample_rows[0][3], "Příjemce Žluťoučký");
}

#[test]
fn csv_preset_wire_format_is_camel_case_with_nulls() {
    let preset = crate::services::csv_presets::get_preset("revolut").unwrap();
    let json = serde_json::to_value(preset).unwrap();
    assert_eq!(json["matchHeaders"][0], "Completed Date");
    assert_eq!(json["dateFormat"], "%Y-%m-%d %H:%M:%S");
    assert_eq!(json["rowFilter"]["column"], "State");
    assert!(json["debitColumn"].is_null());
    assert!(json["counterpartyColumn"].is_null());
    assert_eq!(json["sampleFixture"], "revolut.csv");
}
