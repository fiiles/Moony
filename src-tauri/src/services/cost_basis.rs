//! Historical-rate cost basis derivation from transactions.
//!
//! Cost basis is never stored — it is derived from the transaction record,
//! converting every transaction at the exchange rate of its own day
//! (ADR 0001: "converting old values at today's rate is wrong").
//! Weighted-average-cost semantics match `recalculate_investment_metrics`:
//! buys accumulate cost, sells release cost proportionally at the running
//! average.
//!
//! Two aggregates are accumulated in one pass:
//! - `cost_basis_target`: every buy converted to the target currency at its day's rate.
//! - `cost_basis_native`: every buy converted to the position's
//!   primary currency (the currency of its first transaction) at its day's
//!   cross rate. For single-currency positions (stocks enforce this at write
//!   time) no conversion happens and the value is exact.
//!
//! A buy whose day has no common quote for both currencies (ten-day walk-back) is
//! converted with the current quotes and the position is flagged `fx_estimated`; when even
//! those are missing the buy is left out of that aggregate and its currency is reported in
//! the returned `Quality` (never valued 1:1).

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;
use crate::services::fx::{self, History, Quality, Quotes};

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
    pub cost_basis_target: f64,
    pub cost_basis_native: f64,
    /// Quantity whose buys are included in `cost_basis_native`. Smaller than `quantity` when a
    /// buy could not be converted to the primary currency; the average uses this so it is not
    /// diluted by quantity that carries no cost.
    pub native_quantity: f64,
    pub primary_currency: String,
    /// Currencies of buys that could not be converted for this position (left out of the cost
    /// aggregates). Empty when the cost basis is complete.
    pub missing_currencies: Vec<String>,
    /// True when at least one conversion used current instead of day quotes.
    pub fx_estimated: bool,
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

/// Converts at a transaction's day, falling back to current quotes and recording what could
/// not be converted at all.
struct DayConverter<'a> {
    history: &'a History,
    current: &'a Quotes,
    estimated: bool,
    quality: Quality,
}

