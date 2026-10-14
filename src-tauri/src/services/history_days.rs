//! History days derived from dated facts (spec
//! `docs/specs/2026-10-13-history-from-dated-facts-design.md`, A1-A4 and A8).
//!
//! The value of a class on a past day is computed from the facts dated on or before it,
//! with the rule the domain's own page uses (P1, P3). Nothing is copied from another day
//! when a dated fact exists.
//!
//! * [`Facts`] reads every fact once (accounts and their balance records, properties and
//!   their valuations, loan ledgers, other assets, bonds, the ticker series, and what the
//!   stored live rows hold beyond the facts). Its [`Facts::day_values`] then walks the days
//!   with per-item cursors, so a rebuild costs no query per day.
//! * What the facts do not date (an account without balance records, a bond without a purchase
//!   date, an other asset with a quantity and no transactions) is not copied from today: a
//!   backfill day takes it
//!   from the nearest live row that counts the same items, as the remainder of that row over the
//!   facts derived for its day ([`LiveRemainder`], [`nearest_live`], A3), so a class does not
//!   jump between rebuilt days and live ones.
//! * Loans are the exception: they are computed values, so the ledger walk is their value on
//!   every day, a live row's recorded loans included, and they take no remainder.
//! * [`earliest_fact_day`] is the first day any fact is dated (A2): history starts there.
//! * [`rebuild_days`] writes one row for every day of a range (A3, A4), in chunks of
//!   [`CHUNK_DAYS`] days, one transaction each. [`plan_rebuild`], [`prune_before_first_fact`]
//!   and [`rebuild_chunk`] are its steps, for a caller that yields between chunks
//!   (`history_recalc`).
//!
//! Every write goes through `services/portfolio_history.rs`, the single writer of
//! `portfolio_metrics_history`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::Connection;

use crate::error::Result;
use crate::services::currency::breakdown_from_json;
use crate::services::history_recalc::{chunk_ranges, CHUNK_DAYS};
use crate::services::loan_amortization::{day_floor, loan_balance_at, LoanLedger};
use crate::services::loans;
use crate::services::parsing::parse_money;
use crate::services::portfolio_history::{
    day_sources, delete_backfill_before, investment_real_estate, load_ticker_series,
    update_class_set, upsert_snapshot, ClassBreakdowns, ClassSet, SnapshotSource, TickerSeries,
    SECONDS_PER_DAY,
};
use crate::services::ticker_history::RebuildStats;
use crate::services::valuations::OTHER_ASSET_PRICES;

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
    /// All eight breakdowns. What the facts do not cover of a class is the remainder of the
    /// nearest live row (A3), else the undated items at their current value; an item that is
    /// not counted (an account before its first record) adds nothing of its own.
    pub breakdowns: ClassBreakdowns,
    /// The classes whose every item the facts cover on the day (A1, "known"). A live row
    /// is rewritten for these only (D1). Stocks, crypto and loans are always known (a loan is
    /// the ledger walk on every day); bonds are once every bond has a purchase date.
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

    /// Put the cursor back before the first row.
    fn rewind(&mut self) {
        self.taken = 0;
    }

    /// The newest row passed so far (the last of its day, in the order the rows came in).
    fn latest(&self) -> Option<&T> {
        self.taken.checked_sub(1).map(|index| &self.rows[index].1)
    }

    /// The newest row passed so far with its day.
    fn latest_row(&self) -> Option<&(i64, T)> {
        self.taken.checked_sub(1).map(|index| &self.rows[index])
    }

    /// The first row the cursor has not passed: dated after the day it was advanced to.
    fn next(&self) -> Option<&(i64, T)> {
        self.rows.get(self.taken)
    }

    fn first(&self) -> Option<&(i64, T)> {
        self.rows.first()
    }

    /// No row at all, whatever the cursor passed.
    fn is_empty(&self) -> bool {
        self.rows.is_empty()
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
    /// The first day the property is counted: its purchase date, else its first valuation;
    /// `None` without any valuation (never counted).
    counted_from: Option<i64>,
    purchase_date: Option<i64>,
    purchase_price: f64,
    purchase_currency: String,
    /// One valuation per day: of several on one day, the last created.
    valuations: Steps<Amount>,
}

impl Property {
    /// The value on `day` (which is on or after the purchase date), in the currency of the
    /// point it comes from; `None` while the property is not counted: before its first
    /// valuation, without a purchase date to open the line.
    ///
    /// The line has the purchase (when it precedes the first valuation: on that day the
    /// valuation replaces it) and every valuation as points, and runs straight between two
    /// consecutive points of one currency. Two currencies cannot be drawn between: the earlier
    /// value holds until the next point. After the last point its value holds. The cursor of
    /// `valuations` has passed the points on or before `day`.
    fn value_on(&self, day: i64) -> Option<(f64, &str)> {
        let first_day = self.valuations.first()?.0;
        let opening = self
            .purchase_date
            .filter(|bought| *bought < first_day)
            .map(|bought| (bought, self.purchase_price, self.purchase_currency.as_str()));
        let (from_day, from_value, currency) = match self.valuations.latest_row() {
            Some((valued_at, valuation)) => {
                (*valued_at, valuation.value, valuation.currency.as_str())
            }
            None => opening?,
        };
        let value = match self.valuations.next() {
            Some((to_day, next)) if next.currency == currency && *to_day > from_day => {
                from_value
                    + (next.value - from_value) * (day - from_day) as f64
                        / (*to_day - from_day) as f64
            }
            _ => from_value,
        };
        Some((value, currency))
    }
}

/// A loan: its ledger walk is its value on every day (0 before the start, the estimate until
/// the first manual balance record, then the manual records hold). Computed, never a user fact
/// of its own: no live row pins it and no remainder is added to it.
struct Loan {
    currency: String,
    ledger: LoanLedger,
}

/// A bond: its face value (`coupon_value × quantity`) counts from the day it was bought.
struct BondHolding {
    currency: String,
    value: f64,
    /// The UTC day it was bought; `None` for a bond without a purchase date, which is carried
    /// on every day (undated).
    purchase_date: Option<i64>,
}

struct OtherAsset {
    /// Today's price per unit (`valuations::OTHER_ASSET_PRICES`): the newest valuation in its
    /// currency, else the market price in the asset's.
    price: Amount,
    /// The stored quantity: what an asset without transactions carries.
    stored_quantity: f64,
    /// Signed quantity changes of the transactions.
    changes: Steps<f64>,
    /// The quantity the transactions passed so far leave.
    quantity: f64,
    valuations: Steps<Amount>,
    /// The first day the transactions leave a quantity: from then on the asset is counted
    /// (`None` without transactions: undated).
    counted_from: Option<i64>,
}

/// One class on one day, split by where its amounts come from.
#[derive(Debug, Clone, Default)]
struct ClassDay {
    /// What the dated facts give: the items they cover.
    derived: Breakdown,
    /// Items counted on the day that have no date at all: the current balance of an account
    /// without records, an other asset with a quantity and no transactions, a bond without a
    /// purchase date.
    carried: Breakdown,
    /// Every item of the class is covered by the facts (A1, "known"); `carried` is empty then.
    covered: bool,
    /// The latest first day among the derived items counted on the day: a live row holds all of
    /// them only when it is dated on or after it (see [`nearest_live`]). `None` without one.
    counted_since: Option<i64>,
}

/// Real estate of one day, split by the property's current type. No item of it is carried:
/// a property is either counted from a fact or not counted.
#[derive(Debug, Clone, Default)]
struct RealEstateDay {
    personal: Breakdown,
    investment: Breakdown,
    covered: bool,
    /// As [`ClassDay::counted_since`], for both property types together.
    counted_since: Option<i64>,
}

/// Every class that has no ticker series, on one day.
struct StaticDay {
    savings: ClassDay,
    /// Always the ledger walk alone: loans are never carried and take no remainder.
    loans: Breakdown,
    real_estate: RealEstateDay,
    other_assets: ClassDay,
    bonds: ClassDay,
}

/// What a live row holds beyond what the facts derive for its day: the "unknown remainder" R
/// of A3, per class and per currency, `recorded - derived`. A backfill day takes it from the
/// nearest live row, so the part of a class the facts do not date follows what was recorded
/// instead of jumping between today's undated values and the recorded ones.
///
/// Empty for a class the facts cover completely on the live day: such a live row is rewritten
/// to the derived values (D1), so nothing of it is unexplained. Reading the remainder from the
/// derived values rather than from the stored row also keeps a rebuild idempotent: the
/// rewrite does not change what the next one reads.
///
/// Loans have no remainder: the ledger walk is their value on every day, a live row's
/// recorded loan total included (it came from an older valuation model and is no user fact).
///
/// A negative remainder (the facts derive more than was recorded) is kept for savings only,
/// where an overdraft is a legitimate negative amount; the other classes cannot be negative,
/// so a negative figure there is a stale record and counts as 0. A remainder below half a
/// cent is float residue and is dropped, a zero entry with it.
#[derive(Debug, Clone, Default)]
struct LiveRemainder {
    day: i64,
    savings: Breakdown,
    bonds: Breakdown,
    personal: Breakdown,
    investment: Breakdown,
    other_assets: Breakdown,
}

/// Below this a remainder is float residue, not money.
const REMAINDER_EPSILON: f64 = 0.005;

