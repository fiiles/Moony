//! History days derived from dated facts (spec
//! `docs/specs/2026-10-13-history-from-dated-facts-design.md`, A1-A4 and A8).
//!
//! The value of a class on a past day is computed from the facts dated on or before it,
//! with the rule the domain's own page uses (P1, P3). Nothing is copied from another day
//! when a dated fact exists.
//!
//! * [`Facts`] reads every fact once (accounts and their balance records, properties and
//!   their valuations, loan ledgers, other assets, bonds, the ticker series). Its
//!   [`Facts::day_values`] then walks the days with per-item cursors, so a rebuild costs
//!   no query per day.
//! * [`earliest_fact_day`] is the first day any fact is dated (A2): history starts there.
//! * [`rebuild_days`] writes one row for every day of a range (A3, A4), in chunks of
//!   [`CHUNK_DAYS`] days, one transaction each. [`plan_rebuild`], [`prune_before_first_fact`]
//!   and [`rebuild_chunk`] are its steps, for a caller that yields between chunks
//!   (`history_recalc`).
//!
//! Every write goes through `services/portfolio_history.rs`, the single writer of
//! `portfolio_metrics_history`.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;

use crate::error::Result;
use crate::services::history_recalc::{chunk_ranges, CHUNK_DAYS};
use crate::services::loan_amortization::{day_floor, loan_balance_at, LedgerEventKind, LoanLedger};
use crate::services::loans;
use crate::services::parsing::parse_money;
use crate::services::portfolio_history::{
    day_sources, delete_backfill_before, load_ticker_series, update_class_set, upsert_snapshot,
    ClassBreakdowns, ClassSet, SnapshotSource, TickerSeries, SECONDS_PER_DAY,
};
use crate::services::ticker_history::RebuildStats;

const DAY: i64 = SECONDS_PER_DAY;

/// 2000-01-01: no history starts before it, however old a fact is dated.
pub const EARLIEST_DAY_FLOOR: i64 = 946_684_800;

/// Below this a held quantity is float residue of buys and sells, not a position.
const QUANTITY_EPSILON: f64 = 0.000_001;

type Breakdown = HashMap<String, f64>;

fn add(breakdown: &mut Breakdown, currency: &str, amount: f64) {
    *breakdown.entry(currency.to_string()).or_insert(0.0) += amount;
}

/// A class without its zero entries: an empty class is `{}`, not `{"CZK": 0}`.
fn without_zeros(mut breakdown: Breakdown) -> Breakdown {
    breakdown.retain(|_, amount| *amount != 0.0);
    breakdown
}

/// One day of every class, with the classes the facts cover completely.
#[derive(Debug, Clone, Default)]
pub struct DayValues {
    /// All eight breakdowns. A class' items the facts do not cover are carried when they
    /// are undated accounts and bonds, and left out otherwise (A3).
    pub breakdowns: ClassBreakdowns,
    /// The classes whose every item the facts cover on the day (A1, "known"). A live row
    /// is rewritten for these only (D1). Stocks and crypto are always known; bonds never.
    pub known: ClassSet,
}

/// Rows dated by day, oldest first, with a forward cursor. Days only grow while a rebuild
/// walks them, so every row is passed once.
struct Steps<T> {
    rows: Vec<(i64, T)>,
    taken: usize,
}

impl<T> Steps<T> {
    fn new(rows: Vec<(i64, T)>) -> Self {
        Self { rows, taken: 0 }
    }

    /// Move the cursor past every row dated on or before `day`; the rows newly passed.
    fn advance(&mut self, day: i64) -> &[(i64, T)] {
        let before = self.taken;
        while self.taken < self.rows.len() && self.rows[self.taken].0 <= day {
            self.taken += 1;
        }
        &self.rows[before..self.taken]
    }

    /// The newest row passed so far (the last of its day, in the order the rows came in).
    fn latest(&self) -> Option<&T> {
        self.taken.checked_sub(1).map(|index| &self.rows[index].1)
    }

    fn first(&self) -> Option<&(i64, T)> {
        self.rows.first()
    }
}

/// Rows of `(owner, day, row)` grouped by owner, each group in the order given.
fn grouped<T>(rows: Vec<(String, i64, T)>) -> HashMap<String, Vec<(i64, T)>> {
    let mut groups: HashMap<String, Vec<(i64, T)>> = HashMap::new();
    for (owner, day, row) in rows {
        groups.entry(owner).or_default().push((day, row));
    }
    groups
}

/// An amount in its own currency (a balance record, a valuation).
struct Amount {
    value: f64,
    currency: String,
}

struct Account {
    currency: String,
    /// The account's current balance: what an account without records carries.
    current_balance: f64,
    records: Steps<Amount>,
}

struct Property {
    personal: bool,
    purchase_date: Option<i64>,
    purchase_price: f64,
    purchase_currency: String,
    valuations: Steps<Amount>,
}

struct Loan {
    currency: String,
    ledger: LoanLedger,
    /// The day of the first manual balance: before it the loan is estimated (D5).
    first_manual_day: Option<i64>,
}

struct OtherAsset {
    currency: String,
    market_price: f64,
    /// Signed quantity changes of the transactions.
    changes: Steps<f64>,
    quantity: f64,
    valuations: Steps<Amount>,
}

/// Every fact the history of a profile is derived from, read once.
pub struct Facts {
    accounts: Vec<Account>,
    properties: Vec<Property>,
    loans: Vec<Loan>,
    assets: Vec<OtherAsset>,
    /// Today's bonds: no purchase date, so they carry on every day (D3).
    bonds: Breakdown,
    stocks: BTreeMap<String, TickerSeries>,
    crypto: BTreeMap<String, TickerSeries>,
    /// The last day valued: the cursors only move forward.
    last_day: Option<i64>,
}

impl Facts {
    /// Read the facts of every class. `until_exclusive` bounds the ticker series: the days
    /// to value are before it (a rebuild passes today's start).
    pub fn load(conn: &Connection, until_exclusive: i64) -> Result<Self> {
        Ok(Self {
            accounts: load_accounts(conn)?,
            properties: load_properties(conn)?,
            loans: load_loans(conn)?,
            assets: load_other_assets(conn)?,
            bonds: load_bonds(conn)?,
            stocks: load_ticker_series(
                conn,
                "stock_value_history",
                "investment_transactions",
                until_exclusive,
            )?,
            crypto: load_ticker_series(
                conn,
                "crypto_value_history",
                "crypto_transactions",
                until_exclusive,
            )?,
            last_day: None,
        })
    }

    /// The value of every class on `day` (spec A1). The cursors only move forward, so the
    /// days of one `Facts` must not decrease.
    pub fn day_values(&mut self, day: i64) -> DayValues {
        debug_assert!(
            self.last_day.is_none_or(|last| day >= last),
            "history days must be valued in ascending order"
        );
        self.last_day = Some(day);

        let (savings, savings_known) = self.savings_on(day);
        let (loans, loans_known) = self.loans_on(day);
        let (personal, investment, real_estate_known) = self.real_estate_on(day);
        let (other_assets, other_assets_known) = self.other_assets_on(day);
        DayValues {
            breakdowns: ClassBreakdowns {
                savings: without_zeros(savings),
                investments: without_zeros(ticker_value(&mut self.stocks, day)),
                crypto: without_zeros(ticker_value(&mut self.crypto, day)),
                bonds: without_zeros(self.bonds.clone()),
                real_estate_personal: without_zeros(personal),
                real_estate_investment: without_zeros(investment),
                loans: without_zeros(loans),
                other_assets: without_zeros(other_assets),
            },
            known: ClassSet {
                savings: savings_known,
                investments: true,
                crypto: true,
                bonds: false,
                real_estate: real_estate_known,
                loans: loans_known,
                other_assets: other_assets_known,
            },
        }
    }

    /// Savings: the newest balance record on or before the day, in its own currency. An
    /// account is not counted before its first record (D4); an account without any record
    /// carries its current balance. Both are unknown. Accounts excluded from the balance
    /// are never loaded.
    fn savings_on(&mut self, day: i64) -> (Breakdown, bool) {
        let mut breakdown = Breakdown::new();
        let mut known = true;
        for account in &mut self.accounts {
            account.records.advance(day);
            match account.records.latest() {
                Some(record) => add(&mut breakdown, &record.currency, record.value),
                None if account.records.first().is_none() => {
                    add(&mut breakdown, &account.currency, account.current_balance);
                    known = false;
                }
                None => known = false,
            }
        }
        (breakdown, known)
    }

