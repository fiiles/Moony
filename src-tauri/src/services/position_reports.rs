//! Wire responses of the stock, crypto and stocks-analysis commands, built from the positions
//! service (ADR 0011). Every response captures one quote snapshot, declares its report currency
//! and reports what it could not convert in `fxQuality`. Converted amounts are decimal TEXT at
//! the report currency's minor unit, `null` when unknown; native prices and dividends keep their
//! own currency.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::{
    CryptoPositions, EnrichedCryptoInvestment, EnrichedStockInvestment,
    PositionTotals as WirePositionTotals, StockInvestmentWithTags, StockPosition, StockPositions,
    StockTag, StocksAnalysis,
};
use crate::services::positions::{
    self, resolve_dividend, AssetKind, PositionTotals, PositionValue,
};
use crate::services::pricing;
use crate::services::reporting::money_in_currency;

/// Unix seconds now: the day up to which cost-basis day quotes are read.
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn money(value: Option<f64>, currency: &str) -> Option<String> {
    value.map(|v| money_in_currency(v, currency))
}

fn wire_totals(totals: &PositionTotals, currency: &str) -> WirePositionTotals {
    WirePositionTotals {
        value: money(totals.value, currency),
        cost_basis: money(totals.cost_basis, currency),
        unrealized_gain: money(totals.unrealized_gain, currency),
        unrealized_gain_percent: totals.unrealized_gain_percent(),
        realized_gain: money(totals.realized_gain, currency),
        yearly_dividend: money(totals.yearly_dividend, currency),
    }
}

/// The wire quantity of a position: the quantity its value describes (`PositionValue.quantity`,
/// replayed from the transactions). The stored text is kept when it holds that same quantity,
/// so its precision survives (no float noise from the replay).
pub(crate) fn wire_quantity(stored: &str, valued: f64) -> String {
    let same = stored
        .trim()
        .parse::<f64>()
        .is_ok_and(|q| (q - valued).abs() <= 1e-9 * valued.abs().max(1.0));
    if same {
        stored.to_string()
    } else {
        valued.to_string()
    }
}

