//! Which history work a start or a write needs (spec
//! `docs/specs/2026-10-13-history-from-dated-facts-design.md`, A6 and A7).
//!
//! Pure planning: these functions read facts through a plain `&Connection` and return
//! [`Job`]s for the queue in `services/history_recalc.rs`; they queue nothing and fetch
//! nothing, so every rule is unit-tested with a hand-written schema.
//!
//! * **Startup (A7).** [`plan_startup`]: the first start of a version rebuilds everything
//!   once; later starts only fill the days that are missing.
//! * **Triggers (A6).** A write to a static class (accounts and balance records, real
//!   estate, loans, bonds, other assets) queues [`all_days`], `Days(0)`:
//!   `history_days::plan_rebuild` clamps it to the first fact, and a days-only rebuild is
//!   local and cheap. The day of the edit is no bound on the days it changes. A day that the
//!   facts do not cover takes the remainder of a live row (`recorded - derived`), and the
//!   derived part of that live row moves with every item, so one change can move every earlier
//!   day. Only the ticker jobs (network) keep the date they are queued with. The helpers here
//!   only decide whether an edit touches a fact the history reads.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::{BankAccount, Bond, RealEstate};
use crate::services::history_days::earliest_fact_day;
use crate::services::history_recalc::{
    all_ticker_jobs, crypto_rebuild_jobs, rebuild_jobs, BatchOutcome, Job,
};
use crate::services::portfolio_history::SECONDS_PER_DAY;
use crate::services::valuations::{self, ValuationKind};

const DAY: i64 = SECONDS_PER_DAY;

/// The version of the history rebuild a profile has had. Version 1 was the first rebuild from
/// dated facts: stored history of earlier versions copied static classes from the nearest
/// live row and valued past days with today's holdings. Version 2 repeats it: a version 1
/// profile may hold days that a date-precise trigger left on an old remainder (a date added to
/// a property after the rebuild), and crypto days older than CoinGecko's year now come from
/// Yahoo instead of one stale price. Version 3 repeats it once more: loans are always the
/// ledger walk now, so a profile that ran version 2 may hold days (backfill and live) that still
/// carry a phantom loan remainder taken from a live row recorded by the old valuation model.
/// Version 4 repeats it for stock splits: the full-range fetch of every ticker records the
/// splits Yahoo reports for its holding period and rewrites the stock history with split-adjusted
/// closes (spec `2026-10-24-stock-splits-design.md`, D16).
pub const REBUILD_VERSION: i64 = 4;

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

/// The job every write to a static class queues: re-derive every day from the first fact
/// (`history_days::plan_rebuild` clamps `Days(0)` to it). A change to any static item (dating or
/// undating it, a record, a valuation, a value, a currency, a type, creating or deleting it) can
/// change every earlier day, because a day the facts do not cover takes the remainder of a live
/// row, `recorded - derived`, and `derived` moves with every item. A days-only rebuild reads
/// the facts once and writes locally, so there is nothing to save by narrowing it.
pub fn all_days() -> Job {
    Job::Days(0)
}

/// An edited bank account: leaving or entering the balance, or a currency change, moves the
/// account on every day. Nothing else about an account is part of the history.
pub fn account_updated(old: &BankAccount, new: &BankAccount) -> Option<Job> {
    (old.exclude_from_balance != new.exclude_from_balance
        || !old.currency.eq_ignore_ascii_case(&new.currency))
    .then(all_days)
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

/// An edited property: a change of a fact the history reads (type, purchase date or price,
/// currency), or an edit that wrote the property's first estimate (a price change is a
/// valuation dated today, and the first one counts back to the purchase date). A later estimate
/// on top of an earlier log only moves today. `earliest_valuation` is the earliest valuation day
/// before and after the edit. Anything else (a name, a rent, a photo) is not part of the history.
pub fn property_updated(
    old: &PropertyFacts,
    new: &PropertyFacts,
    earliest_valuation: (Option<i64>, Option<i64>),
) -> Option<Job> {
    let (before, after) = earliest_valuation;
    (old != new || before != after).then(all_days)
}

/// The days of an other asset's history that an edit can change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherAssetEdit<'a> {
    pub old_currency: &'a str,
    /// The currency of the edit; `None` keeps the stored one.
    pub new_currency: Option<&'a str>,
    /// The asset has transactions. One without is carried at today's quantity and price on
    /// every day (`history_days`).
    pub has_transactions: bool,
    /// The stored quantity changed (the history of an asset with transactions comes from them).
    pub quantity_changed: bool,
    /// The earliest valuation day after the edit when it wrote the valuation log (a price
    /// change is a valuation dated today), else `None`.
    pub logged_earliest: Option<i64>,
    /// Today's UTC day.
    pub today: i64,
}

