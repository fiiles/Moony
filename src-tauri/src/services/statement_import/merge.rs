//! Merging the statements of one file that belong to the same account and
//! currency (the pages of one MT940 statement, the statements of one CAMT.053
//! file), and the balance check of the merged result.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::decimal::Dec;
use super::model::{AccountRef, Statement};
use crate::services::financial_semantics::canonical_account_identifier;

/// Whether the declared balances agree with the entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum BalanceCheck {
    #[serde(rename = "ok")]
    AddsUp,
    #[serde(rename = "mismatch")]
    Mismatch,
    #[serde(rename = "unavailable")]
    Unavailable,
}

/// One account's statements of a file as a single statement.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedStatement {
    pub statement: Statement,
    pub balance_check: BalanceCheck,
    /// Declared minus computed, in the statement's currency; set on a mismatch.
    pub balance_difference: Option<String>,
}

/// Groups the statements by account and currency (groups keep the order of
/// their first statement in the file), orders each group by statement number,
/// concatenates the entries and checks the declared balances.
pub fn merge(statements: Vec<Statement>) -> Vec<MergedStatement> {
    let mut groups: Vec<Vec<Statement>> = Vec::new();
    let mut slots: HashMap<(String, String), usize> = HashMap::new();
    for statement in statements {
        let key = (
            account_key(&statement.account),
            statement
                .currency
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_uppercase(),
        );
        let slot = *slots.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[slot].push(statement);
    }
    groups.into_iter().filter_map(merge_group).collect()
}

/// The canonical identifier, else `name:<name>`, else the empty key.
fn account_key(account: &AccountRef) -> String {
    let present = |text: &Option<String>| {
        text.as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    if let Some(identifier) = present(&account.identifier) {
        canonical_account_identifier(&identifier, account.bank_code.as_deref())
    } else if let Some(name) = present(&account.name) {
        format!("name:{name}")
    } else {
        String::new()
    }
}

/// A statement number. Numbers sort before text so the order stays total (a
/// comparison that switched between numbers and strings pairwise could cycle).
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Sequence<'a> {
    Number(u64),
    Text(&'a str),
}

/// `00009/001` (statement/page) is statement 9: everything from the first `/`
/// is dropped before the number is read.
fn sequence_key(statement: &Statement) -> Option<Sequence<'_>> {
    let text = statement.sequence.as_deref()?;
    let head = text.split('/').next().unwrap_or(text).trim();
    Some(
        head.parse::<u64>()
            .map_or(Sequence::Text(text), Sequence::Number),
    )
}

fn order(a: &Statement, b: &Statement) -> Ordering {
    sequence_key(a)
        .cmp(&sequence_key(b))
        .then_with(|| a.period.map(|p| p.0).cmp(&b.period.map(|p| p.0)))
}

/// `None` only for an empty group, which `merge` never builds.
fn merge_group(mut pieces: Vec<Statement>) -> Option<MergedStatement> {
    pieces.sort_by(order); // stable: equal statements keep their file order
    let (balance_check, balance_difference) =
        check_balances(&pieces).unwrap_or((BalanceCheck::Unavailable, None));
    let closing = pieces.last()?.closing.clone();
    let period = pieces
        .iter()
        .filter_map(|piece| piece.period)
        .reduce(|(from, to), (f, t)| (from.min(f), to.max(t)));
    let mut pieces = pieces.into_iter();
    let mut statement = pieces.next()?;
    statement
        .entries
        .extend(pieces.flat_map(|piece| piece.entries));
    statement.closing = closing;
    statement.period = period;
    Some(MergedStatement {
        statement,
        balance_check,
        balance_difference,
    })
}