/// `recorded - derived` per currency, see [`LiveRemainder`].
fn remainder(recorded: &Breakdown, derived: &Breakdown, allow_negative: bool) -> Breakdown {
    let currencies: BTreeSet<&String> = recorded.keys().chain(derived.keys()).collect();
    let mut rest = Breakdown::new();
    for currency in currencies {
        let amount = recorded.get(currency).copied().unwrap_or(0.0)
            - derived.get(currency).copied().unwrap_or(0.0);
        if amount.abs() < REMAINDER_EPSILON || (amount < 0.0 && !allow_negative) {
            continue;
        }
        rest.insert(currency.clone(), amount);
    }
    rest
}

/// The remainder of a class on a live day: none when the facts cover the class there.
fn class_remainder(day: &ClassDay, recorded: &Breakdown, allow_negative: bool) -> Breakdown {
    if day.covered {
        Breakdown::new()
    } else {
        remainder(recorded, &day.derived, allow_negative)
    }
}

/// The live row a class of a backfill day takes its remainder from: one at which every item
/// the facts count on the day was counted too, so that the remainder never holds an item
/// again that the derived part already counts. An item is counted from its first day on (its
/// first record, valuation or purchase), so that means a live row dated on or after
/// `counted_since`, the latest such first day among the items counted on the day. The nearest
/// such row on or before the day wins, else the earliest later one (always dated after the
/// day, hence after every first day; today's row, the latest, serves when it is the only one).
/// `None` when no row qualifies: the undated items then keep their current value. `live` is
/// sorted by day.
fn nearest_live(
    live: &[LiveRemainder],
    day: i64,
    counted_since: Option<i64>,
) -> Option<&LiveRemainder> {
    let later = live.partition_point(|row| row.day <= day);
    later
        .checked_sub(1)
        .and_then(|index| live.get(index))
        .filter(|row| counted_since.is_none_or(|since| row.day >= since))
        .or_else(|| live.get(later))
}

/// `latest` moved up to `first` when that is later.
fn latest_first_day(latest: Option<i64>, first: Option<i64>) -> Option<i64> {
    match (latest, first) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// A class on a day: what the facts derive, plus what they do not cover. An uncovered class
/// takes the remainder of the nearest live row (`remainder`), or, with no live row at all,
/// the undated items at their current value (`carried`). A covered class is the derived
/// amounts alone.
fn completed(
    derived: &Breakdown,
    carried: &Breakdown,
    uncovered: bool,
    remainder: Option<&Breakdown>,
) -> Breakdown {
    let mut value = derived.clone();
    let rest = match remainder {
        Some(rest) if uncovered => rest,
        _ => carried,
    };
    for (currency, amount) in rest {
        add(&mut value, currency, *amount);
    }
    without_zeros(value)
}

/// The facts of every class that has no ticker series, with their forward cursors.
struct Statics {
    accounts: Vec<Account>,
    properties: Vec<Property>,
    loans: Vec<Loan>,
    assets: Vec<OtherAsset>,
    /// Every bond: a purchase date dates it, without one it is carried at today's value.
    bonds: Vec<BondHolding>,
}

impl Statics {
    fn load(conn: &Connection) -> Result<Self> {
        Ok(Self {
            accounts: load_accounts(conn)?,
            properties: load_properties(conn)?,
            loans: load_loans(conn)?,
            assets: load_other_assets(conn)?,
            bonds: load_bonds(conn)?,
        })
    }

    /// Put every cursor back before the first day, to walk the days again from the start.
    fn rewind(&mut self) {
        for account in &mut self.accounts {
            account.records.rewind();
        }
        for property in &mut self.properties {
            property.valuations.rewind();
        }
        for asset in &mut self.assets {
            asset.changes.rewind();
            asset.valuations.rewind();
            asset.quantity = 0.0;
        }
    }

    /// Every static class on `day` (spec A1). The cursors only move forward, so the days of
    /// one walk must not decrease.
    fn on(&mut self, day: i64) -> StaticDay {
        StaticDay {
            savings: self.savings_on(day),
            loans: self.loans_on(day),
            real_estate: self.real_estate_on(day),
            other_assets: self.other_assets_on(day),
            bonds: self.bonds_on(day),
        }
    }

    /// Bonds: a bond with a purchase date is worth its face value from that day on and nothing
    /// before it (a fact in both cases). A bond without one is carried at today's value on every
    /// day and is not a fact, so the class is uncovered while there is one. A dated bond counts
    /// from its purchase day, which is therefore the "counted since" day of the remainder rule.
    fn bonds_on(&self, day: i64) -> ClassDay {
        let mut bonds = ClassDay {
            covered: true,
            ..ClassDay::default()
        };
        for bond in &self.bonds {
            match bond.purchase_date {
                None => {
                    add(&mut bonds.carried, &bond.currency, bond.value);
                    bonds.covered = false;
                }
                Some(bought) if day >= bought => {
                    add(&mut bonds.derived, &bond.currency, bond.value);
                    bonds.counted_since = latest_first_day(bonds.counted_since, Some(bought));
                }
                Some(_) => {}
            }
        }
        bonds
    }

    /// Savings: the newest balance record on or before the day, in its own currency. An
    /// account is not counted before its first record (D4); an account without any record
    /// carries its current balance. Both are uncovered. Accounts excluded from the balance
    /// are never loaded.
    fn savings_on(&mut self, day: i64) -> ClassDay {
        let mut savings = ClassDay {
            covered: true,
            ..ClassDay::default()
        };
        for account in &mut self.accounts {
            account.records.advance(day);
            let first_record = account.records.first().map(|(first, _)| *first);
            match account.records.latest() {
                Some(record) => {
                    add(&mut savings.derived, &record.currency, record.value);
                    savings.counted_since = latest_first_day(savings.counted_since, first_record);
                }
                None if account.records.first().is_none() => {
                    add(
                        &mut savings.carried,
                        &account.currency,
                        account.current_balance,
                    );
                    savings.covered = false;
                }
                None => savings.covered = false,
            }
        }
        savings
    }

    /// Loans: the ledger walk of the day (0 before the start; the estimate until the first
    /// manual balance record, which then holds). Every day is derived from the ledger, a past
    /// live day included: only a manual balance is a user fact and it is in the ledger already.
    fn loans_on(&self, day: i64) -> Breakdown {
        let mut loans = Breakdown::new();
        for loan in &self.loans {
            add(
                &mut loans,
                &loan.currency,
                loan_balance_at(&loan.ledger, day).balance,
            );
        }
        loans
    }

    /// Real estate, split by the property's current type. A property without any valuation is
    /// not counted on any day. Before the purchase date a property is worth 0. From there its
    /// value is the line through the purchase (when it precedes the first valuation) and the
    /// valuations, straight between two points of one currency ([`Property::value_on`]); of
    /// several valuations on one day the last entered counts. A property without a purchase date
    /// counts nothing before its first valuation, and that day is uncovered.
    fn real_estate_on(&mut self, day: i64) -> RealEstateDay {
        let mut real_estate = RealEstateDay {
            covered: true,
            ..RealEstateDay::default()
        };
        for property in &mut self.properties {
            property.valuations.advance(day);
            // No estimate at all, no value line (`valuationTrace`; today's market price is 0
            // too): the purchase price opens the line only once an estimate exists.
            if property.valuations.first().is_none()
                || property.purchase_date.is_some_and(|bought| day < bought)
            {
                continue;
            }
            let Some((value, currency)) = property.value_on(day) else {
                real_estate.covered = false;
                continue;
            };
            let target = if property.personal {
                &mut real_estate.personal
            } else {
                &mut real_estate.investment
            };
            add(target, currency, value);
            real_estate.counted_since =
                latest_first_day(real_estate.counted_since, property.counted_from);
        }
        real_estate
    }

    /// Other assets: the quantity the transactions leave on the day times the valuation of
    /// the day (`valuations::value_at_day`: the newest valuation on or before it, else the
    /// earliest, else the market price), in the valuation's currency. An asset with no
    /// transactions but a stored quantity (an MCP-created one) has nothing dated: like an
    /// undated account it carries today's quantity times today's price in that price's
    /// currency (`net_worth::compute_breakdowns`) and is not a fact.
    fn other_assets_on(&mut self, day: i64) -> ClassDay {
        let mut assets = ClassDay {
            covered: true,
            ..ClassDay::default()
        };
        for asset in &mut self.assets {
            if asset.changes.is_empty() {
                if asset.stored_quantity > QUANTITY_EPSILON {
                    add(
                        &mut assets.carried,
                        &asset.price.currency,
                        asset.stored_quantity * asset.price.value,
                    );
                    assets.covered = false;
                }
                continue;
            }
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
                        &mut assets.derived,
                        &valuation.currency,
                        asset.quantity * valuation.value,
                    );
                }
                None => add(
                    &mut assets.derived,
                    &asset.price.currency,
                    asset.quantity * asset.price.value,
                ),
            }
            assets.counted_since = latest_first_day(assets.counted_since, asset.counted_from);
        }
        assets
    }
}

/// One stored live row, its classes as native breakdowns.
struct LiveRow {
    day: i64,
    savings: Breakdown,
    bonds: Breakdown,
    /// Personal and investment property together, as stored (I2).
    real_estate: Breakdown,
    real_estate_personal: Breakdown,
    other_assets: Breakdown,
}

