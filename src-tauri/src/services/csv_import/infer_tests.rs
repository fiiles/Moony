//! Tests of the content-based column inference (`infer.rs`): the shipped presets' sample files
//! must infer the columns their preset names, and synthetic files pin each rule.

use super::infer::{infer_columns, infer_csv, InferOptions};
use super::inspect::{inspect_csv, InspectOptions};
use super::test_support::fixture_bytes;
use super::types::{InferredColumns, InferredField, SignCheck};
use crate::services::csv_presets::{all_presets, resolve_preset};

/// Headers and rows of a `;`-separated table written inline (no quoting needed).
fn grid(lines: &[&str]) -> (Vec<String>, Vec<Vec<String>>) {
    let split = |line: &str| line.split(';').map(str::to_string).collect::<Vec<String>>();
    let headers = split(lines[0]);
    let rows = lines[1..].iter().map(|line| split(line)).collect();
    (headers, rows)
}

fn infer(lines: &[&str]) -> InferredColumns {
    let (headers, rows) = grid(lines);
    infer_columns(&headers, &rows)
}

fn column(field: &Option<InferredField>) -> Option<&str> {
    field.as_ref().map(|f| f.column.as_str())
}

fn nothing() -> InferredColumns {
    InferredColumns {
        date: None,
        date_format: None,
        date_ambiguous: false,
        amount: None,
        debit: None,
        credit: None,
        balance: None,
        sign_check: SignCheck::Unchecked,
        currency: None,
        counterparty_iban: None,
        counterparty: None,
        transaction_id: None,
        description: Vec::new(),
        decimal_separator: ".".to_string(),
    }
}

// ---------------------------------------------------------------------------
// The presets' own sample files: the inference must find the date and the amount (or the
// debit + credit pair) the preset names, from the content, with the settings the inspection
// reports (what the phone passes back to `infer_csv_columns`).
// ---------------------------------------------------------------------------

/// `Err` with the reason when the inference disagrees with the preset on `fixture`.
fn preset_disagreement(fixture: &str) -> Result<(), String> {
    let preset = all_presets()
        .iter()
        .find(|p| p.sample_fixture.as_deref() == Some(fixture))
        .ok_or_else(|| format!("no preset reads {fixture}"))?;
    let bytes = fixture_bytes(fixture);
    let inspection = inspect_csv(&bytes, &InspectOptions::default())
        .map_err(|e| format!("{fixture}: inspection failed: {e}"))?;
    let inferred = infer_csv(
        &bytes,
        &InferOptions {
            delimiter: inspection.delimiter.chars().next(),
            header_row: Some(inspection.header_row),
            encoding: Some(&inspection.encoding),
            country: None,
        },
    );
    let resolved = resolve_preset(preset, &inspection.headers)
        .ok_or_else(|| format!("{fixture}: the preset does not resolve"))?;

    let mut problems = Vec::new();
    let mut check = |role: &str, got: Option<&str>, want: Option<&str>| {
        if got != want {
            problems.push(format!("{role}: inferred {got:?}, preset {want:?}"));
        }
    };
    check("date", column(&inferred.date), Some(resolved.date.as_str()));
    check(
        "amount",
        column(&inferred.amount),
        resolved.amount.as_deref(),
    );
    check("debit", column(&inferred.debit), resolved.debit.as_deref());
    check(
        "credit",
        column(&inferred.credit),
        resolved.credit.as_deref(),
    );
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{fixture} ({}): {}",
            preset.id,
            problems.join("; ")
        ))
    }
}

fn assert_infers_the_preset_columns(fixture: &str) {
    if let Err(problem) = preset_disagreement(fixture) {
        panic!("{problem}");
    }
}