/// Exact decimal arithmetic throughout; values are compared by subtracting
/// (a difference of zero), and `None` (an unreadable amount or an overflow)
/// means the check cannot be made.
fn check_balances(pieces: &[Statement]) -> Option<(BalanceCheck, Option<String>)> {
    for pair in pieces.windows(2) {
        if let (Some(closing), Some(opening)) = (&pair[0].closing, &pair[1].opening) {
            let gap = Dec::parse(&opening.amount)?.add(Dec::parse(&closing.amount)?.neg())?;
            if !gap.is_zero() {
                return Some((BalanceCheck::Mismatch, Some(gap.to_text())));
            }
        }
    }
    let (Some(opening), Some(closing)) = (&pieces.first()?.opening, &pieces.last()?.closing) else {
        return Some((BalanceCheck::Unavailable, None));
    };
    let mut expected = Dec::parse(&opening.amount)?;
    let counted = pieces
        .iter()
        .flat_map(|piece| &piece.entries)
        .filter(|entry| entry.skip_reason.is_none() && !entry.pending);
    for entry in counted {
        let amount = Dec::parse(&entry.amount)?;
        expected = expected.add(if entry.is_credit {
            amount
        } else {
            amount.neg()
        })?;
    }
    let difference = Dec::parse(&closing.amount)?.add(expected.neg())?;
    Some(if difference.is_zero() {
        (BalanceCheck::AddsUp, None)
    } else {
        (BalanceCheck::Mismatch, Some(difference.to_text()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::csv_import::types::DeclaredBalance;
    use crate::services::statement_import::model::Entry;

    fn bal(amount: &str, day: i64) -> Option<DeclaredBalance> {
        Some(DeclaredBalance {
            amount: amount.into(),
            currency: "CZK".into(),
            as_of: day * 86_400,
        })
    }
    fn e(amount: &str, credit: bool, day: i64) -> Entry {
        Entry {
            amount: amount.into(),
            is_credit: credit,
            booking_date: day * 86_400,
            ..Entry::default()
        }
    }
    fn st(iban: &str, seq: &str, opening: &str, closing: &str, entries: Vec<Entry>) -> Statement {
        Statement {
            account: AccountRef {
                identifier: Some(iban.into()),
                ..AccountRef::default()
            },
            currency: Some("CZK".into()),
            sequence: Some(seq.into()),
            opening: bal(opening, 1),
            closing: bal(closing, 2),
            entries,
            ..Statement::default()
        }
    }
    const A: &str = "CZ6508000000192000145399";

    #[test]
    fn same_account_pages_merge_in_sequence_order() {
        let merged = merge(vec![
            st(A, "00002", "1100", "1000", vec![e("100", false, 2)]),
            st(
                &A.to_lowercase(),
                "00001",
                "1000",
                "1100",
                vec![e("100", true, 1)],
            ),
        ]);
        assert_eq!(merged.len(), 1);
        let m = &merged[0];
        assert_eq!(m.statement.entries.len(), 2);
        assert!(m.statement.entries[0].is_credit);
        assert_eq!(m.statement.opening.as_ref().unwrap().amount, "1000");
        assert_eq!(m.statement.closing.as_ref().unwrap().amount, "1000");
        assert_eq!(m.balance_check, BalanceCheck::AddsUp);
    }

    #[test]
    fn chain_break_is_a_mismatch_with_its_difference() {
        let merged = merge(vec![
            st(A, "1", "1000", "1100", vec![e("100", true, 1)]),
            st(A, "2", "1150", "1150", vec![]),
        ]);
        assert_eq!(
            (
                merged[0].balance_check,
                merged[0].balance_difference.as_deref()
            ),
            (BalanceCheck::Mismatch, Some("50"))
        );
    }

    #[test]
    fn sum_mismatch_pending_and_skipped_excluded() {
        let mut pending = e("999", false, 1);
        pending.pending = true;
        let mut skipped = e("5", true, 1);
        skipped.skip_reason = Some("statementImport.notBooked");
        let ok = merge(vec![st(
            A,
            "1",
            "0.1",
            "0.3",
            vec![e("0.2", true, 1), pending, skipped],
        )]);
        assert_eq!(ok[0].balance_check, BalanceCheck::AddsUp);
        let bad = merge(vec![st(A, "1", "10", "30", vec![e("15", true, 1)])]);
        assert_eq!(
            (bad[0].balance_check, bad[0].balance_difference.as_deref()),
            (BalanceCheck::Mismatch, Some("5"))
        );
    }

    #[test]
    fn different_accounts_or_currencies_stay_apart_and_missing_balance_is_unavailable() {
        let mut eur = st(A, "1", "0", "0", vec![]);
        eur.currency = Some("EUR".into());
        let mut open_only = st("CZ5508000000001234567899", "1", "0", "0", vec![]);
        open_only.closing = None;
        let merged = merge(vec![st(A, "1", "0", "0", vec![]), eur, open_only]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[2].balance_check, BalanceCheck::Unavailable);
    }

    #[test]
    fn mt940_sequences_order_numerically_ignoring_the_page_suffix() {
        let merged = merge(vec![
            st(A, "10/001", "2", "3", vec![e("1", true, 3)]),
            st(A, "9/001", "1", "2", vec![e("1", true, 2)]),
            st(A, "2/001", "0", "1", vec![e("1", true, 1)]),
        ]);
        assert_eq!(merged.len(), 1);
        let days: Vec<i64> = merged[0]
            .statement
            .entries
            .iter()
            .map(|e| e.booking_date / 86_400)
            .collect();
        assert_eq!(days, [1, 2, 3]);
        assert_eq!(merged[0].balance_check, BalanceCheck::AddsUp);
    }

    #[test]
    fn equal_sequences_fall_back_to_period_start_then_file_order() {
        let mut later = st(A, "1", "10", "20", vec![e("10", true, 5)]);
        later.period = Some((5 * 86_400, 9 * 86_400));
        let mut earlier = st(A, "1", "0", "10", vec![e("10", true, 1)]);
        earlier.period = Some((86_400, 4 * 86_400));
        let merged = merge(vec![later, earlier]);
        assert_eq!(merged[0].statement.entries[0].booking_date, 86_400);
        assert_eq!(
            merged[0].statement.period,
            Some((86_400, 9 * 86_400)),
            "period spans all statements"
        );
        assert_eq!(merged[0].balance_check, BalanceCheck::AddsUp);
    }

    #[test]
    fn groups_keep_first_appearance_order_and_unnamed_accounts_group_by_name() {
        let named = |name: &str| Statement {
            account: AccountRef {
                name: Some(name.into()),
                ..AccountRef::default()
            },
            currency: Some("czk".into()),
            ..Statement::default()
        };
        let merged = merge(vec![
            named("Cash"),
            st(A, "1", "0", "0", vec![]),
            named("Cash"),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].statement.account.name.as_deref(), Some("Cash"));
        assert_eq!(merged[1].statement.account.identifier.as_deref(), Some(A));
    }

    #[test]
    fn balances_compare_by_value_not_by_text() {
        let merged = merge(vec![
            st(A, "1", "0.00", "0.30", vec![e("0.3", true, 1)]),
            st(A, "2", "0.3", "0.30", vec![]),
        ]);
        assert_eq!(merged[0].balance_check, BalanceCheck::AddsUp);
        assert_eq!(merged[0].balance_difference, None);
    }

    #[test]
    fn unparsable_or_overflowing_amounts_are_unavailable() {
        let bad_balance = merge(vec![st(A, "1", "1 000", "1000", vec![])]);
        assert_eq!(bad_balance[0].balance_check, BalanceCheck::Unavailable);
        let bad_entry = merge(vec![st(A, "1", "0", "1", vec![e("one", true, 1)])]);
        assert_eq!(bad_entry[0].balance_check, BalanceCheck::Unavailable);
        let huge = "170141183460469231731687303715884105727";
        let overflow = merge(vec![st(
            A,
            "1",
            huge,
            "0",
            vec![e(huge, true, 1), e(huge, true, 1)],
        )]);
        assert_eq!(overflow[0].balance_check, BalanceCheck::Unavailable);
        assert_eq!(overflow[0].balance_difference, None);
    }

    #[test]
    fn no_statements_merge_to_nothing() {
        assert!(merge(Vec::new()).is_empty());
    }
}