/// `(company_name, stored quantity)` of every stock holding, keyed by id.
fn stock_names(conn: &Connection) -> Result<HashMap<String, (String, String)>> {
    let mut stmt = conn.prepare("SELECT id, company_name, quantity FROM stock_investments")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The trimmed, non-empty sector of every ticker in the cached quote data (`stock_data`).
fn stock_sectors(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT ticker, sector FROM stock_data WHERE sector IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut sectors = HashMap::new();
    for row in rows {
        let (ticker, sector) = row?;
        let sector = sector.trim();
        if !sector.is_empty() {
            sectors.insert(ticker, sector.to_string());
        }
    }
    Ok(sectors)
}

fn enrich_stock(
    conn: &Connection,
    item: &PositionValue,
    company_name: String,
    quantity: String,
    currency: &str,
) -> Result<EnrichedStockInvestment> {
    let price = pricing::resolve_stock_price(conn, &item.ticker);
    let dividend = resolve_dividend(conn, &item.ticker)?;
    Ok(EnrichedStockInvestment {
        id: item.investment_id.clone(),
        ticker: item.ticker.clone(),
        company_name,
        quantity,
        average_price: item.average_price.to_string(),
        average_price_currency: item.average_price_currency.clone(),
        original_price: price
            .as_ref()
            .map_or_else(|| "0".to_string(), |p| p.original_price.clone()),
        currency: price.as_ref().map_or_else(
            || item.average_price_currency.clone(),
            |p| p.currency.clone(),
        ),
        fetched_at: price.as_ref().and_then(|p| p.fetched_at),
        is_manual_price: price.as_ref().is_some_and(|p| p.is_manual),
        original_dividend_yield: dividend.as_ref().map_or(0.0, |d| d.per_share),
        dividend_currency: item.dividend_currency.clone(),
        is_manual_dividend: dividend.as_ref().is_some_and(|d| d.is_manual),
        price: price.as_ref().map(|p| p.original_price.clone()),
        price_currency: item.price_currency.clone(),
        value: money(item.value, currency),
        cost_basis: money(item.cost_basis, currency),
        unrealized_gain: money(item.unrealized_gain, currency),
        unrealized_gain_percent: item.unrealized_gain_percent,
        realized_gain: money(item.realized_gain, currency),
        dividend_per_share: item.dividend_per_share.map(|d| d.to_string()),
        yearly_dividend: money(item.yearly_dividend, currency),
    })
}

/// Every stock position in `report_currency`.
pub fn stock_positions(conn: &Connection, report_currency: &str) -> Result<StockPositions> {
    let positions = positions::value_positions(conn, AssetKind::Stock, report_currency, now())?;
    let mut names = stock_names(conn)?;
    let currency = positions.currency.clone();
    let mut items = Vec::with_capacity(positions.items.len());
    for item in &positions.items {
        let (name, stored) = names.remove(&item.investment_id).unwrap_or_default();
        let quantity = wire_quantity(&stored, item.quantity);
        items.push(enrich_stock(conn, item, name, quantity, &currency)?);
    }
    Ok(StockPositions {
        fx_quality: ConversionQuality::from(&positions.quality),
        totals: wire_totals(&positions::totals(&positions.items), &currency),
        items,
        currency,
    })
}

/// One stock position in `report_currency`; `fxQuality` covers this position only.
pub fn stock_position(conn: &Connection, id: &str, report_currency: &str) -> Result<StockPosition> {
    let positions = positions::value_positions(conn, AssetKind::Stock, report_currency, now())?;
    let item = positions
        .items
        .iter()
        .find(|item| item.investment_id == id)
        .ok_or_else(|| AppError::NotFound("Investment not found".into()))?;
    let (name, stored) = stock_names(conn)?.remove(id).unwrap_or_default();
    let quantity = wire_quantity(&stored, item.quantity);
    Ok(StockPosition {
        currency: positions.currency.clone(),
        fx_quality: ConversionQuality::from(&item.quality),
        investment: enrich_stock(conn, item, name, quantity, &positions.currency)?,
    })
}

/// Every crypto position in `report_currency`.
pub fn crypto_positions(conn: &Connection, report_currency: &str) -> Result<CryptoPositions> {
    let positions = positions::value_positions(conn, AssetKind::Crypto, report_currency, now())?;
    let mut meta: HashMap<String, (String, String, String)> = {
        let mut stmt =
            conn.prepare("SELECT id, coingecko_id, name, quantity FROM crypto_investments")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    (
                        row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        row.get(2)?,
                        row.get(3)?,
                    ),
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    let currency = positions.currency.clone();
    let items = positions
        .items
        .iter()
        .map(|item| {
            let (coingecko_id, name, stored) = meta.remove(&item.investment_id).unwrap_or_default();
            let quantity = wire_quantity(&stored, item.quantity);
            let price = pricing::resolve_crypto_price(conn, &item.ticker);
            EnrichedCryptoInvestment {
                id: item.investment_id.clone(),
                ticker: item.ticker.clone(),
                coingecko_id,
                name,
                quantity,
                average_price: item.average_price.to_string(),
                average_price_currency: item.average_price_currency.clone(),
                original_price: price
                    .as_ref()
                    .map_or_else(|| "0".to_string(), |p| p.original_price.clone()),
                currency: price.as_ref().map_or_else(
                    || item.average_price_currency.clone(),
                    |p| p.currency.clone(),
                ),
                fetched_at: price.as_ref().and_then(|p| p.fetched_at),
                is_manual_price: price.as_ref().is_some_and(|p| p.is_manual),
                price: price.as_ref().map(|p| p.original_price.clone()),
                price_currency: item.price_currency.clone(),
                value: money(item.value, &currency),
                cost_basis: money(item.cost_basis, &currency),
                unrealized_gain: money(item.unrealized_gain, &currency),
                unrealized_gain_percent: item.unrealized_gain_percent,
                realized_gain: money(item.realized_gain, &currency),
            }
        })
        .collect();
    Ok(CryptoPositions {
        fx_quality: ConversionQuality::from(&positions.quality),
        totals: wire_totals(&positions::totals(&positions.items), &currency),
        items,
        currency,
    })
}

fn read_tag(row: &rusqlite::Row, offset: usize) -> rusqlite::Result<StockTag> {
    Ok(StockTag {
        id: row.get(offset)?,
        name: row.get(offset + 1)?,
        color: row.get(offset + 2)?,
        group_id: row.get(offset + 3)?,
        created_at: row.get(offset + 4)?,
    })
}

/// The tags of every tagged stock investment, keyed by investment id, by tag name.
fn tags_by_investment(conn: &Connection) -> Result<HashMap<String, Vec<StockTag>>> {
    let mut stmt = conn.prepare(
        "SELECT sit.investment_id, t.id, t.name, t.color, t.group_id, t.created_at
         FROM stock_tags t
         INNER JOIN stock_investment_tags sit ON t.id = sit.tag_id
         ORDER BY t.name",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, read_tag(row, 1)?)))?;
    let mut map: HashMap<String, Vec<StockTag>> = HashMap::new();
    for row in rows {
        let (investment_id, tag) = row?;
        map.entry(investment_id).or_default().push(tag);
    }
    Ok(map)
}

/// Every stock position with its tags in `report_currency`, by company name.
pub fn stocks_analysis(conn: &Connection, report_currency: &str) -> Result<StocksAnalysis> {
    let positions = positions::value_positions(conn, AssetKind::Stock, report_currency, now())?;
    let mut names = stock_names(conn)?;
    let mut tags = tags_by_investment(conn)?;
    let sectors = stock_sectors(conn)?;
    let currency = positions.currency.clone();
    let mut items: Vec<StockInvestmentWithTags> = positions
        .items
        .iter()
        .map(|item| {
            let (company_name, stored) = names.remove(&item.investment_id).unwrap_or_default();
            let quantity = wire_quantity(&stored, item.quantity);
            StockInvestmentWithTags {
                id: item.investment_id.clone(),
                ticker: item.ticker.clone(),
                company_name,
                quantity,
                average_price: item.average_price.to_string(),
                average_price_currency: item.average_price_currency.clone(),
                price: item.price.map(|p| p.to_string()),
                price_currency: item.price_currency.clone(),
                value: money(item.value, &currency),
                cost_basis: money(item.cost_basis, &currency),
                unrealized_gain: money(item.unrealized_gain, &currency),
                unrealized_gain_percent: item.unrealized_gain_percent,
                yearly_dividend: money(item.yearly_dividend, &currency),
                sector: sectors.get(&item.ticker).cloned(),
                tags: tags.remove(&item.investment_id).unwrap_or_default(),
            }
        })
        .collect();
    items.sort_by(|a, b| a.company_name.cmp(&b.company_name));
    Ok(StocksAnalysis {
        currency,
        fx_quality: ConversionQuality::from(&positions.quality),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fx::Quality;

    #[test]
    fn wire_quantity_is_the_valued_quantity() {
        // The stored text survives when it holds the valued quantity
        assert_eq!(wire_quantity("0.30000000", 0.1 + 0.2), "0.30000000");
        assert_eq!(wire_quantity("2", 2.0), "2");
        // A stale stored quantity gives way to the replayed one
        assert_eq!(wire_quantity("5", 2.0), "2");
        assert_eq!(wire_quantity("", 1.5), "1.5");
    }

    /// A minimal database for `stocks_analysis`: three CZK stocks held without transactions
    /// (so no rates are needed); AAPL has a padded sector, ETF1 an empty one and NOQUOTE no
    /// `stock_data` row at all.
    fn setup_analysis_db() -> Connection {
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
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                import_batch_id TEXT,
                external_id TEXT
            );
            CREATE TABLE stock_data (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                original_price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                price_date INTEGER NOT NULL,
                fetched_at INTEGER NOT NULL DEFAULT (unixepoch()),
                sector TEXT
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
            CREATE TABLE stock_tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                color TEXT,
                created_at INTEGER NOT NULL DEFAULT 0,
                group_id TEXT
            );
            CREATE TABLE stock_investment_tags (
                investment_id TEXT NOT NULL,
                tag_id TEXT NOT NULL,
                PRIMARY KEY (investment_id, tag_id)
            );
            "#,
        )
        .expect("schema");
        for (id, ticker, sector) in [
            ("i-aapl", "AAPL", Some(" Technology ")),
            ("i-etf", "ETF1", Some("   ")),
            ("i-none", "NOQUOTE", None),
        ] {
            conn.execute(
                "INSERT INTO stock_investments (id, ticker, company_name, quantity)
                 VALUES (?1, ?2, ?2, '1')",
                rusqlite::params![id, ticker],
            )
            .expect("investment");
            if id != "i-none" {
                conn.execute(
                    "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, sector)
                     VALUES (?1, ?2, '100', 'CZK', 0, ?3)",
                    rusqlite::params![id, ticker, sector],
                )
                .expect("quote");
            }
        }
        conn
    }

    #[test]
    fn analysis_items_carry_the_trimmed_sector() {
        let conn = setup_analysis_db();

        let analysis = stocks_analysis(&conn, "CZK").expect("analysis");

        let sector_of = |ticker: &str| {
            analysis
                .items
                .iter()
                .find(|item| item.ticker == ticker)
                .unwrap_or_else(|| panic!("{ticker} missing"))
                .sector
                .clone()
        };
        assert_eq!(sector_of("AAPL").as_deref(), Some("Technology"));
        // blank sector (an ETF) and a missing quote row both report no sector
        assert_eq!(sector_of("ETF1"), None);
        assert_eq!(sector_of("NOQUOTE"), None);
    }

    fn item(id: &str, value: Option<f64>, cost: Option<f64>, dividend: f64) -> PositionValue {
        let mut quality = Quality::default();
        if value.is_none() {
            quality.track("KWD", None);
        }
        PositionValue {
            investment_id: id.to_string(),
            ticker: id.to_uppercase(),
            quantity: 1.0,
            price: Some(1.0),
            price_currency: Some("EUR".to_string()),
            value,
            cost_basis: cost,
            average_price: 0.0,
            average_price_currency: "EUR".to_string(),
            unrealized_gain: value.zip(cost).map(|(v, c)| v - c),
            unrealized_gain_percent: None,
            realized_gain: Some(0.0),
            dividend_per_share: None,
            dividend_currency: None,
            yearly_dividend: Some(dividend),
            quality,
            has_cost_record: cost.is_some(),
        }
    }

    #[test]
    fn totals_are_text_in_the_report_currency_and_null_when_nothing_converted() {
        // Transactions exist, but neither the value nor the cost of one position converted
        let mut failed = item("c", None, None, 0.0);
        failed.has_cost_record = true;
        let all = wire_totals(
            &positions::totals(&[item("a", Some(300.0), Some(200.0), 6.0), failed.clone()]),
            "EUR",
        );
        // the portfolio still knows its converted position
        assert_eq!(all.value.as_deref(), Some("300.00"));
        assert_eq!(all.cost_basis.as_deref(), Some("200.00"));

        // a priced position whose value and cost failed: unknown, not 0
        let lost = wire_totals(&positions::totals(&[failed]), "EUR");
        assert_eq!(lost.value, None);
        assert_eq!(lost.cost_basis, None);
        assert_eq!(lost.unrealized_gain, None);
        assert_eq!(lost.yearly_dividend.as_deref(), Some("0.00"));
    }
}