impl DayConverter<'_> {
    /// The day's quotes (ten-day walk-back), else the current quotes (flagging `estimated`),
    /// else `None` with the unresolvable currency recorded in `quality`.
    fn convert(&mut self, amount: f64, from: &str, to: &str, day: i64) -> Option<f64> {
        if let Some(value) = self.history.convert_on(amount, from, to, day) {
            return Some(value);
        }
        match self.current.convert(amount, from, to) {
            Some(value) => {
                self.estimated = true;
                Some(value)
            }
            None => {
                let missing = if self.current.units(from).is_none() {
                    from
                } else {
                    to
                };
                self.quality.track(missing, None);
                None
            }
        }
    }
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

    // Chronological replay. Dates are day-granular, so same-day transactions
    // share a timestamp; within a day buys are processed before sells so a
    // sell can never precede the buy it depends on. The stable
    // sort keeps the caller's created_at/rowid order for the rest.
    let mut order: Vec<&TxRow> = txs.iter().collect();
    order.sort_by_key(|tx| (tx.transaction_date, tx.tx_type == "sell"));

    let primary_currency = order[0].currency.to_uppercase();

    let mut quantity = 0.0_f64;
    let mut cost_target = 0.0_f64;
    let mut cost_native = 0.0_f64;
    let mut native_quantity = 0.0_f64;

    for tx in order {
        if tx.tx_type == "buy" {
            let day = tx.transaction_date;
            let amount = tx.quantity * tx.price_per_unit;
            // A buy that cannot be converted adds no cost to that aggregate (never 1:1).
            let amount_target = converter
                .convert(amount, &tx.currency, target, day)
                .unwrap_or(0.0);
            let amount_native = converter.convert(amount, &tx.currency, &primary_currency, day);
            if let Some(native) = amount_native {
                cost_native += native;
                native_quantity += tx.quantity;
            }
            cost_target += amount_target;
            quantity += tx.quantity;
        } else if tx.tx_type == "sell" {
            // Release cost proportionally at the running average, matching
            // recalculate_investment_metrics.
            // A sell larger than the open quantity only closes what is held
            //: the excess has no cost to release and must not drive
            // the quantity negative ahead of a later buy.
            let sold = tx.quantity.min(quantity.max(0.0));
            if quantity > 0.0 && sold > 0.0 {
                let fraction = sold / quantity;
                cost_target -= cost_target * fraction;
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

    (
        Some(PositionCostBasis {
            quantity,
            cost_basis_target: cost_target,
            cost_basis_native: cost_native,
            native_quantity,
            primary_currency,
            fx_estimated: converter.estimated,
            missing_currencies: converter.quality.missing_currencies(),
        }),
        converter.quality,
    )
}

/// Derive cost basis for every position in `table`, keyed by investment id.
/// Loads the FX timeseries once for the whole transaction date range. The returned `Quality`
/// lists currencies whose buys could not be converted (and were left out of the cost).
/// Phase 1: the target currency is CZK.
pub fn cost_basis_for_all(
    conn: &Connection,
    table: TxTable,
) -> Result<(HashMap<String, PositionCostBasis>, Quality)> {
    let target = "CZK";
    let sql = format!(
        "SELECT investment_id, type, quantity, price_per_unit, currency, transaction_date
         FROM {} ORDER BY transaction_date ASC, created_at ASC, rowid ASC",
        table.table_name()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<(String, TxRow)> = stmt
        .query_map([], |row| {
            let qty_str: String = row.get(2)?;
            let price_str: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                TxRow {
                    tx_type: row.get(1)?,
                    // Tolerant parsing: legacy data may contain suffixes
                    // (matches recalculate_investment_metrics).
                    quantity: qty_str
                        .split_whitespace()
                        .next()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0.0),
                    price_per_unit: price_str
                        .split_whitespace()
                        .next()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0.0),
                    currency: row.get(4)?,
                    transaction_date: row.get(5)?,
                },
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        return Ok((HashMap::new(), Quality::default()));
    }

    let min_day = rows
        .iter()
        .map(|(_, tx)| tx.transaction_date)
        .min()
        .unwrap_or(0);
    let max_day = rows
        .iter()
        .map(|(_, tx)| tx.transaction_date)
        .max()
        .unwrap_or(0);
    // `History::load` pads the range with the look-back window itself.
    let history = History::load(conn, min_day, max_day)?;
    let current = fx::current();

    let mut grouped: HashMap<String, Vec<TxRow>> = HashMap::new();
    for (investment_id, tx) in rows {
        grouped.entry(investment_id).or_default().push(tx);
    }

    let mut quality = Quality::default();
    let mut positions = HashMap::new();
    for (id, txs) in grouped {
        let (position, position_quality) = position_cost_basis(&txs, &history, &current, target);
        quality.merge(&position_quality);
        if let Some(position) = position {
            positions.insert(id, position);
        }
    }
    Ok((positions, quality))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const DAY: i64 = 86_400;
    const D1: i64 = 1_700_006_400; // 2023-11-15 (midnight UTC)
    const D2: i64 = D1 + DAY;

    /// History from `(day, currency, CZK per unit)` rows.
    fn rates(entries: &[(i64, &str, f64)]) -> History {
        let mut days: BTreeMap<i64, HashMap<String, f64>> = BTreeMap::new();
        for (day, ccy, rate) in entries {
            days.entry(*day)
                .or_default()
                .insert((*ccy).to_string(), *rate);
        }
        History::from_days(
            days.into_iter()
                .map(|(day, map)| (day, Quotes::from_czk_per_unit(&map)))
                .collect(),
        )
    }

    fn current() -> Quotes {
        Quotes::from_czk_per_unit(&HashMap::from([
            ("USD".to_string(), 23.0),
            ("EUR".to_string(), 25.0),
        ]))
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
    fn cost_basis_uses_each_transactions_day_rate() {
        // Two identical USD buys on days with different rates: the CZK cost
        // must be the per-day sum, NOT quantity × average × any single rate.
        let rates = rates(&[(D1, "USD", 25.0), (D2, "USD", 20.0)]);
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
        let rates = rates(&[(D1, "USD", 25.0), (D2, "USD", 20.0)]);
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
        let rates = rates(&[(D1, "USD", 25.0), (D2, "USD", 25.0)]);
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
        let rates = rates(&[(D1, "USD", 25.0)]);
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
        let rates = rates(&[(D1, "USD", 25.0), (D2, "USD", 20.0)]);
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
        let rates = rates(&[(D1, "USD", 25.0), (D2, "USD", 20.0)]);
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
        let rates = rates(&[(D1, "USD", 20.0), (D2, "USD", 25.0)]);
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
        let rates = rates(&[(D1 - 30 * DAY, "EUR", 24.0)]);
        let txs = vec![buy(10.0, 100.0, "USD", D1)];

        let result = derive(&txs, &rates, &current());

        assert!((result.cost_basis_target - 23_000.0).abs() < 1e-9);
        assert!(result.fx_estimated);
    }

    #[test]
    fn day_rates_do_not_flag_the_position_as_estimated() {
        let rates = rates(&[(D1, "USD", 25.0)]);
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
        let rates = rates(&[(D1, "USD", 25.0)]);
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
        let rates = rates(&[(D1, "USD", 25.0)]);
        let txs = vec![buy(10.0, 100.0, "USD", D1), sell(15.0, 100.0, "USD", D1)];

        let result = derive(&txs, &rates, &current());

        assert!(result.quantity.abs() < 1e-9);
        assert!(result.cost_basis_target.abs() < 1e-9);
        assert!(result.cost_basis_native.abs() < 1e-9);
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
                ({d1}, 'USD', 25.0), ({d2}, 'USD', 20.0);
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

        let (map, quality) = cost_basis_for_all(&conn, TxTable::Stocks).expect("derive");
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
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES ({d1}, 'USD', 20.0);
            INSERT INTO crypto_transactions
                (id, investment_id, type, ticker, name, quantity, price_per_unit, currency, transaction_date)
            VALUES
                ('c1', 'btc', 'buy', 'BTC', 'Bitcoin', '1', '500000', 'CZK', {d1}),
                ('c2', 'btc', 'buy', 'BTC', 'Bitcoin', '1', '30000', 'USD', {d1});
            "#,
            d1 = D1,
        ))
        .expect("seed");

        let (map, _) = cost_basis_for_all(&conn, TxTable::Crypto).expect("derive");

        let btc = map.get("btc").expect("btc");
        assert!((btc.quantity - 2.0).abs() < 1e-9);
        assert!((btc.cost_basis_target - 1_100_000.0).abs() < 1e-9);
        assert_eq!(btc.primary_currency, "CZK");
    }
}
