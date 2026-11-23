//! Lot matching of the stock transactions report: replays every stock trade in the order of
//! `cost_basis::chronological` (per day buys before sells) and matches each sale to the oldest
//! open buy lots of its position (FIFO). Pure bookkeeping in native quantities; money and
//! conversion belong to the report service.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::models::validation::utc_day;
use crate::services::reporting::parse_money;

/// Quantities below this are rounding dust (0.1 + 0.2 - 0.3).
const EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradeKind {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub id: String,
    pub investment_id: String,
    pub ticker: String,
    pub company_name: String,
    pub kind: TradeKind,
    pub quantity: f64,
    pub price: f64,
    pub currency: String,
    /// UTC midnight of the trade date.
    pub day: i64,
    /// In the trade currency, 0.0 when none.
    pub fee: f64,
}

/// `quantity` shares of the sale `sale` come from the buy `buy` (indices into `Replay::trades`).
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub sale: usize,
    pub buy: usize,
    pub quantity: f64,
}

#[derive(Debug)]
pub struct Replay {
    /// Replay order.
    pub trades: Vec<Trade>,
    /// In sale order, oldest lot first within a sale.
    pub matches: Vec<Match>,
    /// `(buy index, open quantity > 0)`, oldest first.
    pub open: Vec<(usize, f64)>,
    /// `(sale index, quantity with no lot)`.
    pub uncovered: Vec<(usize, f64)>,
}

/// Every stock trade, in stored order (`transaction_date`, `created_at`, `rowid`). Ticker and
/// company name come from the position. Strict: a row that does not parse fails the report (a
/// skipped row would silently shift every later match), with an error naming its id.
pub fn load_trades(conn: &Connection) -> Result<Vec<Trade>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.investment_id, s.ticker, s.company_name, t.type, t.quantity,
                t.price_per_unit, t.currency, t.transaction_date, t.fee
         FROM investment_transactions t
         JOIN stock_investments s ON s.id = t.investment_id
         WHERE t.type IN ('buy', 'sell')
         ORDER BY t.transaction_date, t.created_at, t.rowid",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, Option<String>>(9)?,
        ))
    })?;
    let mut trades = Vec::new();
    for row in rows {
        let (id, investment_id, ticker, company_name, kind, quantity, price, currency, date, fee) =
            row?;
        let parsed = parse_numbers(&quantity, &price, date, fee.as_deref());
        let (quantity, price, day, fee) = parsed.map_err(|error| {
            log::error!("[STOCK REPORT] Transaction {id} is unreadable: {error}");
            match error {
                AppError::Validation(key) => {
                    AppError::validation_with(&key, serde_json::json!({ "transactionId": id }))
                }
                other => other,
            }
        })?;
        trades.push(Trade {
            id,
            investment_id,
            ticker,
            company_name,
            kind: if kind == "sell" {
                TradeKind::Sell
            } else {
                TradeKind::Buy
            },
            quantity,
            price,
            currency,
            day,
            fee,
        });
    }
    Ok(trades)
}

/// Quantity, price, UTC day and fee (0 when none) of a stored trade row.
fn parse_numbers(
    quantity: &str,
    price: &str,
    date: i64,
    fee: Option<&str>,
) -> Result<(f64, f64, i64, f64)> {
    Ok((
        parse_money(quantity)?,
        parse_money(price)?,
        utc_day(date, "validation.invalidDate")?,
        match fee.map(str::trim).filter(|f| !f.is_empty()) {
            Some(text) => parse_money(text)?,
            None => 0.0,
        },
    ))
}

