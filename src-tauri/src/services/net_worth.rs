//! Net worth and its history in a report currency (ADR 0011), for the UI commands and the MCP
//! tools alike.
//!
//! Native per-currency breakdowns are the facts. The current metrics convert today's
//! breakdowns with one quote snapshot; the history converts each stored row's breakdowns at
//! that row's day (common-day rule, `fx::History::convert_on`). An amount that cannot be
//! converted is left out of every sum and recorded in the response's quality; a class is
//! `None` only when nothing in it converts. The snapshot writer (`portfolio_history`) still
//! stores CZK totals next to the breakdowns; they are read here only for legacy rows.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::models::fx::ConversionQuality;
use crate::models::{PortfolioHistoryPoint, PortfolioHistoryResponse, PortfolioMetrics};
use crate::services::currency::breakdown_from_json;
use crate::services::fx::{self, History, Quality, Quotes};
use crate::services::loan_amortization::today_utc_day;
use crate::services::parsing::parse_money;
use crate::services::portfolio_history::ClassBreakdowns;
use crate::services::reporting::{money_in_currency, normalize_currency};

/// Every asset class and the loans in the report currency; `None` when nothing in the class
/// could be converted.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClassTotals {
    pub savings: Option<f64>,
    pub investments: Option<f64>,
    pub crypto: Option<f64>,
    pub bonds: Option<f64>,
    pub real_estate_personal: Option<f64>,
    pub real_estate_investment: Option<f64>,
    pub other_assets: Option<f64>,
    pub loans: Option<f64>,
}

/// Today's net worth in `currency`. The totals sum every class that converted (personal real
/// estate left out when excluded); `quality` names what did not.
#[derive(Debug, Clone)]
pub struct NetWorth {
    pub currency: String,
    pub classes: ClassTotals,
    pub total_assets: f64,
    pub total_liabilities: f64,
    pub net_worth: f64,
    pub quality: Quality,
    /// The native breakdowns the classes were converted from (real estate split).
    pub breakdowns: ClassBreakdowns,
}

/// One stored snapshot day in the report currency.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryPoint {
    pub recorded_at: i64,
    pub source: String,
    pub is_stale: bool,
    pub classes: ClassTotals,
    /// Every converted asset class (personal real estate included) minus the converted loans;
    /// `None` when no class converted at all.
    pub net_worth: Option<f64>,
}

/// Today's native breakdowns of every class from the live tables (real estate split into
/// personal and investment). Shared by the current metrics and the snapshot writer.
pub fn compute_breakdowns(conn: &Connection) -> Result<ClassBreakdowns> {
    let mut breakdowns = ClassBreakdowns::default();

    let mut stmt =
        conn.prepare("SELECT balance, currency FROM bank_accounts WHERE exclude_from_balance = 0")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for (balance, currency) in rows.filter_map(|r| r.ok()) {
        let balance = parse_money(&balance, 0.0, "bank_accounts.balance");
        *breakdowns.savings.entry(currency).or_insert(0.0) += balance;
    }

    let mut stmt = conn.prepare("SELECT coupon_value, quantity, currency FROM bonds")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for (value, quantity, currency) in rows.filter_map(|r| r.ok()) {
        let value = parse_money(&value, 0.0, "bonds.coupon_value");
        let quantity = parse_money(&quantity, 1.0, "bonds.quantity");
        *breakdowns.bonds.entry(currency).or_insert(0.0) += value * quantity;
    }

    // Liabilities: the amortized balance as of today
    breakdowns.loans = crate::services::loans::outstanding_by_currency(conn, today_utc_day())?;

    let mut stmt =
        conn.prepare("SELECT type, market_price, market_price_currency FROM real_estate")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for (kind, price, currency) in rows.filter_map(|r| r.ok()) {
        let price = parse_money(&price, 0.0, "real_estate.market_price");
        let class = if kind == "personal" {
            &mut breakdowns.real_estate_personal
        } else {
            &mut breakdowns.real_estate_investment
        };
        *class.entry(currency).or_insert(0.0) += price;
    }

    let mut stmt = conn.prepare("SELECT ticker, quantity FROM stock_investments")?;
    let stocks: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();
    for (ticker, quantity) in stocks {
        let quantity = parse_money(&quantity, 0.0, "stock_investments.quantity");
        if let Some(resolved) = crate::services::pricing::resolve_stock_price(conn, &ticker) {
            let price = parse_money(
                &resolved.original_price,
                0.0,
                "resolved stock price (original_price)",
            );
            *breakdowns
                .investments
                .entry(resolved.currency)
                .or_insert(0.0) += price * quantity;
        }
    }

    let mut stmt = conn.prepare("SELECT ticker, quantity FROM crypto_investments")?;
    let coins: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();
    for (ticker, quantity) in coins {
        let quantity = parse_money(&quantity, 0.0, "crypto_investments.quantity");
        if let Some(resolved) = crate::services::pricing::resolve_crypto_price(conn, &ticker) {
            let price = parse_money(
                &resolved.original_price,
                0.0,
                "resolved crypto price (original_price)",
            );
            *breakdowns.crypto.entry(resolved.currency).or_insert(0.0) += price * quantity;
        }
    }

    let mut stmt = conn.prepare("SELECT quantity, market_price, currency FROM other_assets")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for (quantity, price, currency) in rows.filter_map(|r| r.ok()) {
        let quantity = parse_money(&quantity, 0.0, "other_assets.quantity");
        let price = parse_money(&price, 0.0, "other_assets.market_price");
        *breakdowns.other_assets.entry(currency).or_insert(0.0) += quantity * price;
    }

    Ok(breakdowns)
}

