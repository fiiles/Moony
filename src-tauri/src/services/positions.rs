//! Stock and crypto positions valued in a requested report currency (ADR 0011).
//!
//! One service for the UI commands and the MCP tools. Native facts stay authoritative: the
//! current price comes from the price resolver in its own currency, the cost basis converts
//! every transaction at its own day (`cost_basis`), and only the report figures are converted.
//! A position whose figures cannot be converted stays listed with its native price; the
//! converted fields are `None` and the currency is reported in `Positions::quality` (never 0,
//! never 1:1).

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;
use crate::services::cost_basis::{self, PositionCostBasis, TxTable};
use crate::services::fx::{self, Quality, Quotes};
use crate::services::parsing::parse_money;
pub use crate::services::pricing::AssetKind;
use crate::services::pricing::{self, ResolvedPrice};
use crate::services::reporting::normalize_currency;

/// One position valued in the report currency.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionValue {
    pub investment_id: String,
    pub ticker: String,
    /// The quantity the transactions replay to (the stored holding quantity when the position
    /// has no transactions), so value and cost basis describe the same units.
    pub quantity: f64,
    /// Native current price (resolver), `None` when unpriced.
    pub price: Option<f64>,
    pub price_currency: Option<String>,
    /// Report currency, current snapshot.
    pub value: Option<f64>,
    /// Report currency, each transaction at its day (estimated flag in the quality). `None`
    /// when a buy cannot be converted, or for an open position without any transaction.
    pub cost_basis: Option<f64>,
    /// Native weighted average cost in `average_price_currency` (the primary currency).
    pub average_price: f64,
    pub average_price_currency: String,
    /// `value - cost_basis`.
    pub unrealized_gain: Option<f64>,
    /// `unrealized_gain / cost_basis * 100`; 0 for a zero cost basis.
    pub unrealized_gain_percent: Option<f64>,
    /// Report currency: sell proceeds at the sell day minus the released average cost.
    pub realized_gain: Option<f64>,
    /// Native yearly dividend per share; `None` without dividend data (and for crypto).
    pub dividend_per_share: Option<f64>,
    pub dividend_currency: Option<String>,
    /// Report currency, current snapshot. `Some(0)` for a stock without dividend data,
    /// `None` for crypto (no dividends) or when not convertible.
    pub yearly_dividend: Option<f64>,
    /// What this position could not convert (cost basis, value and dividend together).
    pub quality: Quality,
}

/// Every position of one asset class in `currency`, with what could not be converted.
#[derive(Debug, Clone)]
pub struct Positions {
    pub currency: String,
    pub items: Vec<PositionValue>,
    pub quality: Quality,
}

/// Sums over a group of positions in the report currency: every known amount counts.
///
/// `value`, `cost_basis` and `realized_gain` sum the positions whose amount is known;
/// `unrealized_gain` sums the positions whose value and cost are both known (their cost is
/// `unrealized_cost_basis`, the base of the percentage); `yearly_dividend` sums the known
/// dividends of positions whose value is known (a row left out of the value leaves its
/// dividend out too). An unknown amount is never counted as 0 and never converted 1:1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PositionTotals {
    pub value: f64,
    pub cost_basis: f64,
    pub unrealized_gain: f64,
    /// Cost basis of the positions in `unrealized_gain`.
    pub unrealized_cost_basis: f64,
    pub realized_gain: f64,
    pub yearly_dividend: f64,
    /// Positions in the group, summed or not.
    pub count: usize,
    /// Everything the group's positions could not convert.
    pub quality: Quality,
}

impl PositionTotals {
    fn add(&mut self, item: &PositionValue) {
        self.count += 1;
        self.quality.merge(&item.quality);
        if let Some(value) = item.value {
            self.value += value;
            self.yearly_dividend += item.yearly_dividend.unwrap_or(0.0);
        }
        if let Some(cost) = item.cost_basis {
            self.cost_basis += cost;
        }
        if let (Some(value), Some(cost)) = (item.value, item.cost_basis) {
            self.unrealized_gain += value - cost;
            self.unrealized_cost_basis += cost;
        }
        if let Some(realized) = item.realized_gain {
            self.realized_gain += realized;
        }
    }