/// The stored live rows, one per day (of several the newest), oldest first.
fn load_live_rows(conn: &Connection) -> Result<Vec<LiveRow>> {
    let mut stmt = conn.prepare(
        "SELECT recorded_at, savings_by_currency, bonds_by_currency, real_estate_by_currency,
                real_estate_personal_by_currency, other_assets_by_currency
         FROM portfolio_metrics_history WHERE source = 'live' ORDER BY recorded_at",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(LiveRow {
                day: day_floor(row.get::<_, i64>(0)?),
                savings: breakdown_from_json(&row.get::<_, String>(1)?),
                bonds: breakdown_from_json(&row.get::<_, String>(2)?),
                real_estate: breakdown_from_json(&row.get::<_, String>(3)?),
                real_estate_personal: breakdown_from_json(&row.get::<_, String>(4)?),
                other_assets: breakdown_from_json(&row.get::<_, String>(5)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut live: Vec<LiveRow> = Vec::with_capacity(rows.len());
    for row in rows {
        if live.last().is_some_and(|last| last.day == row.day) {
            live.pop();
        }
        live.push(row);
    }
    Ok(live)
}

/// The remainder of every live row (A3): what the row holds beyond the facts dated on or
/// before its day, found with a walk of the static facts over the live days. Loans are no
/// part of it (always derived).
fn load_live_remainders(conn: &Connection, statics: &mut Statics) -> Result<Vec<LiveRemainder>> {
    let rows = load_live_rows(conn)?;
    let mut remainders = Vec::with_capacity(rows.len());
    for row in rows {
        let day = statics.on(row.day);
        let real_estate = &day.real_estate;
        let (personal, investment) = if real_estate.covered {
            (Breakdown::new(), Breakdown::new())
        } else {
            let recorded_investment =
                investment_real_estate(&row.real_estate, &row.real_estate_personal);
            (
                remainder(&row.real_estate_personal, &real_estate.personal, false),
                remainder(&recorded_investment, &real_estate.investment, false),
            )
        };
        remainders.push(LiveRemainder {
            day: row.day,
            savings: class_remainder(&day.savings, &row.savings, true),
            bonds: class_remainder(&day.bonds, &row.bonds, false),
            personal,
            investment,
            other_assets: class_remainder(&day.other_assets, &row.other_assets, false),
        });
    }
    Ok(remainders)
}

/// Every fact the history of a profile is derived from, read once.
pub struct Facts {
    statics: Statics,
    stocks: BTreeMap<String, TickerSeries>,
    crypto: BTreeMap<String, TickerSeries>,
    /// What the stored live rows hold beyond the facts, oldest first (see [`LiveRemainder`]).
    live: Vec<LiveRemainder>,
    /// The last day valued: the cursors only move forward.
    last_day: Option<i64>,
}

impl Facts {
    /// Read the facts of every class. `until_exclusive` bounds the ticker series: the days
    /// to value are before it (a rebuild passes today's start).
    pub fn load(conn: &Connection, until_exclusive: i64) -> Result<Self> {
        let mut statics = Statics::load(conn)?;
        // A second walk of the static facts, over the live days, before the days to value
        let live = load_live_remainders(conn, &mut statics)?;
        statics.rewind();
        Ok(Self {
            statics,
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
            live,
            last_day: None,
        })
    }

    /// The value of every class on `day` (spec A1, A3). The cursors only move forward, so the
    /// days of one `Facts` must not decrease.
    ///
    /// A class the facts cover completely is what they derive. Otherwise the part the facts
    /// do not date is the remainder of the nearest live row (the nearest one on or before the
    /// day, else the earliest later one), and the current value of the undated items when
    /// there is no live row at all. Live rows themselves are rewritten for the covered classes
    /// only (D1), which the remainder never touches.
    pub fn day_values(&mut self, day: i64) -> DayValues {
        debug_assert!(
            self.last_day.is_none_or(|last| day >= last),
            "history days must be valued in ascending order"
        );
        self.last_day = Some(day);

        let statics = self.statics.on(day);
        // Each class takes its remainder from its own live row: one that counts the same items
        let live = &self.live;
        let rest = |since: Option<i64>, class: fn(&LiveRemainder) -> &Breakdown| {
            nearest_live(live, day, since).map(class)
        };
        let real_estate = &statics.real_estate;
        DayValues {
            breakdowns: ClassBreakdowns {
                savings: completed(
                    &statics.savings.derived,
                    &statics.savings.carried,
                    !statics.savings.covered,
                    rest(statics.savings.counted_since, |row| &row.savings),
                ),
                investments: without_zeros(ticker_value(&mut self.stocks, day)),
                crypto: without_zeros(ticker_value(&mut self.crypto, day)),
                bonds: completed(
                    &statics.bonds.derived,
                    &statics.bonds.carried,
                    !statics.bonds.covered,
                    rest(statics.bonds.counted_since, |row| &row.bonds),
                ),
                real_estate_personal: completed(
                    &real_estate.personal,
                    &Breakdown::new(),
                    !real_estate.covered,
                    rest(real_estate.counted_since, |row| &row.personal),
                ),
                real_estate_investment: completed(
                    &real_estate.investment,
                    &Breakdown::new(),
                    !real_estate.covered,
                    rest(real_estate.counted_since, |row| &row.investment),
                ),
                loans: without_zeros(statics.loans),
                other_assets: completed(
                    &statics.other_assets.derived,
                    &statics.other_assets.carried,
                    !statics.other_assets.covered,
                    rest(statics.other_assets.counted_since, |row| &row.other_assets),
                ),
            },
            known: ClassSet {
                savings: statics.savings.covered,
                investments: true,
                crypto: true,
                bonds: statics.bonds.covered,
                real_estate: real_estate.covered,
                loans: true,
                other_assets: statics.other_assets.covered,
            },
        }
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

/// `rows` (oldest first, a day's rows in the order they were entered) with one row per day:
/// the last of it.
fn last_of_each_day<T>(rows: Vec<(i64, T)>) -> Vec<(i64, T)> {
    let mut days: Vec<(i64, T)> = Vec::with_capacity(rows.len());
    for row in rows {
        match days.last_mut() {
            Some(last) if last.0 == row.0 => *last = row,
            _ => days.push(row),
        }
    }
    days
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
            |(id, kind, purchase_price, purchase_currency, purchase_date)| {
                let valuations =
                    Steps::new(last_of_each_day(valuations.remove(&id).unwrap_or_default()));
                let purchase_date = purchase_date.map(day_floor);
                Property {
                    personal: kind == "personal",
                    // A property without a valuation is never counted
                    counted_from: valuations
                        .first()
                        .map(|(first, _)| purchase_date.unwrap_or(*first)),
                    purchase_date,
                    purchase_price: parse_money(&purchase_price, 0.0, "real_estate.purchase_price"),
                    purchase_currency,
                    valuations,
                }
            },
        )
        .collect())
}

fn load_loans(conn: &Connection) -> Result<Vec<Loan>> {
    Ok(loans::load_ledgers(conn)?
        .into_iter()
        .map(|entry| Loan {
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

    let mut stmt = conn.prepare(&format!(
        "SELECT a.id, p.currency, p.price, a.quantity
         FROM other_assets a JOIN ({OTHER_ASSET_PRICES}) p ON p.asset_id = a.id
         ORDER BY a.id"
    ))?;
    let assets = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(assets
        .into_iter()
        .map(|(id, currency, price, quantity)| {
            let changes = changes.remove(&id).unwrap_or_default();
            OtherAsset {
                price: Amount {
                    value: parse_money(&price, 0.0, "other_assets.market_price"),
                    currency,
                },
                stored_quantity: parse_money(&quantity, 0.0, "other_assets.quantity"),
                counted_from: first_held_day(&changes),
                changes: Steps::new(changes),
                quantity: 0.0,
                valuations: Steps::new(valuations.remove(&id).unwrap_or_default()),
            }
        })
        .collect())
}

/// The first day the signed quantity changes (oldest first) leave a quantity, all of the day's
/// changes applied (the rule of `Statics::other_assets_on`); `None` when they never do.
fn first_held_day(changes: &[(i64, f64)]) -> Option<i64> {
    let mut held = 0.0;
    for (index, (day, change)) in changes.iter().enumerate() {
        held += change;
        let last_of_day = changes.get(index + 1).is_none_or(|(next, _)| next != day);
        if last_of_day && held > QUANTITY_EPSILON {
            return Some(*day);
        }
    }
    None
}

fn load_bonds(conn: &Connection) -> Result<Vec<BondHolding>> {
    let mut stmt =
        conn.prepare("SELECT coupon_value, quantity, currency, purchase_date FROM bonds")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .map(|(value, quantity, currency, purchase_date)| {
            let value = parse_money(&value, 0.0, "bonds.coupon_value");
            let quantity = parse_money(&quantity, 1.0, "bonds.quantity");
            BondHolding {
                currency,
                value: value * quantity,
                purchase_date: purchase_date.map(day_floor),
            }
        })
        .collect())
}

/// The first day any fact is dated (A2), the start of every history: the first transaction
/// of stocks, crypto and other assets, the first balance record of an account that counts, each
/// property's purchase date (else its first valuation), each loan's start and each bond's
/// purchase date. Never before 2000-01-01. Bonds without a purchase date, accounts without
/// records and other assets without transactions have no date and do not count. `None` without
/// a dated fact.
pub fn earliest_fact_day(conn: &Connection) -> Result<Option<i64>> {
    const SOURCES: [&str; 7] = [
        "SELECT MIN(transaction_date) FROM investment_transactions",
        "SELECT MIN(transaction_date) FROM crypto_transactions",
        "SELECT MIN(transaction_date) FROM other_asset_transactions",
        // The history never counts an account outside the balance (`load_accounts`)
        "SELECT MIN(as_of) FROM bank_balance_records
         WHERE bank_account_id IN (SELECT id FROM bank_accounts WHERE exclude_from_balance = 0)",
        "SELECT MIN(COALESCE(purchase_date,
                (SELECT MIN(valued_at) FROM real_estate_valuations
                 WHERE real_estate_id = real_estate.id)))
         FROM real_estate",
        "SELECT MIN(start_date) FROM loans",
        "SELECT MIN(purchase_date) FROM bonds",
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
                currency TEXT NOT NULL DEFAULT 'CZK',
                purchase_date INTEGER
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

    /// A bond bought on `day` (a UTC midnight).
    fn dated_bond(
        conn: &Connection,
        id: &str,
        coupon: &str,
        quantity: &str,
        currency: &str,
        day: i64,
    ) {
        conn.execute(
            "INSERT INTO bonds (id, coupon_value, quantity, currency, purchase_date)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, coupon, quantity, currency, day],
        )
        .expect("dated bond");
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
    fn a_manual_balance_holds_and_the_estimated_past_before_it_is_derived_too() {
        let conn = setup_test_db();
        yearly_loan(&conn);
        loan_event(&conn, "l", "balance_check", iso("2024-06-20"), "4000", 1);

        // before the first manual record the loan follows the estimate (D5): a computed
        // value, re-derived from the ledger like every loan day, so the class is known
        let before = values_on(&conn, iso("2024-05-10"));
        assert_eq!(before.breakdowns.loans, map(&[("CZK", 8_000.0)]));
        assert!(before.known.loans);

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
    fn a_purchase_price_in_another_currency_holds_until_the_first_valuation() {
        // Two currencies cannot be drawn between: the purchase price holds, in its own
        // currency, until the valuation replaces it
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

    /// The real estate value (investment) of `day`, as `(currency, amount)` pairs.
    fn invested_on(conn: &Connection, day: i64) -> Breakdown {
        values_on(conn, day).breakdowns.real_estate_investment
    }

    #[test]
    fn the_value_runs_in_a_straight_line_from_the_purchase_to_the_first_valuation() {
        let conn = setup_test_db();
        property(
            &conn,
            "p",
            "investment",
            Some((d(10), "3000", "CZK")),
            ("4000", "CZK"),
        );
        property_valuation(&conn, "p", d(20), "4000", "CZK", 1);

        assert!(invested_on(&conn, d(9)).is_empty(), "not owned yet");
        approx(&invested_on(&conn, d(10)), &map(&[("CZK", 3000.0)]));
        approx(&invested_on(&conn, d(15)), &map(&[("CZK", 3500.0)]));
        approx(&invested_on(&conn, d(19)), &map(&[("CZK", 3900.0)]));
        approx(&invested_on(&conn, d(20)), &map(&[("CZK", 4000.0)]));
        approx(&invested_on(&conn, d(90)), &map(&[("CZK", 4000.0)]));
        assert!(values_on(&conn, d(15)).known.real_estate);
    }

    #[test]
    fn the_value_runs_in_a_straight_line_between_two_valuations_and_holds_after_the_last() {
        let conn = setup_test_db();
        property(&conn, "p", "investment", None, ("300", "CZK"));
        property_valuation(&conn, "p", d(10), "100", "CZK", 1);
        property_valuation(&conn, "p", d(30), "300", "CZK", 2);
        property_valuation(&conn, "p", d(40), "250", "CZK", 3);

        approx(&invested_on(&conn, d(10)), &map(&[("CZK", 100.0)]));
        approx(&invested_on(&conn, d(20)), &map(&[("CZK", 200.0)]));
        approx(&invested_on(&conn, d(29)), &map(&[("CZK", 290.0)]));
        approx(&invested_on(&conn, d(30)), &map(&[("CZK", 300.0)]));
        approx(&invested_on(&conn, d(35)), &map(&[("CZK", 275.0)]));
        approx(&invested_on(&conn, d(40)), &map(&[("CZK", 250.0)]));
        approx(&invested_on(&conn, d(90)), &map(&[("CZK", 250.0)]));
    }

    #[test]
    fn two_valuations_in_different_currencies_are_a_step_not_a_slope() {
        let conn = setup_test_db();
        property(&conn, "p", "investment", None, ("300", "EUR"));
        property_valuation(&conn, "p", d(10), "100", "CZK", 1);
        property_valuation(&conn, "p", d(30), "300", "EUR", 2);

        approx(&invested_on(&conn, d(10)), &map(&[("CZK", 100.0)]));
        approx(&invested_on(&conn, d(20)), &map(&[("CZK", 100.0)]));
        approx(&invested_on(&conn, d(29)), &map(&[("CZK", 100.0)]));
        approx(&invested_on(&conn, d(30)), &map(&[("EUR", 300.0)]));
        approx(&invested_on(&conn, d(60)), &map(&[("EUR", 300.0)]));
    }

    #[test]
    fn a_purchase_after_the_first_valuation_is_no_point_of_the_line() {
        let conn = setup_test_db();
        // valued on day 10, but only owned from day 15: the purchase price (90) is ignored
        // as a point, the valuations draw the line, and nothing counts before the purchase
        property(
            &conn,
            "p",
            "investment",
            Some((d(15), "90", "CZK")),
            ("200", "CZK"),
        );
        property_valuation(&conn, "p", d(10), "100", "CZK", 1);
        property_valuation(&conn, "p", d(20), "200", "CZK", 2);

        assert!(invested_on(&conn, d(12)).is_empty());
        assert!(invested_on(&conn, d(14)).is_empty());
        approx(&invested_on(&conn, d(15)), &map(&[("CZK", 150.0)]));
        approx(&invested_on(&conn, d(18)), &map(&[("CZK", 180.0)]));
        approx(&invested_on(&conn, d(20)), &map(&[("CZK", 200.0)]));
    }

    #[test]
    fn a_purchase_on_the_day_of_the_first_valuation_is_replaced_by_it() {
        let conn = setup_test_db();
        property(
            &conn,
            "p",
            "investment",
            Some((d(10), "3000", "CZK")),
            ("4500", "CZK"),
        );
        property_valuation(&conn, "p", d(10), "3500", "CZK", 1);
        property_valuation(&conn, "p", d(20), "4500", "CZK", 2);

        assert!(invested_on(&conn, d(9)).is_empty());
        approx(&invested_on(&conn, d(10)), &map(&[("CZK", 3500.0)]));
        approx(&invested_on(&conn, d(15)), &map(&[("CZK", 4000.0)]));
    }

    #[test]
    fn of_several_valuations_on_one_day_the_last_created_draws_the_line() {
        let conn = setup_test_db();
        property(&conn, "p", "investment", None, ("0", "CZK"));
        property_valuation(&conn, "p", d(10), "100", "CZK", 1);
        property_valuation(&conn, "p", d(30), "300", "CZK", 5);
        property_valuation(&conn, "p", d(30), "350", "CZK", 9);
        property_valuation(&conn, "p", d(30), "320", "CZK", 7);
        property_valuation(&conn, "p", d(10), "150", "CZK", 2);

        // day 10 was last entered at 150, day 30 at 350
        approx(&invested_on(&conn, d(10)), &map(&[("CZK", 150.0)]));
        approx(&invested_on(&conn, d(20)), &map(&[("CZK", 250.0)]));
        approx(&invested_on(&conn, d(30)), &map(&[("CZK", 350.0)]));
        approx(&invested_on(&conn, d(90)), &map(&[("CZK", 350.0)]));
    }

    #[test]
    fn a_rebuild_walks_the_line_across_its_chunks() {
        let conn = setup_test_db();
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        property(&conn, "p", "investment", None, ("1900", "CZK"));
        property_valuation(&conn, "p", d(0), "1000", "CZK", 1);
        property_valuation(&conn, "p", d(90), "1900", "CZK", 2);
        // 150 days: three chunks of 60, the second valuation inside the second one
        rebuild_days(&conn, d(0), d(150)).expect("rebuild");

        let value = |day: i64| column(&conn, d(day), "real_estate_by_currency");
        approx(&value(0), &map(&[("CZK", 1000.0)]));
        approx(&value(59), &map(&[("CZK", 1590.0)]));
        approx(&value(60), &map(&[("CZK", 1600.0)]));
        approx(&value(89), &map(&[("CZK", 1890.0)]));
        approx(&value(90), &map(&[("CZK", 1900.0)]));
        approx(&value(149), &map(&[("CZK", 1900.0)]));
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

    #[test]
    fn an_other_asset_with_a_quantity_and_no_transactions_is_carried_as_unknown() {
        // An MCP-created asset: a quantity and a price, no dated transaction. Like an undated
        // account it carries today's quantity times today's price on every day.
        let conn = setup_test_db();
        asset(&conn, "mcp", "CZK", "3", "80");
        asset(&conn, "empty", "CZK", "0", "50");
        asset(&conn, "unvalued", "CZK", "1", "100");
        asset_valuation(&conn, "mcp", d(5), "80", "EUR", 1);

        for day in [-1000, 0, 7] {
            let values = values_on(&conn, d(day));
            assert_eq!(
                values.breakdowns.other_assets,
                map(&[("EUR", 240.0), ("CZK", 100.0)]),
                "today's quantity times today's price (the newest valuation in its currency, \
                 else the market price in the asset's), like today's breakdown: {day}"
            );
            assert!(!values.known.other_assets, "{day}");
        }
    }

    #[test]
    fn a_carried_other_asset_makes_the_class_unknown_next_to_dated_ones() {
        let conn = setup_test_db();
        asset(&conn, "dated", "CZK", "1", "10");
        asset_tx(&conn, "dated", "buy", "1", d(3));
        asset(&conn, "mcp", "EUR", "2", "5");

        let values = values_on(&conn, d(4));
        assert_eq!(
            values.breakdowns.other_assets,
            map(&[("CZK", 10.0), ("EUR", 10.0)])
        );
        assert!(!values.known.other_assets);
        // an asset that was bought and sold out is dated, not carried
        asset_tx(&conn, "dated", "sell", "1", d(6));
        assert_eq!(
            values_on(&conn, d(7)).breakdowns.other_assets,
            map(&[("EUR", 10.0)])
        );
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
    fn a_dated_bond_counts_nothing_before_its_purchase_day_and_its_value_from_it_on() {
        let conn = setup_test_db();
        dated_bond(&conn, "b", "1000", "3", "CZK", d(10));

        // before the purchase the bond is not counted, and that is a fact, not a gap
        for day in [-500, 0, 9] {
            let values = values_on(&conn, d(day));
            assert!(values.breakdowns.bonds.is_empty(), "day {day}");
            assert!(values.known.bonds, "day {day}");
        }
        for day in [10, 11, 400] {
            let values = values_on(&conn, d(day));
            assert_eq!(
                values.breakdowns.bonds,
                map(&[("CZK", 3000.0)]),
                "day {day}"
            );
            assert!(values.known.bonds, "day {day}");
        }
    }

    #[test]
    fn a_bond_bought_at_a_later_hour_counts_from_its_day() {
        let conn = setup_test_db();
        dated_bond(&conn, "b", "10", "1", "EUR", d(10) + 13 * 3_600);
        assert!(values_on(&conn, d(9)).breakdowns.bonds.is_empty());
        assert_eq!(
            values_on(&conn, d(10)).breakdowns.bonds,
            map(&[("EUR", 10.0)])
        );
    }

    #[test]
    fn an_undated_bond_next_to_a_dated_one_keeps_the_class_unknown_and_stays_carried() {
        let conn = setup_test_db();
        bond(&conn, "undated", "500", "1", "CZK");
        dated_bond(&conn, "dated", "1000", "1", "CZK", d(10));
        dated_bond(&conn, "foreign", "20", "2", "EUR", d(20));

        let early = values_on(&conn, d(5));
        assert_eq!(early.breakdowns.bonds, map(&[("CZK", 500.0)]));
        assert!(!early.known.bonds);
        let mid = values_on(&conn, d(15));
        assert_eq!(mid.breakdowns.bonds, map(&[("CZK", 1500.0)]));
        assert!(!mid.known.bonds);
        let late = values_on(&conn, d(25));
        assert_eq!(
            late.breakdowns.bonds,
            map(&[("CZK", 1500.0), ("EUR", 40.0)])
        );
        assert!(!late.known.bonds);
    }

    #[test]
    fn a_profile_without_bonds_has_nothing_to_carry_and_the_class_is_known() {
        let conn = setup_test_db();
        let values = values_on(&conn, d(5));
        assert!(values.breakdowns.bonds.is_empty());
        assert!(
            values.known.bonds,
            "no item is left that the facts do not cover"
        );
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
    fn an_undated_bond_does_not_define_the_start_but_a_dated_one_can() {
        let conn = setup_test_db();
        bond(&conn, "undated", "1", "1", "CZK");
        assert_eq!(earliest_fact_day(&conn).unwrap(), None);

        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(50), "1", "CZK");
        dated_bond(&conn, "dated", "1", "1", "CZK", d(20));
        assert_eq!(earliest_fact_day(&conn).unwrap(), Some(d(20)));
    }

    #[test]
    fn balance_records_of_an_account_outside_the_balance_are_not_a_fact() {
        // The history never counts such an account, so its records must not start it earlier.
        let conn = setup_test_db();
        account(&conn, "out", "CZK", "1", true);
        record(&conn, "out", d(-100), "1", "CZK");
        assert_eq!(earliest_fact_day(&conn).unwrap(), None);

        account(&conn, "in", "CZK", "1", false);
        record(&conn, "in", d(-10), "1", "CZK");
        assert_eq!(earliest_fact_day(&conn).unwrap(), Some(d(-10)));
    }

    #[test]
    fn every_dated_fact_can_be_the_earliest_day() {
        type Seed = fn(&Connection, i64);
        let sources: [(&str, Seed); 7] = [
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
            ("bond purchase date", |c, day| {
                dated_bond(c, "b", "1", "1", "CZK", day)
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

    /// A live row of `day` with only the given columns set to JSON breakdowns; the others are
    /// empty. It replaces any row the day already has (a rebuilt one).
    fn live_with(conn: &Connection, day: i64, columns: &[(&str, &str)]) {
        conn.execute(
            "DELETE FROM portfolio_metrics_history WHERE recorded_at >= ?1 AND recorded_at < ?2",
            [day, day + DAY],
        )
        .expect("clear the day");
        let names = columns
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        let marks = (0..columns.len())
            .map(|index| format!("?{}", index + 3))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values: Vec<rusqlite::types::Value> =
            vec![format!("live-{day}").into(), (day + 3_600).into()];
        values.extend(columns.iter().map(|(_, json)| (*json).to_string().into()));
        conn.execute(
            &format!(
                "INSERT INTO portfolio_metrics_history (id, recorded_at, source, {names})
                 VALUES (?1, ?2, 'live', {marks})"
            ),
            rusqlite::params_from_iter(values),
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
        // loans are always derived from their ledger: every live day is rewritten
        for day in [5, 15, 25] {
            assert_eq!(
                at(day, "loans_by_currency"),
                map(&[("CZK", 6000.0)]),
                "{day}"
            );
            // stocks and crypto always are (nothing is held: empty)
            assert!(at(day, "investments_by_currency").is_empty(), "{day}");
            assert!(at(day, "crypto_by_currency").is_empty(), "{day}");
            // an undated bond is not a fact: the recorded value stays
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
    fn a_live_row_is_rewritten_for_bonds_once_every_bond_is_dated() {
        let conn = setup_test_db();
        let today = d(30);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        dated_bond(&conn, "b", "1000", "1", "CZK", d(10));
        for day in [5, 15, 25] {
            live_row(&conn, d(day));
        }

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let bonds = |day: i64| column(&conn, d(day), "bonds_by_currency");
        // the recorded 333 of a live row is replaced by what the facts say on its day
        assert!(bonds(5).is_empty(), "before the purchase");
        assert_eq!(bonds(15), map(&[("CZK", 1000.0)]));
        assert_eq!(bonds(25), map(&[("CZK", 1000.0)]));
        // ... and so are the days between
        assert!(bonds(9).is_empty());
        assert_eq!(bonds(10), map(&[("CZK", 1000.0)]));

        // one undated bond makes the class unknown again: live rows set back to what they
        // recorded keep it
        bond(&conn, "undated", "50", "1", "CZK");
        for day in [5, 15, 25] {
            conn.execute(
                "DELETE FROM portfolio_metrics_history WHERE recorded_at >= ?1 AND recorded_at < ?2",
                [d(day), d(day) + DAY],
            )
            .expect("clear the day");
            live_row(&conn, d(day));
        }
        rebuild_days(&conn, d(0), today).expect("rebuild with an undated bond");
        for day in [5, 15, 25] {
            assert_eq!(bonds(day), map(&[("CZK", 333.0)]), "live day {day}");
        }
    }

    #[test]
    fn a_live_row_is_rewritten_when_the_last_bond_is_gone() {
        // no bond is left that the facts do not cover: a deleted bond leaves the live rows too
        let conn = setup_test_db();
        let today = d(20);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        live_with(&conn, d(10), &[("bonds_by_currency", r#"{"CZK":900}"#)]);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        for day in [0, 5, 10, 11, 19] {
            assert!(
                column(&conn, d(day), "bonds_by_currency").is_empty(),
                "day {day}"
            );
        }
    }

    #[test]
    fn a_live_row_keeps_its_other_assets_while_a_carried_asset_is_not_a_fact() {
        let conn = setup_test_db();
        let today = d(30);
        account(&conn, "a", "CZK", "100", false);
        record(&conn, "a", d(0), "100", "CZK");
        asset(&conn, "mcp", "CZK", "3", "100");
        live_row(&conn, d(10));

        rebuild_days(&conn, d(0), today).expect("rebuild");

        assert_eq!(
            column(&conn, d(10), "other_assets_by_currency"),
            map(&[("CZK", 888.0)]),
            "the recorded value stays on the live row"
        );
        for day in [0, 9, 11, 29] {
            assert_eq!(
                column(&conn, d(day), "other_assets_by_currency"),
                map(&[("CZK", 888.0)]),
                "a new row takes the undated asset from the nearest live row, day {day}"
            );
        }
    }

    #[test]
    fn a_new_row_takes_what_the_facts_do_not_date_from_the_nearest_live_row() {
        let conn = setup_test_db();
        let today = d(60);
        account(&conn, "a", "CZK", "300", false);
        record(&conn, "a", d(0), "100", "CZK");
        record(&conn, "a", d(20), "300", "CZK");
        // two accounts without any record: today's balance, which was not always theirs
        account(&conn, "u1", "CZK", "700", false);
        account(&conn, "u2", "EUR", "40", false);
        let live = |day: i64, savings: &str| {
            live_with(&conn, d(day), &[("savings_by_currency", savings)]);
        };
        live(10, r#"{"CZK":800,"EUR":40}"#);
        live(30, r#"{"CZK":950,"EUR":35}"#);
        live(50, r#"{"CZK":1000,"EUR":40}"#);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let savings = |day: i64| column(&conn, d(day), "savings_by_currency");
        // before the first live row: its remainder (800 - the dated 100 = 700 CZK, 40 EUR)
        // next to the dated account's own record
        assert_eq!(savings(0), map(&[("CZK", 800.0), ("EUR", 40.0)]));
        assert_eq!(savings(9), map(&[("CZK", 800.0), ("EUR", 40.0)]));
        // after a live row: the nearest earlier one, not the nearer later one, until the
        // next live row takes over. The day after live row 10 continues it without a jump
        assert_eq!(savings(11), savings(10), "no jump off the live row");
        assert_eq!(savings(11), map(&[("CZK", 800.0), ("EUR", 40.0)]));
        // the dated account's record of day 20 moves the dated part only
        assert_eq!(savings(25), map(&[("CZK", 1000.0), ("EUR", 40.0)]));
        assert_eq!(savings(29), map(&[("CZK", 1000.0), ("EUR", 40.0)]));
        // live row 30: 950 - the dated 300 = 650 CZK, 35 EUR
        assert_eq!(savings(31), map(&[("CZK", 950.0), ("EUR", 35.0)]));
        assert_eq!(savings(49), map(&[("CZK", 950.0), ("EUR", 35.0)]));
        assert_eq!(savings(51), map(&[("CZK", 1000.0), ("EUR", 40.0)]));
        assert_eq!(savings(59), map(&[("CZK", 1000.0), ("EUR", 40.0)]));
        // the live rows keep what was recorded: undated accounts are not a fact (D1)
        assert_eq!(
            savings(30),
            map(&[("CZK", 950.0), ("EUR", 35.0)]),
            "the live row"
        );

        // a second rebuild changes nothing
        let rows = dump(&conn);
        rebuild_days(&conn, d(0), today).expect("again");
        assert_eq!(dump(&conn), rows);
    }

    #[test]
    fn with_only_todays_live_row_the_remainder_comes_from_it_and_with_none_from_the_items() {
        let conn = setup_test_db();
        let today = d(40);
        account(&conn, "a", "CZK", "300", false);
        record(&conn, "a", d(0), "100", "CZK");
        record(&conn, "a", d(20), "300", "CZK");
        account(&conn, "u", "CZK", "700", false);

        // no live row at all: the undated account at its current balance
        rebuild_days(&conn, d(0), today).expect("no live row");
        let savings = |day: i64| column(&conn, d(day), "savings_by_currency");
        assert_eq!(savings(5), map(&[("CZK", 800.0)]));
        assert_eq!(savings(25), map(&[("CZK", 1000.0)]));

        // today's row is the only live row: it serves every past day (999 - the dated 300)
        live_with(&conn, today, &[("savings_by_currency", r#"{"CZK":999}"#)]);
        rebuild_days(&conn, d(0), today).expect("only today's row");
        assert_eq!(savings(5), map(&[("CZK", 100.0 + 699.0)]));
        assert_eq!(savings(25), map(&[("CZK", 300.0 + 699.0)]));
        assert_eq!(
            column(&conn, today, "savings_by_currency"),
            map(&[("CZK", 999.0)]),
            "today's row stays the live snapshot's"
        );

        // a live row before today wins over today's row for the days after it
        live_with(&conn, d(10), &[("savings_by_currency", r#"{"CZK":850}"#)]);
        rebuild_days(&conn, d(0), today).expect("a live row and today's");
        assert_eq!(savings(5), map(&[("CZK", 100.0 + 750.0)]));
        assert_eq!(savings(25), map(&[("CZK", 300.0 + 750.0)]));
    }

    #[test]
    fn a_class_the_facts_cover_completely_ignores_the_live_rows() {
        let conn = setup_test_db();
        let today = d(30);
        account(&conn, "a", "CZK", "300", false);
        record(&conn, "a", d(0), "100", "CZK");
        record(&conn, "a", d(20), "300", "CZK");
        // a recorded value that disagrees with the facts, on a day they cover
        live_with(&conn, d(10), &[("savings_by_currency", r#"{"CZK":999}"#)]);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let savings = |day: i64| column(&conn, d(day), "savings_by_currency");
        for (day, expected) in [
            (0, 100.0),
            (9, 100.0),
            (10, 100.0),
            (11, 100.0),
            (25, 300.0),
        ] {
            assert_eq!(savings(day), map(&[("CZK", expected)]), "day {day}");
        }
        // ... and the second rebuild reads the rewritten live row: the same rows
        let rows = dump(&conn);
        rebuild_days(&conn, d(0), today).expect("again");
        assert_eq!(dump(&conn), rows);
    }

    #[test]
    fn bonds_on_a_new_row_equal_the_nearest_live_rows_and_today_s_without_one() {
        let conn = setup_test_db();
        let today = d(40);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        bond(&conn, "b", "1000", "1", "CZK");

        rebuild_days(&conn, d(0), today).expect("no live row");
        let bonds = |day: i64| column(&conn, d(day), "bonds_by_currency");
        assert_eq!(bonds(0), map(&[("CZK", 1000.0)]), "today's value");
        assert_eq!(bonds(39), map(&[("CZK", 1000.0)]));

        live_with(&conn, d(10), &[("bonds_by_currency", r#"{"CZK":900}"#)]);
        live_with(
            &conn,
            d(30),
            &[("bonds_by_currency", r#"{"CZK":950,"EUR":10}"#)],
        );
        rebuild_days(&conn, d(0), today).expect("live rows");
        assert_eq!(
            bonds(0),
            map(&[("CZK", 900.0)]),
            "the earliest later live row"
        );
        assert_eq!(bonds(9), map(&[("CZK", 900.0)]));
        assert_eq!(bonds(11), map(&[("CZK", 900.0)]), "the nearest earlier");
        assert_eq!(bonds(29), map(&[("CZK", 900.0)]));
        assert_eq!(bonds(31), map(&[("CZK", 950.0), ("EUR", 10.0)]));
        assert_eq!(bonds(39), map(&[("CZK", 950.0), ("EUR", 10.0)]));
        // the live rows themselves keep what was recorded (an undated bond is not a fact)
        assert_eq!(bonds(10), map(&[("CZK", 900.0)]));
        assert_eq!(bonds(30), map(&[("CZK", 950.0), ("EUR", 10.0)]));
    }

    #[test]
    fn a_dated_bond_is_counted_from_its_purchase_day_and_the_remainder_holds_only_the_undated_ones()
    {
        let conn = setup_test_db();
        let today = d(40);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        // an undated bond (today 500) and one bought on day 10 (1000)
        bond(&conn, "undated", "500", "1", "CZK");
        dated_bond(&conn, "dated", "1000", "1", "CZK", d(10));
        // a live row before the purchase (the undated bond was worth 400 then) and one after it
        // (400 + 1000 + 100 more of the undated one)
        live_with(&conn, d(8), &[("bonds_by_currency", r#"{"CZK":400}"#)]);
        live_with(&conn, d(30), &[("bonds_by_currency", r#"{"CZK":1500}"#)]);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let bonds = |day: i64| column(&conn, d(day), "bonds_by_currency");
        // before the purchase the nearest live row counts the same items: its 400 are all
        // undated; on or before day 8 the earliest row is the one to follow
        assert_eq!(bonds(0), map(&[("CZK", 400.0)]));
        assert_eq!(bonds(9), map(&[("CZK", 400.0)]));
        // from the purchase on, the row of day 8 no longer counts the purchased bond, so the
        // days follow the row of day 30: 1500 - the dated 1000 = 500 undated, plus the bond
        assert_eq!(bonds(10), map(&[("CZK", 1500.0)]));
        assert_eq!(bonds(20), map(&[("CZK", 1500.0)]));
        assert_eq!(bonds(35), map(&[("CZK", 1500.0)]));
        // the live rows keep what was recorded: an undated bond is not a fact
        assert_eq!(bonds(8), map(&[("CZK", 400.0)]));
        assert_eq!(bonds(30), map(&[("CZK", 1500.0)]));

        // a second rebuild changes nothing
        let rows = dump(&conn);
        rebuild_days(&conn, d(0), today).expect("again");
        assert_eq!(dump(&conn), rows);
    }

    #[test]
    fn without_a_live_row_the_undated_bonds_keep_today_s_value_next_to_the_dated_ones() {
        let conn = setup_test_db();
        let today = d(40);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        bond(&conn, "undated", "500", "1", "CZK");
        dated_bond(&conn, "dated", "1000", "1", "EUR", d(10));

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let bonds = |day: i64| column(&conn, d(day), "bonds_by_currency");
        assert_eq!(bonds(0), map(&[("CZK", 500.0)]));
        assert_eq!(bonds(9), map(&[("CZK", 500.0)]));
        assert_eq!(bonds(10), map(&[("CZK", 500.0), ("EUR", 1000.0)]));
        assert_eq!(bonds(39), map(&[("CZK", 500.0), ("EUR", 1000.0)]));
    }

    #[test]
    fn other_assets_without_transactions_follow_the_live_rows_and_a_stale_record_adds_nothing() {
        let conn = setup_test_db();
        let today = d(30);
        account(&conn, "a", "CZK", "100", false);
        record(&conn, "a", d(0), "100", "CZK");
        // an MCP-created asset (a quantity, no transactions) and a dated one
        asset(&conn, "mcp", "CZK", "3", "100");
        asset(&conn, "gold", "CZK", "1", "20");
        asset_tx(&conn, "gold", "buy", "1", d(5));
        asset_valuation(&conn, "gold", d(5), "20", "CZK", 1);
        live_with(
            &conn,
            d(10),
            &[("other_assets_by_currency", r#"{"CZK":520}"#)],
        );

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let assets = |day: i64| column(&conn, d(day), "other_assets_by_currency");
        // the remainder of live row 10 is 520 - 20 (the dated asset) = 500, whatever today's
        // 300 says; days before the dated asset's purchase carry it alone
        assert_eq!(assets(0), map(&[("CZK", 500.0)]));
        assert_eq!(assets(4), map(&[("CZK", 500.0)]));
        assert_eq!(assets(5), map(&[("CZK", 520.0)]));
        assert_eq!(assets(11), map(&[("CZK", 520.0)]));
        assert_eq!(assets(29), map(&[("CZK", 520.0)]));

        // a record below what the facts derive is stale: it adds nothing, and does not
        // subtract (an other asset cannot be negative)
        conn.execute("DELETE FROM portfolio_metrics_history", [])
            .expect("clear");
        live_with(
            &conn,
            d(10),
            &[("other_assets_by_currency", r#"{"CZK":5}"#)],
        );
        rebuild_days(&conn, d(0), today).expect("stale");
        assert_eq!(assets(4), Breakdown::new());
        assert_eq!(assets(11), map(&[("CZK", 20.0)]));
    }

    #[test]
    fn a_negative_savings_remainder_is_kept_for_an_overdraft_account() {
        let conn = setup_test_db();
        let today = d(20);
        account(&conn, "a", "CZK", "100", false);
        record(&conn, "a", d(0), "100", "CZK");
        account(&conn, "overdraft", "CZK", "-200", false);
        // 100 on the dated account, -200 on the overdraft one, as recorded then
        live_with(&conn, d(10), &[("savings_by_currency", r#"{"CZK":-100}"#)]);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let savings = |day: i64| column(&conn, d(day), "savings_by_currency");
        assert_eq!(savings(0), map(&[("CZK", -100.0)]));
        assert_eq!(savings(15), map(&[("CZK", -100.0)]));
    }

    #[test]
    fn an_uncovered_property_follows_the_live_row_until_its_first_valuation() {
        let conn = setup_test_db();
        let today = d(60);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        // no purchase date: not counted before its first valuation (day 30)
        property(&conn, "p", "personal", None, ("700", "CZK"));
        property_valuation(&conn, "p", d(30), "700", "CZK", 1);
        live_with(
            &conn,
            d(10),
            &[
                ("real_estate_by_currency", r#"{"CZK":500}"#),
                ("real_estate_personal_by_currency", r#"{"CZK":500}"#),
            ],
        );
        live_with(
            &conn,
            d(40),
            &[
                ("real_estate_by_currency", r#"{"CZK":999}"#),
                ("real_estate_personal_by_currency", r#"{"CZK":999}"#),
            ],
        );

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let personal = |day: i64| column(&conn, d(day), "real_estate_personal_by_currency");
        let combined = |day: i64| column(&conn, d(day), "real_estate_by_currency");
        for day in [0, 5, 11, 29] {
            assert_eq!(personal(day), map(&[("CZK", 500.0)]), "day {day}");
            assert_eq!(combined(day), map(&[("CZK", 500.0)]), "day {day}");
        }
        // the live row before the first valuation keeps its record; from the valuation on
        // the facts cover the class, every day and the live row 40 included
        assert_eq!(personal(10), map(&[("CZK", 500.0)]));
        for day in [30, 35, 40, 41, 59] {
            assert_eq!(personal(day), map(&[("CZK", 700.0)]), "day {day}");
        }
    }

    /// Savings with a dated account `z` (from day 0), an account `a` whose first record is day 20,
    /// and an undated one `u`. A live row on day 10 was recorded before `a`'s first record, one
    /// on day 40 after it; both hold every account at its balance of the day.
    fn savings_with_a_late_first_record(conn: &Connection, live_days: &[i64]) {
        account(conn, "z", "CZK", "100", false);
        record(conn, "z", d(0), "100", "CZK");
        account(conn, "a", "CZK", "300", false);
        record(conn, "a", d(20), "300", "CZK");
        account(conn, "u", "CZK", "650", false);
        for day in live_days {
            let recorded = match day {
                10 => r#"{"CZK":1020}"#, // z 100 + a 280 + u 640
                _ => r#"{"CZK":1050}"#,  // z 100 + a 300 + u 650
            };
            live_with(conn, d(*day), &[("savings_by_currency", recorded)]);
        }
    }

    #[test]
    fn a_live_row_before_an_accounts_first_record_is_not_the_source_of_the_days_after_it() {
        let conn = setup_test_db();
        let today = d(60);
        savings_with_a_late_first_record(&conn, &[10, 40]);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let savings = |day: i64| column(&conn, d(day), "savings_by_currency");
        // before the record `a` is not counted, and live row 10 (which holds it) is the source
        assert_eq!(savings(5), map(&[("CZK", 1020.0)]));
        assert_eq!(savings(15), map(&[("CZK", 1020.0)]));
        // from the record on `a` is counted: live row 10 holds it too, so the remainder comes
        // from live row 40 instead (1050 - the dated 400 = 650), and `a` is counted once
        for day in [20, 25, 39] {
            assert_eq!(savings(day), map(&[("CZK", 1050.0)]), "day {day}");
        }
        // after live row 40 it is the nearest one, and it counts `a`
        for day in [41, 59] {
            assert_eq!(savings(day), map(&[("CZK", 1050.0)]), "day {day}");
        }
        // the live rows keep what was recorded
        assert_eq!(savings(10), map(&[("CZK", 1020.0)]));
        assert_eq!(savings(40), map(&[("CZK", 1050.0)]));

        let rows = dump(&conn);
        rebuild_days(&conn, d(0), today).expect("again");
        assert_eq!(dump(&conn), rows, "idempotent");
    }

    #[test]
    fn when_no_live_row_counts_the_same_items_the_undated_items_keep_today_s_value() {
        let conn = setup_test_db();
        let today = d(60);
        // only the live row before `a`'s first record, and none after it
        savings_with_a_late_first_record(&conn, &[10]);

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let savings = |day: i64| column(&conn, d(day), "savings_by_currency");
        assert_eq!(
            savings(15),
            map(&[("CZK", 1020.0)]),
            "live row 10 counts z only"
        );
        // day 25 counts `a`; live row 10 does not hold the same items: today's value of `u`
        // (z 100 + a 300 + u 650)
        assert_eq!(savings(25), map(&[("CZK", 1050.0)]));
        assert_eq!(savings(59), map(&[("CZK", 1050.0)]));
    }

    #[test]
    fn the_chunks_take_the_same_remainder_as_a_single_pass() {
        let today = d(400);
        let seed = |conn: &Connection| {
            account(conn, "z", "CZK", "100", false);
            record(conn, "z", today - 350 * DAY, "100", "CZK");
            account(conn, "a", "CZK", "300", false);
            record(conn, "a", today - 200 * DAY, "300", "CZK");
            account(conn, "u", "CZK", "650", false);
            live_with(
                conn,
                today - 300 * DAY,
                &[("savings_by_currency", r#"{"CZK":1020}"#)],
            );
            live_with(
                conn,
                today - 50 * DAY,
                &[("savings_by_currency", r#"{"CZK":1050}"#)],
            );
        };
        let chunked = setup_test_db();
        seed(&chunked);
        rebuild_days(&chunked, today - 1_000 * DAY, today).expect("chunked");

        let single = setup_test_db();
        seed(&single);
        let mut plan = plan_rebuild(&single, today - 1_000 * DAY, today).expect("plan");
        prune_before_first_fact(&single, &plan).expect("prune");
        let start = plan.first_fact_day().expect("first fact day");
        rebuild_chunk(&single, &mut plan, start, today).expect("one pass");

        assert_eq!(dump(&single), dump(&chunked));
        let savings =
            |conn: &Connection, ago: i64| column(conn, today - ago * DAY, "savings_by_currency");
        assert_eq!(
            savings(&chunked, 250),
            map(&[("CZK", 1020.0)]),
            "before the record"
        );
        assert_eq!(
            savings(&chunked, 150),
            map(&[("CZK", 1050.0)]),
            "after it, once"
        );
    }

    #[test]
    fn an_other_asset_bought_after_a_live_row_is_not_counted_twice() {
        let conn = setup_test_db();
        let today = d(60);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        asset(&conn, "mcp", "CZK", "3", "100");
        asset(&conn, "gold", "CZK", "1", "20");
        asset_tx(&conn, "gold", "buy", "1", d(30));
        asset_valuation(&conn, "gold", d(30), "20", "CZK", 1);
        // live row 10 already holds the gold (500 undated + 20), live row 40 holds it too
        live_with(
            &conn,
            d(10),
            &[("other_assets_by_currency", r#"{"CZK":520}"#)],
        );
        live_with(
            &conn,
            d(40),
            &[("other_assets_by_currency", r#"{"CZK":330}"#)],
        );

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let assets = |day: i64| column(&conn, d(day), "other_assets_by_currency");
        assert_eq!(
            assets(15),
            map(&[("CZK", 520.0)]),
            "gold not counted yet, row 10"
        );
        for day in [30, 35, 39, 41] {
            assert_eq!(assets(day), map(&[("CZK", 330.0)]), "day {day}");
        }
    }

    #[test]
    fn an_asset_sold_out_since_still_lets_a_later_live_row_serve() {
        let conn = setup_test_db();
        let today = d(60);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        asset(&conn, "mcp", "CZK", "3", "100");
        asset(&conn, "gold", "CZK", "0", "20");
        asset_tx(&conn, "gold", "buy", "1", d(5));
        asset_tx(&conn, "gold", "sell", "1", d(20));
        asset_valuation(&conn, "gold", d(5), "20", "CZK", 1);
        // the gold is sold out on the later live row: it holds none of it, nothing to count twice
        live_with(
            &conn,
            d(30),
            &[("other_assets_by_currency", r#"{"CZK":310}"#)],
        );

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let assets = |day: i64| column(&conn, d(day), "other_assets_by_currency");
        assert_eq!(
            assets(10),
            map(&[("CZK", 330.0)]),
            "20 of gold + the 310 remainder"
        );
        assert_eq!(assets(25), map(&[("CZK", 310.0)]));
    }

    #[test]
    fn a_property_valued_after_a_live_row_is_not_counted_twice() {
        let conn = setup_test_db();
        let today = d(80);
        account(&conn, "a", "CZK", "1", false);
        record(&conn, "a", d(0), "1", "CZK");
        // two properties without a purchase date: first valued on day 30 and on day 50
        property(&conn, "p1", "personal", None, ("800", "CZK"));
        property_valuation(&conn, "p1", d(30), "800", "CZK", 1);
        property(&conn, "p2", "personal", None, ("700", "CZK"));
        property_valuation(&conn, "p2", d(50), "700", "CZK", 1);
        // row 10 holds both at their then value, row 40 holds p1 and p2 (p2 not valued yet)
        for (day, total) in [(10, r#"{"CZK":1500}"#), (40, r#"{"CZK":1500}"#)] {
            live_with(
                &conn,
                d(day),
                &[
                    ("real_estate_by_currency", total),
                    ("real_estate_personal_by_currency", total),
                ],
            );
        }

        rebuild_days(&conn, d(0), today).expect("rebuild");

        let personal = |day: i64| column(&conn, d(day), "real_estate_personal_by_currency");
        assert_eq!(personal(15), map(&[("CZK", 1500.0)]));
        // p1 counted from day 30, which live row 10 holds: row 40 supplies the remainder
        // (1500 - the dated 800 = 700), and p1 is counted once
        for day in [30, 35, 39, 41, 49] {
            assert_eq!(personal(day), map(&[("CZK", 1500.0)]), "day {day}");
        }
        // from day 50 both are valued: the class is covered, no remainder
        assert_eq!(personal(55), map(&[("CZK", 1500.0)]));
    }

    #[test]
    fn the_estimated_past_of_an_overridden_loan_is_rewritten_on_live_rows_too() {
        let conn = setup_test_db();
        let today = iso("2025-01-10");
        yearly_loan(&conn);
        loan_event(&conn, "l", "balance_check", iso("2024-06-20"), "4000", 1);
        // the estimate of 2024-03-15 is 10 000; the row recorded 10 500 (an old valuation model)
        live_with(
            &conn,
            iso("2024-03-15"),
            &[("loans_by_currency", r#"{"CZK":10500}"#)],
        );

        rebuild_days(&conn, iso("2024-01-01"), today).expect("rebuild");

        let loans = |date: &str| column(&conn, iso(date), "loans_by_currency");
        // the ledger walk alone: no remainder of the live row moves the estimated past
        assert_eq!(loans("2024-01-10"), map(&[("CZK", 12_000.0)]));
        assert_eq!(loans("2024-02-20"), map(&[("CZK", 11_000.0)]));
        assert_eq!(loans("2024-03-16"), map(&[("CZK", 10_000.0)]));
        assert_eq!(loans("2024-04-12"), map(&[("CZK", 9_000.0)]));
        assert_eq!(loans("2024-06-19"), map(&[("CZK", 7_000.0)]));
        assert_eq!(loans("2024-06-20"), map(&[("CZK", 4_000.0)]));
        assert_eq!(loans("2024-12-31"), map(&[("CZK", 4_000.0)]));
        // the live row is rewritten as well: the recorded loan value was computed, not a fact
        assert_eq!(loans("2024-03-15"), map(&[("CZK", 10_000.0)]));
    }

    #[test]
    fn a_live_row_whose_recorded_loans_differ_from_the_ledger_takes_the_ledger_value() {
        let conn = setup_test_db();
        let today = iso("2024-08-01");
        yearly_loan(&conn);
        loan_event(&conn, "l", "balance_check", iso("2024-06-20"), "4000", 1);
        // two live rows recorded loan totals of the old valuation model; both lie before the
        // first manual balance, so each is the estimate of its day
        for (date, recorded) in [
            ("2024-03-15", r#"{"CZK":2000000}"#),
            ("2024-05-20", r#"{"CZK":3}"#),
        ] {
            live_with(&conn, iso(date), &[("loans_by_currency", recorded)]);
        }

        rebuild_days(&conn, iso("2024-01-01"), today).expect("rebuild");

        let loans = |date: &str| column(&conn, iso(date), "loans_by_currency");
        assert_eq!(loans("2024-03-15"), map(&[("CZK", 10_000.0)]));
        assert_eq!(loans("2024-05-20"), map(&[("CZK", 8_000.0)]));
        // the live rows stay live
        assert!(dump(&conn)
            .iter()
            .any(|(at, source, _)| *at == iso("2024-03-15") + 3_600 && source == "live"));
    }

    #[test]
    fn days_before_a_loan_started_have_no_loans_whatever_a_live_row_recorded() {
        let conn = setup_test_db();
        let today = iso("2024-04-01");
        // an older fact starts the history; the loan only starts on 2024-01-10
        account(&conn, "a", "CZK", "100", false);
        record(&conn, "a", iso("2023-11-01"), "100", "CZK");
        yearly_loan(&conn);
        loan_event(&conn, "l", "balance_check", iso("2024-03-01"), "9500", 1);
        // a live row after the start, before the first manual balance, recorded a loan total
        // that the ledger does not hold
        live_with(
            &conn,
            iso("2024-02-15"),
            &[("loans_by_currency", r#"{"CZK":2011416}"#)],
        );

        rebuild_days(&conn, iso("2023-01-01"), today).expect("rebuild");

        for date in ["2023-11-01", "2023-12-31", "2024-01-09"] {
            assert!(
                column(&conn, iso(date), "loans_by_currency").is_empty(),
                "{date}: no loan exists yet"
            );
        }
        let loans = |date: &str| column(&conn, iso(date), "loans_by_currency");
        assert_eq!(loans("2024-01-10"), map(&[("CZK", 12_000.0)]));
        assert_eq!(
            loans("2024-02-15"),
            map(&[("CZK", 11_000.0)]),
            "the live row"
        );
        assert_eq!(
            loans("2024-03-20"),
            map(&[("CZK", 9_500.0)]),
            "the manual balance"
        );
        // the rebuild is idempotent: a second one changes nothing
        let first = dump(&conn);
        rebuild_days(&conn, iso("2023-01-01"), today).expect("again");
        assert_eq!(dump(&conn), first);
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
        // a dated one next to it: counted from the day it was bought
        dated_bond(conn, "b2", "250", "2", "EUR", today - 120 * DAY);
        live_row(conn, today);
        live_row(conn, today - 50 * DAY);
        // what the undated account and the bond were worth on a live day far back
        live_with(
            conn,
            today - 150 * DAY,
            &[
                ("savings_by_currency", r#"{"CZK":130,"EUR":18}"#),
                ("bonds_by_currency", r#"{"CZK":777}"#),
            ],
        );
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
        // a bond bought long ago and one bought today count today like the live snapshot
        dated_bond(&conn, "old", "300", "2", "EUR", today - 400 * DAY);
        dated_bond(&conn, "new", "40", "1", "CZK", today);
        let live = compute_breakdowns(&conn).expect("live breakdowns");
        approx(&values_on(&conn, today).breakdowns.bonds, &live.bonds);
        assert_eq!(live.bonds, map(&[("CZK", 2040.0), ("EUR", 600.0)]));
        // the fixture is not vacuous
        assert_eq!(live.savings, map(&[("CZK", 1500.0), ("EUR", 200.0)]));
        assert!(live.loans["CZK"] > 0.0 && live.loans["EUR"] > 0.0);
        assert_eq!(live.other_assets, map(&[("CZK", 140.0)]));
    }

    /// The parent keeps its own currency while the price mirrors the newest valuation: today's
    /// breakdown and the day values both count the asset in that valuation's currency.
    #[test]
    fn todays_other_assets_are_in_the_currency_of_the_newest_valuation() {
        let conn = setup_test_db();
        let today = today_utc_day();
        // dated: bought 2, re-estimated in USD
        asset(&conn, "gold", "CZK", "2", "3");
        asset_tx(&conn, "gold", "buy", "2", today - 100 * DAY);
        asset_valuation(&conn, "gold", today - 100 * DAY, "70", "CZK", 1);
        asset_valuation(&conn, "gold", today - 10 * DAY, "3", "USD", 2);
        // carried: a quantity without transactions, estimated in EUR
        asset(&conn, "mcp", "CZK", "4", "80");
        asset_valuation(&conn, "mcp", today - 5 * DAY, "80", "EUR", 1);

        let live = compute_breakdowns(&conn).expect("live breakdowns");
        assert_eq!(live.other_assets, map(&[("USD", 6.0), ("EUR", 320.0)]));
        approx(
            &values_on(&conn, today).breakdowns.other_assets,
            &live.other_assets,
        );
    }
}
