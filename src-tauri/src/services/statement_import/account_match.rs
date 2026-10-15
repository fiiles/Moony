//! Whether the account a statement names is the account the user picked.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::model::AccountRef;
use crate::error::Result;
use crate::services::financial_semantics::{accounts_matching_identifier, IdentifierMatch};
use rusqlite::{Connection, OptionalExtension};

/// How the account named in the file relates to the selected account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AccountMatch {
    #[serde(rename = "selected")]
    Selected,
    #[serde(rename = "other")]
    Other,
    #[serde(rename = "none")]
    NoMatch,
    #[serde(rename = "unknown")]
    Unknown,
}

/// Compares the account the file names with the selected account. `Other` carries the
/// id of the one other account that matches; `NoMatch` means the file names a valid IBAN
/// that no account has while the selected account does store an identifier, so the user
/// is asked to confirm. A file identifier that is not a valid IBAN or a long domestic
/// number, or an ambiguous match, never forces that confirmation (`Unknown`).
pub fn match_account(
    conn: &Connection,
    selected_id: &str,
    account: &AccountRef,
) -> Result<(AccountMatch, Option<String>)> {
    let Some(identifier) = account
        .identifier
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    else {
        return Ok((AccountMatch::Unknown, None));
    };
    let (ids, by_iban) =
        match accounts_matching_identifier(conn, identifier, account.bank_code.as_deref())? {
            IdentifierMatch::Iban(ids) => (ids, true),
            IdentifierMatch::Number(ids) => (ids, false),
            IdentifierMatch::Unrecognized => return Ok((AccountMatch::Unknown, None)),
        };
    if ids.iter().any(|id| id == selected_id) {
        return Ok((AccountMatch::Selected, Some(selected_id.to_string())));
    }
    match ids.as_slice() {
        [only] => Ok((AccountMatch::Other, Some(only.clone()))),
        [] if by_iban && has_identifier(conn, selected_id)? => Ok((AccountMatch::NoMatch, None)),
        _ => Ok((AccountMatch::Unknown, None)),
    }
}

/// The selected account stores an iban or a bban.
fn has_identifier(conn: &Connection, account_id: &str) -> Result<bool> {
    let stored = conn
        .query_row(
            "SELECT iban, bban FROM bank_accounts WHERE id = ?1",
            [account_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    Ok(stored.is_some_and(|(iban, bban)| {
        [iban, bban]
            .into_iter()
            .flatten()
            .any(|text| !text.trim().is_empty())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::statement_import::test_support::setup_statement_db;

    const IBAN: &str = "CZ6508000000192000145399";

    fn db() -> rusqlite::Connection {
        let conn = setup_statement_db();
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, iban, bban) VALUES
               ('main', 'Main', 'CZK', 'cz65 0800 0000 1920 0014 5399', NULL),
               ('domestic', 'Old', 'CZK', NULL, '19-2000145399/0800'),
               ('savings', 'Savings', 'CZK', 'CZ5508000000001234567899', NULL),
               ('bare', 'Cash', 'CZK', NULL, NULL);",
        )
        .unwrap();
        conn
    }
    fn file(id: &str) -> AccountRef {
        AccountRef {
            identifier: Some(id.into()),
            ..AccountRef::default()
        }
    }

    #[test]
    fn formatting_differences_still_match() {
        let conn = db();
        conn.execute("DELETE FROM bank_accounts WHERE id = 'domestic'", [])
            .unwrap();
        assert_eq!(
            match_account(&conn, "main", &file("CZ65 0800 0000 1920 0014 5399")).unwrap(),
            (AccountMatch::Selected, Some("main".into()))
        );
    }

    #[test]
    fn czech_domestic_bban_matches_the_iban() {
        let conn = db();
        conn.execute("DELETE FROM bank_accounts WHERE id = 'main'", [])
            .unwrap();
        assert_eq!(
            match_account(&conn, "domestic", &file(IBAN)).unwrap(),
            (AccountMatch::Selected, Some("domestic".into()))
        );
    }

    #[test]
    fn other_none_and_unknown() {
        let conn = db();
        conn.execute("DELETE FROM bank_accounts WHERE id = 'domestic'", [])
            .unwrap();
        assert_eq!(
            match_account(&conn, "savings", &file(IBAN)).unwrap(),
            (AccountMatch::Other, Some("main".into()))
        );
        assert_eq!(
            match_account(&conn, "savings", &file("DE89370400440532013000")).unwrap(),
            (AccountMatch::NoMatch, None)
        );
        assert_eq!(
            match_account(&conn, "bare", &file("DE89370400440532013000")).unwrap(),
            (AccountMatch::Unknown, None)
        );
        assert_eq!(
            match_account(&conn, "savings", &AccountRef::default()).unwrap(),
            (AccountMatch::Unknown, None)
        );
        // R5: an unrecognized domestic notation never forces the confirmation
        assert_eq!(
            match_account(&conn, "savings", &file("ACC-77")).unwrap(),
            (AccountMatch::Unknown, None)
        );
    }

    #[test]
    fn several_other_matches_are_unknown() {
        let conn = db();
        // 'domestic' and 'main' both name the account; neither is selected
        assert_eq!(
            match_account(&conn, "savings", &file(IBAN)).unwrap(),
            (AccountMatch::Unknown, None)
        );
    }

    #[test]
    fn long_domestic_numbers_match_but_never_force_the_confirmation() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, bban) VALUES
               ('us', 'US', 'USD', '021000021123456789');",
        )
        .unwrap();
        let routed = AccountRef {
            identifier: Some("123456789".into()),
            bank_code: Some("021000021".into()),
            ..AccountRef::default()
        };
        assert_eq!(
            match_account(&conn, "us", &routed).unwrap(),
            (AccountMatch::Selected, Some("us".into()))
        );
        assert_eq!(
            match_account(&conn, "savings", &routed).unwrap(),
            (AccountMatch::Other, Some("us".into()))
        );
        // R5: a number no account stores is Unknown, not NoMatch, however the selected account is set up
        assert_eq!(
            match_account(&conn, "savings", &file("999999999999")).unwrap(),
            (AccountMatch::Unknown, None)
        );
    }
}