    /// `unrealized_gain / unrealized_cost_basis * 100`; 0 for a zero cost basis.
    pub fn unrealized_gain_percent(&self) -> f64 {
        if self.unrealized_cost_basis > 0.0 {
            self.unrealized_gain / self.unrealized_cost_basis * 100.0
        } else {
            0.0
        }
    }
}

/// Totals over `items` (see `PositionTotals`).
pub fn totals<'a>(items: impl IntoIterator<Item = &'a PositionValue>) -> PositionTotals {
    let mut totals = PositionTotals::default();
    for item in items {
        totals.add(item);
    }
    totals
}

/// Totals per tag. `tag_map` lists the tag ids of each investment id; a position counts in
/// every tag it carries. Tags without a valued position are absent.
pub fn tag_totals(
    positions: &Positions,
    tag_map: &HashMap<String, Vec<String>>,
) -> HashMap<String, PositionTotals> {
    let mut by_tag: HashMap<String, PositionTotals> = HashMap::new();
    for item in &positions.items {
        for tag_id in tag_map.get(&item.investment_id).into_iter().flatten() {
            by_tag.entry(tag_id.clone()).or_default().add(item);
        }
    }
    by_tag
}

/// The tag ids of every tagged stock investment (`stock_investment_tags`), keyed by
/// investment id: the `tag_map` of `tag_totals`.
pub fn investment_tag_ids(conn: &Connection) -> Result<HashMap<String, Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT investment_id, tag_id FROM stock_investment_tags ORDER BY investment_id, tag_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let (investment_id, tag_id) = row?;
        map.entry(investment_id).or_default().push(tag_id);
    }
    Ok(map)
}

/// A stock's yearly dividend per share in its own currency.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDividend {
    pub per_share: f64,
    pub currency: String,
    /// The user's override (`true`) wins over fetched dividend data (`false`).
    pub is_manual: bool,
}

