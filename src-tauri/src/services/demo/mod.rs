//! Demo profile generator: a believable four-year financial life for the fictional Alex Meyer,
//! written into an already migrated database.
//!
//! Everything is relative to [`DemoOptions::now`]; nothing reads the wall clock, so the same
//! options always produce the same data. Spec: `docs/specs/2026-11-23-demo-profile-design.md`,
//! plan: `docs/plans/2026-11-23-demo-profile-data.md`.

pub mod assets;
pub mod bank;
pub mod clock;
pub mod history;
pub mod invest;
pub mod planning;
pub mod prices;
pub mod rng;
pub mod schedule;
pub mod text;

use crate::error::Result;
use rusqlite::Connection;
use std::collections::BTreeMap;

/// First name of the persona.
pub const PERSONA_NAME: &str = "Alex";
/// Surname of the persona.
pub const PERSONA_SURNAME: &str = "Meyer";
/// Email address of the persona (reserved example domain).
pub const PERSONA_EMAIL: &str = "alex.meyer@example.com";
/// Home currency of the persona.
pub const PERSONA_CURRENCY: &str = "EUR";

/// What to generate.
#[derive(Debug, Clone)]
pub struct DemoOptions {
    /// UI language code for all free text (en, cs, de, es, it, nb, pl, sv, ja, zh).
    pub language: String,
    /// Unix timestamp of "today"; every date is derived from it.
    pub now: i64,
    /// Attach documents (purchase contract, policy) to the encrypted attachments store.
    pub attachments: bool,
}

/// What a run created.
#[derive(Debug, Clone, Default)]
pub struct DemoSummary {
    /// Rows created per table or kind (filled in by the parts).
    pub counts: BTreeMap<&'static str, usize>,
    /// Net worth today in EUR.
    pub net_worth_eur: f64,
}