#[test]
fn infers_the_preset_columns_of_every_preset_fixture() {
    let fixtures: Vec<&str> = all_presets()
        .iter()
        .filter_map(|p| p.sample_fixture.as_deref())
        .collect();
    assert!(
        fixtures.len() >= 8,
        "the presets' sample files: {fixtures:?}"
    );
    let problems: Vec<String> = fixtures
        .iter()
        .filter_map(|f| preset_disagreement(f).err())
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn infers_the_preset_columns_fio() {
    assert_infers_the_preset_columns("fio.csv");
}

#[test]
fn infers_the_preset_columns_ceska_sporitelna() {
    assert_infers_the_preset_columns("ceska-sporitelna.csv");
}

#[test]
fn infers_the_preset_columns_revolut() {
    assert_infers_the_preset_columns("revolut.csv");
}

#[test]
fn infers_the_preset_columns_wise() {
    assert_infers_the_preset_columns("wise.csv");
}

#[test]
fn infers_the_preset_columns_n26() {
    assert_infers_the_preset_columns("n26.csv");
}

#[test]
fn infers_the_preset_columns_de_sparkasse() {
    assert_infers_the_preset_columns("de-sparkasse.csv");
}

#[test]
fn infers_the_preset_columns_us_chase() {
    assert_infers_the_preset_columns("us-chase.csv");
}

#[test]
fn infers_the_preset_columns_uk_debit_credit() {
    assert_infers_the_preset_columns("uk-debit-credit.csv");
}

#[test]
fn the_preset_fixtures_also_prove_balances_ids_and_accounts() {
    let infer_fixture = |name: &str| infer_csv(&fixture_bytes(name), &InferOptions::default());

    // Chase and Wise print a running balance that agrees with the amounts.
    let chase = infer_fixture("us-chase.csv");
    assert_eq!(column(&chase.balance), Some("Balance"));
    assert_eq!(chase.sign_check, SignCheck::Verified);
    assert_eq!(chase.date_format.as_deref(), Some("%m/%d/%Y"));
    assert!(!chase.date_ambiguous, "09/13/2026 decides month-first");

    let wise = infer_fixture("wise.csv");
    assert_eq!(column(&wise.balance), Some("Running Balance"));
    assert_eq!(wise.sign_check, SignCheck::Verified);
    assert_eq!(column(&wise.transaction_id), Some("TransferWise ID"));
    assert_eq!(column(&wise.currency), Some("Currency"));

    // Revolut's balance also moves by the fee on one row: still 5 of 6 pairs.
    let revolut = infer_fixture("revolut.csv");
    assert_eq!(column(&revolut.balance), Some("Balance"));
    assert_eq!(revolut.sign_check, SignCheck::Verified);

    // The UK pair: the balance proves which side is the debit.
    let uk = infer_fixture("uk-debit-credit.csv");
    assert_eq!(column(&uk.balance), Some("Balance"));
    assert_eq!(uk.sign_check, SignCheck::Verified);

    // Fio: the operation id is unique and not a row counter.
    let fio = infer_fixture("fio.csv");
    assert_eq!(column(&fio.transaction_id), Some("ID operace"));
    assert_eq!(column(&fio.currency), Some("Měna"));
    assert_eq!(fio.decimal_separator, ",");

    // Česká spořitelna: Czech domestic account numbers are account identifiers.
    let cs = infer_fixture("ceska-sporitelna.csv");
    assert_eq!(column(&cs.counterparty_iban), Some("Číslo protiúčtu"));
    assert_eq!(cs.decimal_separator, ",");

    // Sparkasse: the statement's own IBAN repeats on every row; the counterparty's varies.
    let sparkasse = infer_fixture("de-sparkasse.csv");
    assert_eq!(
        column(&sparkasse.counterparty_iban),
        Some("Kontonummer/IBAN")
    );
    assert_eq!(sparkasse.decimal_separator, ",");
    assert!(sparkasse.date_ambiguous, "01.09.26 reads in every order");
}

// ---------------------------------------------------------------------------
// Synthetic files, one rule each.
// ---------------------------------------------------------------------------

/// Review Focus 4: an unknown Czech export read on a phone set to any language. Decimal comma,
/// NBSP thousands, U+2212 minus, dd.mm.yyyy — and headers no synonym knows.
#[test]
fn infers_czech_comma_amounts_with_nbsp_thousands() {
    let inferred = infer(&[
        "Sloupec 1;Sloupec 2;Sloupec 3;Sloupec 4;Sloupec 5",
        "02.09.2026;\u{2212}1\u{a0}249,00;CZK;Albert;Nákup potravin Albert Praha 4",
        "05.09.2026;45\u{a0}000,00;CZK;Zaměstnavatel s.r.o.;Mzda za srpen 2026",
        "13.09.2026;\u{2212}89,00;CZK;Spotify;Předplatné Spotify Premium",
        "14.09.2026;\u{2212}15\u{a0}500,00;CZK;SVJ Dlouhá;Nájem a zálohy na září",
        "20.09.2026;\u{2212}1\u{a0}249,00;CZK;Albert;Nákup potravin Albert Praha 4, druhý",
        "21.09.2026;2\u{a0}300,50;CZK;Jan Novák;Vrácení za lístky do divadla",
    ]);

    assert_eq!(column(&inferred.date), Some("Sloupec 1"));
    assert_eq!(inferred.date_format.as_deref(), Some("%d.%m.%Y"));
    assert!(!inferred.date_ambiguous, "13.09. decides day-first");
    assert_eq!(column(&inferred.amount), Some("Sloupec 2"));
    assert_eq!(inferred.decimal_separator, ",");
    assert_eq!(column(&inferred.currency), Some("Sloupec 3"));
    assert_eq!(column(&inferred.counterparty), Some("Sloupec 4"));
    assert_eq!(inferred.description, vec!["Sloupec 5".to_string()]);
    assert!(inferred.debit.is_none() && inferred.credit.is_none());
    assert_eq!(inferred.sign_check, SignCheck::Unchecked);
    // Only the content decided: every cell matched, 0.1 off for the missing synonym.
    let date = inferred.date.as_ref().expect("date");
    let amount = inferred.amount.as_ref().expect("amount");
    assert!((date.confidence - 0.9).abs() < 1e-6, "{}", date.confidence);
    assert!(
        (amount.confidence - 0.9).abs() < 1e-6,
        "{}",
        amount.confidence
    );
}

#[test]
fn a_header_synonym_keeps_the_full_confidence() {
    let inferred = infer(&[
        "Datum;Částka;Popis",
        "02.09.2026;-100,00;Kafe a croissant",
        "15.09.2026;2 000,00;Vrácená záloha",
    ]);
    assert_eq!(inferred.date.as_ref().map(|f| f.confidence), Some(1.0));
    assert_eq!(inferred.amount.as_ref().map(|f| f.confidence), Some(1.0));
}

#[test]
fn balance_proves_the_sign_newest_first() {
    let inferred = infer(&[
        "A;B;C;D",
        "30.09.2026;Kafe;-50,00;1 150,00",
        "20.09.2026;Mzda;1 000,00;1 200,00",
        "10.09.2026;Nájem;-300,00;200,00",
        "01.09.2026;Nákup;-500,00;500,00",
    ]);
    assert_eq!(column(&inferred.date), Some("A"));
    assert_eq!(column(&inferred.amount), Some("C"));
    assert_eq!(column(&inferred.balance), Some("D"));
    assert_eq!(inferred.sign_check, SignCheck::Verified);
}

#[test]
fn balance_shows_inverted_signs() {
    // A card statement printing expenses positive: the balance falls when the amount rises.
    let inferred = infer(&[
        "A;B;C;D",
        "01.09.2026;Vklad;-1 000,00;1 000,00",
        "02.09.2026;Kafe;100,00;900,00",
        "03.09.2026;Vratka;-50,00;950,00",
        "04.09.2026;Nájem;300,00;650,00",
    ]);
    assert_eq!(column(&inferred.amount), Some("C"));
    assert_eq!(column(&inferred.balance), Some("D"));
    assert_eq!(inferred.sign_check, SignCheck::Inverted);
}

#[test]
fn a_balance_that_does_not_agree_is_no_balance() {
    let inferred = infer(&[
        "A;B;C;D",
        "01.09.2026;Kafe;-50.00;17.00",
        "02.09.2026;Mzda;1000.00;4.00",
        "03.09.2026;Nájem;-300.00;99.00",
    ]);
    assert_eq!(column(&inferred.amount), Some("C"));
    assert_eq!(inferred.balance, None);
    assert_eq!(inferred.sign_check, SignCheck::Unchecked);
}

#[test]
fn infers_a_debit_credit_pair() {
    // The money-in column comes first, so "left is the debit" would be wrong: the running
    // balance decides the sides.
    let inferred = infer(&[
        "H1;H2;H3;H4;H5",
        "01/09/2026;SALARY;2850.00;;3420.10",
        "02/09/2026;GAS;;78.50;3341.60",
        "03/09/2026;SHOP;;45.21;3296.39",
        "15/09/2026;SAVINGS;;500.00;2796.39",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(column(&inferred.debit), Some("H4"));
    assert_eq!(column(&inferred.credit), Some("H3"));
    assert_eq!(column(&inferred.balance), Some("H5"));
    assert_eq!(inferred.sign_check, SignCheck::Verified);
    assert_eq!(inferred.date_format.as_deref(), Some("%d/%m/%Y"));
}

#[test]
fn a_proven_pair_beats_an_overdrawn_balance() {
    // The overdrawn balance has both signs, so it alone looks like a signed amount; the pair
    // it proves wins.
    let inferred = infer(&[
        "H1;H2;H3;H4;H5",
        "01.09.2026;Kafe;50,00;;-100,00",
        "02.09.2026;Mzda;;1 000,00;900,00",
        "03.09.2026;Nájem;1 200,00;;-300,00",
        "04.09.2026;Vratka;;100,00;-200,00",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(column(&inferred.debit), Some("H3"));
    assert_eq!(column(&inferred.credit), Some("H4"));
    assert_eq!(column(&inferred.balance), Some("H5"));
    assert_eq!(inferred.sign_check, SignCheck::Verified);
}

#[test]
fn a_pair_without_balance_takes_the_negative_side_as_debit() {
    let inferred = infer(&[
        "H1;H2;H3;H4",
        "01.09.2026;Mzda;45 000,00;",
        "02.09.2026;Nájem;;-15 500,00",
        "03.09.2026;Nákup;;-1 249,00",
        "04.09.2026;Kafe;;-89,00",
    ]);
    assert_eq!(column(&inferred.debit), Some("H4"));
    assert_eq!(column(&inferred.credit), Some("H3"));
    assert_eq!(inferred.balance, None);
    assert_eq!(inferred.sign_check, SignCheck::Unchecked);
}

#[test]
fn flags_ambiguous_day_month() {
    let inferred = infer(&[
        "Datum;Částka;Popis",
        "01.02.2026;-100,00;Kafe",
        "03.04.2026;200,00;Mzda",
        "05.06.2026;-50,00;Nákup",
    ]);
    assert_eq!(column(&inferred.date), Some("Datum"));
    assert_eq!(inferred.date_format.as_deref(), Some("%d.%m.%Y"));
    assert!(inferred.date_ambiguous);
}

#[test]
fn no_date_column_yields_no_date() {
    let inferred = infer(&[
        "Popis;Částka",
        "Kafe;-50,00",
        "Mzda;1 000,00",
        "Nájem;-300,00",
    ]);
    assert_eq!(inferred.date, None);
    assert_eq!(inferred.date_format, None);
    assert!(!inferred.date_ambiguous);
    // What the file does have is still read.
    assert_eq!(column(&inferred.amount), Some("Částka"));
}

#[test]
fn a_mostly_unreadable_date_column_is_no_date() {
    // 8 of 10 values read as dates: below the 90 % bar.
    let mut lines = vec!["Kdy;Částka".to_string()];
    for day in 1..=8 {
        lines.push(format!("{day:02}.09.2026;-1{day},00"));
    }
    lines.push("včera;-5,00".to_string());
    lines.push("dnes;7,00".to_string());
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    assert_eq!(infer(&lines).date, None);
}

#[test]
fn a_row_counter_is_not_a_transaction_id() {
    let counted = infer(&[
        "Nr;Datum;Částka;Popis",
        "1;01.09.2026;-100,00;Kafe",
        "2;02.09.2026;200,00;Mzda",
        "3;03.09.2026;-50,00;Nákup",
        "4;04.09.2026;-75,00;Oběd",
    ]);
    assert_eq!(counted.transaction_id, None);

    // Newest first, counting down to 1: still a counter.
    let reversed = infer(&[
        "Nr;Datum;Částka",
        "3;03.09.2026;-50,00",
        "2;02.09.2026;200,00",
        "1;01.09.2026;-100,00",
    ]);
    assert_eq!(reversed.transaction_id, None);

    // A real id next to the counter is found.
    let with_id = infer(&[
        "Nr;Datum;Částka;Reference;Popis",
        "1;01.09.2026;-100,00;TX-20260901-A1;Kafe",
        "2;02.09.2026;200,00;TX-20260902-B7;Mzda",
        "3;03.09.2026;-50,00;TX-20260903-C2;Nákup",
        "4;04.09.2026;-75,00;TX-20260904-D9;Oběd",
    ]);
    assert_eq!(column(&with_id.transaction_id), Some("Reference"));
}

#[test]
fn a_repeated_value_is_not_a_transaction_id() {
    let inferred = infer(&[
        "Datum;Částka;Kód",
        "01.09.2026;-100,00;A-17",
        "02.09.2026;200,00;A-18",
        "03.09.2026;-50,00;A-17",
    ]);
    assert_eq!(inferred.transaction_id, None);
}

#[test]
fn infers_counterparty_accounts_iban_or_czech_domestic() {
    let inferred = infer(&[
        "Účet;Datum;Částka;Protiúčet;Jiný účet",
        "CZ6508000000192000145399;01.09.2026;-100,00;DE89370400440532013000;19-2000145399/0800",
        "CZ6508000000192000145399;02.09.2026;200,00;GB82 WEST 1234 5698 7654 32;2101234567/2010",
        "CZ6508000000192000145399;03.09.2026;-50,00;;",
    ]);
    // The own account repeats on every row; of the two that vary the header decides.
    assert_eq!(column(&inferred.counterparty_iban), Some("Protiúčet"));

    let czech = infer(&[
        "Datum;Částka;Sloupec 3",
        "01.09.2026;-100,00;2101234567/2010",
        "02.09.2026;200,00;19-2000145399/0800",
        "03.09.2026;-50,00;",
    ]);
    assert_eq!(column(&czech.counterparty_iban), Some("Sloupec 3"));

    // A checksum-invalid IBAN is not an account identifier.
    let invalid = infer(&[
        "Datum;Částka;Sloupec 3",
        "01.09.2026;-100,00;DE89370400440532013001",
        "02.09.2026;200,00;DE89370400440532013002",
    ]);
    assert_eq!(invalid.counterparty_iban, None);
}

#[test]
fn a_currency_column_has_few_iso_codes() {
    let inferred = infer(&[
        "Datum;Částka;Měna;Typ",
        "01.09.2026;-100,00;CZK;POS",
        "02.09.2026;200,00;EUR;ATM",
        "03.09.2026;-50,00;CZK;POS",
    ]);
    // POS/ATM are three capitals but no ISO 4217 codes.
    assert_eq!(column(&inferred.currency), Some("Měna"));
}

#[test]
fn an_empty_file_infers_nothing() {
    assert_eq!(infer_columns(&[], &[]), nothing());
    let headers: Vec<String> = ["Datum", "Částka"].iter().map(|s| s.to_string()).collect();
    assert_eq!(infer_columns(&headers, &[]), nothing());
    assert_eq!(infer_csv(b"", &InferOptions::default()), nothing());
    assert_eq!(
        infer_csv(b"Datum;Castka\n", &InferOptions::default()),
        nothing()
    );
}

#[test]
fn only_the_first_sample_rows_are_read() {
    // 200 good rows, then rows that would break the amount rule: never looked at.
    let mut lines = vec!["Datum;Částka".to_string()];
    for i in 0..super::rows::SAMPLE_ROWS {
        let sign = if i % 2 == 0 { "-" } else { "" };
        lines.push(format!("{:02}.09.2026;{sign}{i},50", i % 28 + 1));
    }
    for _ in 0..500 {
        lines.push("bad;bad".to_string());
    }
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    let inferred = infer(&lines);
    assert_eq!(column(&inferred.date), Some("Datum"));
    assert_eq!(column(&inferred.amount), Some("Částka"));
}

#[test]
fn inferred_columns_serialize_in_camel_case() {
    let field = |c: &str| {
        Some(InferredField {
            column: c.to_string(),
            confidence: 0.9,
        })
    };
    let value = serde_json::to_value(InferredColumns {
        date: field("Datum"),
        date_format: Some("%d.%m.%Y".into()),
        counterparty_iban: field("Protiúčet"),
        transaction_id: field("ID"),
        sign_check: SignCheck::Verified,
        ..nothing()
    })
    .expect("json");
    assert_eq!(value["date"]["column"], "Datum");
    assert_eq!(value["dateFormat"], "%d.%m.%Y");
    assert_eq!(value["dateAmbiguous"], false);
    assert_eq!(value["signCheck"], "verified");
    assert_eq!(value["counterpartyIban"]["column"], "Protiúčet");
    assert_eq!(value["transactionId"]["column"], "ID");
    assert_eq!(value["decimalSeparator"], ".");
    assert!(value["amount"].is_null());
    assert_eq!(
        serde_json::to_value(SignCheck::Inverted).expect("json"),
        "inverted"
    );
    assert_eq!(
        serde_json::to_value(SignCheck::Unchecked).expect("json"),
        "unchecked"
    );
}
