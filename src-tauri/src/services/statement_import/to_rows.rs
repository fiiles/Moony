//! A parsed [`Statement`] as the rows the shared import core takes.

use std::collections::HashMap;

use crate::error::AppError;
use crate::models::currency::is_iso_4217;
use crate::models::InsertBankTransaction;
use crate::services::csv_import::{CsvDateRange, CsvRowMessage, ParsedImport, ParsedRow, RowState};
use crate::services::dedup::canonical_amount;

use super::model::{Entry, Statement};

/// The statement's entries as rows of the shared import core, in file order.
/// Row `line` is the 1-based ordinal of the entry (statements have no lines).
pub fn to_rows(statement: &Statement, account_id: &str) -> ParsedImport {
    let mut entries = statement.entries.clone();
    drop_repeated_ids(&mut entries);
    let rows: Vec<ParsedRow> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| entry_row(i + 1, e, statement, account_id))
        .collect();
    ParsedImport {
        total_rows: rows.len(),
        rows,
        period: statement.period.map(|(from, to)| CsvDateRange { from, to }),
        declared_balance: statement.closing.clone(),
    }
}

/// D6: an ID that is not unique in the import would make the duplicate check treat the
/// second entry as a certain duplicate and drop a real payment.
fn drop_repeated_ids(entries: &mut [Entry]) {
    let clean = |id: &Option<String>| {
        id.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for e in entries.iter() {
        if let Some(id) = clean(&e.bank_id) {
            *counts.entry(id).or_default() += 1;
        }
    }
    for e in entries.iter_mut() {
        e.bank_id = clean(&e.bank_id).filter(|id| counts.get(id) == Some(&1));
    }
}

fn message(line: usize, key: &str, detail: Option<&str>) -> CsvRowMessage {
    CsvRowMessage {
        line,
        key: key.to_string(),
        detail: detail.map(str::to_string),
    }
}

fn entry_row(line: usize, e: &Entry, statement: &Statement, account_id: &str) -> ParsedRow {
    let currency = [e.currency.as_deref(), statement.currency.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|c| !c.is_empty())
        .map(str::to_ascii_uppercase);
    let amount = canonical_amount(&e.amount);
    let signed_amount = format!("{}{amount}", if e.is_credit { "" } else { "-" });
    let row = |state: RowState| ParsedRow {
        line,
        booking_date: Some(e.booking_date),
        signed_amount: Some(signed_amount.clone()),
        currency: currency.clone(),
        description: e.description.clone(),
        counterparty: e.counterparty_name.clone(),
        balance: None,
        source_category: e.source_category.clone(),
        state,
    };

    if let Some(key) = e.skip_reason {
        return row(RowState::SkippedFiltered(message(line, key, None)));
    }
    if amount == "0" {
        return row(RowState::SkippedZero(message(
            line,
            "csvImport.zeroAmount",
            None,
        )));
    }
    // Well-formed but not a currency ("XYZ", a metal or fund code): the wire-level
    // check in validate() only knows "3 letters".
    if let Some(unknown) = currency.as_deref().filter(|c| !is_iso_4217(c)) {
        return row(RowState::Error(message(
            line,
            "validation.currencyUnknown",
            Some(unknown),
        )));
    }

    let insert = InsertBankTransaction {
        bank_account_id: account_id.to_string(),
        transaction_id: e.bank_id.clone(),
        tx_type: if e.is_credit { "credit" } else { "debit" }.to_string(),
        flow_kind: None,
        amount,
        currency: currency.clone(),
        description: e.description.clone(),
        counterparty_name: e.counterparty_name.clone(),
        counterparty_iban: e.counterparty_account.clone(),
        counterparty_bank_code: e.counterparty_bank_code.clone(),
        counterparty_country: e.counterparty_country.clone(),
        booking_date: e.booking_date,
        value_date: e.value_date,
        category_id: None,
        status: Some(if e.pending { "pending" } else { "booked" }.to_string()),
    };
    match insert.validate() {
        Ok(()) => row(RowState::Candidate(Box::new(insert))),
        Err(AppError::Validation(key)) => row(RowState::Error(message(line, &key, None))),
        Err(other) => row(RowState::Error(message(
            line,
            "csvImport.rowCannotParse",
            Some(&other.to_string()),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::csv_import::{DeclaredBalance, RowState};

    fn entry(amount: &str, credit: bool, id: Option<&str>) -> Entry {
        Entry {
            booking_date: 20_000 * 86_400,
            amount: amount.into(),
            is_credit: credit,
            bank_id: id.map(Into::into),
            ..Entry::default()
        }
    }

    fn statement(entries: Vec<Entry>) -> Statement {
        Statement {
            currency: Some("CZK".into()),
            entries,
            ..Statement::default()
        }
    }

    fn insert(
        row: &crate::services::csv_import::ParsedRow,
    ) -> &crate::models::InsertBankTransaction {
        match &row.state {
            RowState::Candidate(i) => i,
            other => panic!("not a candidate: {other:?}"),
        }
    }

    #[test]
    fn entries_become_candidates_with_ordinals() {
        let parsed = to_rows(
            &statement(vec![
                entry("500.5", true, Some("A")),
                entry("250", false, Some("B")),
            ]),
            "acc",
        );
        assert_eq!(
            parsed.rows.iter().map(|r| r.line).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(parsed.rows[1].signed_amount.as_deref(), Some("-250"));
        let second = insert(&parsed.rows[1]);
        assert_eq!(
            (second.tx_type.as_str(), second.amount.as_str()),
            ("debit", "250")
        );
        assert_eq!(second.currency.as_deref(), Some("CZK"));
        assert_eq!(second.status.as_deref(), Some("booked"));
    }

    #[test]
    fn repeated_ids_are_dropped_unique_ids_trimmed() {
        let parsed = to_rows(
            &statement(vec![
                entry("1", true, Some("X")),
                entry("2", true, Some(" X ")),
                entry("3", true, Some(" Y ")),
            ]),
            "acc",
        );
        assert_eq!(insert(&parsed.rows[0]).transaction_id, None);
        assert_eq!(insert(&parsed.rows[1]).transaction_id, None);
        assert_eq!(insert(&parsed.rows[2]).transaction_id.as_deref(), Some("Y"));
    }

    #[test]
    fn pending_zero_skipped_and_bad_currency() {
        let mut pending = entry("1", true, None);
        pending.pending = true;
        let mut skipped = entry("2", true, None);
        skipped.skip_reason = Some("statementImport.notBooked");
        let mut bad = entry("3", true, None);
        bad.currency = Some("XYZ".into());
        let parsed = to_rows(
            &statement(vec![pending, entry("0.00", true, None), skipped, bad]),
            "acc",
        );
        assert_eq!(insert(&parsed.rows[0]).status.as_deref(), Some("pending"));
        assert!(
            matches!(&parsed.rows[1].state, RowState::SkippedZero(m) if m.key == "csvImport.zeroAmount")
        );
        assert!(
            matches!(&parsed.rows[2].state, RowState::SkippedFiltered(m) if m.key == "statementImport.notBooked")
        );
        assert!(
            matches!(&parsed.rows[3].state, RowState::Error(m) if m.key == "validation.currencyUnknown")
        );
    }

    #[test]
    fn closing_balance_and_period_are_declared() {
        let mut s = statement(vec![]);
        s.period = Some((1, 2));
        s.closing = Some(DeclaredBalance {
            amount: "5".into(),
            currency: "CZK".into(),
            as_of: 2,
        });
        let parsed = to_rows(&s, "acc");
        assert_eq!(parsed.period.map(|p| (p.from, p.to)), Some((1, 2)));
        assert_eq!(parsed.declared_balance.map(|b| b.amount), Some("5".into()));
        assert_eq!(parsed.total_rows, 0);
    }

    #[test]
    fn currency_is_the_entrys_own_else_the_statements_uppercased() {
        let mut own = entry("1", true, None);
        own.currency = Some(" eur ".into());
        let mut none = entry("2", true, None);
        none.currency = None;
        let mut s = statement(vec![own, none]);
        s.currency = Some("czk".into());
        let parsed = to_rows(&s, "acc");
        assert_eq!(parsed.rows[0].currency.as_deref(), Some("EUR"));
        assert_eq!(insert(&parsed.rows[0]).currency.as_deref(), Some("EUR"));
        assert_eq!(parsed.rows[1].currency.as_deref(), Some("CZK"));

        let mut no_currency = statement(vec![entry("3", true, None)]);
        no_currency.currency = None;
        let parsed = to_rows(&no_currency, "acc");
        assert_eq!(parsed.rows[0].currency, None);
        assert_eq!(insert(&parsed.rows[0]).currency, None);
    }

    #[test]
    fn unknown_currency_names_the_code_in_the_detail() {
        let mut bad = entry("3", true, None);
        bad.currency = Some("xyz".into());
        let parsed = to_rows(&statement(vec![bad]), "acc");
        assert!(matches!(
            &parsed.rows[0].state,
            RowState::Error(m) if m.line == 1 && m.detail.as_deref() == Some("XYZ")
        ));
    }

    #[test]
    fn fields_are_carried_into_the_insert() {
        let mut e = entry("1250.50", true, Some("ID1"));
        e.value_date = Some(19_999 * 86_400);
        e.description = Some("Rent | May".into());
        e.counterparty_name = Some("Landlord".into());
        e.counterparty_account = Some("CZ6508000000192000145399".into());
        e.counterparty_bank_code = Some("0800".into());
        e.counterparty_country = Some("CZ".into());
        e.source_category = Some("Housing".into());
        let parsed = to_rows(&statement(vec![e]), "acc-1");
        let row = &parsed.rows[0];
        assert_eq!(row.booking_date, Some(20_000 * 86_400));
        assert_eq!(row.signed_amount.as_deref(), Some("1250.5"));
        assert_eq!(row.counterparty.as_deref(), Some("Landlord"));
        assert_eq!(row.description.as_deref(), Some("Rent | May"));
        assert_eq!(row.source_category.as_deref(), Some("Housing"));
        assert_eq!(row.balance, None);
        let i = insert(row);
        assert_eq!(i.bank_account_id, "acc-1");
        assert_eq!(i.transaction_id.as_deref(), Some("ID1"));
        assert_eq!(i.tx_type, "credit");
        assert_eq!(i.amount, "1250.5");
        assert_eq!(i.value_date, Some(19_999 * 86_400));
        assert_eq!(
            i.counterparty_iban.as_deref(),
            Some("CZ6508000000192000145399")
        );
        assert_eq!(i.counterparty_bank_code.as_deref(), Some("0800"));
        assert_eq!(i.counterparty_country.as_deref(), Some("CZ"));
        assert_eq!(i.category_id, None);
        assert_eq!(i.flow_kind, None);
    }

    #[test]
    fn blank_ids_are_dropped_and_the_input_is_untouched() {
        let s = statement(vec![
            entry("1", true, Some("  ")),
            entry("2", true, Some("Z")),
        ]);
        let parsed = to_rows(&s, "acc");
        assert_eq!(insert(&parsed.rows[0]).transaction_id, None);
        assert_eq!(insert(&parsed.rows[1]).transaction_id.as_deref(), Some("Z"));
        assert_eq!(s.entries[0].bank_id.as_deref(), Some("  "));
    }

    #[test]
    fn validation_failures_become_error_rows_with_the_key() {
        let mut no_date = entry("1", true, None);
        no_date.booking_date = 0;
        let negative = entry("-5", true, None);
        let garbage = entry("abc", true, None);
        let parsed = to_rows(&statement(vec![no_date, negative, garbage]), "acc");
        let keys: Vec<_> = parsed
            .rows
            .iter()
            .map(|r| match &r.state {
                RowState::Error(m) => m.key.as_str(),
                other => panic!("not an error: {other:?}"),
            })
            .collect();
        assert_eq!(
            keys,
            vec![
                "validation.bookingDateInvalid",
                "validation.amountRequired",
                "validation.invalidAmount"
            ]
        );
        assert_eq!(parsed.total_rows, 3);
    }

    #[test]
    fn skipped_entries_still_show_their_date_and_amount() {
        let mut skipped = entry("2", false, None);
        skipped.skip_reason = Some("statementImport.notBooked");
        let parsed = to_rows(&statement(vec![skipped]), "acc");
        assert_eq!(parsed.rows[0].booking_date, Some(20_000 * 86_400));
        assert_eq!(parsed.rows[0].signed_amount.as_deref(), Some("-2"));
    }
}
