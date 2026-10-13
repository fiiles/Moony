//! Which history work a start or a write needs (spec
//! `docs/specs/2026-10-13-history-from-dated-facts-design.md`, A6 and A7).
//!
//! Pure planning: these functions read facts through a plain `&Connection` and return
//! [`Job`]s for the queue in `services/history_recalc.rs`; they queue nothing and fetch
//! nothing, so every rule is unit-tested with a hand-written schema.
//!
//! * **Startup (A7).** [`plan_startup`]: the first start of a version rebuilds everything
//!   once; later starts only fill the days that are missing.
//! * **Triggers (A6).** One helper per write that has a rule beyond "this day": an edit
//!   queues from the earlier of its old and new date, and a change that moves a whole class
//!   (an account leaving the balance, a property changing type) queues from the first fact,
//!   which is `Days(0)` — `history_days::plan_rebuild` clamps a day before the first fact
//!   to it.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::{BankAccount, RealEstate};
use crate::services::history_days::earliest_fact_day;
use crate::services::history_recalc::{
    all_ticker_jobs, crypto_rebuild_jobs, rebuild_jobs, BatchOutcome, Job,
};
use crate::services::portfolio_history::SECONDS_PER_DAY;

const DAY: i64 = SECONDS_PER_DAY;

/// The version of the history rebuild a profile has had. Version 1 is the first rebuild from
/// dated facts: stored history of earlier versions copied static classes from the nearest
/// live row and valued past days with today's holdings.
pub const REBUILD_VERSION: i64 = 1;

const VERSION_KEY: &str = "history_rebuild_version";

/// The rebuild version this profile has completed; 0 for a profile that never had one.
pub fn stored_rebuild_version(conn: &Connection) -> Result<i64> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [VERSION_KEY],
            |row| row.get(0),
        )
        .optional()?;
    Ok(stored
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0))
}

/// Does the finished batch of the one-time rebuild earn the version flag? The rebuild is the
/// days re-derived from the dated facts; a ticker the provider cannot serve (a delisted symbol,
/// a CoinGecko rate limit) is reported `failed` and logged, and a later write of that ticker or
/// the startup gap fill retries it, but it must not run the whole rebuild again at every start.
pub fn rebuild_version_earned(outcome: &BatchOutcome) -> bool {
    outcome.days_ok
}

/// Record that the rebuild of [`REBUILD_VERSION`] finished.
pub fn store_rebuild_version(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![VERSION_KEY, REBUILD_VERSION.to_string()],
    )?;
    Ok(())
}

/// What a start queues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupPlan {
    pub jobs: Vec<Job>,
    /// The one-time rebuild of this version: its batch stores the version flag.
    pub one_time: bool,
    /// The day the days rebuild starts at, when one is planned.
    pub from_day: Option<i64>,
}

/// Plan what a start has to rebuild (A7). A profile below [`REBUILD_VERSION`] rebuilds
/// everything once: every stock and crypto ticker from its first transaction, and the days
/// from the first fact. Otherwise the first missing day `G` in `[first fact, yesterday]`
/// starts a repair: the tickers held from `G`, and the days from `G`. `today` is today's UTC
/// day start.
pub fn plan_startup(conn: &Connection, today: i64) -> Result<StartupPlan> {
    let first_fact = earliest_fact_day(conn)?;
    if stored_rebuild_version(conn)? < REBUILD_VERSION {
        let mut jobs = all_ticker_jobs(conn)?;
        jobs.extend(first_fact.map(Job::Days));
        return Ok(StartupPlan {
            jobs,
            one_time: true,
            from_day: first_fact,
        });
    }

    let gap = match first_fact {
        Some(first) => first_missing_day(conn, first, today - DAY)?,
        None => None,
    };
    let Some(gap) = gap else {
        return Ok(StartupPlan {
            jobs: Vec::new(),
            one_time: false,
            from_day: None,
        });
    };
    let mut jobs = held_ticker_jobs(conn, gap)?;
    jobs.push(Job::Days(gap));
    Ok(StartupPlan {
        jobs,
        one_time: false,
        from_day: Some(gap),
    })
}