/// An edited other asset. A currency change relabels the asset in every day's other-assets
/// breakdown. An asset without transactions is carried at today's quantity and price, so a
/// changed quantity or price moves every day. A price change is a valuation dated today; for an
/// asset with transactions it moves history only when it is the earliest estimate of the asset
/// (the days before the first valuation use it). Quantities and costs of an asset with
/// transactions come from them, so nothing else is part of the history.
pub fn other_asset_updated(edit: &OtherAssetEdit) -> Option<Job> {
    let currency_changed = edit
        .new_currency
        .is_some_and(|new| !edit.old_currency.trim().eq_ignore_ascii_case(new.trim()));
    let changed = currency_changed
        || if edit.has_transactions {
            edit.logged_earliest
                .is_some_and(|earliest| earliest >= edit.today)
        } else {
            edit.quantity_changed || edit.logged_earliest.is_some()
        };
    changed.then(all_days)
}

/// Does an other asset have transactions? One without is carried at today's quantity and price.
pub fn has_asset_transactions(conn: &Connection, asset_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM other_asset_transactions WHERE asset_id = ?1)",
        [asset_id],
        |row| row.get(0),
    )?)
}

/// The earliest valuation day of a property or other asset; `None` without valuations.
pub fn earliest_valuation_day(
    conn: &Connection,
    kind: ValuationKind,
    owner_id: &str,
) -> Result<Option<i64>> {
    Ok(valuations::list_valuations(conn, kind, owner_id)?
        .first()
        .map(|valuation| valuation.valued_at))
}

/// The value of a bond in its own currency: `coupon_value × quantity`; `None` when a number does
/// not parse (a legacy row).
fn bond_value(bond: &Bond) -> Option<f64> {
    let coupon = bond.coupon_value.trim().parse::<f64>().ok()?;
    let quantity = bond.quantity.trim().parse::<f64>().ok()?;
    Some(coupon * quantity)
}

