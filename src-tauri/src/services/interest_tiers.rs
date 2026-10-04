//! Progressive interest tiers of a bank account.
//!
//! One implementation for every caller (the cashflow report, the account
//! commands); the TypeScript mirror in `src/utils/bank-account-zones.ts` is
//! checked against the same cases in `shared/fixtures/interest-tiers.json`.
//!
//! Semantics (what the zones dialog shows): a tier is a band of the balance,
//! `[from, to]`, and only the part of the balance that lies inside the band
//! earns its rate. Tiers need not be contiguous — a balance that falls in a gap
//! earns nothing for the gap — and they need not be sorted.

use crate::error::{AppError, Result};
use rusqlite::Connection;

/// One band of the balance with its yearly rate.
#[derive(Debug, Clone, PartialEq)]
pub struct InterestTier {
    /// Lower bound of the band, in the account currency.
    pub from: f64,
    /// Upper bound; `None` is unlimited.
    pub to: Option<f64>,
    /// Interest rate in percent per year.
    pub rate: f64,
}

impl InterestTier {
    /// Parse a stored zone. A missing, empty or `0` upper bound is unlimited
    /// (a real band always ends above its start, so `0` can only be the legacy
    /// "no limit" encoding the UI also understands). Unparseable numbers are an
    /// error, never a silent zero (ADR 0001).
    pub fn parse(from_amount: &str, to_amount: Option<&str>, interest_rate: &str) -> Result<Self> {
        let number = |label: &str, value: &str| -> Result<f64> {
            value.trim().parse::<f64>().map_err(|_| {
                AppError::Validation(format!("Invalid interest tier {label} '{value}'"))
            })
        };
        let to = match to_amount.map(str::trim) {
            None | Some("") => None,
            Some(value) => match number("upper bound", value)? {
                0.0 => None,
                to => Some(to),
            },
        };
        Ok(Self {
            from: number("lower bound", from_amount)?,
            to,
            rate: number("rate", interest_rate)?,
        })
    }
}

/// Yearly interest of `balance` over `tiers`, in the account currency.
pub fn yearly_interest(balance: f64, tiers: &[InterestTier]) -> f64 {
    if balance <= 0.0 {
        return 0.0;
    }
    tiers
        .iter()
        .map(|tier| {
            let upper = tier.to.map_or(balance, |to| balance.min(to));
            let in_band = upper - tier.from;
            if in_band > 0.0 {
                in_band * tier.rate / 100.0
            } else {
                0.0
            }
        })
        .sum()
}

/// Yearly interest as a percentage of the balance (0 for a non-positive balance).
pub fn effective_rate(balance: f64, tiers: &[InterestTier]) -> f64 {
    if balance <= 0.0 {
        return 0.0;
    }
    yearly_interest(balance, tiers) / balance * 100.0
}