/// Sum of a native breakdown, each non-zero `(currency, amount)` through `convert` (which
/// records its own misses). Zero amounts need no conversion. `Some(0.0)` for nothing to
/// convert; `None` only when every non-zero amount failed.
fn sum_converted(
    breakdown: &HashMap<String, f64>,
    mut convert: impl FnMut(f64, &str) -> Option<f64>,
) -> Option<f64> {
    let mut entries: Vec<(&String, f64)> = breakdown
        .iter()
        .map(|(code, amount)| (code, *amount))
        .filter(|(_, amount)| *amount != 0.0)
        .collect();
    if entries.is_empty() {
        return Some(0.0);
    }
    // A fixed order keeps the float sum stable between calls
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .into_iter()
        .filter_map(|(code, amount)| convert(amount, code))
        .fold(None, |total, value| Some(total.unwrap_or(0.0) + value))
}

/// A native breakdown in `target` at the given quotes; misses go to `quality`.
pub fn sum_breakdown(
    breakdown: &HashMap<String, f64>,
    quotes: &Quotes,
    target: &str,
    quality: &mut Quality,
) -> Option<f64> {
    sum_converted(breakdown, |amount, code| {
        quality.track_conversion(quotes, code, target, quotes.convert(amount, code, target))
    })
}

/// The known values summed; `None` when none is known.
fn sum_known(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    values
        .into_iter()
        .flatten()
        .fold(None, |total, value| Some(total.unwrap_or(0.0) + value))
}

/// Today's net worth in `report_currency` at the current quote snapshot.
pub fn current_net_worth(
    conn: &Connection,
    report_currency: &str,
    exclude_personal_real_estate: bool,
) -> Result<NetWorth> {
    let currency = normalize_currency(report_currency)?;
    let breakdowns = compute_breakdowns(conn)?;
    Ok(net_worth_of(
        breakdowns,
        &fx::current(),
        &currency,
        exclude_personal_real_estate,
    ))
}

fn net_worth_of(
    breakdowns: ClassBreakdowns,
    quotes: &Quotes,
    currency: &str,
    exclude_personal_real_estate: bool,
) -> NetWorth {
    let mut quality = Quality::default();
    let mut sum =
        |breakdown: &HashMap<String, f64>| sum_breakdown(breakdown, quotes, currency, &mut quality);
    let classes = ClassTotals {
        savings: sum(&breakdowns.savings),
        investments: sum(&breakdowns.investments),
        crypto: sum(&breakdowns.crypto),
        bonds: sum(&breakdowns.bonds),
        real_estate_personal: sum(&breakdowns.real_estate_personal),
        real_estate_investment: sum(&breakdowns.real_estate_investment),
        other_assets: sum(&breakdowns.other_assets),
        loans: sum(&breakdowns.loans),
    };
    let total_assets = counted_real_estate(&classes, exclude_personal_real_estate)
        .into_iter()
        .chain([
            classes.savings,
            classes.investments,
            classes.crypto,
            classes.bonds,
            classes.other_assets,
        ])
        .flatten()
        .sum::<f64>();
    let total_liabilities = classes.loans.unwrap_or(0.0);
    NetWorth {
        currency: currency.to_string(),
        classes,
        total_assets,
        total_liabilities,
        net_worth: total_assets - total_liabilities,
        quality,
        breakdowns,
    }
}