    /// Loans: the ledger walk of the day (0 before the start). The estimated past of a loan
    /// with manual balances, before the first one, is not a fact (D5).
    fn loans_on(&mut self, day: i64) -> (Breakdown, bool) {
        let mut breakdown = Breakdown::new();
        let mut known = true;
        for loan in &self.loans {
            add(
                &mut breakdown,
                &loan.currency,
                loan_balance_at(&loan.ledger, day).balance,
            );
            if loan.first_manual_day.is_some_and(|first| day < first) {
                known = false;
            }
        }
        (breakdown, known)
    }

    /// Real estate, split by the property's current type: `(personal, investment, known)`.
    /// A property without any valuation is not counted on any day. Before the purchase date
    /// a property is worth 0. Otherwise the newest valuation on or before the day applies
    /// (of one day, the last entered), else the purchase price. A property without a
    /// purchase date counts nothing before its first valuation, and that day is unknown.
    fn real_estate_on(&mut self, day: i64) -> (Breakdown, Breakdown, bool) {
        let mut personal = Breakdown::new();
        let mut investment = Breakdown::new();
        let mut known = true;
        for property in &mut self.properties {
            property.valuations.advance(day);
            // No estimate at all, no value line (`valuationTrace`; today's market price is 0
            // too): the purchase price opens the line only once an estimate exists.
            if property.valuations.first().is_none()
                || property.purchase_date.is_some_and(|bought| day < bought)
            {
                continue;
            }
            let target = if property.personal {
                &mut personal
            } else {
                &mut investment
            };
            match (property.valuations.latest(), property.purchase_date) {
                (Some(valuation), _) => add(target, &valuation.currency, valuation.value),
                (None, Some(_)) => {
                    add(target, &property.purchase_currency, property.purchase_price)
                }
                (None, None) => known = false,
            }
        }
        (personal, investment, known)
    }

    /// Other assets: the quantity the transactions leave on the day times the valuation of
    /// the day (`valuations::value_at_day`: the newest valuation on or before it, else the
    /// earliest, else the market price), in the valuation's currency.
    fn other_assets_on(&mut self, day: i64) -> (Breakdown, bool) {
        let mut breakdown = Breakdown::new();
        for asset in &mut self.assets {
            for (_, change) in asset.changes.advance(day) {
                asset.quantity += change;
            }
            asset.valuations.advance(day);
            if asset.quantity <= QUANTITY_EPSILON {
                continue;
            }
            match asset
                .valuations
                .latest()
                .or_else(|| asset.valuations.first().map(|(_, first)| first))
            {
                Some(valuation) => {
                    add(
                        &mut breakdown,
                        &valuation.currency,
                        asset.quantity * valuation.value,
                    );
                }
                None => add(
                    &mut breakdown,
                    &asset.currency,
                    asset.quantity * asset.market_price,
                ),
            }
        }
        (breakdown, true)
    }
}

/// A ticker class on `day`: every ticker still held, at its latest history row on or before
/// the day, summed per currency (`TickerSeries::value_before`).
fn ticker_value(series: &mut BTreeMap<String, TickerSeries>, day: i64) -> Breakdown {
    let mut breakdown = Breakdown::new();
    for ticker in series.values_mut() {
        if let Some((value, currency)) = ticker.value_before(day + DAY) {
            add(&mut breakdown, currency, value);
        }
    }
    breakdown
}

fn load_accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare(
        "SELECT bank_account_id, as_of, amount, currency FROM bank_balance_records
         WHERE bank_account_id IN (SELECT id FROM bank_accounts WHERE exclude_from_balance = 0)
         ORDER BY bank_account_id, as_of, rowid",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut records = grouped(
        rows.into_iter()
            .map(|(id, as_of, amount, currency)| {
                let value = parse_money(&amount, 0.0, "bank_balance_records.amount");
                (id, as_of, Amount { value, currency })
            })
            .collect(),
    );

    let mut stmt = conn.prepare(
        "SELECT id, currency, balance FROM bank_accounts WHERE exclude_from_balance = 0
         ORDER BY id",
    )?;
    let accounts = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(accounts
        .into_iter()
        .map(|(id, currency, balance)| Account {
            currency,
            current_balance: parse_money(&balance, 0.0, "bank_accounts.balance"),
            records: Steps::new(records.remove(&id).unwrap_or_default()),
        })
        .collect())
}

/// Valuation log rows of `table` (`fk` is the parent column), per parent, in the order the
/// valuation of a day is picked: by day, then by creation.
fn load_valuations(
    conn: &Connection,
    table: &str,
    fk: &str,
) -> Result<HashMap<String, Vec<(i64, Amount)>>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {fk}, valued_at, value, currency FROM {table}
         ORDER BY {fk}, valued_at, created_at, rowid"
    ))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(grouped(
        rows.into_iter()
            .map(|(owner, valued_at, value, currency)| {
                let value = parse_money(&value, 0.0, "valuation value");
                (owner, valued_at, Amount { value, currency })
            })
            .collect(),
    ))
}

fn load_properties(conn: &Connection) -> Result<Vec<Property>> {
    let mut valuations = load_valuations(conn, "real_estate_valuations", "real_estate_id")?;
    let mut stmt = conn.prepare(
        "SELECT id, type, purchase_price, purchase_price_currency, purchase_date
         FROM real_estate ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .map(
            |(id, kind, purchase_price, purchase_currency, purchase_date)| Property {
                personal: kind == "personal",
                purchase_date: purchase_date.map(day_floor),
                purchase_price: parse_money(&purchase_price, 0.0, "real_estate.purchase_price"),
                purchase_currency,
                valuations: Steps::new(valuations.remove(&id).unwrap_or_default()),
            },
        )
        .collect())
}

fn load_loans(conn: &Connection) -> Result<Vec<Loan>> {
    Ok(loans::load_ledgers(conn)?
        .into_iter()
        .map(|entry| Loan {
            first_manual_day: entry
                .ledger
                .events
                .iter()
                .filter(|event| event.kind == LedgerEventKind::BalanceCheck)
                .map(|event| day_floor(event.day))
                .min(),
            currency: entry.currency,
            ledger: entry.ledger,
        })
        .collect())
}

fn load_other_assets(conn: &Connection) -> Result<Vec<OtherAsset>> {
    let mut valuations = load_valuations(conn, "other_asset_valuations", "asset_id")?;

    let mut stmt = conn.prepare(
        "SELECT asset_id, transaction_date,
                CASE WHEN type = 'buy' THEN CAST(quantity AS REAL)
                     ELSE -CAST(quantity AS REAL) END
         FROM other_asset_transactions ORDER BY asset_id, transaction_date, created_at, rowid",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut changes = grouped(rows);

    let mut stmt =
        conn.prepare("SELECT id, currency, market_price FROM other_assets ORDER BY id")?;
    let assets = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(assets
        .into_iter()
        .map(|(id, currency, market_price)| OtherAsset {
            currency,
            market_price: parse_money(&market_price, 0.0, "other_assets.market_price"),
            changes: Steps::new(changes.remove(&id).unwrap_or_default()),
            quantity: 0.0,
            valuations: Steps::new(valuations.remove(&id).unwrap_or_default()),
        })
        .collect())
}

fn load_bonds(conn: &Connection) -> Result<Breakdown> {
    let mut stmt = conn.prepare("SELECT coupon_value, quantity, currency FROM bonds")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut bonds = Breakdown::new();
    for (value, quantity, currency) in rows {
        let value = parse_money(&value, 0.0, "bonds.coupon_value");
        let quantity = parse_money(&quantity, 1.0, "bonds.quantity");
        add(&mut bonds, &currency, value * quantity);
    }
    Ok(bonds)
}

/// The first day any fact is dated (A2), the start of every history: the first transaction
/// of stocks, crypto and other assets, the first balance record, each property's purchase
/// date (else its first valuation) and each loan's start. Never before 2000-01-01. Bonds
/// and accounts without records have no date and do not count. `None` without a dated fact.
pub fn earliest_fact_day(conn: &Connection) -> Result<Option<i64>> {
    const SOURCES: [&str; 6] = [
        "SELECT MIN(transaction_date) FROM investment_transactions",
        "SELECT MIN(transaction_date) FROM crypto_transactions",
        "SELECT MIN(transaction_date) FROM other_asset_transactions",
        "SELECT MIN(as_of) FROM bank_balance_records",
        "SELECT MIN(COALESCE(purchase_date,
                (SELECT MIN(valued_at) FROM real_estate_valuations
                 WHERE real_estate_id = real_estate.id)))
         FROM real_estate",
        "SELECT MIN(start_date) FROM loans",
    ];
    let mut earliest: Option<i64> = None;
    for sql in SOURCES {
        // MIN over no rows is one row holding NULL.
        let day: Option<i64> = conn.query_row(sql, [], |row| row.get(0))?;
        earliest = match (earliest, day) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    Ok(earliest.map(|day| day_floor(day.max(EARLIEST_DAY_FLOOR))))
}

