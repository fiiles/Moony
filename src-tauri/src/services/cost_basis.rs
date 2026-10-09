//! Historical-rate cost basis derivation from transactions.
//!
//! Cost basis is never stored — it is derived from the transaction record,
//! converting every transaction at the exchange rate of its own day
//! (ADR 0001: "converting old values at today's rate is wrong").
//! Weighted-average-cost semantics match `recalculate_investment_metrics`:
//! buys accumulate cost, sells release cost proportionally at the running
//! average.
//!
//! Three figures are accumulated in one pass:
//! - `cost_basis_target`: every buy converted to the target currency at its day's rate.
//! - `cost_basis_native`: every buy converted to the position's
//!   primary currency (the currency of its first transaction) at its day's
//!   cross rate. For single-currency positions (stocks enforce this at write
//!   time) no conversion happens and the value is exact.
//! - `realized_gain_target`: for every sell, the proceeds converted at the sell day minus the
//!   target cost it releases at the running average (port of
//!   `shared/calculations/realized-gains.ts`).
//!
//! A conversion whose day has no common quote for both currencies (ten-day walk-back) uses
//! the current quotes and the position is flagged `fx_estimated` (also marked in the returned
//! `Quality`); when even those are missing the amount is left out of that aggregate and the
//! unresolvable currency is reported in the returned `Quality` (never valued 1:1). Each failed
//! transaction counts as one missing conversion.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;
use crate::services::fx::{History, Quality, Quotes};

/// One transaction row, parsed. Order-independent input; sorting happens in
/// the accumulator.
pub struct TxRow {
    pub tx_type: String,
    pub quantity: f64,
    pub price_per_unit: f64,
    pub currency: String,
    pub transaction_date: i64,
}

/// Derived position metrics. `primary_currency` is the currency of the
/// earliest transaction.
#[derive(Debug, PartialEq)]
pub struct PositionCostBasis {
    pub quantity: f64,
    /// Cost of the open quantity in the target currency. Buys that could not be converted are
    /// left out (see `cost_basis_target_complete`).
    pub cost_basis_target: f64,
    /// True when every buy could be converted to the target currency.
    pub cost_basis_target_complete: bool,
    pub cost_basis_native: f64,
    /// Quantity whose buys are included in `cost_basis_native`. Smaller than `quantity` when a
    /// buy could not be converted to the primary currency; the average uses this so it is not
    /// diluted by quantity that carries no cost.
    pub native_quantity: f64,
    /// Realized gain in the target currency: sell proceeds at the sell day minus the released
    /// average cost. `None` when a sell, or a buy held at the time of a sell, could not be
    /// converted.
    pub realized_gain_target: Option<f64>,
    pub primary_currency: String,
    /// Currencies of transactions that could not be converted for this position. Empty when
    /// every conversion succeeded.
    pub missing_currencies: Vec<String>,
    /// True when at least one conversion used current instead of day quotes.
    pub fx_estimated: bool,
    /// This position's conversion quality (the same misses and estimate as above).
    pub quality: Quality,
}

impl PositionCostBasis {
    /// Weighted average cost per unit in the primary currency, over the buys that could be
    /// converted to it (see `native_quantity`).
    pub fn average_price_native(&self) -> f64 {
        if self.native_quantity > 0.0 {
            self.cost_basis_native / self.native_quantity
        } else {
            0.0
        }
    }

    /// Cost basis in the target currency, `None` unless every buy could be converted.
    pub fn cost_basis_in_target(&self) -> Option<f64> {
        self.cost_basis_target_complete
            .then_some(self.cost_basis_target)
    }
}

/// Which transaction table to derive from. Both share the relevant column
/// names; the enum keeps table names out of caller-supplied strings.
#[derive(Clone, Copy)]
pub enum TxTable {
    Stocks,
    Crypto,
}

impl TxTable {
    fn table_name(self) -> &'static str {
        match self {
            TxTable::Stocks => "investment_transactions",
            TxTable::Crypto => "crypto_transactions",
        }
    }
}

/// Converts at a transaction's day, falling back to current quotes; records what could not
/// be converted at all.
struct DayConverter<'a> {
    history: &'a History,
    current: &'a Quotes,
    estimated: bool,
    quality: Quality,
}