/// The real estate classes that count into the totals.
fn counted_real_estate(classes: &ClassTotals, exclude_personal: bool) -> [Option<f64>; 2] {
    let personal = if exclude_personal {
        None
    } else {
        classes.real_estate_personal
    };
    [personal, classes.real_estate_investment]
}

/// The current metrics as the wire response.
pub fn metrics_report(
    conn: &Connection,
    report_currency: &str,
    exclude_personal_real_estate: bool,
) -> Result<PortfolioMetrics> {
    let worth = current_net_worth(conn, report_currency, exclude_personal_real_estate)?;
    let currency = worth.currency.as_str();
    let text = |value: Option<f64>| value.map(|v| money_in_currency(v, currency));
    let classes = worth.classes;
    let breakdowns = &worth.breakdowns;

    let mut real_estate_by_currency = breakdowns.real_estate_investment.clone();
    if !exclude_personal_real_estate {
        for (code, amount) in &breakdowns.real_estate_personal {
            *real_estate_by_currency.entry(code.clone()).or_insert(0.0) += amount;
        }
    }

    Ok(PortfolioMetrics {
        currency: currency.to_string(),
        fx_quality: ConversionQuality::from(&worth.quality),
        total_savings: text(classes.savings),
        total_investments: text(classes.investments),
        total_crypto: text(classes.crypto),
        total_bonds: text(classes.bonds),
        total_real_estate_personal: text(classes.real_estate_personal),
        total_real_estate_investment: text(classes.real_estate_investment),
        total_real_estate: text(sum_known(counted_real_estate(
            &classes,
            exclude_personal_real_estate,
        ))),
        total_other_assets: text(classes.other_assets),
        total_liabilities: money_in_currency(worth.total_liabilities, currency),
        total_assets: money_in_currency(worth.total_assets, currency),
        net_worth: money_in_currency(worth.net_worth, currency),
        savings_by_currency: breakdowns.savings.clone(),
        investments_by_currency: breakdowns.investments.clone(),
        crypto_by_currency: breakdowns.crypto.clone(),
        bonds_by_currency: breakdowns.bonds.clone(),
        real_estate_by_currency,
        loans_by_currency: breakdowns.loans.clone(),
        other_assets_by_currency: breakdowns.other_assets.clone(),
    })
}

/// One `portfolio_metrics_history` row as stored: native breakdown JSON per class plus the
/// CZK totals (read only for legacy rows and the real estate split).
struct StoredRow {
    recorded_at: i64,
    source: String,
    is_stale: bool,
    /// Savings, loans, investments, crypto, bonds, other assets: `(breakdown JSON, CZK total)`.
    classes: [(String, f64); 6],
    real_estate: String,
    real_estate_personal_czk: f64,
    real_estate_investment_czk: f64,
}