/// A rebuild of `[from, until)`: the facts, read once, and the range to write.
pub struct RebuildPlan {
    facts: Facts,
    first_fact_day: Option<i64>,
    /// `max(from, first fact day)`; meaningful only with a first fact day.
    start: i64,
    until: i64,
}

// A rebuild holds its plan across the awaits between chunks (`history_recalc`).
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<RebuildPlan>();
};

impl RebuildPlan {
    /// The first day any fact is dated (A2); `None` without one.
    pub fn first_fact_day(&self) -> Option<i64> {
        self.first_fact_day
    }

    /// Day ranges `[start, end)` of at most [`CHUNK_DAYS`] days covering the days to write,
    /// oldest first; none when the range is empty or no fact is dated.
    pub fn chunks(&self) -> Vec<(i64, i64)> {
        if self.first_fact_day.is_none() {
            return Vec::new();
        }
        chunk_ranges(self.start, self.until, CHUNK_DAYS * DAY)
    }
}

/// Read the facts and fix the range of a rebuild of `[from_day, until_exclusive)`: it starts
/// at `max(from_day, first fact day)`. `until_exclusive` is today's start, so today's row,
/// which `update_todays_snapshot` owns, is never touched.
pub fn plan_rebuild(conn: &Connection, from_day: i64, until_exclusive: i64) -> Result<RebuildPlan> {
    let first_fact_day = earliest_fact_day(conn)?;
    Ok(RebuildPlan {
        facts: Facts::load(conn, until_exclusive)?,
        first_fact_day,
        start: day_floor(from_day).max(first_fact_day.unwrap_or(0)),
        until: until_exclusive,
    })
}

/// Delete the reconstructed rows dated before the first fact (before `until` when no fact
/// is dated): there is nothing to derive them from. Live rows stay. Returns the rows removed.
pub fn prune_before_first_fact(conn: &Connection, plan: &RebuildPlan) -> Result<usize> {
    let bound = plan
        .first_fact_day
        .map_or(plan.until, |first| first.min(plan.until));
    delete_backfill_before(conn, bound)
}

/// Write the days of `[chunk_start, chunk_end)` in one transaction: every day gets exactly
/// one row (A3). A day without a row, or with a reconstructed one, gets all eight breakdowns
/// from [`Facts::day_values`]. A live row is an authentic record: only the classes the facts
/// cover completely are rewritten (D1), stocks and crypto always, and the others keep what
/// was recorded.
///
/// The chunks of one plan must be written oldest first (the cursors only move forward).
pub fn rebuild_chunk(
    conn: &Connection,
    plan: &mut RebuildPlan,
    chunk_start: i64,
    chunk_end: i64,
) -> Result<RebuildStats> {
    let sources = day_sources(conn, chunk_start, chunk_end)?;
    let tx = conn.unchecked_transaction()?;
    let mut stats = RebuildStats::default();
    let mut day = chunk_start;
    while day < chunk_end {
        let values = plan.facts.day_values(day);
        match sources.get(&day) {
            Some(SnapshotSource::Live) => {
                update_class_set(&tx, day, &values.breakdowns, values.known)?;
            }
            _ => upsert_snapshot(&tx, day, SnapshotSource::Backfill, &values.breakdowns)?,
        }
        stats.upserted += 1;
        day += DAY;
    }
    tx.commit()?;
    Ok(stats)
}