/// Fill a migrated database with the demo data set.
///
/// There is no outer transaction: the bank, budget and plan services open their own
/// (SQLite cannot nest them), so each part batches what it can and commits itself.
pub fn seed(conn: &Connection, opts: &DemoOptions) -> Result<DemoSummary> {
    let mut summary = DemoSummary::default();

    history::seed_fx(conn, opts, &mut summary)?; // USD and DKK rates, loaded for conversions
    bank::seed(conn, opts, &mut summary)?; // accounts, payments, balance records
    planning::seed(conn, opts, &mut summary)?; // budgets, spending plans, cashflow items
    invest::seed(conn, opts, &mut summary)?; // stocks, crypto, bond, watchlist
    assets::seed(conn, opts, &mut summary)?; // property, loans, insurance, other assets
                                             // Task 4: history::seed(conn, opts, &mut summary)?;

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};

    fn now_2026() -> i64 {
        Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0)
            .single()
            .expect("date")
            .timestamp()
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma");
        crate::db::run_migrations(&conn).expect("migrations");
        conn
    }

    fn opts(language: &str, now: i64) -> DemoOptions {
        DemoOptions {
            language: language.into(),
            now,
            attachments: false,
        }
    }

    fn seeded(language: &str, now: i64) -> (Connection, DemoSummary) {
        let conn = db();
        let summary = seed(&conn, &opts(language, now)).expect("seed");
        (conn, summary)
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect("count")
    }

    /// (name, currency, end balance) per account, ordered by name for stable comparison.
    fn end_balances(conn: &Connection) -> Vec<(String, String, f64)> {
        let mut stmt = conn
            .prepare("SELECT name, currency, balance FROM bank_accounts ORDER BY currency, name")
            .expect("prepare");
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?.parse::<f64>().unwrap_or(f64::NAN),
            ))
        })
        .expect("query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("rows")
    }

    fn balance_of(conn: &Connection, currency: &str, savings: bool) -> f64 {
        let kind = if savings { "savings" } else { "checking" };
        conn.query_row(
            "SELECT CAST(balance AS REAL) FROM bank_accounts WHERE currency = ?1 AND account_type = ?2",
            [currency, kind],
            |r| r.get(0),
        )
        .expect("balance")
    }

    fn check_invariants(conn: &Connection, now: i64) {
        // no balance record below zero
        assert_eq!(
            count(
                conn,
                "SELECT COUNT(*) FROM bank_balance_records WHERE CAST(amount AS REAL) < 0"
            ),
            0
        );
        // every payment has a category
        assert_eq!(
            count(
                conn,
                "SELECT COUNT(*) FROM bank_transactions WHERE category_id IS NULL"
            ),
            0
        );
        // salary in each of the 48 months
        let salary_months = count(
            conn,
            "SELECT COUNT(DISTINCT strftime('%Y-%m', booking_date, 'unixepoch'))
             FROM bank_transactions WHERE category_id = 'cat_income' AND tx_type = 'credit'
               AND description = (SELECT description FROM bank_transactions
                                  WHERE category_id = 'cat_income' ORDER BY booking_date LIMIT 1)",
        );
        let today = clock::date_of(clock::today(now));
        // the salary of the current month only exists once the 25th has passed
        let expected = if chrono::Datelike::day(&today) >= 25 {
            49
        } else {
            48
        };
        assert_eq!(salary_months, expected);
        let payments = count(conn, "SELECT COUNT(*) FROM bank_transactions");
        assert!((1_800..=2_800).contains(&payments), "payments {payments}");
        // every account stays non-negative on every day: replay the payments
        let mut stmt = conn
            .prepare(
                "SELECT bank_account_id, tx_type, CAST(amount AS REAL) FROM bank_transactions
                 ORDER BY booking_date, rowid",
            )
            .expect("prepare");
        let mut bal = std::collections::HashMap::<String, f64>::new();
        // opening balance = first month-end record minus the payments up to that day
        for (id, first_as_of, amount) in {
            let mut q = conn
                .prepare(
                    "SELECT r.bank_account_id, r.as_of, CAST(r.amount AS REAL) FROM bank_balance_records r
                     WHERE r.as_of = (SELECT MIN(as_of) FROM bank_balance_records
                                      WHERE bank_account_id = r.bank_account_id)",
                )
                .expect("prepare");
            q.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })
            .expect("query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("rows")
        } {
            let net: f64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(CASE tx_type WHEN 'credit' THEN 1 ELSE -1 END
                                        * CAST(amount AS REAL)), 0)
                     FROM bank_transactions WHERE bank_account_id = ?1 AND booking_date <= ?2",
                    rusqlite::params![id, first_as_of],
                    |r| r.get(0),
                )
                .expect("net");
            bal.insert(id, amount - net);
        }
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })
            .expect("query");
        for row in rows {
            let (id, kind, amount) = row.expect("row");
            let b = bal.entry(id).or_insert(0.0);
            *b += if kind == "credit" { amount } else { -amount };
            assert!(*b > -0.005, "negative balance {b}");
        }
    }

    #[test]
    fn bank_history_matches_the_target_state() {
        let started = std::time::Instant::now();
        let (conn, summary) = seeded("en", now_2026());
        let elapsed = started.elapsed();
        let payments = count(&conn, "SELECT COUNT(*) FROM bank_transactions");
        let (cur, sav, trv) = (
            balance_of(&conn, "EUR", false),
            balance_of(&conn, "EUR", true),
            balance_of(&conn, "USD", false),
        );
        eprintln!(
            "payments {payments}, current {cur}, savings {sav}, travel {trv}, seed {elapsed:?}"
        );
        eprintln!("{:?}", summary.counts);
        check_invariants(&conn, now_2026());
        assert!((2_500.0..=5_500.0).contains(&cur), "current {cur}");
        assert!((9_000.0..=16_000.0).contains(&sav), "savings {sav}");
        assert!((1_000.0..=1_400.0).contains(&trv), "travel {trv}");
    }

    #[test]
    fn budgets_have_two_versions_per_category() {
        let (conn, _) = seeded("en", now_2026());
        let rows = count(
            &conn,
            "SELECT COUNT(*) FROM (SELECT category_id FROM budget_goal_versions
             GROUP BY category_id HAVING COUNT(*) = 2)",
        );
        assert_eq!(rows, 5);
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM budget_goal_versions"),
            10
        );
    }

    #[test]
    fn open_plans_have_tagged_payments_and_last_year_is_closed() {
        let (conn, _) = seeded("en", now_2026());
        // Christmas has no payments before December, so it may be empty
        for name in ["Holiday", "Car"] {
            let tagged = count(
                &conn,
                &format!(
                    "SELECT COUNT(*) FROM bank_transactions b JOIN spending_plans p
                     ON p.id = b.spending_plan_id WHERE p.name = '{name}' AND p.year = 2026"
                ),
            );
            assert!(tagged >= 1, "{name} has no tagged payment");
        }
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM spending_plans"), 4);
        let closed: i64 = conn
            .query_row(
                "SELECT closed_at FROM spending_plans WHERE year = 2025",
                [],
                |r| r.get(0),
            )
            .expect("closed");
        assert_eq!(
            closed,
            clock::day_of(NaiveDate::from_ymd_opt(2025, 12, 31).expect("date"))
        );
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM cashflow_items"), 3);
    }

    #[test]
    fn same_now_gives_identical_data() {
        let (a, _) = seeded("en", now_2026());
        let (b, _) = seeded("en", now_2026());
        assert_eq!(
            count(&a, "SELECT COUNT(*) FROM bank_transactions"),
            count(&b, "SELECT COUNT(*) FROM bank_transactions")
        );
        assert_eq!(end_balances(&a), end_balances(&b));
        let sum = |c: &Connection| -> f64 {
            c.query_row(
                "SELECT SUM(CAST(amount AS REAL)) FROM bank_transactions",
                [],
                |r| r.get(0),
            )
            .expect("sum")
        };
        assert_eq!(sum(&a).to_bits(), sum(&b).to_bits());
    }

    #[test]
    fn an_earlier_now_also_passes_the_invariants() {
        // balance records are validated against the wall clock, so shift into the past
        let now = now_2026() - 200 * 86_400;
        let (conn, _) = seeded("en", now);
        check_invariants(&conn, now);
        assert!(balance_of(&conn, "EUR", true) > 0.0);
    }

    #[test]
    fn czech_run_writes_czech_names() {
        let (conn, _) = seeded("cs", now_2026());
        let names: Vec<String> = end_balances(&conn).into_iter().map(|(n, _, _)| n).collect();
        assert!(names.contains(&"Běžný účet".to_string()), "{names:?}");
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM spending_plans WHERE name = 'Dovolená'"
            ),
            2
        );
    }

    // --- investments and assets -------------------------------------------------------------

    /// The kinds that mean "your data is out of date", which the demo must not open with.
    const UPKEEP_KINDS: [&str; 6] = [
        "backup_stale",
        "balances_stale",
        "valuation_stale",
        "loan_balance_check",
        "loan_fixation_expired",
        "sync_stalled",
    ];

    fn add_profile(conn: &Connection) {
        conn.execute(
            "INSERT INTO user_profile (name, surname, email, currency, language)
             VALUES (?1, ?2, ?3, 'EUR', 'en')",
            rusqlite::params![PERSONA_NAME, PERSONA_SURNAME, PERSONA_EMAIL],
        )
        .expect("profile");
    }

    fn quantity(conn: &Connection, ticker: &str) -> f64 {
        conn.query_row(
            "SELECT CAST(quantity AS REAL) FROM stock_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .expect("quantity")
    }

    fn check_assets(conn: &Connection, now: i64) {
        let today = clock::today(now);
        // every position's quantity is the net of its trades
        let mut stmt = conn
            .prepare(
                "SELECT s.ticker, CAST(s.quantity AS REAL),
                        (SELECT SUM(CASE t.type WHEN 'buy' THEN 1 ELSE -1 END
                                    * CAST(t.quantity AS REAL))
                         FROM investment_transactions t WHERE t.investment_id = s.id)
                 FROM stock_investments s ORDER BY s.ticker",
            )
            .expect("prepare");
        let rows: Vec<(String, f64, f64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .expect("query")
            .collect::<std::result::Result<_, _>>()
            .expect("rows");
        assert_eq!(rows.len(), 7, "{rows:?}");
        for (ticker, held, net) in &rows {
            assert!((held - net).abs() < 1e-9, "{ticker}: {held} vs {net}");
        }
        // PayPal: closed, still a position with its trades
        assert_eq!(quantity(conn, "PYPL"), 0.0);
        assert_eq!(
            count(
                conn,
                "SELECT COUNT(*) FROM investment_transactions WHERE ticker = 'PYPL'"
            ),
            2
        );
        // NVIDIA: 40 bought (today's units), 15 sold
        assert_eq!(quantity(conn, "NVDA"), 25.0);
        // every trade carries the fee
        assert_eq!(
            count(
                conn,
                "SELECT COUNT(*) FROM investment_transactions WHERE fee IS NULL OR fee <> '1'"
            ),
            0
        );
        for ticker in ["MSFT", "AAPL", "NVDA", "ASML.AS", "ALV.DE"] {
            let events = count(
                conn,
                &format!("SELECT COUNT(*) FROM stock_dividend_events WHERE ticker = '{ticker}'"),
            );
            assert!(events > 0, "{ticker} has no dividends");
        }
        assert_eq!(count(conn, "SELECT COUNT(*) FROM real_estate_loans"), 1);
        assert_eq!(
            count(conn, "SELECT COUNT(*) FROM real_estate_insurances"),
            1
        );
        let flat: String = conn
            .query_row(
                "SELECT market_price FROM real_estate WHERE type = 'personal'",
                [],
                |r| r.get(0),
            )
            .expect("flat");
        assert_eq!(flat.parse::<f64>().ok(), Some(312_000.0));
        // no valuation dated today
        for table in ["real_estate_valuations", "other_asset_valuations"] {
            assert_eq!(
                count(
                    conn,
                    &format!("SELECT COUNT(*) FROM {table} WHERE valued_at >= {today}")
                ),
                0,
                "{table}"
            );
        }
        // loan balances from the app's own ledger
        let loans = crate::services::loans::list_loans(conn, today).expect("loans");
        assert_eq!(loans.len(), 2);
        for loan in &loans {
            let balance: f64 = loan.outstanding_balance.parse().expect("balance");
            let principal: f64 = loan.principal.parse().expect("principal");
            eprintln!("loan {} balance {balance}", loan.name);
            if principal > 100_000.0 {
                assert!((125_000.0..=140_000.0).contains(&balance), "{balance}");
            } else {
                assert!(balance > 0.0 && balance < principal, "{balance}");
            }
        }
        // the watch targets are not reached
        for (ticker, target, direction) in {
            let mut q = conn
                .prepare(
                    "SELECT w.ticker, CAST(w.target_price AS REAL), w.target_direction
                     FROM watched_stocks w WHERE w.target_price IS NOT NULL",
                )
                .expect("prepare");
            q.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .expect("query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("rows")
        } {
            let price: f64 = conn
                .query_row(
                    "SELECT CAST(original_price AS REAL) FROM stock_data WHERE ticker = ?1",
                    [&ticker],
                    |r| r.get(0),
                )
                .expect("price");
            let crossed = if direction == "below" {
                price <= target
            } else {
                price >= target
            };
            assert!(!crossed, "{ticker} {price} {direction} {target}");
        }
        // the reminders: the car policy, the savings rate, nothing stale or overdue
        add_profile(conn);
        let list =
            crate::services::milestones::list_milestones(conn, today, None).expect("milestones");
        for m in &list {
            eprintln!(
                "milestone {} {} {} {:?}",
                m.kind, m.stage, m.title, m.due_day
            );
        }
        let car = t_en(text::T::PolCar);
        assert!(
            list.iter()
                .any(|m| m.kind == "insurance_anniversary" && m.title == car),
            "no car anniversary"
        );
        assert!(list.iter().any(|m| m.kind == "savings_rate_end"));
        assert!(list
            .iter()
            .all(|m| !UPKEEP_KINDS.contains(&m.kind.as_str()) && m.kind != "watch_target"));
    }

    fn t_en(key: text::T) -> &'static str {
        text::t("en", key)
    }

    #[test]
    fn investments_and_assets_match_their_facts() {
        let (conn, summary) = seeded("en", now_2026());
        eprintln!("{:?}", summary.counts);
        assert!(invest::nvda_bought_before_split(now_2026()));
        check_assets(&conn, now_2026());
    }

    #[test]
    fn an_earlier_now_keeps_the_asset_invariants() {
        let now = now_2026() - 200 * 86_400;
        let (conn, _) = seeded("en", now);
        check_assets(&conn, now);
    }

    #[test]
    fn nvidia_is_bought_after_the_split_once_m43_passes_it() {
        let later = clock::day_of(NaiveDate::from_ymd_opt(2028, 3, 1).expect("date"));
        assert!(!invest::nvda_bought_before_split(later));
        let earlier = clock::day_of(NaiveDate::from_ymd_opt(2027, 12, 1).expect("date"));
        assert!(invest::nvda_bought_before_split(earlier));
    }
}