fn stored_rows(conn: &Connection, from: Option<i64>, to: Option<i64>) -> Result<Vec<StoredRow>> {
    let czk = |column: &str| format!("COALESCE(CAST({column} AS REAL), 0)");
    let sql = format!(
        "SELECT recorded_at, source, is_stale,
                savings_by_currency, {}, loans_by_currency, {}, investments_by_currency, {},
                crypto_by_currency, {}, bonds_by_currency, {}, other_assets_by_currency, {},
                real_estate_by_currency, {}, {}
         FROM portfolio_metrics_history
         WHERE (?1 IS NULL OR recorded_at >= ?1) AND (?2 IS NULL OR recorded_at <= ?2)
         ORDER BY recorded_at ASC",
        czk("total_savings"),
        czk("total_loans_principal"),
        czk("total_investments"),
        czk("total_crypto"),
        czk("total_bonds"),
        czk("total_other_assets"),
        czk("total_real_estate_personal"),
        czk("total_real_estate_investment"),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from, to], |row| {
        let class =
            |i: usize| -> rusqlite::Result<(String, f64)> { Ok((row.get(i)?, row.get(i + 1)?)) };
        Ok(StoredRow {
            recorded_at: row.get(0)?,
            source: row.get(1)?,
            is_stale: row.get::<_, i64>(2)? != 0,
            classes: [
                class(3)?,
                class(5)?,
                class(7)?,
                class(9)?,
                class(11)?,
                class(13)?,
            ],
            real_estate: row.get(15)?,
            real_estate_personal_czk: row.get(16)?,
            real_estate_investment_czk: row.get(17)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A class of a stored row: its native breakdown, or — for a legacy row whose breakdown is
/// empty while its CZK total is not — that CZK total (phase-2 compatibility; Task 24 removes it).
fn stored_class(
    breakdown: &HashMap<String, f64>,
    czk_total: f64,
    convert: &mut impl FnMut(f64, &str) -> Option<f64>,
) -> Option<f64> {
    if breakdown.is_empty() && czk_total != 0.0 {
        return convert(czk_total, "CZK");
    }
    sum_converted(breakdown, convert)
}

/// Personal and investment real estate of a stored row. The stored breakdown combines both;
/// the split survives only in the row's CZK totals, so the converted combined value is
/// prorated by `personal / (personal + investment)` of those totals. Without a usable ratio
/// (both totals zero, e.g. no CZK quote when the row was written) everything counts as
/// investment. Task 24 stores the personal breakdown natively and drops this.
fn stored_real_estate(
    row: &StoredRow,
    convert: &mut impl FnMut(f64, &str) -> Option<f64>,
) -> (Option<f64>, Option<f64>) {
    let (personal_czk, investment_czk) =
        (row.real_estate_personal_czk, row.real_estate_investment_czk);
    let combined = breakdown_from_json(&row.real_estate);
    if combined.is_empty() {
        let empty = HashMap::new();
        return (
            stored_class(&empty, personal_czk, convert),
            stored_class(&empty, investment_czk, convert),
        );
    }
    let Some(value) = sum_converted(&combined, convert) else {
        return (None, None);
    };
    let total_czk = personal_czk + investment_czk;
    if personal_czk >= 0.0 && investment_czk >= 0.0 && total_czk > 0.0 && total_czk.is_finite() {
        (
            Some(value * personal_czk / total_czk),
            Some(value * investment_czk / total_czk),
        )
    } else {
        (Some(0.0), Some(value))
    }
}

fn history_point(
    row: &StoredRow,
    mut convert: impl FnMut(f64, &str) -> Option<f64>,
) -> HistoryPoint {
    let mut class = |i: usize| {
        let (json, czk_total) = &row.classes[i];
        stored_class(&breakdown_from_json(json), *czk_total, &mut convert)
    };
    let (savings, loans, investments, crypto, bonds, other_assets) =
        (class(0), class(1), class(2), class(3), class(4), class(5));
    let (real_estate_personal, real_estate_investment) = stored_real_estate(row, &mut convert);
    let classes = ClassTotals {
        savings,
        investments,
        crypto,
        bonds,
        real_estate_personal,
        real_estate_investment,
        other_assets,
        loans,
    };
    let assets = sum_known([
        savings,
        investments,
        crypto,
        bonds,
        real_estate_personal,
        real_estate_investment,
        other_assets,
    ]);
    let net_worth = match (assets, loans) {
        (None, None) => None,
        (assets, loans) => Some(assets.unwrap_or(0.0) - loans.unwrap_or(0.0)),
    };
    HistoryPoint {
        recorded_at: row.recorded_at,
        source: row.source.clone(),
        is_stale: row.is_stale,
        classes,
        net_worth,
    }
}

/// Stored snapshot days with `recorded_at` in `[from, to]` (both optional), oldest first, in
/// `report_currency`: every row's breakdowns convert at that row's day, with one rate history
/// loaded for the whole range. Misses of every row are collected in the returned quality.
pub fn history_in(
    conn: &Connection,
    report_currency: &str,
    from: Option<i64>,
    to: Option<i64>,
) -> Result<(Vec<HistoryPoint>, Quality)> {
    let currency = normalize_currency(report_currency)?;
    let rows = stored_rows(conn, from, to)?;
    let mut quality = Quality::default();
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok((Vec::new(), quality));
    };
    let history = History::load(conn, first.recorded_at, last.recorded_at)?;
    let points = rows
        .iter()
        .map(|row| {
            history_point(row, |amount, code| {
                history.convert_on_tracked(&mut quality, amount, code, &currency, row.recorded_at)
            })
        })
        .collect();
    Ok((points, quality))
}

/// The history as the wire response.
pub fn history_report(
    conn: &Connection,
    report_currency: &str,
    from: Option<i64>,
    to: Option<i64>,
) -> Result<PortfolioHistoryResponse> {
    let currency = normalize_currency(report_currency)?;
    let (points, quality) = history_in(conn, &currency, from, to)?;
    let text = |value: Option<f64>| value.map(|v| money_in_currency(v, &currency));
    let points = points
        .into_iter()
        .map(|point| PortfolioHistoryPoint {
            recorded_at: point.recorded_at,
            source: point.source,
            is_stale: point.is_stale,
            total_savings: text(point.classes.savings),
            total_loans_principal: text(point.classes.loans),
            total_investments: text(point.classes.investments),
            total_crypto: text(point.classes.crypto),
            total_bonds: text(point.classes.bonds),
            total_real_estate_personal: text(point.classes.real_estate_personal),
            total_real_estate_investment: text(point.classes.real_estate_investment),
            total_other_assets: text(point.classes.other_assets),
            net_worth: text(point.net_worth),
        })
        .collect();
    Ok(PortfolioHistoryResponse {
        fx_quality: ConversionQuality::from(&quality),
        currency,
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fx::DAY;

    const D: i64 = 20_000 * DAY;

    fn map(entries: &[(&str, f64)]) -> HashMap<String, f64> {
        entries.iter().map(|(c, v)| (c.to_string(), *v)).collect()
    }

    fn approx(value: Option<f64>, expected: f64) {
        let value = value.expect("converted");
        assert!((value - expected).abs() < 1e-9, "{value} != {expected}");
    }

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE portfolio_metrics_history (
                id TEXT PRIMARY KEY,
                total_savings TEXT NOT NULL DEFAULT '0',
                total_loans_principal TEXT NOT NULL DEFAULT '0',
                total_investments TEXT NOT NULL DEFAULT '0',
                total_crypto TEXT NOT NULL DEFAULT '0',
                total_bonds TEXT NOT NULL DEFAULT '0',
                total_real_estate_personal TEXT NOT NULL DEFAULT '0',
                total_real_estate_investment TEXT NOT NULL DEFAULT '0',
                recorded_at INTEGER NOT NULL,
                total_other_assets TEXT NOT NULL DEFAULT '0',
                is_stale INTEGER NOT NULL DEFAULT 0,
                investments_by_currency TEXT NOT NULL DEFAULT '{}',
                crypto_by_currency TEXT NOT NULL DEFAULT '{}',
                savings_by_currency TEXT NOT NULL DEFAULT '{}',
                bonds_by_currency TEXT NOT NULL DEFAULT '{}',
                real_estate_by_currency TEXT NOT NULL DEFAULT '{}',
                loans_by_currency TEXT NOT NULL DEFAULT '{}',
                other_assets_by_currency TEXT NOT NULL DEFAULT '{}',
                source TEXT NOT NULL DEFAULT 'live'
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

    /// One snapshot row: `columns` are `(column, value)` pairs on top of the defaults.
    fn add_row(conn: &Connection, day: i64, columns: &[(&str, &str)]) {
        let names: Vec<&str> = columns.iter().map(|(c, _)| *c).collect();
        let marks: Vec<String> = (0..columns.len()).map(|i| format!("?{}", i + 3)).collect();
        let sql = format!(
            "INSERT INTO portfolio_metrics_history (id, recorded_at{}{}) VALUES (?1, ?2{}{})",
            if names.is_empty() { "" } else { ", " },
            names.join(", "),
            if marks.is_empty() { "" } else { ", " },
            marks.join(", ")
        );
        let mut values: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(format!("row-{day}")), Box::new(day)];
        values.extend(
            columns
                .iter()
                .map(|(_, v)| Box::new(v.to_string()) as Box<dyn rusqlite::ToSql>),
        );
        conn.execute(&sql, rusqlite::params_from_iter(values))
            .expect("snapshot row");
    }

    /// A stored day quote: CZK per 1 unit of `currency`.
    fn add_rate(conn: &Connection, day: i64, currency: &str, czk_per_unit: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            rusqlite::params![day, currency, czk_per_unit],
        )
        .expect("rate");
    }

    #[test]
    fn eur_only_history_needs_no_czk_quote() {
        let conn = setup_db();
        for day in [D, D + DAY, D + 2 * DAY] {
            add_row(
                &conn,
                day + 3600,
                &[("investments_by_currency", r#"{"EUR":1000}"#)],
            );
        }

        let (points, quality) = history_in(&conn, "EUR", None, None).expect("history");

        assert_eq!(points.len(), 3);
        for point in &points {
            approx(point.classes.investments, 1000.0);
            approx(point.net_worth, 1000.0);
        }
        assert!(quality.is_complete());
    }

    #[test]
    fn each_row_converts_at_its_own_day() {
        let conn = setup_db();
        add_rate(&conn, D, "USD", 25.0);
        add_rate(&conn, D, "EUR", 25.0);
        add_rate(&conn, D + DAY, "USD", 25.0);
        add_rate(&conn, D + DAY, "EUR", 20.0);
        add_row(&conn, D, &[("savings_by_currency", r#"{"USD":100}"#)]);
        add_row(&conn, D + DAY, &[("savings_by_currency", r#"{"USD":100}"#)]);

        let (points, quality) = history_in(&conn, "EUR", None, None).expect("history");

        assert_eq!(points.len(), 2);
        // oldest day first
        assert_eq!(points[0].recorded_at, D);
        approx(points[0].classes.savings, 100.0);
        approx(points[1].classes.savings, 125.0);
        assert!(quality.is_complete());
    }

    #[test]
    fn an_unquoted_currency_is_left_out_and_reported() {
        let mut quotes = Quotes::new("CZK");
        quotes.insert("EUR", 1.0 / 25.0);
        let mut quality = Quality::default();

        let total = sum_breakdown(
            &map(&[("KWD", 5.0), ("EUR", 10.0)]),
            &quotes,
            "EUR",
            &mut quality,
        );

        approx(total, 10.0);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
        assert_eq!(quality.missing_count(), 1);
    }

    #[test]
    fn history_leaves_an_unquoted_currency_out_and_reports_it() {
        let conn = setup_db();
        add_rate(&conn, D, "EUR", 25.0);
        add_row(&conn, D, &[("bonds_by_currency", r#"{"KWD":5,"EUR":10}"#)]);

        let (points, quality) = history_in(&conn, "EUR", None, None).expect("history");

        approx(points[0].classes.bonds, 10.0);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
    }

    #[test]
    fn a_class_is_unknown_only_when_nothing_in_it_converts() {
        let quotes = Quotes::new("CZK");
        let mut quality = Quality::default();
        assert_eq!(
            sum_breakdown(&map(&[("KWD", 5.0)]), &quotes, "EUR", &mut quality),
            None
        );
        // nothing to convert: zero, not unknown; zero amounts need no quote
        approx(
            sum_breakdown(&HashMap::new(), &quotes, "EUR", &mut quality),
            0.0,
        );
        approx(
            sum_breakdown(&map(&[("JPY", 0.0)]), &quotes, "EUR", &mut quality),
            0.0,
        );
        assert_eq!(quality.missing_count(), 1);
    }

    #[test]
    fn a_legacy_row_without_breakdown_converts_its_czk_total_at_its_day() {
        let conn = setup_db();
        add_rate(&conn, D, "EUR", 25.0);
        add_row(&conn, D, &[("total_savings", "2500")]);

        let (points, quality) = history_in(&conn, "EUR", None, None).expect("history");

        approx(points[0].classes.savings, 100.0);
        assert!(quality.is_complete());
    }

    #[test]
    fn real_estate_splits_by_the_rows_czk_ratio() {
        let conn = setup_db();
        add_rate(&conn, D, "EUR", 25.0);
        add_row(
            &conn,
            D,
            &[
                ("real_estate_by_currency", r#"{"EUR":300}"#),
                ("total_real_estate_personal", "5000"),
                ("total_real_estate_investment", "2500"),
            ],
        );

        let (points, _) = history_in(&conn, "EUR", None, None).expect("history");

        approx(points[0].classes.real_estate_personal, 200.0);
        approx(points[0].classes.real_estate_investment, 100.0);
        approx(points[0].net_worth, 300.0);
    }

    #[test]
    fn history_net_worth_subtracts_loans_and_carries_the_row_provenance() {
        let conn = setup_db();
        add_row(
            &conn,
            D,
            &[
                ("savings_by_currency", r#"{"EUR":1000}"#),
                ("loans_by_currency", r#"{"EUR":400}"#),
                ("source", "backfill"),
                ("is_stale", "1"),
            ],
        );

        let (points, _) = history_in(&conn, "EUR", None, None).expect("history");

        approx(points[0].classes.loans, 400.0);
        approx(points[0].net_worth, 600.0);
        assert_eq!(points[0].source, "backfill");
        assert!(points[0].is_stale);
    }

    #[test]
    fn history_applies_the_range_inclusively() {
        let conn = setup_db();
        for day in [D, D + DAY, D + 2 * DAY] {
            add_row(&conn, day, &[("savings_by_currency", r#"{"EUR":1}"#)]);
        }

        let (points, _) =
            history_in(&conn, "EUR", Some(D + DAY), Some(D + 2 * DAY)).expect("history");

        let days: Vec<i64> = points.iter().map(|p| p.recorded_at).collect();
        assert_eq!(days, vec![D + DAY, D + 2 * DAY]);
    }

    #[test]
    fn history_rejects_an_invalid_report_currency() {
        assert!(history_in(&setup_db(), "euro", None, None).is_err());
    }

    fn eur_quotes() -> Quotes {
        let mut quotes = Quotes::new("CZK");
        quotes.insert("EUR", 1.0 / 25.0);
        quotes.insert("USD", 1.0 / 20.0);
        quotes
    }

    #[test]
    fn current_net_worth_sums_the_converted_breakdowns() {
        let breakdowns = ClassBreakdowns {
            savings: map(&[("EUR", 2000.0), ("CZK", 25_000.0)]),
            real_estate_personal: map(&[("USD", 100.0)]),
            real_estate_investment: map(&[("EUR", 500.0)]),
            loans: map(&[("CZK", 2500.0)]),
            other_assets: map(&[("KWD", 9.0)]),
            ..Default::default()
        };

        let worth = net_worth_of(breakdowns.clone(), &eur_quotes(), "EUR", false);

        approx(worth.classes.savings, 3000.0);
        approx(worth.classes.real_estate_personal, 80.0);
        approx(worth.classes.real_estate_investment, 500.0);
        approx(worth.classes.loans, 100.0);
        approx(worth.classes.investments, 0.0);
        assert_eq!(worth.classes.other_assets, None);
        assert!((worth.total_assets - 3580.0).abs() < 1e-9);
        assert!((worth.total_liabilities - 100.0).abs() < 1e-9);
        assert!((worth.net_worth - 3480.0).abs() < 1e-9);
        assert_eq!(worth.quality.missing_currencies(), vec!["KWD".to_string()]);

        // Excluding personal real estate leaves it out of the totals, still reported.
        let excluded = net_worth_of(breakdowns, &eur_quotes(), "EUR", true);
        approx(excluded.classes.real_estate_personal, 80.0);
        assert!((excluded.total_assets - 3500.0).abs() < 1e-9);
        assert!((excluded.net_worth - 3400.0).abs() < 1e-9);
    }

    #[test]
    fn current_net_worth_blames_an_unquoted_report_currency() {
        let breakdowns = ClassBreakdowns {
            savings: map(&[("EUR", 10.0), ("JPY", 1000.0)]),
            ..Default::default()
        };

        let worth = net_worth_of(breakdowns, &eur_quotes(), "JPY", false);

        // the JPY amount is identity; the EUR one cannot reach JPY
        approx(worth.classes.savings, 1000.0);
        assert_eq!(worth.quality.missing_currencies(), vec!["JPY".to_string()]);
    }
}