impl DayConverter<'_> {
    /// The day's quotes (ten-day walk-back), else the current quotes (flagging `estimated`),
    /// else `None`. Records nothing; see `record_miss`.
    fn convert(&mut self, amount: f64, from: &str, to: &str, day: i64) -> Option<f64> {
        if let Some(value) = self.history.convert_on(amount, from, to, day) {
            return Some(value);
        }
        let value = self.current.convert(amount, from, to)?;
        self.estimated = true;
        Some(value)
    }

    /// Records one failed conversion, blaming the side(s) the current quotes cannot resolve.
    fn record_miss(&mut self, from: &str, to: &str) {
        self.quality.track_conversion(self.current, from, to, None);
    }
}

/// Chronological replay order. Dates are day-granular, so same-day transactions share a
/// timestamp; within a day buys are processed before sells so a sell can never precede the
/// buy it depends on. The stable sort keeps the caller's created_at/rowid order for the rest.
fn chronological(txs: &[TxRow]) -> Vec<&TxRow> {
    let mut order: Vec<&TxRow> = txs.iter().collect();
    order.sort_by_key(|tx| (tx.transaction_date, tx.tx_type == "sell"));
    order
}

/// Units a sell of `requested` closes out of `open`: a sell larger than the open quantity
/// only closes what is held (the excess has no cost to release and must not drive the
/// quantity negative ahead of a later buy).
fn units_sold(open: f64, requested: f64) -> f64 {
    requested.min(open.max(0.0))
}

/// The quantity a position's transactions replay to — the same rule as
/// `PositionCostBasis::quantity`, without converting anything.
pub fn replayed_quantity(txs: &[TxRow]) -> f64 {
    let mut quantity = 0.0_f64;
    for tx in chronological(txs) {
        if tx.tx_type == "buy" {
            quantity += tx.quantity;
        } else if tx.tx_type == "sell" {
            quantity -= units_sold(quantity, tx.quantity);
        }
    }
    quantity.max(0.0)
}

/// The replayed quantity of every position in `table` that has transactions, keyed by
/// investment id. A position without transactions is absent (its stored quantity holds).
pub fn replayed_quantities(conn: &Connection, table: TxTable) -> Result<HashMap<String, f64>> {
    Ok(load_transactions(conn, table)?
        .into_iter()
        .map(|(id, txs)| {
            let quantity = replayed_quantity(&txs);
            (id, quantity)
        })
        .collect())
}

/// Accumulate one position's transactions into historical-rate cost basis in `target`.
/// Returns `None` for an empty transaction list (no position can be derived).
pub fn position_cost_basis(
    txs: &[TxRow],
    history: &History,
    current: &Quotes,
    target: &str,
) -> (Option<PositionCostBasis>, Quality) {
    if txs.is_empty() {
        return (None, Quality::default());
    }
    let mut converter = DayConverter {
        history,
        current,
        estimated: false,
        quality: Quality::default(),
    };

    let order = chronological(txs);
    let primary_currency = order[0].currency.to_uppercase();

    let mut quantity = 0.0_f64;
    let mut cost_target = 0.0_f64;
    let mut target_complete = true;
    let mut cost_native = 0.0_f64;
    let mut native_quantity = 0.0_f64;
    let mut realized = 0.0_f64;
    let mut realized_known = true;

    for tx in order {
        let day = tx.transaction_date;
        if tx.tx_type == "buy" {
            let amount = tx.quantity * tx.price_per_unit;
            // A buy that cannot be converted adds no cost to that aggregate (never 1:1).
            let amount_target = converter.convert(amount, &tx.currency, target, day);
            let amount_native = converter.convert(amount, &tx.currency, &primary_currency, day);
            // One failed buy is one missing conversion, even when both aggregates miss it.
            match (amount_target, amount_native) {
                (None, _) => converter.record_miss(&tx.currency, target),
                (Some(_), None) => converter.record_miss(&tx.currency, &primary_currency),
                _ => {}
            }
            match amount_target {
                Some(value) => cost_target += value,
                None => target_complete = false,
            }
            if let Some(native) = amount_native {
                cost_native += native;
                native_quantity += tx.quantity;
            }
            quantity += tx.quantity;
        } else if tx.tx_type == "sell" {
            // Release cost proportionally at the running average, matching
            // recalculate_investment_metrics.
            let sold = units_sold(quantity, tx.quantity);
            if quantity > 0.0 && sold > 0.0 {
                let fraction = sold / quantity;
                let released_target = cost_target * fraction;
                // Only the held quantity realizes a gain; proceeds convert at the sell day.
                if !target_complete {
                    // The released cost is unknown; that buy's miss is already recorded.
                    realized_known = false;
                } else {
                    let proceeds =
                        converter.convert(sold * tx.price_per_unit, &tx.currency, target, day);
                    match proceeds {
                        Some(proceeds) => realized += proceeds - released_target,
                        None => {
                            converter.record_miss(&tx.currency, target);
                            realized_known = false;
                        }
                    }
                }
                cost_target -= released_target;
                cost_native -= cost_native * fraction;
                native_quantity -= native_quantity * fraction;
            }
            quantity -= sold;
        }
    }

    if quantity < 0.0 {
        quantity = 0.0;
    }
    if cost_target < 0.0 {
        cost_target = 0.0;
    }
    if cost_native < 0.0 {
        cost_native = 0.0;
    }
    if quantity == 0.0 || native_quantity < 0.0 {
        native_quantity = 0.0;
    }
    if quantity == 0.0 {
        // A fully closed position holds no cost.
        cost_target = 0.0;
        cost_native = 0.0;
    }
    if converter.estimated {
        converter.quality.mark_estimated();
    }

    (
        Some(PositionCostBasis {
            quantity,
            cost_basis_target: cost_target,
            cost_basis_target_complete: target_complete,
            cost_basis_native: cost_native,
            native_quantity,
            realized_gain_target: realized_known.then_some(realized),
            primary_currency,
            fx_estimated: converter.estimated,
            missing_currencies: converter.quality.missing_currencies(),
            quality: converter.quality.clone(),
        }),
        converter.quality,
    )
}