/// The yearly dividend per share of `ticker`: the user's override, else the fetched data.
pub fn resolve_dividend(conn: &Connection, ticker: &str) -> Result<Option<ResolvedDividend>> {
    for (sql, is_manual) in [
        (
            "SELECT yearly_dividend_sum, currency FROM dividend_overrides WHERE ticker = ?1",
            true,
        ),
        (
            "SELECT yearly_dividend_sum, currency FROM dividend_data WHERE ticker = ?1",
            false,
        ),
    ] {
        let row: Option<(String, String)> = conn
            .query_row(sql, [ticker], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        if let Some((sum, currency)) = row {
            return Ok(Some(ResolvedDividend {
                per_share: parse_money(&sum, 0.0, "yearly_dividend_sum"),
                currency,
                is_manual,
            }));
        }
    }
    Ok(None)
}

/// Values every position of `kind` in `report_currency` with the current quote snapshot.
pub fn value_positions(
    conn: &Connection,
    kind: AssetKind,
    report_currency: &str,
    today: i64,
) -> Result<Positions> {
    value_positions_with_quotes(conn, kind, report_currency, &fx::current(), today)
}

/// Like `value_positions`, with the request's quote snapshot passed in (C-A: one snapshot
/// per request when a caller values more than one asset class). Day quotes for the cost
/// basis are read up to `today`.
pub fn value_positions_with_quotes(
    conn: &Connection,
    kind: AssetKind,
    report_currency: &str,
    quotes: &Quotes,
    today: i64,
) -> Result<Positions> {
    let currency = normalize_currency(report_currency)?;
    let table = tx_table(kind);
    // The per-position cost qualities are merged below, only for positions that are held.
    let (costs, _) = cost_basis::cost_basis_as_of(conn, table, &currency, quotes, today)?;

    let mut items = Vec::new();
    let mut quality = Quality::default();
    for holding in load_holdings(conn, kind)? {
        let price = pricing::resolve_price(conn, kind, &holding.ticker);
        let dividend = match kind {
            AssetKind::Stock => Some(resolve_dividend(conn, &holding.ticker)?),
            AssetKind::Crypto => None,
        };
        let cost = costs.get(&holding.id);
        let item = value_position(holding, cost, price, dividend, quotes, &currency);
        quality.merge(&item.quality);
        items.push(item);
    }
    Ok(Positions {
        currency,
        items,
        quality,
    })
}

fn tx_table(kind: AssetKind) -> TxTable {
    match kind {
        AssetKind::Stock => TxTable::Stocks,
        AssetKind::Crypto => TxTable::Crypto,
    }
}

/// Native value of every priced position of `kind` (`quantity × price` in the price's own
/// currency), summed per currency. The quantity is the one `value_positions` values: the
/// replayed transaction quantity when the holding has transactions, else the stored one.
pub fn native_value_breakdown(conn: &Connection, kind: AssetKind) -> Result<HashMap<String, f64>> {
    let replayed = cost_basis::replayed_quantities(conn, tx_table(kind))?;
    let mut breakdown = HashMap::new();
    for holding in load_holdings(conn, kind)? {
        let quantity = replayed
            .get(&holding.id)
            .copied()
            .unwrap_or(holding.quantity);
        if let Some(price) = pricing::resolve_price(conn, kind, &holding.ticker) {
            *breakdown.entry(price.currency).or_insert(0.0) += quantity * price.price;
        }
    }
    Ok(breakdown)
}

/// One row of `stock_investments` / `crypto_investments`.
struct Holding {
    id: String,
    ticker: String,
    quantity: f64,
    /// The stored average-price currency, used when there are no transactions.
    currency: String,
}

fn load_holdings(conn: &Connection, kind: AssetKind) -> Result<Vec<Holding>> {
    let sql = match kind {
        AssetKind::Stock => {
            "SELECT id, ticker, quantity, currency FROM stock_investments ORDER BY ticker"
        }
        AssetKind::Crypto => {
            "SELECT id, ticker, quantity, currency FROM crypto_investments ORDER BY ticker"
        }
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        let quantity: String = row.get(2)?;
        Ok(Holding {
            id: row.get(0)?,
            ticker: row.get(1)?,
            quantity: parse_money(&quantity, 0.0, "position quantity"),
            currency: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Converts with the request's current quotes, recording failed conversions.
struct CurrentConverter<'a> {
    quotes: &'a Quotes,
    target: &'a str,
    quality: &'a mut Quality,
}

impl CurrentConverter<'_> {
    /// Zero is zero in every currency and needs no quote.
    fn convert(&mut self, amount: f64, from: &str) -> Option<f64> {
        if amount == 0.0 {
            return Some(0.0);
        }
        let result = self.quotes.convert(amount, from, self.target);
        self.quality
            .track_conversion(self.quotes, from, self.target, result)
    }
}

fn value_position(
    holding: Holding,
    cost: Option<&PositionCostBasis>,
    price: Option<ResolvedPrice>,
    dividend: Option<Option<ResolvedDividend>>,
    quotes: &Quotes,
    report_currency: &str,
) -> PositionValue {
    let mut quality = cost.map(|c| c.quality.clone()).unwrap_or_default();
    let mut converter = CurrentConverter {
        quotes,
        target: report_currency,
        quality: &mut quality,
    };
    // The replayed quantity values exactly the units the cost basis describes.
    let quantity = cost.map_or(holding.quantity, |c| c.quantity);
    let value = price
        .as_ref()
        .and_then(|p| converter.convert(quantity * p.price, &p.currency));
    // Without transactions nothing is known about the cost of an open position; an empty
    // position without transactions cost and realized nothing.
    let unknown_cost = || (quantity == 0.0).then_some(0.0);
    let cost_basis = cost.map_or_else(unknown_cost, PositionCostBasis::cost_basis_in_target);
    let realized_gain = cost.map_or_else(unknown_cost, |c| c.realized_gain_target);
    let unrealized_gain = value.zip(cost_basis).map(|(v, c)| v - c);
    let unrealized_gain_percent =
        unrealized_gain
            .zip(cost_basis)
            .map(|(gain, cost)| if cost > 0.0 { gain / cost * 100.0 } else { 0.0 });
    let (average_price, average_price_currency) = match cost {
        Some(c) => (c.average_price_native(), c.primary_currency.clone()),
        None => (0.0, holding.currency.to_uppercase()),
    };
    let (dividend_per_share, dividend_currency, yearly_dividend) = match dividend {
        Some(Some(d)) => {
            let yearly = converter.convert(quantity * d.per_share, &d.currency);
            (Some(d.per_share), Some(d.currency), yearly)
        }
        // A stock without dividend data pays nothing.
        Some(None) => (None, None, Some(0.0)),
        // Crypto has no dividends.
        None => (None, None, None),
    };
    PositionValue {
        investment_id: holding.id,
        ticker: holding.ticker,
        quantity,
        price: price.as_ref().map(|p| p.price),
        price_currency: price.map(|p| p.currency),
        value,
        cost_basis,
        average_price,
        average_price_currency,
        unrealized_gain,
        unrealized_gain_percent,
        realized_gain,
        dividend_per_share,
        dividend_currency,
        yearly_dividend,
        quality,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fx::DAY;

    fn setup() -> Connection {
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
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            , import_batch_id TEXT, external_id TEXT);
            CREATE TABLE stock_data (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                original_price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                price_date INTEGER NOT NULL,
                fetched_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE stock_price_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE dividend_data (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                yearly_dividend_sum TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'USD',
                last_fetched_at INTEGER NOT NULL DEFAULT (unixepoch()),
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE dividend_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                yearly_dividend_sum TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            CREATE TABLE crypto_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                coingecko_id TEXT NOT NULL,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL REFERENCES crypto_investments(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE crypto_prices (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL UNIQUE,
                coingecko_id TEXT,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                fetched_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE crypto_price_overrides (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL UNIQUE,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    /// `exchange_rate_history` rows: CZK per unit on `day`.
    fn seed_rates(conn: &Connection, day: i64, czk_per_unit: &[(&str, f64)]) {
        for (code, rate) in czk_per_unit {
            conn.execute(
                "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
                rusqlite::params![day, code, rate],
            )
            .expect("rate");
        }
    }

    /// A stock with its transactions `(type, quantity, price, day)` in `currency` and an API
    /// price of `current_price` in the same currency.
    fn seed_stock(
        conn: &Connection,
        id: &str,
        ticker: &str,
        currency: &str,
        txs: &[(&str, f64, f64, i64)],
        current_price: f64,
    ) {
        let quantity: f64 = txs
            .iter()
            .map(|(kind, qty, _, _)| if *kind == "buy" { *qty } else { -*qty })
            .sum();
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
             VALUES (?1, ?2, ?2, ?3, ?4)",
            rusqlite::params![id, ticker, quantity.to_string(), currency],
        )
        .expect("stock");
        for (n, (kind, qty, price, day)) in txs.iter().enumerate() {
            conn.execute(
                "INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name,
                    quantity, price_per_unit, currency, transaction_date, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    format!("{id}-{n}"),
                    id,
                    kind,
                    ticker,
                    qty.to_string(),
                    price.to_string(),
                    currency,
                    day,
                    n as i64
                ],
            )
            .expect("transaction");
        }
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at)
             VALUES (?1, ?2, ?3, ?4, 0, 100)",
            rusqlite::params![id, ticker, current_price.to_string(), currency],
        )
        .expect("price");
    }

    fn set_current_czk(czk_per_unit: &[(&str, f64)]) {
        fx::set_current_for_test(czk_per_unit);
    }

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn eur_investor_gain_on_usd_stock_ignores_czk_moves() {
        // C1: bought 10 x 100 USD when USDCZK = EURCZK = 25; today USDCZK 25, EURCZK 22.
        let conn = setup();
        let buy_day = 19_000 * DAY;
        seed_rates(&conn, buy_day, &[("USD", 25.0), ("EUR", 25.0)]);
        seed_stock(
            &conn,
            "s1",
            "AAPL",
            "USD",
            &[("buy", 10.0, 100.0, buy_day)],
            100.0,
        );
        set_current_czk(&[("USD", 25.0), ("EUR", 22.0)]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        let item = &p.items[0];
        assert_close(item.cost_basis.expect("cost"), 1000.0);
        assert_close(item.value.expect("value"), 1136.3636363636);
        assert_close(item.unrealized_gain.expect("gain"), 136.3636363636);
        assert!(p.quality.is_complete());
    }

    #[test]
    fn realized_gain_uses_sell_day_rate() {
        let conn = setup();
        let (d1, d2) = (19_000 * DAY, 19_100 * DAY);
        seed_rates(&conn, d1, &[("USD", 20.0)]);
        seed_rates(&conn, d2, &[("USD", 25.0)]);
        seed_stock(
            &conn,
            "s1",
            "AAPL",
            "USD",
            &[("buy", 10.0, 100.0, d1), ("sell", 5.0, 100.0, d2)],
            100.0,
        );
        set_current_czk(&[("USD", 25.0)]);
        let p = value_positions(&conn, AssetKind::Stock, "CZK", 20_000 * DAY).expect("positions");
        // released cost 5*100*20 = 10000 CZK, proceeds 5*100*25 = 12500 CZK
        assert_close(p.items[0].realized_gain.expect("realized"), 2500.0);
    }

    #[test]
    fn unconvertible_position_is_listed_and_reported() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "KWFH",
            "KWD",
            &[("buy", 1.0, 10.0, 19_000 * DAY)],
            10.0,
        );
        // EUR is quoted so that only the position's own currency is unconvertible (C-C blames
        // every unquoted side, the report currency included).
        set_current_czk(&[("USD", 25.0), ("EUR", 25.0)]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].value, None);
        assert_eq!(p.items[0].price, Some(10.0));
        assert_eq!(p.quality.missing_currencies(), vec!["KWD".to_string()]);
        let item = &p.items[0];
        assert_eq!(item.cost_basis, None);
        assert_eq!(item.unrealized_gain, None);
        assert_eq!(item.unrealized_gain_percent, None);
        assert_eq!(item.price_currency.as_deref(), Some("KWD"));
        assert_close(item.average_price, 10.0);
        assert_eq!(item.average_price_currency, "KWD");
        // one failed cost conversion (the buy) and one failed value conversion
        assert_eq!(p.quality.missing_count(), 2);
    }

    #[test]
    fn same_currency_needs_no_rates() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "SAP.DE",
            "EUR",
            &[("buy", 2.0, 100.0, 19_000 * DAY)],
            120.0,
        );
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        assert_close(p.items[0].unrealized_gain.expect("gain"), 40.0);
        assert_close(p.items[0].unrealized_gain_percent.expect("percent"), 20.0);
        assert_eq!(p.items[0].realized_gain, Some(0.0));
        assert!(p.quality.is_complete());
        assert!(!p.quality.is_estimated());
    }

    #[test]
    fn cost_basis_without_day_quotes_is_estimated_from_current_quotes() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "AAPL",
            "USD",
            &[("buy", 10.0, 100.0, 19_000 * DAY)],
            110.0,
        );
        set_current_czk(&[("USD", 25.0), ("EUR", 20.0)]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        assert_close(p.items[0].cost_basis.expect("cost"), 1250.0);
        assert_close(p.items[0].value.expect("value"), 1375.0);
        assert!(p.quality.is_estimated());
        assert!(p.quality.is_complete());
    }

    #[test]
    fn unpriced_position_is_listed_without_value() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "XYZ",
            "EUR",
            &[("buy", 2.0, 100.0, 19_000 * DAY)],
            1.0,
        );
        conn.execute("DELETE FROM stock_data", []).expect("delete");
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        let item = &p.items[0];
        assert_eq!(item.price, None);
        assert_eq!(item.price_currency, None);
        assert_eq!(item.value, None);
        assert_eq!(item.unrealized_gain, None);
        assert_close(item.cost_basis.expect("cost"), 200.0);
        // no conversion failed: an unpriced position is not an FX gap
        assert!(p.quality.is_complete());
    }

    #[test]
    fn manual_price_override_wins_until_a_newer_api_price() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "SAP.DE",
            "EUR",
            &[("buy", 1.0, 100.0, 19_000 * DAY)],
            120.0,
        );
        conn.execute(
            "INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
             VALUES ('o1', 'SAP.DE', '130', 'EUR', 200)",
            [],
        )
        .expect("override");
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        assert_eq!(p.items[0].price, Some(130.0));
        assert_close(p.items[0].value.expect("value"), 130.0);
    }

    #[test]
    fn yearly_dividend_converts_the_per_share_dividend_with_current_quotes() {
        let conn = setup();
        let day = 19_000 * DAY;
        seed_rates(&conn, day, &[("USD", 25.0), ("EUR", 25.0)]);
        seed_stock(
            &conn,
            "s1",
            "AAPL",
            "USD",
            &[("buy", 10.0, 100.0, day)],
            100.0,
        );
        conn.execute(
            "INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency)
             VALUES ('d1', 'AAPL', '1.0', 'USD')",
            [],
        )
        .expect("dividend");
        set_current_czk(&[("USD", 25.0), ("EUR", 20.0)]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        let item = &p.items[0];
        assert_eq!(item.dividend_per_share, Some(1.0));
        assert_eq!(item.dividend_currency.as_deref(), Some("USD"));
        assert_close(item.yearly_dividend.expect("dividend"), 12.5);

        // the user's override wins over the fetched dividend
        conn.execute(
            "INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency)
             VALUES ('o1', 'AAPL', '50', 'CZK')",
            [],
        )
        .expect("override");
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        assert_eq!(p.items[0].dividend_per_share, Some(50.0));
        assert_close(p.items[0].yearly_dividend.expect("dividend"), 25.0);
    }

    #[test]
    fn missing_dividend_is_zero_and_unconvertible_dividend_is_reported() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "SAP.DE",
            "EUR",
            &[("buy", 1.0, 100.0, 19_000 * DAY)],
            120.0,
        );
        set_current_czk(&[("EUR", 25.0)]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        assert_eq!(p.items[0].dividend_per_share, None);
        assert_eq!(p.items[0].yearly_dividend, Some(0.0));

        conn.execute(
            "INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency)
             VALUES ('d1', 'SAP.DE', '2', 'KWD')",
            [],
        )
        .expect("dividend");
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        assert_eq!(p.items[0].dividend_per_share, Some(2.0));
        assert_eq!(p.items[0].yearly_dividend, None);
        assert_eq!(p.quality.missing_currencies(), vec!["KWD".to_string()]);
        // the dividend gap does not hide the value
        assert_close(p.items[0].value.expect("value"), 120.0);
    }

    #[test]
    fn crypto_with_mixed_transaction_currencies_converts_each_buy_directly() {
        let conn = setup();
        let (d1, d2) = (19_000 * DAY, 19_050 * DAY);
        seed_rates(&conn, d1, &[("USD", 20.0), ("EUR", 25.0)]);
        seed_rates(&conn, d2, &[("USD", 25.0), ("EUR", 25.0)]);
        conn.execute_batch(&format!(
            r#"
            INSERT INTO crypto_investments (id, ticker, coingecko_id, name, quantity, currency)
                VALUES ('c1', 'BTC', 'bitcoin', 'Bitcoin', '2', 'CZK');
            INSERT INTO crypto_transactions (id, investment_id, type, ticker, name, quantity,
                price_per_unit, currency, transaction_date, created_at) VALUES
                ('t1', 'c1', 'buy', 'BTC', 'Bitcoin', '1', '500000', 'CZK', {d1}, 1),
                ('t2', 'c1', 'buy', 'BTC', 'Bitcoin', '1', '30000', 'USD', {d2}, 2);
            INSERT INTO crypto_prices (id, symbol, price, currency, fetched_at)
                VALUES ('p1', 'BTC', '40000', 'USD', 100);
            "#
        ))
        .expect("seed");
        set_current_czk(&[("USD", 25.0), ("EUR", 25.0)]);
        let p = value_positions(&conn, AssetKind::Crypto, "EUR", 20_000 * DAY).expect("positions");
        let item = &p.items[0];
        assert_eq!(item.ticker, "BTC");
        // 500 000 CZK / 25 + 30 000 USD * 25 / 25
        assert_close(item.cost_basis.expect("cost"), 50_000.0);
        assert_close(item.value.expect("value"), 80_000.0);
        assert_close(item.unrealized_gain.expect("gain"), 30_000.0);
        // CZK primary: 500 000 + 30 000 * 25 = 1 250 000 for 2 coins
        assert_eq!(item.average_price_currency, "CZK");
        assert_close(item.average_price, 625_000.0);
        assert_eq!(item.dividend_per_share, None);
        assert!(p.quality.is_complete());
    }

    #[test]
    fn tag_totals_sum_only_convertible_positions_and_report_the_rest() {
        let conn = setup();
        let day = 19_000 * DAY;
        seed_rates(&conn, day, &[("USD", 25.0), ("EUR", 25.0)]);
        seed_stock(
            &conn,
            "s1",
            "AAPL",
            "USD",
            &[("buy", 10.0, 100.0, day)],
            100.0,
        );
        seed_stock(&conn, "s2", "KWFH", "KWD", &[("buy", 1.0, 10.0, day)], 10.0);
        conn.execute(
            "INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency)
             VALUES ('d1', 'KWFH', '1', 'USD')",
            [],
        )
        .expect("dividend");
        set_current_czk(&[("USD", 25.0), ("EUR", 22.0)]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        let tag_map: HashMap<String, Vec<String>> = [
            ("s1".to_string(), vec!["tech".to_string()]),
            (
                "s2".to_string(),
                vec!["tech".to_string(), "gulf".to_string()],
            ),
        ]
        .into_iter()
        .collect();

        let by_tag = tag_totals(&p, &tag_map);

        let tech = &by_tag["tech"];
        assert_eq!(tech.count, 2);
        assert_close(tech.value, 1136.3636363636);
        assert_close(tech.cost_basis, 1000.0);
        assert_close(tech.unrealized_gain, 136.3636363636);
        assert_close(tech.unrealized_gain_percent(), 13.636363636);
        // the KWD position's (convertible) USD dividend stays out with its value
        assert_close(tech.yearly_dividend, 0.0);
        assert_eq!(tech.quality.missing_currencies(), vec!["KWD".to_string()]);
        let gulf = &by_tag["gulf"];
        assert_eq!(gulf.count, 1);
        assert_close(gulf.value, 0.0);
        assert_eq!(gulf.quality.missing_currencies(), vec!["KWD".to_string()]);
        assert!(!by_tag.contains_key("untagged"));
    }

    #[test]
    fn totals_keep_realized_gains_of_positions_without_a_value() {
        let conn = setup();
        let day = 19_000 * DAY;
        seed_stock(
            &conn,
            "s1",
            "SAP.DE",
            "EUR",
            &[("buy", 2.0, 100.0, day), ("sell", 2.0, 150.0, day + DAY)],
            120.0,
        );
        // closed and no longer priced
        conn.execute("DELETE FROM stock_data", []).expect("delete");
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");

        let t = totals(&p.items);

        assert_eq!(t.count, 1);
        assert_close(t.value, 0.0);
        assert_close(t.realized_gain, 100.0);
        assert!(t.quality.is_complete());
    }

    #[test]
    fn an_open_holding_without_transactions_has_no_known_cost() {
        let conn = setup();
        conn.execute_batch(
            r#"
            INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                VALUES ('s1', 'SAP.DE', 'SAP', '3', 'EUR'), ('s2', 'OLD.DE', 'Old', '0', 'EUR');
            INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at)
                VALUES ('p1', 'SAP.DE', '100', 'EUR', 0, 100);
            "#,
        )
        .expect("seed");
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        let open = p.items.iter().find(|i| i.ticker == "SAP.DE").expect("open");
        assert_close(open.value.expect("value"), 300.0);
        assert_eq!(open.cost_basis, None);
        assert_eq!(open.unrealized_gain, None);
        assert_eq!(open.unrealized_gain_percent, None);
        assert_eq!(open.realized_gain, None);
        let empty = p
            .items
            .iter()
            .find(|i| i.ticker == "OLD.DE")
            .expect("empty");
        assert_eq!(empty.cost_basis, Some(0.0));
        assert_eq!(empty.realized_gain, Some(0.0));
        // an unknown cost is not an FX gap
        assert!(p.quality.is_complete());
        // its known value still counts; its unknown cost and gain do not
        let t = totals(&p.items);
        assert_close(t.value, 300.0);
        assert_close(t.cost_basis, 0.0);
        assert_close(t.unrealized_gain, 0.0);
    }

    #[test]
    fn totals_sum_every_known_amount() {
        let conn = setup();
        let day = 19_000 * DAY;
        seed_stock(
            &conn,
            "s1",
            "SAP.DE",
            "EUR",
            &[("buy", 2.0, 100.0, day)],
            120.0,
        );
        seed_stock(
            &conn,
            "s2",
            "XYZ.DE",
            "EUR",
            &[("buy", 1.0, 50.0, day)],
            1.0,
        );
        // the second position has no price
        conn.execute("DELETE FROM stock_data WHERE ticker = 'XYZ.DE'", [])
            .expect("unprice");
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");

        let t = totals(&p.items);

        assert_eq!(t.count, 2);
        assert_close(t.cost_basis, 250.0);
        assert_close(t.value, 240.0);
        assert_close(t.unrealized_gain, 40.0);
        assert_close(t.unrealized_cost_basis, 200.0);
        assert_close(t.unrealized_gain_percent(), 20.0);
        assert!(t.quality.is_complete());
    }

    #[test]
    fn value_uses_the_replayed_quantity_of_the_cost_basis() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "SAP.DE",
            "EUR",
            &[("buy", 2.0, 100.0, 19_000 * DAY)],
            120.0,
        );
        // a stale stored quantity (metrics not recalculated)
        conn.execute("UPDATE stock_investments SET quantity = '5'", [])
            .expect("stale");
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        let item = &p.items[0];
        assert_close(item.quantity, 2.0);
        assert_close(item.value.expect("value"), 240.0);
        assert_close(item.unrealized_gain.expect("gain"), 40.0);
    }

    #[test]
    fn native_breakdown_values_the_same_quantity_as_positions() {
        let conn = setup();
        seed_stock(
            &conn,
            "s1",
            "SAP.DE",
            "EUR",
            &[("buy", 2.0, 100.0, 19_000 * DAY)],
            120.0,
        );
        seed_stock(&conn, "s2", "NOTX", "USD", &[], 10.0);
        conn.execute(
            "UPDATE stock_investments SET quantity = '3' WHERE id = 's2'",
            [],
        )
        .expect("stored quantity");
        // A stale stored quantity (metrics not recalculated): positions value the replay
        conn.execute(
            "UPDATE stock_investments SET quantity = '5' WHERE id = 's1'",
            [],
        )
        .expect("stale");

        let breakdown = native_value_breakdown(&conn, AssetKind::Stock).expect("breakdown");

        // SAP: replayed 2 × 120 EUR; NOTX without transactions: stored 3 × 10 USD.
        assert_close(breakdown["EUR"], 240.0);
        assert_close(breakdown["USD"], 30.0);
        set_current_czk(&[]);
        let p = value_positions(&conn, AssetKind::Stock, "EUR", 20_000 * DAY).expect("positions");
        let sap = p
            .items
            .iter()
            .find(|i| i.investment_id == "s1")
            .expect("s1");
        assert_close(sap.value.expect("value"), breakdown["EUR"]);
    }

    #[test]
    fn invalid_report_currency_is_rejected() {
        let conn = setup();
        assert!(value_positions(&conn, AssetKind::Stock, "euro", 20_000 * DAY).is_err());
    }
}