/// Rebuild the history of `[from_day, until_exclusive)` from the dated facts (A4): start at
/// `max(from_day, first fact day)`, one row for every day, reconstructed rows before the
/// first fact deleted, [`CHUNK_DAYS`] days per transaction. Idempotent. `until_exclusive`
/// is today's start (today's row is never touched).
pub fn rebuild_days(
    conn: &Connection,
    from_day: i64,
    until_exclusive: i64,
) -> Result<RebuildStats> {
    let mut plan = plan_rebuild(conn, from_day, until_exclusive)?;
    let mut stats = RebuildStats {
        upserted: 0,
        deleted: prune_before_first_fact(conn, &plan)?,
    };
    for (chunk_start, chunk_end) in plan.chunks() {
        let chunk = rebuild_chunk(conn, &mut plan, chunk_start, chunk_end)?;
        stats.upserted += chunk.upserted;
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::currency::breakdown_from_json;
    use crate::services::loan_amortization::today_utc_day;
    use crate::services::net_worth::compute_breakdowns;
    use crate::services::valuations::{value_at_day, ValuationKind};
    use rusqlite::params;

    /// A UTC midnight (the day tests count from).
    const BASE: i64 = 1_700_006_400;

    fn d(n: i64) -> i64 {
        BASE + n * DAY
    }

    /// UTC-midnight unix seconds of an ISO date.
    fn iso(date: &str) -> i64 {
        let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        date.signed_duration_since(epoch).num_days() * DAY
    }

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
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
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                as_of INTEGER NOT NULL
            );
            CREATE TABLE bonds (
                id TEXT PRIMARY KEY,
                coupon_value TEXT NOT NULL,
                quantity TEXT NOT NULL DEFAULT '1',
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE loans (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                principal TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                interest_rate TEXT NOT NULL DEFAULT '0',
                interest_rate_validity_date INTEGER,
                monthly_payment TEXT NOT NULL DEFAULT '0',
                start_date INTEGER NOT NULL DEFAULT (unixepoch()),
                end_date INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                balance_anchor_amount TEXT,
                balance_anchor_date INTEGER
            );
            CREATE TABLE loan_events (
                id TEXT PRIMARY KEY,
                loan_id TEXT NOT NULL REFERENCES loans(id) ON DELETE CASCADE,
                kind TEXT NOT NULL CHECK (kind IN ('extra_payment', 'rate_change', 'balance_check')),
                event_day INTEGER NOT NULL,
                amount TEXT,
                rate TEXT,
                monthly_payment TEXT,
                note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                previous_rate TEXT,
                previous_monthly_payment TEXT
            );
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                purchase_price TEXT NOT NULL DEFAULT '0',
                purchase_price_currency TEXT NOT NULL DEFAULT 'CZK',
                market_price TEXT NOT NULL DEFAULT '0',
                market_price_currency TEXT NOT NULL DEFAULT 'CZK',
                purchase_date INTEGER
            );
            CREATE TABLE real_estate_valuations (
                id TEXT PRIMARY KEY,
                real_estate_id TEXT NOT NULL,
                value TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                valued_at INTEGER NOT NULL,
                note TEXT,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE other_assets (
                id TEXT PRIMARY KEY,
                quantity TEXT NOT NULL DEFAULT '0',
                market_price TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE other_asset_transactions (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL,
                type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE other_asset_valuations (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL,
                value TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                valued_at INTEGER NOT NULL,
                note TEXT,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, quantity TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE crypto_investments (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, quantity TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY, investment_id TEXT NOT NULL DEFAULT '', ticker TEXT NOT NULL,
                type TEXT NOT NULL, quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL DEFAULT '0', currency TEXT NOT NULL DEFAULT 'CZK',
                transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY, investment_id TEXT NOT NULL DEFAULT '', ticker TEXT NOT NULL,
                type TEXT NOT NULL, quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL DEFAULT '0', currency TEXT NOT NULL DEFAULT 'CZK',
                transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL, price TEXT NOT NULL,
                currency TEXT NOT NULL, UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE crypto_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL, price TEXT NOT NULL,
                currency TEXT NOT NULL, UNIQUE(ticker, recorded_at)
            );
            "#,
        )
        .expect("schema");
        conn
    }

    // --- fixture helpers ---------------------------------------------------

    fn account(conn: &Connection, id: &str, currency: &str, balance: &str, excluded: bool) {
        conn.execute(
            "INSERT INTO bank_accounts (id, currency, balance, exclude_from_balance)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, currency, balance, excluded],
        )
        .expect("account");
    }

    fn record(conn: &Connection, account: &str, as_of: i64, amount: &str, currency: &str) {
        conn.execute(
            "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                format!("{account}-{as_of}"),
                account,
                amount,
                currency,
                as_of
            ],
        )
        .expect("record");
    }

    fn property(
        conn: &Connection,
        id: &str,
        kind: &str,
        purchase: Option<(i64, &str, &str)>,
        market: (&str, &str),
    ) {
        conn.execute(
            "INSERT INTO real_estate (id, type, purchase_price, purchase_price_currency,
                market_price, market_price_currency, purchase_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                kind,
                purchase.map_or("0", |p| p.1),
                purchase.map_or("CZK", |p| p.2),
                market.0,
                market.1,
                purchase.map(|p| p.0),
            ],
        )
        .expect("property");
    }

    /// A row of a valuation log: `(table, parent column)` names the log.
    fn valuation(
        conn: &Connection,
        log: (&str, &str),
        owner: &str,
        valued_at: i64,
        amount: (&str, &str),
        created_at: i64,
    ) {
        let (table, fk) = log;
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, {fk}, value, currency, valued_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            ),
            params![
                format!("{owner}-{valued_at}-{created_at}"),
                owner,
                amount.0,
                amount.1,
                valued_at,
                created_at
            ],
        )
        .expect("valuation");
    }

    fn property_valuation(
        conn: &Connection,
        property: &str,
        valued_at: i64,
        value: &str,
        currency: &str,
        created_at: i64,
    ) {
        valuation(
            conn,
            ("real_estate_valuations", "real_estate_id"),
            property,
            valued_at,
            (value, currency),
            created_at,
        );
    }

    fn asset_valuation(
        conn: &Connection,
        asset: &str,
        valued_at: i64,
        value: &str,
        currency: &str,
        created_at: i64,
    ) {
        valuation(
            conn,
            ("other_asset_valuations", "asset_id"),
            asset,
            valued_at,
            (value, currency),
            created_at,
        );
    }

    fn asset(conn: &Connection, id: &str, currency: &str, quantity: &str, market_price: &str) {
        conn.execute(
            "INSERT INTO other_assets (id, quantity, market_price, currency)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, quantity, market_price, currency],
        )
        .expect("asset");
    }

    fn asset_tx(conn: &Connection, asset: &str, kind: &str, quantity: &str, day: i64) {
        conn.execute(
            "INSERT INTO other_asset_transactions (id, asset_id, type, quantity, transaction_date)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![format!("{asset}-{kind}-{day}"), asset, kind, quantity, day],
        )
        .expect("asset transaction");
    }

    fn loan(conn: &Connection, id: &str, currency: &str, terms: (&str, &str, &str), start: i64) {
        conn.execute(
            "INSERT INTO loans (id, name, principal, currency, interest_rate, monthly_payment,
                start_date)
             VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, terms.0, currency, terms.1, terms.2, start],
        )
        .expect("loan");
    }

    fn loan_event(
        conn: &Connection,
        loan: &str,
        kind: &str,
        day: i64,
        amount: &str,
        created_at: i64,
    ) {
        conn.execute(
            "INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                format!("{loan}-{kind}-{day}-{created_at}"),
                loan,
                kind,
                day,
                amount,
                created_at
            ],
        )
        .expect("loan event");
    }

    fn bond(conn: &Connection, id: &str, coupon: &str, quantity: &str, currency: &str) {
        conn.execute(
            "INSERT INTO bonds (id, coupon_value, quantity, currency) VALUES (?1, ?2, ?3, ?4)",
            params![id, coupon, quantity, currency],
        )
        .expect("bond");
    }

    /// A stock or crypto ticker's history row and transaction.
    fn ticker_row(conn: &Connection, table: &str, ticker: &str, day: i64, price: f64, ccy: &str) {
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, ticker, recorded_at, quantity, price, currency)
                 VALUES (?1, ?2, ?3, '1', ?4, ?5)"
            ),
            params![
                format!("{ticker}-{day}"),
                ticker,
                day,
                price.to_string(),
                ccy
            ],
        )
        .expect("ticker row");
    }

    fn ticker_tx(conn: &Connection, table: &str, ticker: &str, kind: &str, day: i64) {
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, ticker, type, quantity, transaction_date)
                 VALUES (?1, ?2, ?3, '1', ?4)"
            ),
            params![format!("{ticker}-{kind}-{day}"), ticker, kind, day],
        )
        .expect("ticker transaction");
    }

    /// The values of one day, from the facts as they are in `conn`.
    fn values_on(conn: &Connection, day: i64) -> DayValues {
        Facts::load(conn, day + DAY).expect("facts").day_values(day)
    }

    fn map(entries: &[(&str, f64)]) -> Breakdown {
        entries.iter().map(|(c, v)| (c.to_string(), *v)).collect()
    }

    fn approx(actual: &Breakdown, expected: &Breakdown) {
        let currencies: std::collections::BTreeSet<&String> =
            actual.keys().chain(expected.keys()).collect();
        for currency in currencies {
            let a = actual.get(currency).copied().unwrap_or(0.0);
            let e = expected.get(currency).copied().unwrap_or(0.0);
            assert!(
                (a - e).abs() < 1e-6,
                "{currency}: {a} != {e} ({actual:?} vs {expected:?})"
            );
        }
    }

    // --- savings -------------------------------------------------------------

    #[test]
    fn savings_take_the_newest_record_on_or_before_the_day() {
        let conn = setup_test_db();
        account(&conn, "a", "CZK", "200", false);
        record(&conn, "a", d(10), "100", "CZK");
        record(&conn, "a", d(20), "200", "CZK");

        // before the first record the account is not counted, and that day is unknown (D4)
        let before = values_on(&conn, d(9));
        assert!(before.breakdowns.savings.is_empty());
        assert!(!before.known.savings);

        for (day, expected) in [(10, 100.0), (19, 100.0), (20, 200.0), (50, 200.0)] {
            let values = values_on(&conn, d(day));
            assert_eq!(
                values.breakdowns.savings,
                map(&[("CZK", expected)]),
                "{day}"
            );
            assert!(values.known.savings, "{day}");
        }
    }

    #[test]
    fn an_account_without_records_carries_its_current_balance_as_unknown() {
        let conn = setup_test_db();
        account(&conn, "undated", "EUR", "50", false);
        account(&conn, "dated", "CZK", "0", false);
        record(&conn, "dated", d(10), "100", "CZK");

        let values = values_on(&conn, d(15));
        assert_eq!(
            values.breakdowns.savings,
            map(&[("CZK", 100.0), ("EUR", 50.0)])
        );
        assert!(!values.known.savings, "the undated balance is not a fact");
        // ... on every day, before the dated account's first record too
        let early = values_on(&conn, d(1));
        assert_eq!(early.breakdowns.savings, map(&[("EUR", 50.0)]));
    }

    #[test]
    fn an_account_excluded_from_the_balance_is_never_counted() {
        let conn = setup_test_db();
        account(&conn, "counted", "CZK", "10", false);
        record(&conn, "counted", d(1), "10", "CZK");
        account(&conn, "excluded", "CZK", "99999", true);
        record(&conn, "excluded", d(1), "99999", "CZK");
        account(&conn, "excluded-undated", "CZK", "777", true);

        let values = values_on(&conn, d(5));
        assert_eq!(values.breakdowns.savings, map(&[("CZK", 10.0)]));
        assert!(values.known.savings, "an excluded account is no item");
    }

    #[test]
    fn savings_are_known_only_when_every_account_is() {
        let conn = setup_test_db();
        account(&conn, "a", "CZK", "0", false);
        account(&conn, "b", "USD", "0", false);
        record(&conn, "a", d(5), "100", "CZK");
        record(&conn, "b", d(10), "7", "USD");

        let partial = values_on(&conn, d(7));
        assert_eq!(partial.breakdowns.savings, map(&[("CZK", 100.0)]));
        assert!(!partial.known.savings);

        let full = values_on(&conn, d(10));
        assert_eq!(
            full.breakdowns.savings,
            map(&[("CZK", 100.0), ("USD", 7.0)])
        );
        assert!(full.known.savings);
    }

    // --- loans ---------------------------------------------------------------

    /// 12 000 at 0 %, 1 000 a month on the 10th from 2024-01-10.
    fn yearly_loan(conn: &Connection) {
        loan(conn, "l", "CZK", ("12000", "0", "1000"), iso("2024-01-10"));
    }

    #[test]
    fn a_loan_is_zero_before_its_start_and_amortizes_after_it() {
        let conn = setup_test_db();
        yearly_loan(&conn);

        let before = values_on(&conn, iso("2024-01-09"));
        assert!(before.breakdowns.loans.is_empty());
        assert!(before.known.loans);

        let balance = |date: &str| values_on(&conn, iso(date)).breakdowns.loans;
        assert_eq!(balance("2024-01-10"), map(&[("CZK", 12_000.0)]));
        assert_eq!(balance("2024-03-09"), map(&[("CZK", 11_000.0)]));
        assert_eq!(balance("2024-03-10"), map(&[("CZK", 10_000.0)]));
        assert!(values_on(&conn, iso("2024-03-10")).known.loans);
    }

    #[test]
    fn a_manual_balance_holds_and_the_estimated_past_before_it_is_unknown() {
        let conn = setup_test_db();
        yearly_loan(&conn);
        loan_event(&conn, "l", "balance_check", iso("2024-06-20"), "4000", 1);

        // before the first manual record the loan follows the estimate (D5), unknown
        let before = values_on(&conn, iso("2024-05-10"));
        assert_eq!(before.breakdowns.loans, map(&[("CZK", 8_000.0)]));
        assert!(!before.known.loans);

        // from the record on it holds: no amortization after it (D6)
        for date in ["2024-06-20", "2024-09-10", "2025-12-31"] {
            let values = values_on(&conn, iso(date));
            assert_eq!(values.breakdowns.loans, map(&[("CZK", 4_000.0)]), "{date}");
            assert!(values.known.loans, "{date}");
        }
    }

    // --- real estate ---------------------------------------------------------

    #[test]
    fn a_property_is_worth_nothing_before_its_purchase_date() {
        let conn = setup_test_db();
        property(
            &conn,
            "p",
            "investment",
            Some((d(10), "3000", "CZK")),
            ("3500", "CZK"),
        );
        property_valuation(&conn, "p", d(20), "3500", "CZK", 1);

        let before = values_on(&conn, d(9));
        assert!(before.breakdowns.real_estate_investment.is_empty());
        assert!(before.known.real_estate);
    }

    #[test]
    fn the_purchase_price_applies_until_the_first_valuation() {
        let conn = setup_test_db();
        property(
            &conn,
            "p",
            "investment",
            Some((d(10), "3000", "USD")),
            ("3500", "CZK"),
        );
        property_valuation(&conn, "p", d(20), "3500", "CZK", 1);

        let bought = values_on(&conn, d(10));
        assert_eq!(
            bought.breakdowns.real_estate_investment,
            map(&[("USD", 3000.0)]),
            "the purchase price, in its own currency"
        );
        assert!(bought.known.real_estate);
        assert_eq!(
            values_on(&conn, d(19)).breakdowns.real_estate_investment,
            map(&[("USD", 3000.0)])
        );
        assert_eq!(
            values_on(&conn, d(20)).breakdowns.real_estate_investment,
            map(&[("CZK", 3500.0)]),
            "the valuation replaces it, in the valuation's currency"
        );
    }

    #[test]
    fn a_property_without_any_valuation_is_not_counted_on_any_day() {
        let conn = setup_test_db();
        // Bought for 3000 but never estimated: the purchase price opens the value line only
        // once the property has an estimate (`valuationTrace`), and today's market price is 0.
        property(
            &conn,
            "p",
            "investment",
            Some((d(10), "3000", "CZK")),
            ("0", "CZK"),
        );

        for day in [d(9), d(10), d(40)] {
            let values = values_on(&conn, day);
            assert!(
                values.breakdowns.real_estate_investment.is_empty(),
                "day {day}"
            );
            assert!(values.known.real_estate, "nothing to cover on day {day}");
        }
    }

    #[test]
    fn the_newest_valuation_applies_and_of_one_day_the_last_created() {
        let conn = setup_test_db();
        property(&conn, "p", "investment", None, ("0", "CZK"));
        property_valuation(&conn, "p", d(10), "100", "CZK", 1);
        property_valuation(&conn, "p", d(30), "300", "CZK", 5);
        property_valuation(&conn, "p", d(30), "350", "CZK", 9);
        property_valuation(&conn, "p", d(30), "320", "CZK", 7);

        let value = |day: i64| values_on(&conn, d(day)).breakdowns.real_estate_investment;
        assert_eq!(value(10), map(&[("CZK", 100.0)]));
        assert_eq!(value(29), map(&[("CZK", 100.0)]));
        assert_eq!(value(30), map(&[("CZK", 350.0)]), "entered last");
        assert_eq!(value(90), map(&[("CZK", 350.0)]));
    }

    #[test]
    fn without_a_purchase_date_the_days_before_the_first_valuation_are_not_counted() {
        let conn = setup_test_db();
        property(&conn, "p", "personal", None, ("500", "CZK"));
        property_valuation(&conn, "p", d(10), "500", "CZK", 1);

        let before = values_on(&conn, d(9));
        assert!(before.breakdowns.real_estate_personal.is_empty());
        assert!(!before.known.real_estate);
        let after = values_on(&conn, d(10));
        assert_eq!(
            after.breakdowns.real_estate_personal,
            map(&[("CZK", 500.0)])
        );
        assert!(after.known.real_estate);
    }

    #[test]
    fn real_estate_is_split_into_personal_and_investment_by_the_current_type() {
        let conn = setup_test_db();
        property(&conn, "home", "personal", None, ("800", "EUR"));
        property_valuation(&conn, "home", d(1), "800", "EUR", 1);
        property(&conn, "flat", "investment", None, ("300", "EUR"));
        property_valuation(&conn, "flat", d(1), "300", "EUR", 1);
        property(&conn, "garage", "something-else", None, ("50", "CZK"));
        property_valuation(&conn, "garage", d(1), "50", "CZK", 1);

        let values = values_on(&conn, d(5));
        assert_eq!(
            values.breakdowns.real_estate_personal,
            map(&[("EUR", 800.0)])
        );
        assert_eq!(
            values.breakdowns.real_estate_investment,
            map(&[("EUR", 300.0), ("CZK", 50.0)]),
            "every type but personal is an investment property"
        );
    }

    // --- other assets --------------------------------------------------------

    #[test]
    fn other_assets_are_the_quantity_times_the_valuation_of_the_day() {
        let conn = setup_test_db();
        asset(&conn, "gold", "CZK", "0", "99");
        asset_tx(&conn, "gold", "buy", "2", d(5));
        asset_tx(&conn, "gold", "buy", "1", d(15));
        asset_tx(&conn, "gold", "sell", "3", d(30));
        asset_valuation(&conn, "gold", d(10), "10", "CZK", 1);
        asset_valuation(&conn, "gold", d(20), "20", "EUR", 2);

        let value = |day: i64| values_on(&conn, d(day));
        assert!(
            value(4).breakdowns.other_assets.is_empty(),
            "nothing held yet"
        );
        assert_eq!(
            value(5).breakdowns.other_assets,
            map(&[("CZK", 20.0)]),
            "before every valuation the earliest applies"
        );
        assert_eq!(value(10).breakdowns.other_assets, map(&[("CZK", 20.0)]));
        assert_eq!(value(15).breakdowns.other_assets, map(&[("CZK", 30.0)]));
        assert_eq!(
            value(20).breakdowns.other_assets,
            map(&[("EUR", 60.0)]),
            "in the valuation's currency"
        );
        let sold = value(30);
        assert!(sold.breakdowns.other_assets.is_empty(), "sold out");
        assert!(sold.known.other_assets);
    }

    #[test]
    fn an_other_asset_without_valuations_uses_its_market_price() {
        let conn = setup_test_db();
        asset(&conn, "car", "EUR", "1", "4000");
        asset_tx(&conn, "car", "buy", "1", d(3));

        assert!(values_on(&conn, d(2)).breakdowns.other_assets.is_empty());
        let values = values_on(&conn, d(3));
        assert_eq!(values.breakdowns.other_assets, map(&[("EUR", 4000.0)]));
        assert!(values.known.other_assets);
    }

    /// The valuation rule is `valuations::value_at_day`'s: the amounts must not drift apart.
    #[test]
    fn other_asset_values_follow_value_at_day() {
        let conn = setup_test_db();
        asset(&conn, "x", "CZK", "1", "5");
        asset_tx(&conn, "x", "buy", "1", d(0));
        asset_valuation(&conn, "x", d(10), "11", "CZK", 1);
        asset_valuation(&conn, "x", d(20), "22", "CZK", 2);
        asset_valuation(&conn, "x", d(20), "23", "CZK", 3);
        asset_valuation(&conn, "x", d(40), "44", "CZK", 4);

        let mut facts = Facts::load(&conn, d(60)).expect("facts");
        for n in 0..60 {
            let expected = value_at_day(&conn, ValuationKind::OtherAsset, "x", d(n))
                .expect("value")
                .expect("has valuations");
            let values = facts.day_values(d(n));
            assert_eq!(
                values.breakdowns.other_assets,
                map(&[("CZK", expected)]),
                "day {n}"
            );
        }
    }

    // --- bonds, stocks, crypto ----------------------------------------------

    #[test]
    fn bonds_carry_their_current_value_on_every_day_as_unknown() {
        let conn = setup_test_db();
        bond(&conn, "b1", "1000", "3", "CZK");
        bond(&conn, "b2", "50", "2", "EUR");

        for day in [-500, 0, 400] {
            let values = values_on(&conn, d(day));
            assert_eq!(
                values.breakdowns.bonds,
                map(&[("CZK", 3000.0), ("EUR", 100.0)])
            );
            assert!(!values.known.bonds);
        }
    }

    #[test]
    fn stocks_and_crypto_come_from_the_ticker_series_and_are_always_known() {
        let conn = setup_test_db();
        ticker_tx(&conn, "investment_transactions", "AAA", "buy", d(1));
        ticker_tx(&conn, "investment_transactions", "AAA", "sell", d(5));
        ticker_row(&conn, "stock_value_history", "AAA", d(1), 100.0, "CZK");
        ticker_row(&conn, "stock_value_history", "AAA", d(3), 120.0, "CZK");
        ticker_tx(&conn, "crypto_transactions", "BTC", "buy", d(2));
        ticker_row(&conn, "crypto_value_history", "BTC", d(2), 7.0, "USD");

        let mut facts = Facts::load(&conn, d(10)).expect("facts");
        let expected = [
            (0, None, None),
            (1, Some(100.0), None),
            (2, Some(100.0), Some(7.0)),
            (3, Some(120.0), Some(7.0)),
            (4, Some(120.0), Some(7.0)),
            (5, None, Some(7.0)),
        ];
        for (n, stock, crypto) in expected {
            let values = facts.day_values(d(n));
            let stocks = stock.map_or_else(Breakdown::new, |v| map(&[("CZK", v)]));
            let coins = crypto.map_or_else(Breakdown::new, |v| map(&[("USD", v)]));
            assert_eq!(values.breakdowns.investments, stocks, "stocks, day {n}");
            assert_eq!(values.breakdowns.crypto, coins, "crypto, day {n}");
            assert!(values.known.investments && values.known.crypto);
        }
    }

    // --- earliest fact day ---------------------------------------------------

    #[test]
    fn there_is_no_earliest_day_without_a_dated_fact() {
        let conn = setup_test_db();
        account(&conn, "undated", "CZK", "10", false);
        bond(&conn, "b", "1", "1", "CZK");
        property(&conn, "p", "personal", None, ("0", "CZK"));
        assert_eq!(earliest_fact_day(&conn).unwrap(), None);
    }

    #[test]
    fn every_dated_fact_can_be_the_earliest_day() {
        type Seed = fn(&Connection, i64);
        let sources: [(&str, Seed); 6] = [
            ("stock transaction", |c, day| {
                ticker_tx(c, "investment_transactions", "A", "buy", day)
            }),
            ("crypto transaction", |c, day| {
                ticker_tx(c, "crypto_transactions", "B", "buy", day)
            }),
            ("other asset transaction", |c, day| {
                asset(c, "x", "CZK", "1", "1");
                asset_tx(c, "x", "buy", "1", day);
            }),
            ("balance record", |c, day| {
                account(c, "a", "CZK", "1", false);
                record(c, "a", day, "1", "CZK");
            }),
            ("property purchase date", |c, day| {
                property(c, "p", "personal", Some((day, "1", "CZK")), ("0", "CZK"))
            }),
            ("loan start", |c, day| {
                loan(c, "l", "CZK", ("1", "0", "0"), day)
            }),
        ];
        for (name, seed) in sources {
            let conn = setup_test_db();
            // a later decoy from another source, then the fact under test
            account(&conn, "decoy", "CZK", "1", false);
            record(&conn, "decoy", d(500), "1", "CZK");
            seed(&conn, d(100));
            assert_eq!(earliest_fact_day(&conn).unwrap(), Some(d(100)), "{name}");
        }
    }

    #[test]
    fn a_property_without_a_purchase_date_starts_at_its_first_valuation() {
        let conn = setup_test_db();
        property(&conn, "p", "personal", None, ("1", "CZK"));
        property_valuation(&conn, "p", d(40), "1", "CZK", 1);
        property_valuation(&conn, "p", d(30), "1", "CZK", 2);
        assert_eq!(earliest_fact_day(&conn).unwrap(), Some(d(30)));

        // a purchase date wins over the property's own older valuation: q counts from
        // d(60), not from d(20), so p's first valuation is still the earliest day
        property(
            &conn,
            "q",
            "personal",
            Some((d(60), "1", "CZK")),
            ("1", "CZK"),
        );
        property_valuation(&conn, "q", d(20), "1", "CZK", 1);
        assert_eq!(earliest_fact_day(&conn).unwrap(), Some(d(30)));
    }

    #[test]
    fn the_earliest_day_is_never_before_2000() {
        let conn = setup_test_db();
        ticker_tx(
            &conn,
            "investment_transactions",
            "OLD",
            "buy",
            iso("1990-05-05"),
        );
        assert_eq!(earliest_fact_day(&conn).unwrap(), Some(EARLIEST_DAY_FLOOR));
    }

    // --- rebuild_days --------------------------------------------------------

    /// A canonical text of a row's breakdowns: currencies sorted, amounts fixed.
    fn canonical(json: &str) -> String {
        let mut entries: Vec<String> = breakdown_from_json(json)
            .into_iter()
            .map(|(currency, amount)| format!("{currency}={amount:.4}"))
            .collect();
        entries.sort();
        entries.join(",")
    }

    /// Every stored row as `(recorded_at, source, every breakdown)`, oldest first. The ids are
    /// left out: they are random.
    fn dump(conn: &Connection) -> Vec<(i64, String, Vec<String>)> {
        let mut stmt = conn
            .prepare(
                "SELECT recorded_at, source, savings_by_currency, investments_by_currency,
                        crypto_by_currency, bonds_by_currency, real_estate_by_currency,
                        real_estate_personal_by_currency, loans_by_currency,
                        other_assets_by_currency
                 FROM portfolio_metrics_history ORDER BY recorded_at",
            )
            .expect("prepare");
        stmt.query_map([], |row| {
            let classes = (2..10)
                .map(|index| row.get::<_, String>(index).map(|json| canonical(&json)))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((row.get(0)?, row.get(1)?, classes))
        })
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows")
    }

    fn row_ids(conn: &Connection) -> Vec<(i64, String)> {
        let mut stmt = conn
            .prepare("SELECT recorded_at, id FROM portfolio_metrics_history ORDER BY recorded_at")
            .expect("prepare");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("rows")
    }

    fn column(conn: &Connection, day: i64, column: &str) -> Breakdown {
        let json: String = conn
            .query_row(
                &format!(
                    "SELECT {column} FROM portfolio_metrics_history
                     WHERE recorded_at >= ?1 AND recorded_at < ?2"
                ),
                [day, day + DAY],
                |row| row.get(0),
            )
            .expect("breakdown column");
        breakdown_from_json(&json)
    }

    fn row_days(conn: &Connection) -> Vec<i64> {
        dump(conn)
            .into_iter()
            .map(|(recorded_at, _, _)| recorded_at.div_euclid(DAY) * DAY)
            .collect()
    }

    /// A live row of `day` holding a sentinel in every class that a rebuild may or may not
    /// rewrite.
    fn live_row(conn: &Connection, day: i64) {
        conn.execute(
            "INSERT INTO portfolio_metrics_history (id, recorded_at, savings_by_currency,
                investments_by_currency, crypto_by_currency, bonds_by_currency,
                real_estate_by_currency, real_estate_personal_by_currency, loans_by_currency,
                other_assets_by_currency, source)
             VALUES (?1, ?2, '{\"CZK\":999}', '{\"CZK\":555}', '{\"CZK\":444}', '{\"CZK\":333}',
                '{\"CZK\":222}', '{\"CZK\":111}', '{\"CZK\":7777}', '{\"CZK\":888}', 'live')",
            params![format!("live-{day}"), day + 3_600],
        )
        .expect("live row");
    }

    #[test]
    fn a_fresh_profile_gets_every_day_since_its_first_balance_record() {
        let conn = setup_test_db();
        let today = d(400);
        account(&conn, "a", "CZK", "500", false);
        record(&conn, "a", today - 365 * DAY, "100", "CZK");
        record(&conn, "a", today - 100 * DAY, "300", "CZK");
        record(&conn, "a", today - 30 * DAY, "500", "CZK");
        // a fresh profile: only today's live row exists
        live_row(&conn, today);

        // the range reaches far before the first fact: it starts there
        let stats = rebuild_days(&conn, today - 3_000 * DAY, today).expect("rebuild");
        assert_eq!(stats.upserted, 365);

        let days = row_days(&conn);
        assert_eq!(days.len(), 366, "365 past days and today's live row");
        assert_eq!(days.first(), Some(&(today - 365 * DAY)));
        assert!(days.windows(2).all(|pair| pair[1] - pair[0] == DAY));
        let savings = |ago: i64| column(&conn, today - ago * DAY, "savings_by_currency");
        assert_eq!(savings(365), map(&[("CZK", 100.0)]));
        assert_eq!(savings(101), map(&[("CZK", 100.0)]));
        assert_eq!(savings(100), map(&[("CZK", 300.0)]));
        assert_eq!(savings(31), map(&[("CZK", 300.0)]));
        assert_eq!(savings(30), map(&[("CZK", 500.0)]));
        assert_eq!(savings(1), map(&[("CZK", 500.0)]));
        // today's row is the live snapshot's
        assert_eq!(
            column(&conn, today, "savings_by_currency"),
            map(&[("CZK", 999.0)])
        );
    }

    #[test]
    fn a_new_row_carries_undated_items_and_leaves_the_other_unknown_ones_out() {
        let conn = setup_test_db();
        let today = d(40);
        account(&conn, "dated", "CZK", "100", false);
        record(&conn, "dated", d(10), "100", "CZK");
        account(&conn, "undated", "EUR", "50", false);
        bond(&conn, "b", "1000", "1", "CZK");
        // no purchase date: not counted before its first valuation
        property(&conn, "p", "personal", None, ("700", "CZK"));
        property_valuation(&conn, "p", d(30), "700", "CZK", 1);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        // before the dated account's first record: the undated balance still carries
        assert_eq!(
            column(&conn, d(10), "savings_by_currency"),
            map(&[("CZK", 100.0), ("EUR", 50.0)])
        );
        assert_eq!(
            column(&conn, d(10), "bonds_by_currency"),
            map(&[("CZK", 1000.0)])
        );
        assert!(column(&conn, d(10), "real_estate_by_currency").is_empty());
        assert_eq!(
            column(&conn, d(30), "real_estate_by_currency"),
            map(&[("CZK", 700.0)])
        );
        assert_eq!(
            column(&conn, d(30), "real_estate_personal_by_currency"),
            map(&[("CZK", 700.0)])
        );
    }

    #[test]
    fn a_live_row_is_rewritten_only_for_the_classes_the_facts_cover() {
        let conn = setup_test_db();
        let today = d(30);
        account(&conn, "a", "CZK", "100", false);
        record(&conn, "a", d(10), "100", "CZK");
        account(&conn, "b", "CZK", "50", false);
        record(&conn, "b", d(20), "50", "CZK");
        // no payment: the balance stays at the principal
        loan(&conn, "l", "CZK", ("6000", "0", "0"), d(0));
        bond(&conn, "bond", "1", "1", "CZK");
        for day in [5, 15, 25] {
            live_row(&conn, d(day));
        }

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let at = |day: i64, column_name: &str| column(&conn, d(day), column_name);
        // account b has no record yet on day 5 and day 15: the savings keep what was recorded
        assert_eq!(at(5, "savings_by_currency"), map(&[("CZK", 999.0)]));
        assert_eq!(at(15, "savings_by_currency"), map(&[("CZK", 999.0)]));
        // both accounts are dated on day 25
        assert_eq!(at(25, "savings_by_currency"), map(&[("CZK", 150.0)]));
        // loans are fully dated: every live day is rewritten
        for day in [5, 15, 25] {
            assert_eq!(
                at(day, "loans_by_currency"),
                map(&[("CZK", 6000.0)]),
                "{day}"
            );
            // stocks and crypto always are (nothing is held: empty)
            assert!(at(day, "investments_by_currency").is_empty(), "{day}");
            assert!(at(day, "crypto_by_currency").is_empty(), "{day}");
            // bonds are never a fact: the recorded value stays
            assert_eq!(
                at(day, "bonds_by_currency"),
                map(&[("CZK", 333.0)]),
                "{day}"
            );
        }
        // the live rows stay live, at their own time of day
        let live: Vec<(i64, String)> = dump(&conn)
            .into_iter()
            .filter(|(_, source, _)| source == "live")
            .map(|(recorded_at, source, _)| (recorded_at, source))
            .collect();
        assert_eq!(
            live,
            vec![
                (d(5) + 3_600, "live".to_string()),
                (d(15) + 3_600, "live".to_string()),
                (d(25) + 3_600, "live".to_string())
            ]
        );
        // the days between are new rows
        assert_eq!(
            dump(&conn)
                .iter()
                .filter(|(_, source, _)| source == "backfill")
                .count(),
            27
        );
    }

    #[test]
    fn deleting_the_earliest_fact_deletes_the_reconstructed_rows_before_the_new_first_fact() {
        let conn = setup_test_db();
        let today = d(40);
        account(&conn, "a", "CZK", "200", false);
        record(&conn, "a", d(10), "100", "CZK");
        record(&conn, "a", d(20), "200", "CZK");
        live_row(&conn, d(15));

        rebuild_days(&conn, d(0), today).expect("first");
        assert_eq!(row_days(&conn).first(), Some(&d(10)));
        assert_eq!(row_days(&conn).len(), 30);

        conn.execute("DELETE FROM bank_balance_records WHERE as_of = ?1", [d(10)])
            .expect("delete the earliest record");
        let stats = rebuild_days(&conn, d(0), today).expect("second");
        assert_eq!(stats.deleted, 9, "d(10)..d(19) minus the live d(15)");

        let days = row_days(&conn);
        assert_eq!(
            days.first(),
            Some(&d(15)),
            "the live row before the first fact stays"
        );
        assert_eq!(days[1], d(20));
        assert_eq!(days.len(), 1 + 20);

        // with no fact left at all, every reconstructed row goes; live rows stay
        conn.execute("DELETE FROM bank_balance_records", [])
            .expect("delete every record");
        rebuild_days(&conn, d(0), today).expect("third");
        assert_eq!(row_days(&conn), vec![d(15)]);
    }

    /// A profile with a fact of every kind, spanning more days than one chunk.
    fn seed_history(conn: &Connection, today: i64) {
        account(conn, "a", "CZK", "500", false);
        record(conn, "a", today - 200 * DAY, "100", "CZK");
        record(conn, "a", today - 100 * DAY, "300", "CZK");
        record(conn, "a", today - 30 * DAY, "500", "CZK");
        account(conn, "undated", "EUR", "20", false);
        loan(conn, "l", "CZK", ("12000", "0", "1000"), today - 300 * DAY);
        property(
            conn,
            "p",
            "personal",
            Some((today - 150 * DAY, "3000000", "CZK")),
            ("3500000", "CZK"),
        );
        property_valuation(conn, "p", today - 120 * DAY, "3500000", "CZK", 1);
        asset(conn, "gold", "CZK", "2", "20");
        asset_tx(conn, "gold", "buy", "2", today - 90 * DAY);
        asset_valuation(conn, "gold", today - 90 * DAY, "10", "CZK", 1);
        asset_valuation(conn, "gold", today - 45 * DAY, "20", "CZK", 2);
        ticker_tx(
            conn,
            "investment_transactions",
            "AAA",
            "buy",
            today - 60 * DAY,
        );
        for ago in 1..=60 {
            ticker_row(
                conn,
                "stock_value_history",
                "AAA",
                today - ago * DAY,
                100.0 + ago as f64,
                "CZK",
            );
        }
        bond(conn, "b", "1000", "1", "CZK");
        live_row(conn, today);
        live_row(conn, today - 50 * DAY);
    }

    #[test]
    fn a_rebuild_twice_changes_nothing_the_second_time() {
        let conn = setup_test_db();
        let today = d(400);
        seed_history(&conn, today);

        rebuild_days(&conn, today - 1_000 * DAY, today).expect("first");
        let rows = dump(&conn);
        let ids = row_ids(&conn);
        assert!(rows.len() > 200);

        rebuild_days(&conn, today - 1_000 * DAY, today).expect("second");
        assert_eq!(dump(&conn), rows);
        assert_eq!(row_ids(&conn), ids, "the same rows, updated in place");
    }

    #[test]
    fn the_chunks_of_a_rebuild_write_what_a_single_pass_writes() {
        let today = d(400);
        let chunked = setup_test_db();
        seed_history(&chunked, today);
        let plan = plan_rebuild(&chunked, today - 1_000 * DAY, today).expect("plan");
        let chunks = plan.chunks();
        assert!(chunks.len() >= 4, "{} chunks", chunks.len());
        assert!(chunks
            .iter()
            .all(|(start, end)| (end - start) <= CHUNK_DAYS * DAY));
        drop(plan);
        rebuild_days(&chunked, today - 1_000 * DAY, today).expect("chunked");

        let single = setup_test_db();
        seed_history(&single, today);
        let mut plan = plan_rebuild(&single, today - 1_000 * DAY, today).expect("plan");
        prune_before_first_fact(&single, &plan).expect("prune");
        let start = plan.first_fact_day().expect("first fact day");
        let stats = rebuild_chunk(&single, &mut plan, start, today).expect("one pass");
        assert_eq!(stats.upserted as i64, (today - start) / DAY);

        assert_eq!(dump(&single), dump(&chunked));
    }

    #[test]
    fn a_rebuild_never_touches_today_and_starts_no_earlier_than_asked() {
        let conn = setup_test_db();
        let today = d(40);
        account(&conn, "a", "CZK", "100", false);
        record(&conn, "a", d(10), "100", "CZK");
        live_row(&conn, today);

        rebuild_days(&conn, d(30), today).expect("rebuild");

        assert_eq!(row_days(&conn).first(), Some(&d(30)), "from the asked day");
        assert_eq!(
            column(&conn, d(30), "savings_by_currency"),
            map(&[("CZK", 100.0)]),
            "the cursors catch up with the facts before the first written day"
        );
        assert_eq!(row_days(&conn).len(), 11, "d(30)..d(39) and today");
        let today_row = dump(&conn).pop().expect("today's row");
        assert_eq!(today_row.0, today + 3_600);
        assert_eq!(today_row.1, "live");
        assert_eq!(
            today_row.2[0], "CZK=999.0000",
            "today's savings are the live ones"
        );
    }

    #[test]
    fn a_manual_loan_balance_in_the_past_holds_in_the_rebuilt_history() {
        let conn = setup_test_db();
        let today = iso("2025-01-10");
        yearly_loan(&conn);
        loan_event(&conn, "l", "balance_check", iso("2024-06-20"), "4000", 1);

        rebuild_days(&conn, iso("2024-01-01"), today).expect("rebuild");

        let loans = |date: &str| column(&conn, iso(date), "loans_by_currency");
        assert_eq!(
            row_days(&conn).first(),
            Some(&iso("2024-01-10")),
            "history starts at the loan's start"
        );
        assert_eq!(loans("2024-01-10"), map(&[("CZK", 12_000.0)]));
        assert_eq!(
            loans("2024-05-10"),
            map(&[("CZK", 8_000.0)]),
            "estimated before it"
        );
        assert_eq!(loans("2024-06-19"), map(&[("CZK", 7_000.0)]));
        assert_eq!(loans("2024-06-20"), map(&[("CZK", 4_000.0)]));
        assert_eq!(
            loans("2024-12-31"),
            map(&[("CZK", 4_000.0)]),
            "held: no amortization"
        );
    }

    // --- consistency with today's live values (A8) ---------------------------

    #[test]
    fn day_values_of_today_agree_with_the_live_breakdowns() {
        let conn = setup_test_db();
        let today = today_utc_day();

        // savings: a dated account, an account without records, an excluded one
        account(&conn, "dated", "CZK", "1500", false);
        record(&conn, "dated", today - 90 * DAY, "1000", "CZK");
        record(&conn, "dated", today - 10 * DAY, "1500", "CZK");
        account(&conn, "undated", "EUR", "200", false);
        account(&conn, "excluded", "CZK", "99999", true);
        record(&conn, "excluded", today - 5 * DAY, "99999", "CZK");

        // loans: an estimated one, and one with a manual balance and an extra payment
        loan(
            &conn,
            "est",
            "CZK",
            ("50000", "5", "1000"),
            today - 400 * DAY,
        );
        loan(&conn, "man", "EUR", ("9000", "3", "300"), today - 200 * DAY);
        loan_event(&conn, "man", "balance_check", today - 30 * DAY, "7000", 1);
        loan_event(&conn, "man", "extra_payment", today - 10 * DAY, "500", 2);

        // real estate: a personal one valued in EUR, an investment one with a purchase date
        // and a property without one (the market price mirrors the newest valuation)
        property(&conn, "home", "personal", None, ("210000", "EUR"));
        property_valuation(&conn, "home", today - 300 * DAY, "200000", "EUR", 1);
        property_valuation(&conn, "home", today - 20 * DAY, "210000", "EUR", 2);
        property(
            &conn,
            "flat",
            "investment",
            Some((today - 500 * DAY, "3000000", "CZK")),
            ("4000000", "CZK"),
        );
        property_valuation(&conn, "flat", today - 400 * DAY, "4000000", "CZK", 1);
        property(&conn, "plot", "investment", None, ("100", "CZK"));
        property_valuation(&conn, "plot", today - 100 * DAY, "100", "CZK", 1);
        // bought but never estimated: no value line, and a market price of 0 today
        property(
            &conn,
            "bare",
            "investment",
            Some((today - 60 * DAY, "9000000", "CZK")),
            ("0", "CZK"),
        );

        // other assets: bought 3, sold 1 (quantity 2), valuations mirrored into the price
        asset(&conn, "gold", "CZK", "2", "70");
        asset_tx(&conn, "gold", "buy", "3", today - 200 * DAY);
        asset_tx(&conn, "gold", "sell", "1", today - 50 * DAY);
        asset_valuation(&conn, "gold", today - 200 * DAY, "50", "CZK", 1);
        asset_valuation(&conn, "gold", today - 30 * DAY, "70", "CZK", 2);

        bond(&conn, "bond", "1000", "2", "CZK");

        let live = compute_breakdowns(&conn).expect("live breakdowns");
        let values = values_on(&conn, today);
        approx(&values.breakdowns.savings, &live.savings);
        approx(&values.breakdowns.loans, &live.loans);
        approx(
            &values.breakdowns.real_estate_personal,
            &live.real_estate_personal,
        );
        approx(
            &values.breakdowns.real_estate_investment,
            &live.real_estate_investment,
        );
        approx(&values.breakdowns.other_assets, &live.other_assets);
        approx(&values.breakdowns.bonds, &live.bonds);
        // the fixture is not vacuous
        assert_eq!(live.savings, map(&[("CZK", 1500.0), ("EUR", 200.0)]));
        assert!(live.loans["CZK"] > 0.0 && live.loans["EUR"] > 0.0);
        assert_eq!(live.other_assets, map(&[("CZK", 140.0)]));
    }
}