/// Match the sales of `trades` to buy lots, FIFO per position. Order key `(day, is sale)` with a
/// stable sort, so within a day buys come first and the stored order breaks the remaining ties
/// (as `cost_basis::chronological`).
pub fn replay(mut trades: Vec<Trade>) -> Replay {
    trades.sort_by_key(|t| (t.day, t.kind == TradeKind::Sell));
    // Open lots per position: (buy index, open quantity), oldest first.
    let mut lots: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
    let mut matches = Vec::new();
    let mut uncovered = Vec::new();
    for (index, trade) in trades.iter().enumerate() {
        let position = lots.entry(trade.investment_id.clone()).or_default();
        match trade.kind {
            TradeKind::Buy => position.push((index, trade.quantity)),
            TradeKind::Sell => {
                let mut remaining = trade.quantity;
                for (buy, open) in position.iter_mut() {
                    if remaining < EPSILON {
                        break;
                    }
                    let taken = remaining.min(*open);
                    if taken < EPSILON {
                        continue;
                    }
                    matches.push(Match {
                        sale: index,
                        buy: *buy,
                        quantity: taken,
                    });
                    *open -= taken;
                    remaining -= taken;
                }
                position.retain(|(_, open)| *open >= EPSILON);
                if remaining >= EPSILON {
                    uncovered.push((index, remaining));
                }
            }
        }
    }
    let mut open: Vec<(usize, f64)> = lots
        .into_values()
        .flatten()
        .filter(|(_, quantity)| *quantity >= EPSILON)
        .collect();
    open.sort_by_key(|(buy, _)| *buy);
    Replay {
        trades,
        matches,
        open,
        uncovered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cost_basis::{replayed_quantity, TxRow};

    const DAY: i64 = 86_400;
    // 2023-01-10, 2024-01-10, 2026-02-10 at UTC midnight.
    const D1: i64 = 1_673_308_800;
    const D2: i64 = 1_704_844_800;
    const D3: i64 = 1_770_681_600;

    fn trade(id: &str, inv: &str, kind: TradeKind, quantity: f64, day: i64) -> Trade {
        Trade {
            id: id.into(),
            investment_id: inv.into(),
            ticker: inv.to_uppercase(),
            company_name: format!("{inv} Inc."),
            kind,
            quantity,
            price: 100.0,
            currency: "USD".into(),
            day,
            fee: 0.0,
        }
    }

    fn buy(id: &str, inv: &str, quantity: f64, day: i64) -> Trade {
        trade(id, inv, TradeKind::Buy, quantity, day)
    }

    fn sell(id: &str, inv: &str, quantity: f64, day: i64) -> Trade {
        trade(id, inv, TradeKind::Sell, quantity, day)
    }

    fn index_of(replay: &Replay, id: &str) -> usize {
        replay.trades.iter().position(|t| t.id == id).expect(id)
    }

    fn m(replay: &Replay, sale: &str, buy: &str, quantity: f64) -> Match {
        Match {
            sale: index_of(replay, sale),
            buy: index_of(replay, buy),
            quantity,
        }
    }

    #[test]
    fn a_sale_consumes_the_oldest_lots_first() {
        let r = replay(vec![
            buy("lot1", "aaa", 10.0, D1),
            buy("lot2", "aaa", 10.0, D2),
            sell("sale", "aaa", 15.0, D3),
        ]);
        assert_eq!(
            r.matches,
            vec![m(&r, "sale", "lot1", 10.0), m(&r, "sale", "lot2", 5.0)]
        );
        assert_eq!(r.open, vec![(index_of(&r, "lot2"), 5.0)]);
        assert!(r.uncovered.is_empty());
    }

    #[test]
    fn a_lot_can_feed_several_sales() {
        let r = replay(vec![
            buy("lot", "aaa", 10.0, D1),
            sell("s1", "aaa", 4.0, D2),
            sell("s2", "aaa", 4.0, D3),
        ]);
        assert_eq!(
            r.matches,
            vec![m(&r, "s1", "lot", 4.0), m(&r, "s2", "lot", 4.0)]
        );
        assert_eq!(r.open.len(), 1);
        assert!((r.open[0].1 - 2.0).abs() < EPSILON);
        assert!(r.uncovered.is_empty());
    }

    #[test]
    fn same_day_buy_comes_before_the_sale_whatever_the_stored_order() {
        let r = replay(vec![
            sell("sale", "aaa", 3.0, D1),
            buy("lot", "aaa", 3.0, D1),
        ]);
        assert_eq!(r.matches, vec![m(&r, "sale", "lot", 3.0)]);
        assert!(r.uncovered.is_empty());
        assert!(r.open.is_empty());
    }

    #[test]
    fn selling_more_than_held_reports_the_uncovered_part() {
        let r = replay(vec![
            buy("lot", "aaa", 5.0, D1),
            sell("sale", "aaa", 8.0, D2),
        ]);
        assert_eq!(r.matches, vec![m(&r, "sale", "lot", 5.0)]);
        assert_eq!(r.uncovered, vec![(index_of(&r, "sale"), 3.0)]);
        assert!(r.open.is_empty());
    }

    #[test]
    fn fractional_quantities_leave_no_dust() {
        let r = replay(vec![
            buy("b1", "aaa", 0.1, D1),
            buy("b2", "aaa", 0.2, D1 + DAY),
            sell("sale", "aaa", 0.3, D2),
        ]);
        assert!(r.open.is_empty(), "{:?}", r.open);
        assert!(r.uncovered.is_empty(), "{:?}", r.uncovered);
        assert_eq!(r.matches.len(), 2);
    }

    #[test]
    fn positions_do_not_share_lots() {
        let r = replay(vec![
            buy("a", "aaa", 10.0, D1),
            buy("b", "bbb", 10.0, D1),
            sell("sale", "bbb", 4.0, D2),
        ]);
        assert_eq!(r.matches, vec![m(&r, "sale", "b", 4.0)]);
        assert_eq!(r.open.len(), 2);
        assert!(r.uncovered.is_empty());
    }

    #[test]
    fn the_replay_agrees_with_the_cost_basis_quantity() {
        let trades = vec![
            sell("s0", "aaa", 3.0, D1),
            buy("b1", "aaa", 5.0, D1),
            sell("s1", "aaa", 9.0, D2),
            buy("b2", "aaa", 2.0, D2 + DAY),
            buy("b3", "aaa", 1.5, D3),
            sell("s2", "aaa", 0.5, D3),
        ];
        let rows: Vec<TxRow> = trades
            .iter()
            .map(|t| TxRow {
                tx_type: if t.kind == TradeKind::Buy {
                    "buy"
                } else {
                    "sell"
                }
                .into(),
                quantity: t.quantity,
                price_per_unit: t.price,
                currency: t.currency.clone(),
                transaction_date: t.day,
            })
            .collect();
        let r = replay(trades);
        let open: f64 = r.open.iter().map(|(_, q)| q).sum();
        assert!((open - replayed_quantity(&rows)).abs() < 1e-9, "{open}");
    }

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
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
                created_at INTEGER NOT NULL DEFAULT 0,
                fee TEXT
            );
            "#,
        )
        .expect("schema");
        conn
    }

    #[test]
    fn an_unreadable_trade_fails_with_its_id() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, company_name, quantity)
             VALUES ('inv', 'AAA', 'Alpha Inc.', '10')",
            [],
        )
        .expect("investment");
        conn.execute(
            "INSERT INTO investment_transactions
               (id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                currency, transaction_date)
             VALUES ('tx-bad', 'inv', 'buy', 'AAA', 'Alpha Inc.', 'ten', '100', 'USD', ?1)",
            [D1],
        )
        .expect("transaction");
        let error = load_trades(&conn).expect_err("unreadable quantity");
        let (key, params) = error.keyed().expect("keyed validation error");
        assert_eq!(key, "validation.invalidAmount");
        assert_eq!(params["transactionId"], "tx-bad");
    }

    #[test]
    fn load_trades_reads_fee_and_day() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, company_name, quantity)
             VALUES ('inv', 'AAA', 'Alpha Inc.', '10')",
            [],
        )
        .expect("investment");
        // 2023-01-10 15:30 UTC with a fee, and 2024-01-10 midnight without one.
        for (id, kind, ts, fee) in [
            ("t1", "buy", D1 + 55_800, Some("1.5")),
            ("t2", "sell", D2, None),
        ] {
            conn.execute(
                "INSERT INTO investment_transactions
                   (id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                    currency, transaction_date, created_at, fee)
                 VALUES (?1, 'inv', ?2, 'AAA', 'Alpha Inc.', '10', '100', 'USD', ?3, ?3, ?4)",
                rusqlite::params![id, kind, ts, fee],
            )
            .expect("transaction");
        }
        let trades = load_trades(&conn).expect("trades");
        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].id, "t1");
        assert_eq!(trades[0].kind, TradeKind::Buy);
        assert_eq!(trades[0].day, D1);
        assert_eq!(trades[0].fee, 1.5);
        assert_eq!(trades[0].ticker, "AAA");
        assert_eq!(trades[0].company_name, "Alpha Inc.");
        assert_eq!(trades[0].quantity, 10.0);
        assert_eq!(trades[0].price, 100.0);
        assert_eq!(trades[1].kind, TradeKind::Sell);
        assert_eq!(trades[1].day, D2);
        assert_eq!(trades[1].fee, 0.0);
    }
}