/// An edited bond. Only its purchase date, its value (`coupon_value × quantity`) and its
/// currency are part of the history; a name, a rate, an ISIN or a maturity moves no day.
pub fn bond_updated(old: &Bond, new: &Bond) -> Option<Job> {
    let unchanged = old.purchase_date == new.purchase_date
        && bond_value(old) == bond_value(new)
        && old
            .currency
            .trim()
            .eq_ignore_ascii_case(new.currency.trim());
    (!unchanged).then(all_days)
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
                quantity TEXT NOT NULL DEFAULT '1', currency TEXT NOT NULL DEFAULT 'CZK',
                purchase_date INTEGER);
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
    fn the_rebuild_version_is_four() {
        // Version 2 repeated the one-time rebuild for profiles that ran version 1 (date-precise
        // static triggers could leave days on an old remainder; crypto days older than
        // CoinGecko's year had one stale price). Version 3 repeats it for loans: they are always
        // derived from their ledger, so a loan remainder taken from a live row is gone. Version 4
        // repeats it for stock splits: the full-range fetch detects past splits and stores
        // split-adjusted closes. Raise this number (and the docs that name it) only for a
        // rebuild that must run again.
        assert_eq!(REBUILD_VERSION, 4);
    }

    #[test]
    fn a_profile_that_ran_version_three_rebuilds_once_more() {
        let conn = setup_test_db();
        balance_record(&conn, d(-30));
        trade(&conn, "investment_transactions", "AAPL", "buy", "2", d(-20));
        trade(&conn, "crypto_transactions", "BTC", "buy", "1", d(-15));
        for offset in -30..0 {
            history_row(&conn, d(offset));
        }
        // a version 3 profile: complete history, the old flag
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('history_rebuild_version', '3')",
            [],
        )
        .expect("old flag");
        assert_eq!(stored_rebuild_version(&conn).expect("stored"), 3);

        let plan = plan_startup(&conn, d(0)).expect("plan");
        assert!(plan.one_time, "the flag is below the current version");
        assert_eq!(plan.from_day, Some(d(-30)));
        assert_eq!(
            plan.jobs,
            vec![
                Job::StockTicker("AAPL".into(), d(-20)),
                Job::CryptoTicker("BTC".into(), d(-15)),
                Job::Days(d(-30)),
            ],
            "every ticker from its first transaction and the days from the first fact, although no day is missing"
        );

        // the flag is stored only once the days rebuild succeeded (`rebuild_version_earned`)
        assert!(!rebuild_version_earned(&outcome(false, &[])));
        assert_eq!(stored_rebuild_version(&conn).expect("unchanged"), 3);
        assert!(rebuild_version_earned(&outcome(true, &["BTC"])));
        store_rebuild_version(&conn).expect("store");
        assert_eq!(
            stored_rebuild_version(&conn).expect("stored"),
            REBUILD_VERSION
        );

        let again = plan_startup(&conn, d(0)).expect("plan");
        assert!(!again.one_time, "it runs once");
        assert!(again.jobs.is_empty(), "nothing is missing any more");
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
    fn a_fresh_profile_balance_a_year_back_queues_every_day_and_the_rebuild_fills_the_history() {
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

        // The write queues every day…
        let job = all_days();
        assert_eq!(job.from_day(), 0);
        // …and the rebuild, clamped to the first fact, writes every day since.
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
    fn a_static_trigger_re_derives_every_day() {
        // `Days(0)` is clamped to the first fact: the whole history, whatever the edit's day.
        assert_eq!(all_days(), Job::Days(0));
        assert_eq!(all_days().from_day(), 0);
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

    const NO_VALUATION: (Option<i64>, Option<i64>) = (None, None);

    #[test]
    fn a_property_edit_of_a_fact_the_history_reads_re_derives_every_day() {
        let old = facts("investment", Some(d(-100)), "5000", "CZK");
        assert_eq!(property_updated(&old, &old.clone(), NO_VALUATION), None);

        for new in [
            // the purchase date moves later or earlier, is set where there was none, or cleared
            facts("investment", Some(d(-40)), "5000", "CZK"),
            facts("investment", Some(d(-300)), "5000", "CZK"),
            facts("investment", None, "5000", "CZK"),
            // a new price, type or currency
            facts("investment", Some(d(-100)), "6000", "CZK"),
            facts("personal", Some(d(-100)), "5000", "CZK"),
            facts("investment", Some(d(-100)), "5000", "EUR"),
        ] {
            assert_eq!(
                property_updated(&old, &new, NO_VALUATION),
                Some(Job::Days(0)),
                "{new:?}"
            );
        }
        // a date set where there was none: the owner's case, no matter how old the valuations are
        assert_eq!(
            property_updated(
                &facts("investment", None, "5000", "CZK"),
                &facts("investment", Some(d(-20)), "5000", "CZK"),
                (Some(d(-200)), Some(d(-200)))
            ),
            Some(Job::Days(0))
        );
    }

    #[test]
    fn a_property_edit_that_wrote_the_first_valuation_re_derives_every_day() {
        // A price change is a valuation dated today. It is the property's first estimate: the
        // property now counts from its purchase date at the purchase price.
        let same = facts("investment", Some(d(-100)), "5000", "CZK");
        assert_eq!(
            property_updated(&same, &same.clone(), (None, Some(d(0)))),
            Some(Job::Days(0))
        );
        // a later estimate on top of an earlier log moves nothing in the past
        assert_eq!(
            property_updated(&same, &same.clone(), (Some(d(-50)), Some(d(-50)))),
            None
        );
    }

    fn bond(purchase_date: Option<i64>, coupon: &str, quantity: &str, currency: &str) -> Bond {
        Bond {
            id: "b".into(),
            name: "Bond".into(),
            isin: None,
            coupon_value: coupon.into(),
            quantity: quantity.into(),
            currency: currency.into(),
            interest_rate: "3".into(),
            maturity_date: None,
            purchase_date,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn a_bond_edit_of_its_date_value_or_currency_re_derives_every_day() {
        let old = bond(Some(d(-50)), "1000", "2", "CZK");
        for new in [
            // the date moved, was set where there was none, or cleared
            bond(Some(d(-10)), "1000", "2", "CZK"),
            bond(None, "1000", "2", "CZK"),
            // the value or the currency changed
            bond(Some(d(-50)), "1500", "2", "CZK"),
            bond(Some(d(-50)), "1000", "3", "CZK"),
            bond(Some(d(-50)), "1000", "2", "EUR"),
        ] {
            assert_eq!(bond_updated(&old, &new), Some(Job::Days(0)), "{new:?}");
        }
        assert_eq!(
            bond_updated(
                &bond(None, "1000", "2", "CZK"),
                &bond(Some(d(-50)), "1000", "2", "CZK")
            ),
            Some(Job::Days(0)),
            "dating an undated bond"
        );
    }

    #[test]
    fn a_bond_edit_that_changes_no_fact_of_the_history_queues_nothing() {
        let mut old = bond(Some(d(-50)), "1000", "2", "CZK");
        let mut new = old.clone();
        new.name = "Renamed".into();
        new.interest_rate = "5".into();
        new.maturity_date = Some(d(400));
        assert_eq!(bond_updated(&old, &new), None);
        // 2000 either way: the same value in the same currency
        assert_eq!(
            bond_updated(
                &bond(None, "1000", "2", "CZK"),
                &bond(None, "2000", "1", "czk")
            ),
            None
        );
        old.purchase_date = None;
        new.purchase_date = None;
        assert_eq!(bond_updated(&old, &new), None);
    }

    #[test]
    fn dating_a_bond_changes_the_days_before_its_purchase_end_to_end() {
        let conn = setup_test_db();
        // history starts at the account record; an undated bond is carried on every day
        balance_record(&conn, d(0));
        conn.execute(
            "INSERT INTO bonds (id, coupon_value, quantity, currency) VALUES ('b', '1000', '1', 'CZK')",
            [],
        )
        .expect("bond");
        rebuild_days(&conn, d(0), d(10)).expect("first rebuild");
        let bonds = |day: i64| -> String {
            conn.query_row(
                "SELECT bonds_by_currency FROM portfolio_metrics_history WHERE recorded_at = ?1",
                [day],
                |row| row.get(0),
            )
            .expect("row")
        };
        assert_eq!(bonds(d(2)), r#"{"CZK":1000.0}"#);

        // dating it at day 5 queues every day: the days before it lose the bond
        let before = bond(None, "1000", "1", "CZK");
        let after = bond(Some(d(5)), "1000", "1", "CZK");
        conn.execute("UPDATE bonds SET purchase_date = ?1", [d(5)])
            .expect("date");
        let Some(job) = bond_updated(&before, &after) else {
            panic!("a dated bond edit queues days");
        };
        rebuild_days(&conn, job.from_day(), d(10)).expect("rebuild from the queued day");
        assert_eq!(bonds(d(2)), "{}");
        assert_eq!(bonds(d(4)), "{}");
        assert_eq!(bonds(d(5)), r#"{"CZK":1000.0}"#);
        assert_eq!(bonds(d(9)), r#"{"CZK":1000.0}"#);
    }

    #[test]
    fn a_purchase_date_added_after_the_rebuild_reaches_every_earlier_day() {
        // The owner's case. A live row recorded the property before its first valuation was
        // entered: the facts do not count it there, so the days up to its valuation hold what
        // was recorded as an unknown remainder. A purchase date makes every day before it
        // covered (the property is simply not owned yet), and that must reach the days
        // BEFORE the purchase date too: a rebuild from the purchase date alone left them on
        // the old remainder, a cliff on the purchase day.
        let conn = setup_test_db();
        balance_record(&conn, d(0));
        conn.execute(
            "INSERT INTO real_estate (id, type, purchase_price, purchase_date)
             VALUES ('flat', 'investment', '900', NULL)",
            [],
        )
        .expect("property");
        conn.execute(
            "INSERT INTO real_estate_valuations (id, real_estate_id, value, valued_at)
             VALUES ('v', 'flat', '1000', ?1)",
            [d(6)],
        )
        .expect("valuation");
        conn.execute(
            "INSERT INTO portfolio_metrics_history
                 (id, recorded_at, source, savings_by_currency, real_estate_by_currency)
             VALUES ('live3', ?1, 'live', '{\"CZK\":100}', '{\"CZK\":1500}')",
            [d(3)],
        )
        .expect("live row");
        let today = d(10);
        rebuild_days(&conn, 0, today).expect("the one-time rebuild");
        let real_estate = |day: i64| -> String {
            conn.query_row(
                "SELECT real_estate_by_currency FROM portfolio_metrics_history WHERE recorded_at = ?1",
                [day],
                |row| row.get(0),
            )
            .expect("row")
        };
        assert_eq!(
            real_estate(d(0)),
            r#"{"CZK":1500.0}"#,
            "before its first valuation the property is the remainder of the live row"
        );

        // The owner dates the purchase on day 2: the property is owned from there.
        conn.execute("UPDATE real_estate SET purchase_date = ?1", [d(2)])
            .expect("purchase date");
        let (old, new) = (
            facts("investment", None, "900", "CZK"),
            facts("investment", Some(d(2)), "900", "CZK"),
        );
        let job = property_updated(&old, &new, (Some(d(6)), Some(d(6)))).expect("a queued job");
        assert_eq!(job, Job::Days(0));

        // What the old trigger did, the earliest of the old date, the new one and the first
        // valuation: days 0 and 1 kept the stale remainder next to the new line from day 2.
        rebuild_days(&conn, d(2), today).expect("narrow rebuild");
        assert_eq!(
            real_estate(d(0)),
            r#"{"CZK":1500.0}"#,
            "stale before the date"
        );
        assert_eq!(real_estate(d(1)), r#"{"CZK":1500.0}"#);
        assert_ne!(
            real_estate(d(2)),
            real_estate(d(1)),
            "the cliff on the purchase day"
        );

        rebuild_days(&conn, job.from_day(), today).expect("rebuild from the first fact");
        assert_eq!(real_estate(d(0)), "{}", "not owned yet");
        assert_eq!(real_estate(d(1)), "{}");
        assert_eq!(
            real_estate(d(2)),
            r#"{"CZK":900.0}"#,
            "owned from its date, at the purchase price"
        );
        assert_eq!(real_estate(d(6)), r#"{"CZK":1000.0}"#, "at its valuation");
    }

    fn edit(new_currency: Option<&str>) -> OtherAssetEdit<'_> {
        OtherAssetEdit {
            old_currency: "CZK",
            new_currency,
            has_transactions: true,
            quantity_changed: false,
            logged_earliest: None,
            today: d(0),
        }
    }

    #[test]
    fn an_other_asset_currency_change_re_derives_every_day() {
        assert_eq!(
            other_asset_updated(&edit(Some("EUR"))),
            Some(Job::Days(0)),
            "every day's other-assets breakdown is in another currency"
        );
        assert_eq!(
            other_asset_updated(&edit(Some("czk"))),
            None,
            "case is no change"
        );
        assert_eq!(other_asset_updated(&edit(Some(" CZK "))), None);
        assert_eq!(
            other_asset_updated(&edit(None)),
            None,
            "no currency in the edit keeps the stored one"
        );
    }

    #[test]
    fn a_price_edit_that_is_the_earliest_valuation_reprices_the_days_before_it() {
        // The log's earliest estimate is today's: the days before it use it
        // (`value_at_day` falls back to the earliest).
        let first_estimate = OtherAssetEdit {
            logged_earliest: Some(d(0)),
            ..edit(None)
        };
        assert_eq!(other_asset_updated(&first_estimate), Some(Job::Days(0)));
        // an estimate on top of an earlier log changes today only
        let later_estimate = OtherAssetEdit {
            logged_earliest: Some(d(-20)),
            ..edit(None)
        };
        assert_eq!(other_asset_updated(&later_estimate), None);
        // a quantity edit of an asset with transactions is not part of the history
        let quantity = OtherAssetEdit {
            quantity_changed: true,
            ..edit(None)
        };
        assert_eq!(other_asset_updated(&quantity), None);
    }

    #[test]
    fn an_asset_without_transactions_is_carried_so_a_quantity_or_price_edit_moves_every_day() {
        let undated = OtherAssetEdit {
            has_transactions: false,
            ..edit(None)
        };
        assert_eq!(other_asset_updated(&undated), None);
        assert_eq!(
            other_asset_updated(&OtherAssetEdit {
                quantity_changed: true,
                ..undated.clone()
            }),
            Some(Job::Days(0))
        );
        assert_eq!(
            other_asset_updated(&OtherAssetEdit {
                logged_earliest: Some(d(-20)),
                ..undated
            }),
            Some(Job::Days(0)),
            "the carried price changed, whatever the log holds"
        );
    }

    #[test]
    fn the_readers_of_an_other_asset_and_a_valuation_log_read_the_database() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO other_assets (id) VALUES ('a'), ('bare')", [])
            .expect("assets");
        conn.execute(
            "INSERT INTO other_asset_transactions (id, asset_id, type, quantity, transaction_date)
             VALUES ('t1', 'a', 'buy', '1', ?1)",
            [d(-30)],
        )
        .expect("transaction");
        assert!(has_asset_transactions(&conn, "a").expect("has"));
        assert!(!has_asset_transactions(&conn, "bare").expect("has not"));
        assert_eq!(
            earliest_valuation_day(&conn, ValuationKind::OtherAsset, "a").expect("none"),
            None
        );
        conn.execute(
            "INSERT INTO other_asset_valuations (id, asset_id, value, valued_at)
             VALUES ('v1', 'a', '10', ?1), ('v2', 'a', '20', ?2)",
            [d(-5), d(-9)],
        )
        .expect("valuations");
        assert_eq!(
            earliest_valuation_day(&conn, ValuationKind::OtherAsset, "a").expect("earliest"),
            Some(d(-9))
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
