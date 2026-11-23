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
// generated data: real rates such as 1.0472 are not approximations of constants
#[allow(clippy::approx_constant)]
#[rustfmt::skip]
mod prices_data;
mod refresh;
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
    /// Today's value of each class in EUR (loans as a positive amount owed).
    pub classes_eur: BTreeMap<&'static str, f64>,
}

/// Fill a migrated database with the demo data set.
///
/// Contract for the caller:
/// - pass a freshly migrated, empty profile database, and call this once per database (a
///   second run duplicates the data or fails on unique keys);
/// - on `Err`, discard the database: there is no outer transaction (the bank, budget and plan
///   services open their own and SQLite cannot nest them), so each part commits itself and a
///   failure leaves the parts before it in place;
/// - it replaces the process-wide exchange-rate snapshot (`services::fx`) with the built-in
///   rates for `opts.now`, which every later conversion in the process uses until the next
///   rate load.
pub fn seed(conn: &Connection, opts: &DemoOptions) -> Result<DemoSummary> {
    let mut summary = DemoSummary::default();

    history::seed_fx(conn, opts, &mut summary)?; // USD and DKK rates, loaded for conversions
    bank::seed(conn, opts, &mut summary)?; // accounts, payments, balance records
    planning::seed(conn, opts, &mut summary)?; // budgets, spending plans, cashflow items
    invest::seed(conn, opts, &mut summary)?; // stocks, crypto, bond, watchlist
    assets::seed(conn, opts, &mut summary)?; // property, loans, insurance, other assets
    history::seed(conn, opts, &mut summary)?; // daily values, portfolio days, markers

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, NaiveDate, TimeZone, Utc};

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

    /// Seeding replaces the process-wide exchange-rate snapshot, so tests that seed run one at
    /// a time (each holds this for its whole body).
    static SEED_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SEED_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        // salary in each of the 49 months (paid on the 1st, so the current month has it)
        let salary_months = count(
            conn,
            "SELECT COUNT(DISTINCT strftime('%Y-%m', booking_date, 'unixepoch'))
             FROM bank_transactions WHERE tx_type = 'credit' AND description = 'Salary'",
        );
        assert_eq!(salary_months, 49);
        // the travel account ends with what the last trip left, unless a trip is under way
        // (its top-up is in, its later payments are not)
        let travel = balance_of(conn, "USD", false);
        let today = clock::today(now);
        let year = clock::date_of(today).year();
        let trip = schedule::ymd(year, schedule::LONG_TRIP.0, schedule::LONG_TRIP.1);
        let under_way =
            today >= trip - 7 * clock::DAY && today < trip + schedule::LONG_TRIP_DAYS * clock::DAY;
        if under_way {
            assert!(travel >= 1_000.0, "travel {travel} during the trip");
        } else {
            assert!((1_000.0..=1_400.0).contains(&travel), "travel {travel}");
        }
        assert!(salary_this_month(conn, now));
        // at most one food delivery a day
        let delivery = text::t("en", text::T::FoodDelivery);
        assert_eq!(
            count(
                conn,
                &format!(
                    "SELECT COUNT(*) FROM (SELECT booking_date FROM bank_transactions
                     WHERE description = '{delivery}' GROUP BY booking_date HAVING COUNT(*) > 1)"
                )
            ),
            0
        );
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
        // opening balance = first record of the payment history minus the payments up to it
        // (the year before the payments has records only)
        for (id, first_as_of, amount) in {
            let mut q = conn
                .prepare(
                    "SELECT r.bank_account_id, r.as_of, CAST(r.amount AS REAL) FROM bank_balance_records r
                     WHERE r.as_of = (SELECT MIN(as_of) FROM bank_balance_records
                                      WHERE bank_account_id = r.bank_account_id
                                        AND as_of >= (SELECT MIN(booking_date)
                                                      FROM bank_transactions))",
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
        let _serial = serial();
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
        // the salary arrives on the 1st, so mid-month the current account is fuller
        assert!((2_500.0..=7_000.0).contains(&cur), "current {cur}");
        let opening: Vec<f64> = {
            let mut q = conn
                .prepare(
                    "SELECT CAST(r.amount AS REAL) FROM bank_balance_records r
                     JOIN bank_accounts a ON a.id = r.bank_account_id
                     WHERE a.currency = 'EUR'
                       AND r.as_of = (SELECT MIN(booking_date) FROM bank_transactions) - 86400
                     ORDER BY a.account_type",
                )
                .expect("prepare");
            q.query_map([], |r| r.get(0))
                .expect("query")
                .collect::<std::result::Result<_, _>>()
                .expect("rows")
        };
        eprintln!("opening balances (current, savings): {opening:?}");
        assert_eq!(opening.len(), 2);
        assert_eq!(opening[0], bank::OPEN_CURRENT_EUR);
        assert!((40_000.0..=90_000.0).contains(&opening[1]), "{opening:?}");
        assert!((9_000.0..=16_000.0).contains(&sav), "savings {sav}");
        assert!((1_000.0..=1_400.0).contains(&trv), "travel {trv}");
    }

    #[test]
    fn budgets_have_two_versions_per_category() {
        let _serial = serial();
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
        let _serial = serial();
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
        let _serial = serial();
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
        let _serial = serial();
        // balance records are validated against the wall clock, so shift into the past
        let now = now_2026() - 200 * 86_400;
        let (conn, _) = seeded("en", now);
        check_invariants(&conn, now);
        assert!(balance_of(&conn, "EUR", true) > 0.0);
    }

    #[test]
    fn czech_run_writes_czech_names() {
        let _serial = serial();
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
        // every trade is booked at the close of a day its instrument traded
        let mut q = conn
            .prepare(
                "SELECT ticker, transaction_date, CAST(price_per_unit AS REAL)
                 FROM investment_transactions",
            )
            .expect("prepare");
        let trades: Vec<(String, i64, f64)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .expect("query")
            .collect::<std::result::Result<_, _>>()
            .expect("rows");
        for (ticker, day, price) in &trades {
            assert_eq!(
                prices::trading_day(ticker, *day),
                *day,
                "{ticker} on a holiday"
            );
            let (close, _) = prices::price_on(ticker, *day).expect("price");
            assert!(
                (price / close - 1.0).abs() < 0.0005,
                "{ticker} {price} vs {close}"
            );
        }
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
        // no premium is paid before its policy starts
        for (policy, premium) in [
            (text::T::PolHome, text::T::PremHome),
            (text::T::PolLife, text::T::PremLife),
            (text::T::PolCar, text::T::PremCar),
            (text::T::PolLiability, text::T::PremLiability),
            (text::T::PolTravel, text::T::PremTravel),
        ] {
            let early = count(
                conn,
                &format!(
                    "SELECT COUNT(*) FROM bank_transactions
                     WHERE description = '{}' AND booking_date <
                         (SELECT start_date FROM insurance_policies WHERE policy_name = '{}')",
                    t_en(premium),
                    t_en(policy)
                ),
            );
            assert_eq!(early, 0, "{policy:?} paid before it starts");
            let paid = count(
                conn,
                &format!(
                    "SELECT COUNT(*) FROM bank_transactions WHERE description = '{}'",
                    t_en(premium)
                ),
            );
            assert!(paid > 0, "{premium:?}");
        }
        // the Ethereum sale credits exactly its proceeds in EUR
        let (proceeds_usd, sold_on): (f64, i64) = conn
            .query_row(
                "SELECT CAST(quantity AS REAL) * CAST(price_per_unit AS REAL), transaction_date
                 FROM crypto_transactions WHERE type = 'sell'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("sale");
        let credited: f64 = conn
            .query_row(
                "SELECT CAST(amount AS REAL) FROM bank_transactions
                 WHERE description = 'Crypto sale'",
                [],
                |r| r.get(0),
            )
            .expect("credit");
        let expected = (proceeds_usd / prices::usd_per_eur(sold_on) * 100.0).round() / 100.0;
        assert!(
            (credited - expected).abs() < 0.005,
            "{credited} vs {expected}"
        );

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
        let _serial = serial();
        let (conn, summary) = seeded("en", now_2026());
        eprintln!("{:?}", summary.counts);
        assert!(schedule::nvda_bought_before_split(now_2026()));
        check_assets(&conn, now_2026());
    }

    #[test]
    fn an_earlier_now_keeps_the_asset_invariants() {
        let _serial = serial();
        let now = now_2026() - 200 * 86_400;
        let (conn, _) = seeded("en", now);
        check_assets(&conn, now);
    }

    #[test]
    fn nvidia_is_bought_after_the_split_once_m43_passes_it() {
        let later = clock::day_of(NaiveDate::from_ymd_opt(2028, 3, 1).expect("date"));
        assert!(!schedule::nvda_bought_before_split(later));
        let earlier = clock::day_of(NaiveDate::from_ymd_opt(2027, 12, 1).expect("date"));
        assert!(schedule::nvda_bought_before_split(earlier));
    }

    // --- derived history ----------------------------------------------------------------------

    /// What the first start plans: nothing while the built-in real data reaches yesterday;
    /// otherwise only a settle of the days after the data (the drift-priced tail), never a
    /// full rebuild.
    fn check_startup_plan(conn: &Connection, now: i64) {
        use crate::services::history_plan;
        let yesterday = clock::today(now) - clock::DAY;
        let plan = history_plan::plan_startup(conn, now).expect("plan");
        assert!(!plan.one_time, "no full rebuild");
        if yesterday <= prices::data_through() {
            assert!(plan.jobs.is_empty(), "{:?}", plan.jobs);
            assert!(plan.from_day.is_none() && plan.settles_through.is_none());
        } else {
            let tail = prices::data_through() + clock::DAY;
            assert_eq!(plan.from_day, Some(tail));
            assert_eq!(plan.settles_through, Some(yesterday));
            assert!(!plan.jobs.is_empty());
            assert!(
                plan.jobs.iter().all(|j| j.from_day() >= tail),
                "{:?}",
                plan.jobs
            );
        }
    }

    fn check_history(conn: &Connection, summary: &DemoSummary, now: i64) {
        use crate::services::{fx_backfill, history_plan};
        let today = clock::today(now);
        let first = clock::months_ago(now, schedule::FLAT_BOUGHT_M, schedule::FLAT_DAY);
        // one row per day from the flat purchase to today; only today's is live
        let days = (today - first) / clock::DAY + 1;
        assert_eq!(
            count(
                conn,
                &format!(
                    "SELECT COUNT(DISTINCT recorded_at / 86400) FROM portfolio_metrics_history
                     WHERE recorded_at >= {first}"
                )
            ),
            days
        );
        assert_eq!(
            count(
                conn,
                &format!(
                    "SELECT COUNT(*) FROM portfolio_metrics_history WHERE recorded_at < {first}"
                )
            ),
            0
        );
        let live: Vec<(i64, String)> = {
            let mut q = conn
                .prepare(
                    "SELECT recorded_at, source FROM portfolio_metrics_history
                     WHERE source = 'live'",
                )
                .expect("prepare");
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .expect("query")
                .collect::<std::result::Result<_, _>>()
                .expect("rows")
        };
        assert_eq!(live.len(), 1);
        assert_eq!(clock::today(live[0].0), today);
        // both markers set; the first unlock plans nothing and fetches no rates
        assert_eq!(
            history_plan::stored_rebuild_version(conn).expect("version"),
            history_plan::REBUILD_VERSION
        );
        assert_eq!(
            history_plan::stored_settled_through(conn).expect("mark"),
            Some(today - clock::DAY)
        );
        check_startup_plan(conn, now);
        assert_eq!(
            fx_backfill::missing_fx_ranges(conn, today).expect("ranges"),
            Vec::<(i64, i64)>::new()
        );
        let quotes = crate::services::fx::Quotes::from_stored([
            ("USD", prices::usd_per_eur(today)),
            ("DKK", prices::dkk_per_eur(today)),
        ]);
        assert_eq!(
            fx_backfill::unquoted_currencies(conn, &quotes).expect("unquoted"),
            Vec::<String>::new()
        );
        assert!(fx_backfill::stale_currencies(conn, today)
            .expect("stale")
            .is_empty());
        // today's net worth and every class
        let nw = summary.net_worth_eur;
        assert!((240_000.0..=380_000.0).contains(&nw), "net worth {nw}");
        for (class, value) in &summary.classes_eur {
            assert!(*value > 0.0, "{class} is empty today");
        }
        assert_eq!(summary.classes_eur.len(), 8);
        // four years ago was poorer, and the stocks never drop to nothing in four years
        let (points, _) =
            crate::services::net_worth::history_in(conn, "EUR", Some(first), Some(now))
                .expect("history");
        assert_eq!(points.len() as i64, days);
        let four_years = clock::months_ago(now, 48, clock::date_of(today).day());
        let then = points
            .iter()
            .find(|p| clock::today(p.recorded_at) == four_years)
            .and_then(|p| p.net_worth)
            .expect("four years ago");
        let last = points.last().and_then(|p| p.net_worth).expect("today");
        assert!(then < last, "{then} vs {last}");
        assert!((last - nw).abs() < 1.0, "{last} vs {nw}");
        // the first trade is at M46, so "four years" starts there
        let first_trade: i64 = conn
            .query_row(
                "SELECT MIN(transaction_date) FROM investment_transactions",
                [],
                |r| r.get(0),
            )
            .expect("first trade");
        assert!(first_trade < clock::months_ago(now, 45, 1));
        for p in points.iter().filter(|p| p.recorded_at >= first_trade) {
            assert!(
                p.classes.investments.is_some_and(|v| v > 0.0),
                "no stocks on {}",
                clock::date_of(p.recorded_at)
            );
            assert!(p.net_worth.is_some(), "{}", clock::date_of(p.recorded_at));
        }
        // the first day already holds cash
        assert!(points[0].classes.savings.is_some_and(|v| v > 0.0));
        // no day moves net worth by more than 12 000 except a revaluation of the flat
        let revaluations: std::collections::HashSet<i64> = {
            let mut q = conn
                .prepare(
                    "SELECT v.valued_at FROM real_estate_valuations v
                     JOIN real_estate r ON r.id = v.real_estate_id WHERE r.type = 'personal'",
                )
                .expect("prepare");
            q.query_map([], |r| r.get(0))
                .expect("query")
                .collect::<std::result::Result<_, _>>()
                .expect("rows")
        };
        let mut largest = (0.0_f64, 0_i64);
        for pair in points.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let delta = b.net_worth.unwrap_or(0.0) - a.net_worth.unwrap_or(0.0);
            let day = clock::today(b.recorded_at);
            if delta.abs() > 12_000.0 {
                eprintln!(
                    "move {delta:.2} on {} (flat revaluation)",
                    clock::date_of(day)
                );
                assert!(
                    revaluations.contains(&day),
                    "{delta} on {}",
                    clock::date_of(day)
                );
            } else if delta.abs() > largest.0.abs() {
                largest = (delta, day);
            }
        }
        eprintln!(
            "largest move without a revaluation: {:.2} on {}",
            largest.0,
            clock::date_of(largest.1)
        );
    }

    #[test]
    fn history_covers_every_day_and_the_first_unlock_has_nothing_to_do() {
        let _serial = serial();
        let started = std::time::Instant::now();
        let (conn, summary) = seeded("en", now_2026());
        let elapsed = started.elapsed();
        eprintln!("seed {elapsed:?}");
        eprintln!(
            "net worth {:.2} EUR, {:?}",
            summary.net_worth_eur, summary.classes_eur
        );
        eprintln!("{:?}", summary.counts);
        check_history(&conn, &summary, now_2026());
    }

    #[test]
    fn an_earlier_now_keeps_the_history_invariants() {
        let _serial = serial();
        let now = now_2026() - 200 * 86_400;
        let (conn, summary) = seeded("en", now);
        check_history(&conn, &summary, now);
    }

    #[test]
    fn every_language_seeds() {
        let _serial = serial();
        for lang in text::LANGUAGES {
            let (conn, summary) = seeded(lang, now_2026());
            assert!(summary.net_worth_eur > 0.0, "{lang}");
            let flat: String = conn
                .query_row(
                    "SELECT name FROM real_estate WHERE type = 'personal'",
                    [],
                    |r| r.get(0),
                )
                .expect("flat");
            assert_eq!(flat, text::t(lang, text::T::PropFlat));
        }
    }

    /// Whether the calendar month of `now` has a salary payment.
    fn salary_this_month(conn: &Connection, now: i64) -> bool {
        let from = clock::month_start(clock::today(now));
        count(
            conn,
            &format!(
                "SELECT COUNT(*) FROM bank_transactions
                 WHERE description = '{}' AND tx_type = 'credit' AND booking_date >= {from}",
                text::t("en", text::T::Salary)
            ),
        ) == 1
    }

    #[test]
    fn the_current_month_has_its_salary_on_any_day() {
        let _serial = serial();
        for (y, m, d) in [(2026, 10, 2), (2026, 9, 14), (2026, 8, 28)] {
            let now = Utc
                .with_ymd_and_hms(y, m, d, 12, 0, 0)
                .single()
                .expect("date")
                .timestamp();
            let (conn, _) = seeded("en", now);
            assert!(salary_this_month(&conn, now), "{y}-{m}-{d}");
            check_invariants(&conn, now);
        }
    }

    #[test]
    fn plans_are_sized_from_their_spend() {
        let _serial = serial();
        let (conn, _) = seeded("en", now_2026());
        let used: std::collections::HashMap<(String, i32), f64> =
            crate::services::spending_plans::plan_index(&conn)
                .expect("plans")
                .into_iter()
                .map(|s| {
                    let spent: f64 = s.spent.and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    let amount: f64 = s.plan.amount.parse().expect("amount");
                    eprintln!(
                        "plan {} {}: {spent:.2} of {amount} ({:.0} %)",
                        s.plan.name,
                        s.plan.year,
                        spent / amount * 100.0
                    );
                    ((s.plan.name, s.plan.year), spent / amount)
                })
                .collect();
        let get = |name: &str, year: i32| used[&(name.to_string(), year)];
        assert!((0.80..=0.95).contains(&get("Holiday", 2026)));
        assert!((0.70..=0.90).contains(&get("Car", 2026)));
        assert!((0.90..=1.05).contains(&get("Holiday", 2025)));
        let christmas: String = conn
            .query_row(
                "SELECT amount FROM spending_plans WHERE name = 'Christmas and gifts'",
                [],
                |r| r.get(0),
            )
            .expect("christmas");
        assert_eq!(christmas.parse::<f64>().ok(), Some(600.0));
    }

    #[test]
    fn broker_deposits_cover_the_stock_purchases() {
        let _serial = serial();
        let (conn, _) = seeded("en", now_2026());
        let deposits: f64 = conn
            .query_row(
                "SELECT SUM(CAST(amount AS REAL)) FROM bank_transactions
                 WHERE description = 'Broker deposit' AND tx_type = 'debit'",
                [],
                |r| r.get(0),
            )
            .expect("deposits");
        // every buy in EUR at its day's rate, fee included
        let cost: f64 = conn
            .query_row(
                "SELECT SUM((CAST(t.quantity AS REAL) * CAST(t.price_per_unit AS REAL)
                             + CAST(t.fee AS REAL))
                            / COALESCE((SELECT r.rate FROM exchange_rate_history r
                                        WHERE r.currency = t.currency
                                          AND r.date = t.transaction_date), 1.0))
                 FROM investment_transactions t WHERE t.type = 'buy'",
                [],
                |r| r.get(0),
            )
            .expect("cost");
        eprintln!("broker deposits {deposits:.2} EUR, stock buys {cost:.2} EUR");
        assert!(
            deposits >= cost && deposits <= cost * 1.10,
            "{deposits} vs {cost}"
        );
    }

    /// Edge dates: month and year ends, a leap-adjacent day, the middle of the September trip,
    /// all in the past of the real clock (the services reject later dates) and late enough that
    /// every scheduled trade has a built-in price.
    #[test]
    fn edge_dates_pass_every_check() {
        let _serial = serial();
        for (y, m, d) in [
            (2026, 1, 1),
            (2026, 2, 28),
            (2026, 3, 31),
            (2026, 8, 31),
            (2026, 9, 8),
            (2025, 12, 1),
        ] {
            let now = Utc
                .with_ymd_and_hms(y, m, d, 12, 0, 0)
                .single()
                .expect("date")
                .timestamp();
            eprintln!("edge date {y}-{m:02}-{d:02}");
            let conn = db();
            let summary = seed(&conn, &opts("en", now)).expect("seed");
            check_invariants(&conn, now);
            check_history(&conn, &summary, now);
            // check_assets adds the profile the reminders read, so it runs last
            check_assets(&conn, now);
        }
    }

    /// The post-2028 NVIDIA path and a leap day cannot be seeded: the services validate dates
    /// against the real clock and refuse a future `now`. What can be checked without a seed is
    /// the trade calendar the seed books.
    #[test]
    fn the_trade_calendar_holds_after_2028_and_on_a_leap_day() {
        for (y, m, d) in [(2028, 3, 1), (2028, 2, 29)] {
            let now = clock::day_of(NaiveDate::from_ymd_opt(y, m, d).expect("date")) + 43_200;
            assert!(!schedule::nvda_bought_before_split(now));
            let trades = schedule::stock_trades(now);
            let mut held = std::collections::HashMap::<&str, f64>::new();
            let mut sorted = trades.clone();
            sorted.sort_by_key(|t| t.day);
            for t in &sorted {
                assert!(t.day > 0 && t.day < clock::today(now), "{t:?}");
                assert!(t.quantity > 0.0);
                assert_eq!(t.units_factor, 1.0, "no split entry: {t:?}");
                let h = held.entry(t.ticker).or_insert(0.0);
                *h += if t.sell { -t.quantity } else { t.quantity };
                assert!(
                    *h >= 0.0,
                    "{} oversold on {}",
                    t.ticker,
                    clock::date_of(t.day)
                );
            }
            assert_eq!(held["NVDA"], 25.0);
            assert_eq!(held["PYPL"], 0.0);
            let etf = schedule::etf_buys(now);
            assert_eq!(etf.len(), schedule::BROKER_FROM_M as usize);
            assert!(etf.iter().all(|b| b.day < clock::today(now)));
            // the car policy still starts at the purchase and its next anniversary is near
            let premiums = schedule::car_premium_days(now);
            assert_eq!(
                premiums.first().copied(),
                Some(schedule::car_bought_day(now))
            );
            let next = schedule::car_policy_anniversary(now) - clock::today(now);
            assert!((30..=40).contains(&(next / clock::DAY)));
        }
    }

    #[test]
    fn counterparties_are_names_or_translated() {
        let _serial = serial();
        const REMOVED: [&str; 10] = [
            "Tax Office",
            "City Hall",
            "Building Association",
            "City Transit",
            "City Pharmacy",
            "Souvenir Market",
            "Pier Market",
            "City Museum",
            "City Art Museum",
            "Notary Hartmann",
        ];
        for lang in ["cs", "ja"] {
            let (conn, _) = seeded(lang, now_2026());
            let mut q = conn
                .prepare("SELECT DISTINCT counterparty_name FROM bank_transactions")
                .expect("prepare");
            let names: Vec<Option<String>> = q
                .query_map([], |r| r.get(0))
                .expect("query")
                .collect::<std::result::Result<_, _>>()
                .expect("rows");
            for name in names.into_iter().flatten() {
                assert!(!REMOVED.contains(&name.as_str()), "{lang}: {name}");
            }
            // trip payments never borrow the description of an everyday category
            assert_eq!(
                count(
                    &conn,
                    &format!(
                        "SELECT COUNT(*) FROM bank_transactions WHERE category_id = 'cat_travel'
                         AND description IN ('{}', '{}', '{}', '{}')",
                        text::t(lang, text::T::Groceries),
                        text::t(lang, text::T::Restaurant),
                        text::t(lang, text::T::Taxi),
                        text::t(lang, text::T::Cafe)
                    )
                ),
                0
            );
        }
    }

    /// A demo opened after the built-in data ends: the first start settles only the tail.
    /// The seed runs at the fixed `now` (the services refuse a later one); the plan is asked for
    /// a later `now`.
    #[test]
    fn a_later_start_settles_only_the_tail_after_the_data() {
        let _serial = serial();
        let (conn, _) = seeded("en", now_2026());
        assert!(clock::today(now_2026()) - clock::DAY <= prices::data_through());
        check_startup_plan(&conn, now_2026());
        check_startup_plan(&conn, now_2026() + 9 * clock::DAY);
    }
}