/// "Recalculate everything": the days from the first fact and every ticker from its first
/// transaction. No jobs without a dated fact.
pub fn plan_everything(conn: &Connection) -> Result<StartupPlan> {
    let first_fact = earliest_fact_day(conn)?;
    let mut jobs = all_ticker_jobs(conn)?;
    jobs.extend(first_fact.map(Job::Days));
    Ok(StartupPlan {
        jobs,
        one_time: false,
        from_day: first_fact,
    })
}

/// The first day in `[from_day, last_day]` without a row in `portfolio_metrics_history`.
fn first_missing_day(conn: &Connection, from_day: i64, last_day: i64) -> Result<Option<i64>> {
    let from_day = from_day.div_euclid(DAY) * DAY;
    if from_day > last_day {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT DISTINCT (recorded_at / 86400) * 86400 FROM portfolio_metrics_history
         WHERE recorded_at >= ?1 AND recorded_at < ?2",
    )?;
    let existing: HashSet<i64> = stmt
        .query_map([from_day, last_day + DAY], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut day = from_day;
    while day <= last_day {
        if !existing.contains(&day) {
            return Ok(Some(day));
        }
        day += DAY;
    }
    Ok(None)
}

/// Ticker jobs for the tickers that matter from `from_day` on: a net position left after
/// their transactions, or a trade on or after the day (a position sold since still has days
/// in the range). Each starts at the later of `from_day` and its first transaction.
fn held_ticker_jobs(conn: &Connection, from_day: i64) -> Result<Vec<Job>> {
    let mut jobs = Vec::new();
    for (table, crypto) in [
        ("investment_transactions", false),
        ("crypto_transactions", true),
    ] {
        let mut stmt = conn.prepare(&format!(
            "SELECT ticker FROM {table} GROUP BY ticker
             HAVING SUM(CASE WHEN type = 'buy' THEN CAST(quantity AS REAL)
                             ELSE -CAST(quantity AS REAL) END) > 0.000001
                 OR MAX(transaction_date) >= ?1
             ORDER BY ticker"
        ))?;
        let tickers: Vec<String> = stmt
            .query_map([from_day], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let firsts = if crypto {
            crypto_rebuild_jobs(conn, &tickers)?
        } else {
            rebuild_jobs(conn, &tickers)?
        };
        for (ticker, first) in firsts {
            let from = from_day.max(first);
            jobs.push(if crypto {
                Job::CryptoTicker(ticker, from)
            } else {
                Job::StockTicker(ticker, from)
            });
        }
    }
    Ok(jobs)
}

// --- triggers (A6) -------------------------------------------------------------------------

/// The earlier of an old and a new date: an edit changes history from whichever came first.
/// A missing old date (nothing was set before) leaves the new one.
pub fn earlier(old: Option<i64>, new: i64) -> i64 {
    old.map_or(new, |old| old.min(new))
}

/// A new bank account: its opening record's day, else (an undated balance, carried on every
/// day) the first fact. An account that counts nothing (excluded from the balance, or a zero
/// balance) changes no day.
pub fn account_created(account: &BankAccount) -> Option<Job> {
    if account.exclude_from_balance {
        return None;
    }
    if let Some(day) = account.balance_as_of {
        return Some(Job::Days(day));
    }
    let nonzero = account
        .balance
        .trim()
        .parse::<f64>()
        .is_ok_and(|balance| balance != 0.0);
    nonzero.then_some(Job::Days(0))
}

/// An edited bank account: leaving or entering the balance, or a currency change, moves the
/// account on every day, so the history is rebuilt from the first fact. Nothing else about an
/// account is part of the history.
pub fn account_updated(old: &BankAccount, new: &BankAccount) -> Option<Job> {
    (old.exclude_from_balance != new.exclude_from_balance
        || !old.currency.eq_ignore_ascii_case(&new.currency))
    .then_some(Job::Days(0))
}

/// A balance recorded on `as_of` for an account in the state `before`. The record holds from
/// its day, so the history changes from there — except for the first dated record of an
/// account whose undated balance was carried on every day: that balance now stops at the
/// record (D4), so every day before it changes too.
pub fn balance_recorded(before: &BankAccount, as_of: i64) -> Job {
    let carried = before.balance_as_of.is_none()
        && !before.exclude_from_balance
        && before
            .balance
            .trim()
            .parse::<f64>()
            .is_ok_and(|balance| balance != 0.0);
    if carried {
        Job::Days(0)
    } else {
        Job::Days(as_of)
    }
}

/// A deleted bank account leaves the savings of every day it was counted on.
pub fn account_deleted() -> Job {
    Job::Days(0)
}

/// The facts of a property the history is derived from (the part of a record an edit can
/// change in the history).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyFacts {
    pub property_type: String,
    pub purchase_date: Option<i64>,
    pub purchase_price: String,
    pub purchase_currency: String,
    pub market_currency: String,
}

impl From<&RealEstate> for PropertyFacts {
    fn from(property: &RealEstate) -> Self {
        Self {
            property_type: property.property_type.to_lowercase(),
            purchase_date: property.purchase_date,
            purchase_price: property.purchase_price.trim().to_string(),
            purchase_currency: property.purchase_price_currency.to_uppercase(),
            market_currency: property.market_price_currency.to_uppercase(),
        }
    }
}

/// A new property: the earlier of its purchase date and its first valuation day (a property
/// is counted from its first estimate, its purchase price from the purchase date on). A
/// property with neither is not counted.
pub fn property_created(purchase_date: Option<i64>, first_valuation: Option<i64>) -> Option<Job> {
    match (purchase_date, first_valuation) {
        (Some(a), Some(b)) => Some(Job::Days(a.min(b))),
        (a, b) => a.or(b).map(Job::Days),
    }
}

/// An edited property. A change of type or currency moves the property between classes or
/// units on every day it is counted: from the first fact. A change of the purchase date or
/// price changes the line from the earlier of the old and new purchase date. Anything else
/// (a name, a rent, a photo) is not part of the history.
pub fn property_updated(old: &PropertyFacts, new: &PropertyFacts) -> Option<Job> {
    if old.property_type != new.property_type
        || old.purchase_currency != new.purchase_currency
        || old.market_currency != new.market_currency
    {
        return Some(Job::Days(0));
    }
    if old.purchase_date == new.purchase_date && old.purchase_price == new.purchase_price {
        return None;
    }
    match (old.purchase_date, new.purchase_date) {
        (Some(a), Some(b)) => Some(Job::Days(a.min(b))),
        (a, b) => a.or(b).map(Job::Days),
    }
}

/// A deleted property leaves the value of every day it was counted on.
pub fn property_deleted() -> Job {
    Job::Days(0)
}

/// An edited other asset. A currency change relabels the asset in every day's other-assets
/// breakdown: from the first fact. `new_currency` is `None` when the edit carries none (the
/// stored one stays). A price edit is a valuation dated today (no history to repair), and
/// quantities and costs come from transactions, so nothing else is part of the history.
pub fn other_asset_updated(old_currency: &str, new_currency: Option<&str>) -> Option<Job> {
    let new_currency = new_currency?.trim();
    (!old_currency.trim().eq_ignore_ascii_case(new_currency)).then_some(Job::Days(0))
}

/// A deleted other asset: from its first transaction (it was counted from there), else the
/// first fact.
pub fn other_asset_deleted(first_transaction: Option<i64>) -> Job {
    Job::Days(first_transaction.unwrap_or(0))
}

/// Bonds have no purchase date: they are carried on every day (D3), so any change to a bond
/// rebuilds from the first fact.
pub fn bond_changed() -> Job {
    Job::Days(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::history_days::rebuild_days;

    const BASE: i64 = 1_700_006_400;

    fn d(n: i64) -> i64 {
        BASE + n * DAY
    }

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE portfolio_metrics_history (
                id TEXT PRIMARY KEY,
                recorded_at INTEGER NOT NULL,
                is_stale INTEGER NOT NULL DEFAULT 0,
                investments_by_currency TEXT NOT NULL DEFAULT '{}',
                crypto_by_currency TEXT NOT NULL DEFAULT '{}',
                savings_by_currency TEXT NOT NULL DEFAULT '{}',
                bonds_by_currency TEXT NOT NULL DEFAULT '{}',
                real_estate_by_currency TEXT NOT NULL DEFAULT '{}',
                loans_by_currency TEXT NOT NULL DEFAULT '{}',
                other_assets_by_currency TEXT NOT NULL DEFAULT '{}',
                source TEXT NOT NULL DEFAULT 'live',
                real_estate_personal_by_currency TEXT NOT NULL DEFAULT '{}'
            );
            CREATE TABLE bank_accounts (
                id TEXT PRIMARY KEY,
                currency TEXT NOT NULL DEFAULT 'CZK',
                balance TEXT NOT NULL DEFAULT '0',
                exclude_from_balance INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE bank_balance_records (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL, amount TEXT NOT NULL,
                currency TEXT NOT NULL, as_of INTEGER NOT NULL
            );
            CREATE TABLE bonds (id TEXT PRIMARY KEY, coupon_value TEXT NOT NULL,
                quantity TEXT NOT NULL DEFAULT '1', currency TEXT NOT NULL DEFAULT 'CZK');
            CREATE TABLE loans (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, principal TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK', interest_rate TEXT NOT NULL DEFAULT '0',
                interest_rate_validity_date INTEGER, monthly_payment TEXT NOT NULL DEFAULT '0',
                start_date INTEGER NOT NULL DEFAULT 0, end_date INTEGER,
                created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0,
                balance_anchor_amount TEXT, balance_anchor_date INTEGER
            );
            CREATE TABLE loan_events (
                id TEXT PRIMARY KEY, loan_id TEXT NOT NULL, kind TEXT NOT NULL, event_day INTEGER NOT NULL,
                amount TEXT, rate TEXT, monthly_payment TEXT, note TEXT,
                created_at INTEGER NOT NULL DEFAULT 0, previous_rate TEXT, previous_monthly_payment TEXT
            );
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, purchase_price TEXT NOT NULL DEFAULT '0',
                purchase_price_currency TEXT NOT NULL DEFAULT 'CZK', market_price TEXT NOT NULL DEFAULT '0',
                market_price_currency TEXT NOT NULL DEFAULT 'CZK', purchase_date INTEGER
            );
            CREATE TABLE real_estate_valuations (
                id TEXT PRIMARY KEY, real_estate_id TEXT NOT NULL, value TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK', valued_at INTEGER NOT NULL, note TEXT,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE other_assets (id TEXT PRIMARY KEY, quantity TEXT NOT NULL DEFAULT '0',
                market_price TEXT NOT NULL DEFAULT '0', currency TEXT NOT NULL DEFAULT 'CZK');
            CREATE TABLE other_asset_transactions (
                id TEXT PRIMARY KEY, asset_id TEXT NOT NULL, type TEXT NOT NULL, quantity TEXT NOT NULL,
                transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE other_asset_valuations (
                id TEXT PRIMARY KEY, asset_id TEXT NOT NULL, value TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK', valued_at INTEGER NOT NULL, note TEXT,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, type TEXT NOT NULL, quantity TEXT NOT NULL,
                transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, type TEXT NOT NULL, quantity TEXT NOT NULL,
                transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL, price TEXT NOT NULL, currency TEXT NOT NULL,
                UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE crypto_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL, price TEXT NOT NULL, currency TEXT NOT NULL,
                UNIQUE(ticker, recorded_at)
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn balance_record(conn: &Connection, as_of: i64) {
        conn.execute(
            "INSERT OR IGNORE INTO bank_accounts (id, balance) VALUES ('acc', '100')",
            [],
        )
        .expect("account");
        conn.execute(
            "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of)
             VALUES (?1, 'acc', '100', 'CZK', ?2)",
            params![format!("r{as_of}"), as_of],
        )
        .expect("record");
    }

    fn trade(conn: &Connection, table: &str, ticker: &str, kind: &str, quantity: &str, day: i64) {
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, ticker, type, quantity, transaction_date)
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![
                format!("{ticker}-{kind}-{day}"),
                ticker,
                kind,
                quantity,
                day
            ],
        )
        .expect("trade");
    }

    fn history_row(conn: &Connection, day: i64) {
        conn.execute(
            "INSERT INTO portfolio_metrics_history (id, recorded_at, source) VALUES (?1, ?2, 'backfill')",
            params![format!("p{day}"), day],
        )
        .expect("row");
    }

    // --- startup (A7) ---------------------------------------------------------------------

    #[test]
    fn the_rebuild_version_is_stored_and_read_back() {
        let conn = setup_test_db();
        assert_eq!(stored_rebuild_version(&conn).expect("none"), 0);
        store_rebuild_version(&conn).expect("store");
        assert_eq!(
            stored_rebuild_version(&conn).expect("stored"),
            REBUILD_VERSION
        );
        store_rebuild_version(&conn).expect("again");
        conn.execute(
            "UPDATE app_config SET value = 'garbage' WHERE key = 'history_rebuild_version'",
            [],
        )
        .expect("corrupt");
        assert_eq!(
            stored_rebuild_version(&conn).expect("unreadable"),
            0,
            "an unreadable flag counts as no rebuild yet"
        );
    }

    #[test]
    fn a_profile_below_the_version_rebuilds_everything_once() {
        let conn = setup_test_db();
        balance_record(&conn, d(-30));
        trade(&conn, "investment_transactions", "AAPL", "buy", "2", d(-20));
        trade(&conn, "investment_transactions", "MSFT", "buy", "1", d(-10));
        trade(&conn, "investment_transactions", "MSFT", "sell", "1", d(-5));
        trade(&conn, "crypto_transactions", "BTC", "buy", "1", d(-15));
        // Even with complete history: the version decides, not the gaps.
        for offset in -30..0 {
            history_row(&conn, d(offset));
        }

        let plan = plan_startup(&conn, d(0)).expect("plan");
        assert!(plan.one_time);
        assert_eq!(plan.from_day, Some(d(-30)));
        assert_eq!(
            plan.jobs,
            vec![
                Job::StockTicker("AAPL".into(), d(-20)),
                Job::StockTicker("MSFT".into(), d(-10)),
                Job::CryptoTicker("BTC".into(), d(-15)),
                Job::Days(d(-30)),
            ],
            "every ticker from its first transaction, sold-out ones included, and the days from the first fact"
        );
    }

    #[test]
    fn a_profile_without_dated_facts_has_nothing_to_rebuild_even_once() {
        let conn = setup_test_db();
        let plan = plan_startup(&conn, d(0)).expect("plan");
        assert!(plan.one_time, "the flag still needs storing");
        assert!(plan.jobs.is_empty());
        assert_eq!(plan.from_day, None);
    }

    #[test]
    fn a_rebuilt_profile_with_every_day_present_queues_nothing() {
        let conn = setup_test_db();
        store_rebuild_version(&conn).expect("version");
        balance_record(&conn, d(-5));
        for offset in -5..0 {
            history_row(&conn, d(offset));
        }
        let plan = plan_startup(&conn, d(0)).expect("plan");
        assert!(!plan.one_time);
        assert!(plan.jobs.is_empty());
    }

    #[test]
    fn the_first_missing_day_starts_the_repair_with_the_tickers_held_from_it() {
        let conn = setup_test_db();
        store_rebuild_version(&conn).expect("version");
        balance_record(&conn, d(-10));
        trade(&conn, "investment_transactions", "AAPL", "buy", "2", d(-9)); // held
        trade(&conn, "investment_transactions", "OLD", "buy", "1", d(-9));
        trade(&conn, "investment_transactions", "OLD", "sell", "1", d(-8)); // sold before the gap
        trade(&conn, "investment_transactions", "GONE", "buy", "1", d(-9));
        trade(&conn, "investment_transactions", "GONE", "sell", "1", d(-3)); // sold inside the gap
        trade(&conn, "crypto_transactions", "BTC", "buy", "1", d(-2)); // bought inside the gap
        for offset in [-10, -9, -8, -7, -6, -5, /* -4 missing */ -3, -2, -1] {
            history_row(&conn, d(offset));
        }

        let plan = plan_startup(&conn, d(0)).expect("plan");
        assert!(!plan.one_time);
        assert_eq!(plan.from_day, Some(d(-4)));
        assert_eq!(
            plan.jobs,
            vec![
                Job::StockTicker("AAPL".into(), d(-4)),
                Job::StockTicker("GONE".into(), d(-4)),
                Job::CryptoTicker("BTC".into(), d(-2)),
                Job::Days(d(-4)),
            ],
            "OLD was sold out before the gap and needs nothing; a ticker starts at its first transaction at the earliest"
        );
    }

    #[test]
    fn yesterday_is_the_last_day_that_counts_and_today_is_not_a_gap() {
        let conn = setup_test_db();
        store_rebuild_version(&conn).expect("version");
        balance_record(&conn, d(-3));
        for offset in [-3, -2] {
            history_row(&conn, d(offset));
        }
        // yesterday (-1) is missing, today (0) has its live row
        history_row(&conn, d(0));
        let plan = plan_startup(&conn, d(0)).expect("plan");
        assert_eq!(plan.jobs, vec![Job::Days(d(-1))]);

        let fresh = setup_test_db();
        store_rebuild_version(&fresh).expect("version");
        balance_record(&fresh, d(0));
        assert!(
            plan_startup(&fresh, d(0)).expect("plan").jobs.is_empty(),
            "a first fact dated today has no past day to fill"
        );
    }

    #[test]
    fn recalculating_everything_is_every_ticker_and_the_days_from_the_first_fact() {
        let conn = setup_test_db();
        assert!(plan_everything(&conn).expect("empty").jobs.is_empty());
        balance_record(&conn, d(-30));
        trade(&conn, "investment_transactions", "AAPL", "buy", "2", d(-20));
        let plan = plan_everything(&conn).expect("plan");
        assert!(!plan.one_time);
        assert_eq!(
            plan.jobs,
            vec![Job::StockTicker("AAPL".into(), d(-20)), Job::Days(d(-30))]
        );
    }

    #[test]
    fn a_fresh_profile_balance_a_year_back_queues_that_day_and_the_rebuild_fills_the_history() {
        let conn = setup_test_db();
        let today = d(0);
        history_row(&conn, today);
        conn.execute("UPDATE portfolio_metrics_history SET source = 'live'", [])
            .expect("today's live row");
        conn.execute(
            "INSERT INTO bank_accounts (id, balance) VALUES ('acc', '100')",
            [],
        )
        .expect("account");
        conn.execute(
            "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of)
             VALUES ('r', 'acc', '100', 'CZK', ?1)",
            [d(-365)],
        )
        .expect("record");

        // The write queues the record's day…
        let job = Job::Days(d(-365));
        assert_eq!(job.from_day(), d(-365));
        // …and the rebuild from that day writes every day since.
        let stats = rebuild_days(&conn, job.from_day(), today).expect("rebuild");
        assert_eq!(stats.upserted, 365);
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM portfolio_metrics_history WHERE source = 'backfill'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(rows, 365, "a year of history from a single balance record");
        let first: String = conn
            .query_row(
                "SELECT savings_by_currency FROM portfolio_metrics_history ORDER BY recorded_at LIMIT 1",
                [],
                |r| r.get(0),
            )
            .expect("first day");
        assert!(
            first.contains("100"),
            "the record holds from its day: {first}"
        );
        // The startup of the same profile finds nothing missing.
        store_rebuild_version(&conn).expect("version");
        assert!(plan_startup(&conn, today).expect("plan").jobs.is_empty());
    }

    // --- triggers (A6) --------------------------------------------------------------------

    fn account(balance: &str, as_of: Option<i64>, excluded: bool, currency: &str) -> BankAccount {
        BankAccount {
            id: "acc".into(),
            name: "Main".into(),
            account_type: "checking".into(),
            iban: None,
            bban: None,
            currency: currency.into(),
            balance: balance.into(),
            balance_as_of: as_of,
            institution_id: None,
            external_account_id: None,
            data_source: "manual".into(),
            last_synced_at: None,
            interest_rate: None,
            has_zone_designation: false,
            termination_date: None,
            interest_rate_valid_until: None,
            exclude_from_balance: excluded,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn a_new_account_queues_its_opening_record_day_or_the_first_fact() {
        assert_eq!(
            account_created(&account("500", Some(d(-40)), false, "CZK")),
            Some(Job::Days(d(-40)))
        );
        assert_eq!(
            account_created(&account("500", None, false, "CZK")),
            Some(Job::Days(0)),
            "an undated balance is carried on every day"
        );
        assert_eq!(account_created(&account("0", None, false, "CZK")), None);
        assert_eq!(account_created(&account("", None, false, "CZK")), None);
        assert_eq!(
            account_created(&account("500", Some(d(-40)), true, "CZK")),
            None,
            "an account excluded from the balance counts nothing"
        );
    }

    #[test]
    fn a_recorded_balance_queues_its_day_unless_it_ends_a_carried_undated_balance() {
        let dated = account("500", Some(d(-40)), false, "CZK");
        assert_eq!(balance_recorded(&dated, d(-100)), Job::Days(d(-100)));

        // the first dated record of an account that carried an undated balance everywhere
        let undated = account("500", None, false, "CZK");
        assert_eq!(balance_recorded(&undated, d(-100)), Job::Days(0));
        // nothing was carried: a zero balance, or an account outside the balance
        assert_eq!(
            balance_recorded(&account("0", None, false, "CZK"), d(-100)),
            Job::Days(d(-100))
        );
        assert_eq!(
            balance_recorded(&account("500", None, true, "CZK"), d(-100)),
            Job::Days(d(-100))
        );
    }

    #[test]
    fn an_account_edit_queues_only_when_it_leaves_the_balance_or_changes_currency() {
        let base = account("500", None, false, "CZK");
        assert_eq!(account_updated(&base, &base), None);
        assert_eq!(
            account_updated(&base, &account("500", None, true, "CZK")),
            Some(Job::Days(0))
        );
        assert_eq!(
            account_updated(&base, &account("500", None, false, "EUR")),
            Some(Job::Days(0))
        );
        assert_eq!(
            account_updated(&base, &account("500", None, false, "czk")),
            None,
            "case is not a change"
        );
        assert_eq!(account_deleted(), Job::Days(0));
    }

    fn facts(kind: &str, date: Option<i64>, price: &str, currency: &str) -> PropertyFacts {
        PropertyFacts {
            property_type: kind.into(),
            purchase_date: date,
            purchase_price: price.into(),
            purchase_currency: currency.into(),
            market_currency: currency.into(),
        }
    }

    #[test]
    fn a_new_property_queues_from_the_earlier_of_its_purchase_and_first_valuation() {
        assert_eq!(
            property_created(Some(d(-100)), Some(d(0))),
            Some(Job::Days(d(-100)))
        );
        assert_eq!(
            property_created(Some(d(-5)), Some(d(-50))),
            Some(Job::Days(d(-50)))
        );
        assert_eq!(property_created(None, Some(d(-7))), Some(Job::Days(d(-7))));
        assert_eq!(property_created(Some(d(-7)), None), Some(Job::Days(d(-7))));
        assert_eq!(property_created(None, None), None);
    }

    #[test]
    fn a_property_edit_queues_from_the_earlier_purchase_date_or_the_first_fact() {
        let old = facts("investment", Some(d(-100)), "5000", "CZK");
        assert_eq!(property_updated(&old, &old.clone()), None);

        // the purchase date moves later: the days it no longer covers change too
        assert_eq!(
            property_updated(&old, &facts("investment", Some(d(-40)), "5000", "CZK")),
            Some(Job::Days(d(-100)))
        );
        // …or earlier
        assert_eq!(
            property_updated(&old, &facts("investment", Some(d(-300)), "5000", "CZK")),
            Some(Job::Days(d(-300)))
        );
        // a new price from the purchase date on
        assert_eq!(
            property_updated(&old, &facts("investment", Some(d(-100)), "6000", "CZK")),
            Some(Job::Days(d(-100)))
        );
        // a date set where there was none
        assert_eq!(
            property_updated(
                &facts("investment", None, "5000", "CZK"),
                &facts("investment", Some(d(-20)), "5000", "CZK")
            ),
            Some(Job::Days(d(-20)))
        );
        // a price change without any purchase date is not part of the history
        assert_eq!(
            property_updated(
                &facts("investment", None, "5000", "CZK"),
                &facts("investment", None, "6000", "CZK")
            ),
            None
        );
        // the type or the currency changes every day the property is counted
        assert_eq!(
            property_updated(&old, &facts("personal", Some(d(-100)), "5000", "CZK")),
            Some(Job::Days(0))
        );
        assert_eq!(
            property_updated(&old, &facts("investment", Some(d(-100)), "5000", "EUR")),
            Some(Job::Days(0))
        );
        assert_eq!(property_deleted(), Job::Days(0));
    }

    #[test]
    fn an_edit_queues_from_the_earlier_of_the_old_and_new_date() {
        assert_eq!(earlier(Some(d(-50)), d(-10)), d(-50));
        assert_eq!(earlier(Some(d(-10)), d(-50)), d(-50));
        assert_eq!(earlier(None, d(-10)), d(-10));
    }

    #[test]
    fn deleting_an_other_asset_queues_from_its_first_transaction_else_the_first_fact() {
        assert_eq!(other_asset_deleted(Some(d(-30))), Job::Days(d(-30)));
        assert_eq!(other_asset_deleted(None), Job::Days(0));
        assert_eq!(bond_changed(), Job::Days(0));
    }

    #[test]
    fn an_other_asset_edit_queues_the_first_fact_only_for_a_currency_change() {
        assert_eq!(
            other_asset_updated("CZK", Some("EUR")),
            Some(Job::Days(0)),
            "every day's other-assets breakdown is in another currency"
        );
        assert_eq!(
            other_asset_updated("CZK", Some("czk")),
            None,
            "case is no change"
        );
        assert_eq!(other_asset_updated("CZK", Some(" CZK ")), None);
        assert_eq!(
            other_asset_updated("CZK", None),
            None,
            "no currency in the edit keeps the stored one"
        );
    }

    fn outcome(days_ok: bool, failed: &[&str]) -> BatchOutcome {
        BatchOutcome {
            days_ok,
            failed_tickers: failed.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn the_rebuild_flag_is_earned_when_the_days_were_rebuilt_whatever_the_tickers_did() {
        assert!(rebuild_version_earned(&outcome(true, &[])));
        assert!(
            rebuild_version_earned(&outcome(true, &["DELISTED", "BTC"])),
            "a ticker the provider keeps rejecting must not rerun the rebuild at every start"
        );
        assert!(
            !rebuild_version_earned(&outcome(false, &[])),
            "the days are the rebuild: without them it runs again at the next start"
        );
        assert!(!rebuild_version_earned(&outcome(false, &["AAPL"])));
        assert!(rebuild_version_earned(&BatchOutcome::nothing_to_do()));
    }
}