/// One `investment_id, type, quantity, price_per_unit, currency, transaction_date` row, parsed.
/// Tolerant parsing: legacy data may contain suffixes (matches
/// `recalculate_investment_metrics`).
fn parse_tx_row(row: &rusqlite::Row<'_>, first: usize) -> rusqlite::Result<TxRow> {
    let qty_str: String = row.get(first + 1)?;
    let price_str: String = row.get(first + 2)?;
    let leading_number = |text: &str| {
        text.split_whitespace()
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0.0)
    };
    Ok(TxRow {
        tx_type: row.get(first)?,
        quantity: leading_number(&qty_str),
        price_per_unit: leading_number(&price_str),
        currency: row.get(first + 3)?,
        transaction_date: row.get(first + 4)?,
    })
}

/// Every transaction of `table`, parsed and grouped by investment id (chronological order
/// within a group, ties by `created_at` then rowid).
fn load_transactions(conn: &Connection, table: TxTable) -> Result<HashMap<String, Vec<TxRow>>> {
    let sql = format!(
        "SELECT investment_id, type, quantity, price_per_unit, currency, transaction_date
         FROM {} ORDER BY transaction_date ASC, created_at ASC, rowid ASC",
        table.table_name()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<(String, TxRow)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, parse_tx_row(row, 1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut grouped: HashMap<String, Vec<TxRow>> = HashMap::new();
    for (investment_id, tx) in rows {
        grouped.entry(investment_id).or_default().push(tx);
    }
    Ok(grouped)
}

/// The transactions of one ticker in `table` (case-insensitive), in the same order as
/// `load_transactions`.
pub fn load_ticker_transactions(
    conn: &Connection,
    table: TxTable,
    ticker: &str,
) -> Result<Vec<TxRow>> {
    let sql = format!(
        "SELECT type, quantity, price_per_unit, currency, transaction_date
         FROM {} WHERE UPPER(ticker) = UPPER(?1)
         ORDER BY transaction_date ASC, created_at ASC, rowid ASC",
        table.table_name()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([ticker], |row| parse_tx_row(row, 0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Derives every grouped position against one history.
fn derive_all(
    grouped: HashMap<String, Vec<TxRow>>,
    history: &History,
    current: &Quotes,
    target: &str,
) -> (HashMap<String, PositionCostBasis>, Quality) {
    let mut quality = Quality::default();
    let mut positions = HashMap::new();
    for (id, txs) in grouped {
        let (position, position_quality) = position_cost_basis(&txs, history, current, target);
        quality.merge(&position_quality);
        if let Some(position) = position {
            positions.insert(id, position);
        }
    }
    (positions, quality)
}

/// The earliest and latest transaction day of the grouped rows.
fn date_range(grouped: &HashMap<String, Vec<TxRow>>) -> Option<(i64, i64)> {
    let dates = grouped.values().flatten().map(|tx| tx.transaction_date);
    let min = dates.clone().min()?;
    let max = dates.max()?;
    Some((min, max))
}

/// Derive cost basis in `target` for every position in `table`, keyed by investment id.
/// Loads the FX timeseries once for the whole transaction date range; `current` is the
/// request's quote snapshot used when a day has no quotes. The returned `Quality` lists
/// currencies of transactions that could not be converted (and were left out).
pub fn cost_basis_for_all(
    conn: &Connection,
    table: TxTable,
    target: &str,
    current: &Quotes,
) -> Result<(HashMap<String, PositionCostBasis>, Quality)> {
    let grouped = load_transactions(conn, table)?;
    let Some((min_day, max_day)) = date_range(&grouped) else {
        return Ok((HashMap::new(), Quality::default()));
    };
    // `History::load` pads the range with the look-back window itself.
    let history = History::load(conn, min_day, max_day)?;
    Ok(derive_all(grouped, &history, current, target))
}

/// Like `cost_basis_for_all`, but day quotes are read only up to `today`: transactions dated
/// later have no historical rate yet and use the current quotes (flagged estimated).
pub fn cost_basis_as_of(
    conn: &Connection,
    table: TxTable,
    target: &str,
    current: &Quotes,
    today: i64,
) -> Result<(HashMap<String, PositionCostBasis>, Quality)> {
    let grouped = load_transactions(conn, table)?;
    let Some((min_day, _)) = date_range(&grouped) else {
        return Ok((HashMap::new(), Quality::default()));
    };
    let history = History::load(conn, min_day.min(today), today)?;
    Ok(derive_all(grouped, &history, current, target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const DAY: i64 = 86_400;
    const D1: i64 = 1_700_006_400; // 2023-11-15 (midnight UTC)
    const D2: i64 = D1 + DAY;

    /// History from `(day, currency, units per EUR)` rows.
    fn rates(entries: &[(i64, &str, f64)]) -> History {
        let mut days: BTreeMap<i64, Quotes> = BTreeMap::new();
        for (day, ccy, units) in entries {
            days.entry(*day)
                .or_insert_with(|| Quotes::new("EUR"))
                .insert(ccy, *units);
        }
        History::from_days(days)
    }

    /// A day on which 1 USD costs `czk_per_usd` CZK (CZK at 25 per EUR).
    fn usd_day(day: i64, czk_per_usd: f64) -> [(i64, &'static str, f64); 2] {
        [(day, "CZK", 25.0), (day, "USD", 25.0 / czk_per_usd)]
    }

    /// Current quotes: 25 CZK and 25/23 USD per EUR (1 USD = 23 CZK).
    fn current() -> Quotes {
        Quotes::from_stored([("CZK", 25.0), ("USD", 25.0 / 23.0)])
    }

    fn derive(txs: &[TxRow], history: &History, current: &Quotes) -> PositionCostBasis {
        position_cost_basis(txs, history, current, "CZK")
            .0
            .expect("position")
    }

    fn buy(qty: f64, price: f64, ccy: &str, date: i64) -> TxRow {
        TxRow {
            tx_type: "buy".into(),
            quantity: qty,
            price_per_unit: price,
            currency: ccy.into(),
            transaction_date: date,
        }
    }

    fn sell(qty: f64, price: f64, ccy: &str, date: i64) -> TxRow {
        TxRow {
            tx_type: "sell".into(),
            quantity: qty,
            price_per_unit: price,
            currency: ccy.into(),
            transaction_date: date,
        }
    }

    #[test]
    fn replayed_quantity_follows_the_cost_basis_replay() {
        // A sell listed before a same-day buy, an oversell, then a later buy.
        let txs = vec![
            sell(3.0, 100.0, "USD", D1),
            buy(5.0, 100.0, "USD", D1),
            sell(9.0, 100.0, "USD", D2),
            buy(2.0, 100.0, "USD", D2 + DAY),
        ];
        let quantity = replayed_quantity(&txs);
        assert!((quantity - 2.0).abs() < 1e-9, "{quantity}");
        let derived = derive(&txs, &rates(&[]), &current());
        assert!((derived.quantity - quantity).abs() < 1e-12);
        assert_eq!(replayed_quantity(&[]), 0.0);
    }

    #[test]
    fn cost_basis_uses_each_transactions_day_rate() {
        // Two identical USD buys on days with different rates: the CZK cost
        // must be the per-day sum, NOT quantity × average × any single rate.
        let rates = rates(&[usd_day(D1, 25.0), usd_day(D2, 20.0)].concat());
        let txs = vec![buy(10.0, 100.0, "USD", D1), buy(10.0, 100.0, "USD", D2)];

        let result = derive(&txs, &rates, &current());

        assert!((result.cost_basis_target - 45_000.0).abs() < 1e-9);
        assert!((result.quantity - 20.0).abs() < 1e-9);
        assert_eq!(result.primary_currency, "USD");
        // Native currency needs no conversion: 20 × 100 USD.
        assert!((result.cost_basis_native - 2_000.0).abs() < 1e-9);
        assert!((result.average_price_native() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn sell_releases_cost_proportionally_at_running_average() {
        let rates = rates(&[usd_day(D1, 25.0), usd_day(D2, 20.0)].concat());
        let txs = vec![
            buy(10.0, 100.0, "USD", D1),
            buy(10.0, 100.0, "USD", D2),
            sell(10.0, 150.0, "USD", D2),
        ];

        let result = derive(&txs, &rates, &current());

        // Half the quantity sold → half of both cost aggregates released.
        assert!((result.quantity - 10.0).abs() < 1e-9);
        assert!((result.cost_basis_target - 22_500.0).abs() < 1e-9);
        assert!((result.cost_basis_native - 1_000.0).abs() < 1e-9);
    }

    #[test]
    fn oversell_only_closes_the_held_quantity() {
        // buy 10 @ 100, sell 15 @ 120, buy 10 @ 200 → 10 @ 200
        let rates = rates(&[usd_day(D1, 25.0), usd_day(D2, 25.0)].concat());
        let txs = vec![
            buy(10.0, 100.0, "USD", D1),
            sell(15.0, 120.0, "USD", D1 + 1),
            buy(10.0, 200.0, "USD", D2),
        ];
        let result = derive(&txs, &rates, &current());
        assert!((result.quantity - 10.0).abs() < 1e-9);
        assert!((result.average_price_native() - 200.0).abs() < 1e-9);
    }

    #[test]
    fn same_day_sell_is_processed_after_same_day_buys() {
        // CSV exports are often newest-first, so a same-day sell can precede
        // its buys in rowid order. Within one day buys come first.
        let rates = rates(&usd_day(D1, 25.0));
        let txs = vec![
            sell(10.0, 150.0, "USD", D1),
            buy(10.0, 100.0, "USD", D1),
            buy(10.0, 100.0, "USD", D1),
        ];

        let result = derive(&txs, &rates, &current());

        assert!((result.quantity - 10.0).abs() < 1e-9);
        assert!((result.cost_basis_target - 25_000.0).abs() < 1e-9);
        assert!((result.average_price_native() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn transactions_are_sorted_by_date_before_accumulation() {
        // Same as the sell test but supplied out of order: the sell happens
        // chronologically last and must release cost from BOTH buys.
        let rates = rates(&[usd_day(D1, 25.0), usd_day(D2, 20.0)].concat());
        let txs = vec![
            sell(10.0, 150.0, "USD", D2 + 1),
            buy(10.0, 100.0, "USD", D2),
            buy(10.0, 100.0, "USD", D1),
        ];

        let result = derive(&txs, &rates, &current());

        assert!((result.cost_basis_target - 22_500.0).abs() < 1e-9);
        assert_eq!(result.primary_currency, "USD");
    }

    #[test]
    fn mixed_currency_transactions_convert_at_their_days_cross_rate() {
        // Crypto case: first buy in CZK, second in USD. Primary currency is
        // CZK; the USD buy converts at ITS day's rate (20), not today's (23).
        let rates = rates(&[usd_day(D1, 25.0), usd_day(D2, 20.0)].concat());
        let txs = vec![
            buy(1.0, 500_000.0, "CZK", D1),
            buy(1.0, 30_000.0, "USD", D2),
        ];

        let result = derive(&txs, &rates, &current());

        assert_eq!(result.primary_currency, "CZK");
        assert!((result.cost_basis_target - 1_100_000.0).abs() < 1e-9);
        assert!((result.cost_basis_native - 1_100_000.0).abs() < 1e-9);
    }

    #[test]
    fn mixed_currency_native_uses_cross_rate_when_primary_is_foreign() {
        // Primary USD; a CZK buy converts to USD at its day's USD rate.
        let rates = rates(&[usd_day(D1, 20.0), usd_day(D2, 25.0)].concat());
        let txs = vec![
            buy(1.0, 30_000.0, "USD", D1),
            buy(1.0, 500_000.0, "CZK", D2),
        ];

        let result = derive(&txs, &rates, &current());

        assert_eq!(result.primary_currency, "USD");
        // CZK: 30 000 × 20 + 500 000 = 1 100 000
        assert!((result.cost_basis_target - 1_100_000.0).abs() < 1e-9);
        // USD: 30 000 + 500 000 / 25 = 50 000
        assert!((result.cost_basis_native - 50_000.0).abs() < 1e-9);
    }

    #[test]
    fn missing_history_falls_back_to_current_rates_and_is_flagged() {
        // No history at all for the transaction's day (beyond walk-back):
        // fall back to the provided current rates (USD 23) and flag the position.
        let rates = rates(&[(D1 - 30 * DAY, "CZK", 24.0)]);
        let txs = vec![buy(10.0, 100.0, "USD", D1)];

        let result = derive(&txs, &rates, &current());

        assert!((result.cost_basis_target - 23_000.0).abs() < 1e-9);
        assert!(result.fx_estimated);
    }

    #[test]
    fn day_rates_do_not_flag_the_position_as_estimated() {
        let rates = rates(&usd_day(D1, 25.0));
        let result = derive(&[buy(10.0, 100.0, "USD", D1)], &rates, &current());
        assert!(!result.fx_estimated);
    }

    #[test]
    fn missing_rate_is_reported_not_booked_at_one() {
        let txs = vec![TxRow {
            tx_type: "buy".into(),
            quantity: 1.0,
            price_per_unit: 100.0,
            currency: "KWD".into(),
            transaction_date: 20_000 * 86_400,
        }];
        let history = crate::services::fx::History::from_days(Default::default());
        let (pos, quality) = position_cost_basis(
            &txs,
            &history,
            &crate::services::fx::Quotes::new("CZK"),
            "CZK",
        );
        let pos = pos.expect("position");
        assert_eq!(pos.cost_basis_target, 0.0);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
    }

    #[test]
    fn unconvertible_buy_does_not_dilute_the_native_average() {
        // Primary USD: 1 @ 100 USD, then 1 @ 50 KWD (no KWD quote). The KWD buy carries no
        // native cost, so it must not lower the USD average either.
        let txs = vec![buy(1.0, 100.0, "USD", D1), buy(1.0, 50.0, "KWD", D2)];
        let (pos, quality) = position_cost_basis(&txs, &rates(&[]), &current(), "CZK");
        let pos = pos.expect("position");
        assert!((pos.quantity - 2.0).abs() < 1e-9);
        assert!((pos.cost_basis_native - 100.0).abs() < 1e-9);
        assert!((pos.average_price_native() - 100.0).abs() < 1e-9);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
    }

    #[test]
    fn only_the_unquoted_side_of_a_cross_conversion_is_reported() {
        // CZK buy into a KWD-primary position: CZK is fine, KWD has no quote.
        let txs = vec![buy(1.0, 100.0, "KWD", D1), buy(1.0, 500.0, "CZK", D2)];
        let (pos, quality) = position_cost_basis(&txs, &rates(&[]), &current(), "CZK");
        let pos = pos.expect("position");
        assert!((pos.cost_basis_target - 500.0).abs() < 1e-9);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
    }

    #[test]
    fn weekend_transaction_walks_back_to_closest_earlier_day() {
        // Rate snapshot exists two days before the transaction (weekend gap).
        let rates = rates(&usd_day(D1, 25.0));
        let txs = vec![buy(10.0, 100.0, "USD", D1 + 2 * DAY)];

        let result = derive(&txs, &rates, &current());

        assert!((result.cost_basis_target - 25_000.0).abs() < 1e-9);
    }

    #[test]
    fn empty_transactions_yield_none() {
        assert!(position_cost_basis(&[], &rates(&[]), &current(), "CZK")
            .0
            .is_none());
    }

    #[test]
    fn czk_transactions_need_no_rates() {
        let result = derive(&[buy(144.0, 1_043.0, "CZK", D1)], &rates(&[]), &current());
        assert!((result.cost_basis_target - 150_192.0).abs() < 1e-9);
        assert_eq!(result.primary_currency, "CZK");
    }

    #[test]
    fn oversell_clamps_to_zero_not_negative() {
        let rates = rates(&usd_day(D1, 25.0));
        let txs = vec![buy(10.0, 100.0, "USD", D1), sell(15.0, 100.0, "USD", D1)];

        let result = derive(&txs, &rates, &current());

        assert!(result.quantity.abs() < 1e-9);
        assert!(result.cost_basis_target.abs() < 1e-9);
        assert!(result.cost_basis_native.abs() < 1e-9);
    }

    #[test]
    fn realized_gain_releases_average_cost_and_converts_proceeds_at_the_sell_day() {
        // buy 10 @ 100 USD (USD 20), buy 10 @ 100 USD (USD 30) -> avg 2 500 CZK per share;
        // sell 5 @ 120 USD on a USD 25 day: proceeds 15 000, released cost 12 500.
        let d3 = D2 + DAY;
        let rates = rates(&[usd_day(D1, 20.0), usd_day(D2, 30.0), usd_day(d3, 25.0)].concat());
        let txs = vec![
            buy(10.0, 100.0, "USD", D1),
            buy(10.0, 100.0, "USD", D2),
            sell(5.0, 120.0, "USD", d3),
        ];
        let result = derive(&txs, &rates, &current());
        assert!((result.realized_gain_target.expect("realized") - 2_500.0).abs() < 1e-9);
        assert!(result.cost_basis_target_complete);
        assert!((result.cost_basis_target - 37_500.0).abs() < 1e-9);
    }

    #[test]
    fn realized_gain_ignores_the_oversold_excess_and_sells_without_holdings() {
        // sell before any buy realizes nothing; oversell realizes only the held quantity.
        let rates = rates(&[usd_day(D1, 20.0), usd_day(D2, 20.0)].concat());
        let txs = vec![
            sell(3.0, 500.0, "USD", D1 - DAY),
            buy(10.0, 100.0, "USD", D1),
            sell(15.0, 110.0, "USD", D2),
        ];
        let result = derive(&txs, &rates, &current());
        // 10 x (110 - 100) USD x 20
        assert!((result.realized_gain_target.expect("realized") - 2_000.0).abs() < 1e-9);
    }

    #[test]
    fn realized_gain_is_unknown_when_a_held_buy_or_the_sell_cannot_be_converted() {
        let empty = Quotes::new("CZK");
        // KWD buy has no quote at all: the released cost is unknown.
        let txs = vec![buy(1.0, 100.0, "KWD", D1), sell(1.0, 120.0, "KWD", D2)];
        let (pos, quality) = position_cost_basis(&txs, &rates(&[]), &empty, "CZK");
        let pos = pos.expect("position");
        assert_eq!(pos.realized_gain_target, None);
        assert!(!pos.cost_basis_target_complete);
        // the buy failed once; the sell is not converted again
        assert_eq!(quality.missing_count(), 1);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);

        // A sell that cannot be converted makes the realized gain unknown.
        // (the sell day is beyond the ten-day walk-back from the only quote day)
        let txs = vec![
            buy(1.0, 100.0, "USD", D1),
            sell(1.0, 120.0, "USD", D1 + 20 * DAY),
        ];
        let (pos, quality) = position_cost_basis(&txs, &rates(&usd_day(D1, 20.0)), &empty, "CZK");
        let pos = pos.expect("position");
        assert!(pos.cost_basis_target_complete);
        assert_eq!(pos.realized_gain_target, None);
        assert_eq!(quality.missing_count(), 1);
    }

    #[test]
    fn positions_without_sells_realize_nothing() {
        let rates = rates(&usd_day(D1, 20.0));
        let result = derive(&[buy(1.0, 100.0, "USD", D1)], &rates, &current());
        assert_eq!(result.realized_gain_target, Some(0.0));
    }

    #[test]
    fn a_buy_failing_both_aggregates_counts_one_missing_conversion() {
        // Primary USD; a KWD buy fails to the target and to the primary currency.
        let txs = vec![buy(1.0, 100.0, "USD", D1), buy(1.0, 50.0, "KWD", D2)];
        let (_, quality) = position_cost_basis(&txs, &rates(&[]), &current(), "CZK");
        assert_eq!(quality.missing_count(), 1);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
    }

    #[test]
    fn estimated_cost_basis_is_flagged_in_the_quality() {
        let (pos, quality) = position_cost_basis(
            &[buy(10.0, 100.0, "USD", D1)],
            &rates(&[]),
            &current(),
            "CZK",
        );
        assert!(pos.expect("position").fx_estimated);
        assert!(quality.is_estimated());
        assert!(quality.is_complete());
    }

    #[test]
    fn target_currency_can_be_any_quoted_currency() {
        // USD 20 and EUR 25 CZK on the buy day: 1 000 USD = 800 EUR.
        let rates = rates(&usd_day(D1, 20.0));
        let (pos, quality) =
            position_cost_basis(&[buy(10.0, 100.0, "USD", D1)], &rates, &current(), "EUR");
        assert!((pos.expect("position").cost_basis_target - 800.0).abs() < 1e-9);
        assert!(quality.is_complete());
    }

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            "#,
        )
        .expect("schema");
        conn
    }

    #[test]
    fn cost_basis_for_all_groups_by_investment_and_uses_day_rates() {
        let conn = setup_db();
        conn.execute_batch(&format!(
            r#"
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES
                ({d1}, 'CZK', 25.0), ({d1}, 'USD', 1.0), ({d2}, 'CZK', 25.0), ({d2}, 'USD', 1.25);
            INSERT INTO investment_transactions
                (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date)
            VALUES
                ('t1', 'inv-a', 'buy', 'AAA', 'A', '10', '100', 'USD', {d1}),
                ('t2', 'inv-a', 'buy', 'AAA', 'A', '10', '100', 'USD', {d2}),
                ('t3', 'inv-b', 'buy', 'BBB', 'B', '5', '1000', 'CZK', {d1});
            "#,
            d1 = D1,
            d2 = D2,
        ))
        .expect("seed");

        let (map, quality) =
            cost_basis_for_all(&conn, TxTable::Stocks, "CZK", &current()).expect("derive");
        assert!(quality.is_complete());

        let a = map.get("inv-a").expect("inv-a");
        assert!((a.cost_basis_target - 45_000.0).abs() < 1e-9);
        assert!((a.quantity - 20.0).abs() < 1e-9);
        let b = map.get("inv-b").expect("inv-b");
        assert!((b.cost_basis_target - 5_000.0).abs() < 1e-9);
        assert_eq!(b.primary_currency, "CZK");
    }

    #[test]
    fn cost_basis_for_all_reads_crypto_table() {
        let conn = setup_db();
        conn.execute_batch(&format!(
            r#"
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES
                ({d1}, 'CZK', 25.0), ({d1}, 'USD', 1.25);
            INSERT INTO crypto_transactions
                (id, investment_id, type, ticker, name, quantity, price_per_unit, currency, transaction_date)
            VALUES
                ('c1', 'btc', 'buy', 'BTC', 'Bitcoin', '1', '500000', 'CZK', {d1}),
                ('c2', 'btc', 'buy', 'BTC', 'Bitcoin', '1', '30000', 'USD', {d1});
            "#,
            d1 = D1,
        ))
        .expect("seed");

        let (map, _) =
            cost_basis_for_all(&conn, TxTable::Crypto, "CZK", &current()).expect("derive");

        let btc = map.get("btc").expect("btc");
        assert!((btc.quantity - 2.0).abs() < 1e-9);
        assert!((btc.cost_basis_target - 1_100_000.0).abs() < 1e-9);
        assert_eq!(btc.primary_currency, "CZK");
    }
}