/// The tiers of one bank account, lowest band first.
pub fn load_account_tiers(conn: &Connection, bank_account_id: &str) -> Result<Vec<InterestTier>> {
    let mut stmt = conn.prepare(
        "SELECT from_amount, to_amount, interest_rate FROM bank_account_zones
         WHERE bank_account_id = ?1 ORDER BY CAST(from_amount AS REAL) ASC",
    )?;
    let rows = stmt.query_map([bank_account_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut tiers = Vec::new();
    for row in rows {
        let (from, to, rate) = row?;
        tiers.push(InterestTier::parse(&from, to.as_deref(), &rate)?);
    }
    Ok(tiers)
}

/// Yearly interest of one account: its tiers when it is zoned and has any,
/// otherwise the flat `base_rate` (percent per year). Account currency.
pub fn account_yearly_interest(
    conn: &Connection,
    bank_account_id: &str,
    balance: f64,
    base_rate: f64,
    has_zones: bool,
) -> Result<f64> {
    if has_zones {
        let tiers = load_account_tiers(conn, bank_account_id)?;
        if !tiers.is_empty() {
            return Ok(yearly_interest(balance, &tiers));
        }
    }
    Ok(balance * base_rate / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct TierJson {
        #[serde(rename = "fromAmount")]
        from_amount: String,
        #[serde(rename = "toAmount")]
        to_amount: Option<String>,
        #[serde(rename = "interestRate")]
        interest_rate: String,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        balance: f64,
        tiers: Vec<TierJson>,
        #[serde(rename = "yearlyInterest")]
        yearly_interest: f64,
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    #[test]
    fn matches_the_shared_cross_check_cases() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../../shared/fixtures/interest-tiers.json"))
                .expect("fixture parses");
        assert!(!fixture.cases.is_empty());
        for case in fixture.cases {
            let tiers: Vec<InterestTier> = case
                .tiers
                .iter()
                .map(|t| {
                    InterestTier::parse(&t.from_amount, t.to_amount.as_deref(), &t.interest_rate)
                        .expect("tier parses")
                })
                .collect();
            let got = yearly_interest(case.balance, &tiers);
            assert!(
                (got - case.yearly_interest).abs() < 1e-6,
                "{}: expected {}, got {}",
                case.name,
                case.yearly_interest,
                got
            );
        }
    }

    #[test]
    fn audit_example_is_progressive_not_fill() {
        // [0-100k @5%], [200k-inf @1%], balance 150k. The old cashflow
        // "fill" model poured the 50k above the first tier into the second one
        // (5 500); the balance never reaches the second tier (5 000).
        let tiers = vec![
            InterestTier::parse("0", Some("100000"), "5").unwrap(),
            InterestTier::parse("200000", None, "1").unwrap(),
        ];
        assert_eq!(yearly_interest(150_000.0, &tiers), 5_000.0);
        assert!((effective_rate(150_000.0, &tiers) - 100.0 * 5_000.0 / 150_000.0).abs() < 1e-9);
    }

    #[test]
    fn effective_rate_of_nothing_is_zero() {
        assert_eq!(effective_rate(0.0, &[]), 0.0);
        let tiers = vec![InterestTier::parse("0", None, "4").unwrap()];
        assert_eq!(effective_rate(0.0, &tiers), 0.0);
        assert_eq!(effective_rate(-1.0, &tiers), 0.0);
    }

    #[test]
    fn parse_rejects_garbage_instead_of_zeroing_it() {
        assert!(InterestTier::parse("abc", None, "1").is_err());
        assert!(InterestTier::parse("0", Some("x"), "1").is_err());
        assert!(InterestTier::parse("0", None, "").is_err());
    }

    #[test]
    fn load_reads_the_zones_of_one_account() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE bank_account_zones (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL,
                from_amount TEXT NOT NULL, to_amount TEXT, interest_rate TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0);
             INSERT INTO bank_account_zones (id, bank_account_id, from_amount, to_amount, interest_rate)
             VALUES ('z2', 'a', '200000', NULL, '1'), ('z1', 'a', '0', '100000', '5'),
                    ('z3', 'other', '0', NULL, '9');",
        )
        .unwrap();
        let tiers = load_account_tiers(&conn, "a").unwrap();
        assert_eq!(tiers.len(), 2);
        assert_eq!(yearly_interest(150_000.0, &tiers), 5_000.0);
        assert!(load_account_tiers(&conn, "none").unwrap().is_empty());
    }
    #[test]
    fn account_interest_uses_the_tiers_of_a_zoned_account() {
        // the cashflow report and the account commands share this.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE bank_account_zones (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL,
                from_amount TEXT NOT NULL, to_amount TEXT, interest_rate TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0);
             INSERT INTO bank_account_zones (id, bank_account_id, from_amount, to_amount, interest_rate)
             VALUES ('z1', 'a', '0', '100000', '5'), ('z2', 'a', '200000', NULL, '1');",
        )
        .unwrap();
        // zoned: progressive tiers, the flat rate is ignored
        assert_eq!(
            account_yearly_interest(&conn, "a", 150_000.0, 3.0, true).unwrap(),
            5_000.0
        );
        // zoned flag without any tier: falls back to the flat rate
        assert_eq!(
            account_yearly_interest(&conn, "none", 150_000.0, 2.0, true).unwrap(),
            3_000.0
        );
        // not zoned: flat rate, even if stray tiers exist
        assert_eq!(
            account_yearly_interest(&conn, "a", 150_000.0, 2.0, false).unwrap(),
            3_000.0
        );
    }
}
