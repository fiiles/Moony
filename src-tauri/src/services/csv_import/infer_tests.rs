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

    // Česká spořitelna: the sample's account numbers (123456789/0800, 987654321/0100) fail the
    // Czech mod-11 check, so they are no account identifiers.
    let cs = infer_fixture("ceska-sporitelna.csv");
    assert_eq!(cs.counterparty_iban, None);
    assert_eq!(cs.decimal_separator, ",");

    // N26: "Original Currency" goes with "Original Amount", not with the account-currency
    // "Amount (EUR)" the import reads.
    let n26 = infer_fixture("n26.csv");
    assert_eq!(n26.currency, None);
    assert_eq!(column(&n26.counterparty_iban), Some("Partner Iban"));

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
        "CZ6508000000192000145399;02.09.2026;200,00;GB82 WEST 1234 5698 7654 32;2101234569/2010",
        "CZ6508000000192000145399;03.09.2026;-50,00;;",
    ]);
    // The own account repeats on every row; of the two that vary the header decides.
    assert_eq!(column(&inferred.counterparty_iban), Some("Protiúčet"));

    let czech = infer(&[
        "Datum;Částka;Sloupec 3",
        "01.09.2026;-100,00;2101234569/2010",
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
fn invoice_numbers_and_periods_are_no_czech_accounts() {
    // `number/four digits` alone is not enough: the Czech mod-11 checksum must hold.
    let inferred = infer(&[
        "Datum;Částka;Faktura;Období",
        "01.09.2026;-100,00;12345/2026;09/2026",
        "02.09.2026;200,00;12346/2026;10/2026",
        "03.09.2026;-50,00;12347/2026;11/2026",
    ]);
    assert_eq!(inferred.counterparty_iban, None);
}

// ---------------------------------------------------------------------------
// Review round 1.
// ---------------------------------------------------------------------------

/// A: an all-expense month (no income row) in a Czech export whose headers no preset knows,
/// newest first, decimal comma with NBSP thousands, a booking and a value date and the
/// statement's own account on every row. The running balance proves the one-signed amounts.
#[test]
fn infers_a_one_signed_amount_the_balance_proves_unicredit_like() {
    let inferred = infer_csv(
        &fixture_bytes("unknown-unicredit-like.csv"),
        &InferOptions::default(),
    );
    assert_eq!(column(&inferred.date), Some("Datum zaúčtování"));
    assert_eq!(column(&inferred.amount), Some("Částka"));
    assert_eq!(column(&inferred.balance), Some("Zůstatek po transakci"));
    assert_eq!(inferred.sign_check, SignCheck::Verified);
    assert_eq!(inferred.decimal_separator, ",");
    assert!(inferred.debit.is_none() && inferred.credit.is_none());
}

/// A: the same month oldest first, `,` and `.`, headers that say nothing.
#[test]
fn infers_a_one_signed_amount_the_balance_proves_opaque_headers() {
    let inferred = infer_csv(
        &fixture_bytes("unknown-opaque-headers.csv"),
        &InferOptions::default(),
    );
    assert_eq!(column(&inferred.date), Some("Pole A"));
    assert_eq!(column(&inferred.amount), Some("Pole C"));
    assert_eq!(column(&inferred.balance), Some("Pole D"));
    assert_eq!(inferred.sign_check, SignCheck::Verified);
    assert_eq!(inferred.decimal_separator, ".");
    // Only the content decided.
    assert!(inferred.amount.as_ref().is_some_and(|f| f.confidence < 1.0));
}

/// A: without a balance a one-signed column is no amount (it may be magnitudes next to a
/// direction word): the person picks it.
#[test]
fn an_unproven_one_signed_column_is_no_amount() {
    let inferred = infer(&[
        "Datum;Částka;Popis",
        "01.09.2026;-1 249,00;Nákup",
        "02.09.2026;-89,00;Spotify",
        "03.09.2026;-15 500,00;Nájem",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(column(&inferred.date), Some("Datum"));
}

/// A: magnitudes beside a direction word. Nine expenses agree with "inverted", but the income
/// row agrees with the other sign: the balance proves no sign convention, so the column is no
/// amount (an "inverted" reading would turn the income negative).
#[test]
fn magnitudes_beside_a_direction_word_are_not_proven() {
    let inferred = infer(&[
        "Datum;Částka;Směr;Zůstatek",
        "01.09.2026;100,00;Výdaj;900,00",
        "02.09.2026;200,00;Výdaj;700,00",
        "03.09.2026;1 000,00;Příjem;1 700,00",
        "04.09.2026;50,00;Výdaj;1 650,00",
        "05.09.2026;25,00;Výdaj;1 625,00",
        "06.09.2026;75,00;Výdaj;1 550,00",
        "07.09.2026;10,00;Výdaj;1 540,00",
        "08.09.2026;40,00;Výdaj;1 500,00",
        "09.09.2026;60,00;Výdaj;1 440,00",
        "10.09.2026;30,00;Výdaj;1 410,00",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(inferred.balance, None);
    assert_eq!(inferred.sign_check, SignCheck::Unchecked);
}

/// The same file overdrawn: the balance now has both signs, which alone reads like a signed
/// amount. A column the header names as the balance is never the amount.
#[test]
fn an_overdrawn_balance_is_no_amount() {
    let inferred = infer(&[
        "Datum;Částka;Směr;Zůstatek",
        "01.09.2026;100,00;Výdaj;-100,00",
        "02.09.2026;200,00;Výdaj;-300,00",
        "03.09.2026;1 000,00;Příjem;700,00",
        "04.09.2026;50,00;Výdaj;650,00",
        "05.09.2026;25,00;Výdaj;625,00",
        "06.09.2026;75,00;Výdaj;550,00",
        "07.09.2026;10,00;Výdaj;540,00",
        "08.09.2026;40,00;Výdaj;500,00",
        "09.09.2026;600,00;Výdaj;-100,00",
        "10.09.2026;30,00;Výdaj;-130,00",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(inferred.debit, None);
    assert_eq!(inferred.credit, None);
    assert_eq!(inferred.balance, None);
    assert_eq!(inferred.sign_check, SignCheck::Unchecked);
    assert_eq!(column(&inferred.date), Some("Datum"));
}

/// B: an all-expense Fio/ČS-like export: the sparse variable symbol is a code, not the credit
/// side of a pair (it once imported as 202609 − 15500).
#[test]
fn an_all_expense_file_with_a_sparse_symbol_infers_no_pair() {
    // One symbol in ten rows: "exactly one side filled" holds on 90 % of the rows.
    let inferred = infer(&[
        "Datum;Objem;VS;KS;Popis",
        "01.09.2026;-1 249,00;;;Nákup",
        "02.09.2026;-15 500,00;202609;0308;Nájem",
        "03.09.2026;-89,00;;;Spotify",
        "04.09.2026;-450,00;;;Oběd",
        "05.09.2026;-1 200,00;;;Pojistka",
        "06.09.2026;-312,00;;;Lékárna",
        "07.09.2026;-75,50;;;Kafe",
        "08.09.2026;-2 400,00;;;Elektřina",
        "09.09.2026;-640,00;;;Benzín",
        "10.09.2026;-128,90;;;Pekárna",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(inferred.debit, None);
    assert_eq!(inferred.credit, None);
}

/// B: Amount | Fee | Balance with expenses only: a fee row has both sides filled, so the
/// columns are no pair; the balance (which moves by amount + fee) still proves the amount on
/// the rows without a fee.
#[test]
fn an_all_expense_amount_fee_balance_file_infers_no_pair() {
    // One fee in ten rows: the old rule paired Amount with Fee and "verified" it.
    let inferred = infer(&[
        "Date;Amount;Fee;Balance",
        "01/09/2026;-10.00;0.00;990.00",
        "02/09/2026;-20.00;0.00;970.00",
        "03/09/2026;-30.00;-1.50;938.50",
        "04/09/2026;-40.00;0.00;898.50",
        "05/09/2026;-50.00;0.00;848.50",
        "06/09/2026;-60.00;0.00;788.50",
        "07/09/2026;-70.00;0.00;718.50",
        "08/09/2026;-80.00;0.00;638.50",
        "09/09/2026;-90.00;0.00;548.50",
        "10/09/2026;-100.00;0.00;448.50",
    ]);
    assert_eq!(inferred.debit, None);
    assert_eq!(inferred.credit, None);
    assert_eq!(column(&inferred.amount), Some("Amount"));
    assert_eq!(column(&inferred.balance), Some("Balance"));
    assert_eq!(inferred.sign_check, SignCheck::Verified);
}

/// C: a "balance before" column in an oldest-first file. Read newest first it would look like
/// an inverted balance; the dates say the file is oldest first, where it agrees with nothing.
#[test]
fn the_dates_decide_the_order_a_balance_is_read_in() {
    let single = infer(&[
        "Datum;Popis;Částka;Zůstatek před",
        "01.09.2026;Kafe;-50,00;1 000,00",
        "02.09.2026;Mzda;1 000,00;950,00",
        "03.09.2026;Nájem;-300,00;1 950,00",
        "04.09.2026;Nákup;-120,00;1 650,00",
    ]);
    assert_eq!(column(&single.amount), Some("Částka"));
    assert_eq!(single.balance, None);
    assert_eq!(single.sign_check, SignCheck::Unchecked);

    // For a pair the wrong order would also have swapped debit and credit.
    let pair = infer(&[
        "Datum;Popis;Debit;Credit;Zůstatek před",
        "01.09.2026;Kafe;50,00;;1 000,00",
        "02.09.2026;Mzda;;1 000,00;950,00",
        "03.09.2026;Nájem;300,00;;1 950,00",
        "04.09.2026;Nákup;120,00;;1 650,00",
    ]);
    assert_eq!(column(&pair.debit), Some("Debit"));
    assert_eq!(column(&pair.credit), Some("Credit"));
    assert_eq!(pair.balance, None);
    assert_eq!(pair.sign_check, SignCheck::Unchecked);
}

/// E: a statement date repeated on every row loses to a booking date that varies, even when
/// only the constant one carries a date synonym.
#[test]
fn a_constant_date_column_yields_to_a_varying_one() {
    let inferred = infer(&[
        "Datum;Kdy;Částka",
        "30.09.2026;01.09.2026;-100,00",
        "30.09.2026;05.09.2026;200,00",
        "30.09.2026;17.09.2026;-50,00",
    ]);
    assert_eq!(column(&inferred.date), Some("Kdy"));

    // Alone, a constant date is still the date.
    let alone = infer(&["Datum;Částka", "30.09.2026;-100,00", "30.09.2026;200,00"]);
    assert_eq!(column(&alone.date), Some("Datum"));
}

// ---------------------------------------------------------------------------
// Review round 2.
// ---------------------------------------------------------------------------

/// Round 2, #1: Paid out / Paid in / Balance where one row fills both sides (no pair). The
/// debit column alone agrees with "inverted" on every row it fills, but the balance also moves
/// on the credit row where it is empty: it does not carry every amount, so it is no amount.
#[test]
fn a_debit_column_is_no_amount_when_a_row_has_both_sides() {
    let inferred = infer(&[
        "Date;Paid out;Paid in;Balance",
        "01/09/2026;10.00;;990.00",
        "02/09/2026;20.00;;970.00",
        "03/09/2026;;500.00;1470.00",
        "04/09/2026;30.00;;1440.00",
        "05/09/2026;40.00;;1400.00",
        "06/09/2026;50.00;5.00;1355.00",
        "07/09/2026;60.00;;1295.00",
        "08/09/2026;70.00;;1225.00",
        "09/09/2026;80.00;;1145.00",
        "10/09/2026;90.00;;1055.00",
        "11/09/2026;100.00;;955.00",
        "12/09/2026;110.00;;845.00",
        "13/09/2026;120.00;;725.00",
        "14/09/2026;130.00;;595.00",
        "15/09/2026;140.00;;455.00",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(inferred.debit, None);
    assert_eq!(inferred.credit, None);
    assert_eq!(inferred.sign_check, SignCheck::Unchecked);
}

/// Round 2, #1: the same with a negative reversal in the credit column (two signs: no pair
/// side). Again the debit column misses the balance moves on its empty rows.
#[test]
fn a_debit_column_is_no_amount_when_a_credit_is_reversed() {
    let inferred = infer(&[
        "Date;Paid out;Paid in;Balance",
        "01/09/2026;10.00;;990.00",
        "02/09/2026;20.00;;970.00",
        "03/09/2026;;500.00;1470.00",
        "04/09/2026;30.00;;1440.00",
        "05/09/2026;40.00;;1400.00",
        "06/09/2026;;-40.00;1360.00",
        "07/09/2026;60.00;;1300.00",
        "08/09/2026;70.00;;1230.00",
        "09/09/2026;80.00;;1150.00",
        "10/09/2026;90.00;;1060.00",
        "11/09/2026;100.00;;960.00",
        "12/09/2026;110.00;;850.00",
        "13/09/2026;120.00;;730.00",
        "14/09/2026;130.00;;600.00",
        "15/09/2026;140.00;;460.00",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(inferred.debit, None);
    assert_eq!(inferred.credit, None);
    assert_eq!(inferred.sign_check, SignCheck::Unchecked);
}

/// Round 2, #2: two proven readings, headers that say nothing. The original amount on the left
/// agrees with the balance on the domestic rows only (16 of 19 pairs); the account amount on
/// every one. The better proof wins, not the leftmost column.
#[test]
fn the_better_proven_amount_wins_over_the_leftmost() {
    let inferred = infer(&[
        "A;B;C;D;E",
        "01.09.2026;Platba 1;-15.25;-15.25;4984.75",
        "02.09.2026;Platba 2;-22.25;-22.25;4962.50",
        "03.09.2026;Platba 3;-29.25;-29.25;4933.25",
        "04.09.2026;Platba 4;-1.45;-36.25;4897.00",
        "05.09.2026;Platba 5;-43.25;-43.25;4853.75",
        "06.09.2026;Platba 6;-50.25;-50.25;4803.50",
        "07.09.2026;Platba 7;-57.25;-57.25;4746.25",
        "08.09.2026;Platba 8;-64.25;-64.25;4682.00",
        "09.09.2026;Platba 9;-71.25;-71.25;4610.75",
        "10.09.2026;Platba 10;-3.13;-78.25;4532.50",
        "11.09.2026;Platba 11;-85.25;-85.25;4447.25",
        "12.09.2026;Platba 12;-92.25;-92.25;4355.00",
        "13.09.2026;Platba 13;-99.25;-99.25;4255.75",
        "14.09.2026;Platba 14;-106.25;-106.25;4149.50",
        "15.09.2026;Platba 15;-113.25;-113.25;4036.25",
        "16.09.2026;Platba 16;-4.81;-120.25;3916.00",
        "17.09.2026;Platba 17;-127.25;-127.25;3788.75",
        "18.09.2026;Platba 18;-134.25;-134.25;3654.50",
        "19.09.2026;Platba 19;-141.25;-141.25;3513.25",
        "20.09.2026;Platba 20;-148.25;-148.25;3365.00",
    ]);
    assert_eq!(column(&inferred.amount), Some("D"));
    assert_eq!(column(&inferred.balance), Some("E"));
    assert_eq!(inferred.sign_check, SignCheck::Verified);
}

/// F: two positive columns, no header synonym, no balance: which one is the debit cannot be
/// told, so nothing is chosen and the person is asked.
#[test]
fn an_unproven_pair_without_a_hint_is_not_guessed() {
    let inferred = infer(&[
        "H1;H2;H3;H4",
        "01.09.2026;Mzda;45 000,00;",
        "02.09.2026;Nájem;;15 500,00",
        "03.09.2026;Nákup;;1 249,00",
        "04.09.2026;Kafe;;89,00",
    ]);
    assert_eq!(inferred.amount, None);
    assert_eq!(inferred.debit, None);
    assert_eq!(inferred.credit, None);

    // The same with Debit/Credit headers is read.
    let named = infer(&[
        "Datum;Popis;Credit;Debit",
        "01.09.2026;Mzda;45 000,00;",
        "02.09.2026;Nájem;;15 500,00",
        "03.09.2026;Nákup;;1 249,00",
        "04.09.2026;Kafe;;89,00",
    ]);
    assert_eq!(column(&named.debit), Some("Debit"));
    assert_eq!(column(&named.credit), Some("Credit"));
}

/// G: a currency column without a currency header next to a second signed amount belongs to
/// that amount (an original or foreign amount), not to the one imported.
#[test]
fn an_original_currency_is_not_the_row_currency() {
    let inferred = infer(&[
        "Datum;Částka;Původní částka;Původní měna",
        "01.09.2026;-100,00;-4,00;EUR",
        "02.09.2026;2 000,00;2 000,00;CZK",
        "03.09.2026;-250,00;-10,00;USD",
    ]);
    assert_eq!(column(&inferred.amount), Some("Částka"));
    assert_eq!(inferred.currency, None);

    // A header that names the currency keeps it.
    let named = infer(&[
        "Datum;Částka;Původní částka;Měna",
        "01.09.2026;-100,00;-4,00;CZK",
        "02.09.2026;2 000,00;2 000,00;CZK",
        "03.09.2026;-250,00;-10,00;CZK",
    ]);
    assert_eq!(column(&named.currency), Some("Měna"));

    // Without a second amount the code column is the currency.
    let single = infer(&[
        "Datum;Částka;Sloupec 3",
        "01.09.2026;-100,00;CZK",
        "02.09.2026;2 000,00;CZK",
    ]);
    assert_eq!(column(&single.currency), Some("Sloupec 3"));
}

/// H: whole-number money is no transaction id, even when every value is unique.
#[test]
fn whole_number_money_is_no_transaction_id() {
    // A file without decimals: the balance (here a "balance before", so unproven) is money.
    let whole = infer(&[
        "Datum;Částka;X",
        "01.09.2026;-50;1000",
        "02.09.2026;1000;950",
        "03.09.2026;-300;1950",
        "04.09.2026;-120;1650",
    ]);
    assert_eq!(whole.transaction_id, None);

    // An overdrawn whole-number column with both signs in a file with decimals.
    let signed = infer(&[
        "Datum;Částka;X",
        "01.09.2026;-50,50;-50",
        "02.09.2026;100,00;20",
        "03.09.2026;-80,25;-30",
    ]);
    assert_eq!(signed.transaction_id, None);
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
