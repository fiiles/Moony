//! Step 1 after a merge (spec §5.2, §7.1, §7.2): the recalculations of every touched parent, run
//! through the same service functions a local write runs. Parents are visited in ascending id.
//!
//! Failure policy (correction 12): a history that sells more than it holds
//! (`validation.sellExceedsHoldings` / `validation.stockSplitOversold`) gets the unchecked net
//! quantity `max(0, Σbuy − Σsell)` and a warning; any other failing recalculation keeps the
//! stored value and adds a warning. A recalculation never fails the merge; SQL errors do.

use std::collections::BTreeSet;

use rusqlite::{params, Connection};

use super::{attempt, RepairReport, Touched};
use crate::error::{AppError, Result};
use crate::services::stock_splits::{self, ratio_parts, StockSplitStatus};
use crate::services::valuations::{self, ValuationKind};
use crate::services::{
    bank_accounts, budgeting, crypto_investments, dedup, investments, loan_events, other_assets,
};

/// A position table, the table of its transactions and the column pointing at the position.
struct Positions {
    table: &'static str,
    transactions: &'static str,
    owner: &'static str,
    recalculate: fn(&Connection, &str) -> Result<()>,
}

const STOCKS: Positions = Positions {
    table: "stock_investments",
    transactions: "investment_transactions",
    owner: "investment_id",
    recalculate: investments::recalculate_investment_metrics,
};
const CRYPTO: Positions = Positions {
    table: "crypto_investments",
    transactions: "crypto_transactions",
    owner: "investment_id",
    recalculate: crypto_investments::recalculate_crypto_metrics,
};
const OTHER_ASSETS: Positions = Positions {
    table: "other_assets",
    transactions: "other_asset_transactions",
    owner: "asset_id",
    recalculate: other_assets::recalculate_asset_totals,
};

/// The tables carrying a `split_basis` (D27).
const BASIS_TABLES: [&str; 4] = [
    "investment_transactions",
    "stock_price_overrides",
    "watched_stocks",
    "dividend_overrides",
];

pub fn run(
    conn: &Connection,
    touched: &Touched,
    now: i64,
    report: &mut RepairReport,
) -> Result<()> {
    for id in with_rows(
        conn,
        touched,
        "bank_accounts",
        "bank_balance_records",
        "bank_account_id",
    )? {
        recalculate(conn, report, "bank_accounts", &id, || {
            bank_accounts::refresh_balance_columns(conn, &id)
        })?;
    }

    for id in existing(conn, "loans", touched.owners("loans"))? {
        recalculate(conn, report, "loans", &id, || {
            loan_events::refresh_anchor_mirror(conn, &id, now)?;
            loan_events::rebuild_terms_chain(conn, &id).map(|_| ())
        })?;
    }

    for (table, kind, valuations, owner) in [
        (
            "real_estate",
            ValuationKind::RealEstate,
            "real_estate_valuations",
            "real_estate_id",
        ),
        (
            "other_assets",
            ValuationKind::OtherAsset,
            "other_asset_valuations",
            "asset_id",
        ),
    ] {
        for id in with_rows(conn, touched, table, valuations, owner)? {
            recalculate(conn, report, table, &id, || {
                valuations::sync_parent_price(conn, kind, &id)
            })?;
        }
    }

    let mut stock_ids = touched.owners(STOCKS.table);
    for ticker in &touched.tickers {
        reconcile(conn, report, ticker, &mut stock_ids)?;
    }
    for (positions, ids) in [
        (&STOCKS, stock_ids),
        (&CRYPTO, touched.owners(CRYPTO.table)),
        (&OTHER_ASSETS, touched.owners(OTHER_ASSETS.table)),
    ] {
        let ids = ids.into_iter().collect();
        for id in only_with_rows(conn, ids, positions.transactions, positions.owner)? {
            recalculate_position(conn, report, positions, &id)?;
        }
    }

    for (category, timeframe) in &touched.budget_keys {
        recalculate(
            conn,
            report,
            "budget_goal_versions",
            &format!("{category}/{timeframe}"),
            || {
                budgeting::merge_equal_neighbours(conn, category, timeframe)?;
                budgeting::rebuild_goal_row(conn, category, timeframe, now)
            },
        )?;
    }
    Ok(())
}

/// Runs one recalculation; a failure keeps the stored values and becomes a warning.
fn recalculate(
    conn: &Connection,
    report: &mut RepairReport,
    table: &str,
    id: &str,
    work: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if let Err(error) = attempt(conn, work)? {
        report
            .warnings
            .push(format!("{table} {id}: {error}; the stored value is kept"));
    }
    Ok(())
}

fn recalculate_position(
    conn: &Connection,
    report: &mut RepairReport,
    positions: &Positions,
    id: &str,
) -> Result<()> {
    let error = match attempt(conn, || (positions.recalculate)(conn, id))? {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    if !is_oversold(&error) {
        report.warnings.push(format!(
            "{} {id}: {error}; the stored value is kept",
            positions.table
        ));
        return Ok(());
    }
    match unchecked_net_quantity(conn, positions, id)? {
        Some(quantity) => {
            conn.execute(
                &format!("UPDATE {} SET quantity = ?1 WHERE id = ?2", positions.table),
                params![quantity, id],
            )?;
            report.warnings.push(format!(
                "{} {id}: the merged history sells more than it holds; quantity set to {quantity}",
                positions.table
            ));
        }
        None => report.warnings.push(format!(
            "{} {id}: the merged history sells more than it holds and a quantity is unreadable; \
             the stored value is kept",
            positions.table
        )),
    }
    Ok(())
}

fn is_oversold(error: &AppError) -> bool {
    matches!(error, AppError::Validation(key)
        if key == "validation.sellExceedsHoldings" || key == "validation.stockSplitOversold")
}

/// Makes the ticker's rows match its merged split statuses (§7.2). A ticker with no split and no
/// row carrying a basis has nothing to reconcile. A split row the reconcile cannot read (ratio,
/// status) or a basis that is not a JSON list skips the ticker with a warning. An oversold history
/// leaves the units as they are and sends the ticker's positions to the quantity fallback, which
/// warns once per position.
fn reconcile(
    conn: &Connection,
    report: &mut RepairReport,
    ticker: &str,
    stock_ids: &mut BTreeSet<String>,
) -> Result<()> {
    let splits: i64 = conn.query_row(
        "SELECT COUNT(*) FROM stock_splits WHERE ticker = ?1",
        [ticker],
        |r| r.get(0),
    )?;
    let mut with_basis = 0_i64;
    for table in BASIS_TABLES {
        with_basis += conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE ticker = ?1 AND split_basis IS NOT NULL"),
            [ticker],
            |r| r.get::<_, i64>(0),
        )?;
    }
    if splits == 0 && with_basis == 0 {
        return Ok(());
    }
    if let Some(problem) = unreadable_split_data(conn, ticker)? {
        report.warnings.push(format!(
            "stock_splits {ticker}: {problem}; the split units of the ticker are not reconciled"
        ));
        return Ok(());
    }
    match attempt(conn, || {
        stock_splits::reconcile_ticker(conn, ticker).map(|_| ())
    })? {
        Ok(()) => {}
        Err(error) if is_oversold(&error) => {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT investment_id FROM investment_transactions WHERE ticker = ?1",
            )?;
            let ids = stmt
                .query_map([ticker], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            stock_ids.extend(ids);
        }
        Err(error) => report.warnings.push(format!(
            "stock_splits {ticker}: {error}; the split units of the ticker are not reconciled"
        )),
    }
    Ok(())
}

/// The first reason `reconcile_ticker` could not read the ticker's data, if any: a split whose
/// ratio or status does not parse, or a `split_basis` that is not a JSON list of IDs.
fn unreadable_split_data(conn: &Connection, ticker: &str) -> Result<Option<String>> {
    let splits: Vec<(String, String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, numerator, denominator, status FROM stock_splits
             WHERE ticker = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([ticker], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    for (id, numerator, denominator, status) in splits {
        if let Err(error) = ratio_parts(&numerator, &denominator) {
            return Ok(Some(format!("split {id}: {error}")));
        }
        if let Err(error) = StockSplitStatus::parse(&status) {
            return Ok(Some(format!("split {id}: {error}")));
        }
    }
    for table in BASIS_TABLES {
        let mut stmt = conn.prepare(&format!(
            "SELECT id, split_basis FROM {table}
             WHERE ticker = ?1 AND split_basis IS NOT NULL ORDER BY id"
        ))?;
        let rows = stmt
            .query_map([ticker], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (id, basis) in rows {
            if serde_json::from_str::<Vec<String>>(&basis).is_err() {
                return Ok(Some(format!("{table} {id}: invalid split_basis {basis}")));
            }
        }
    }
    Ok(None)
}

/// `max(0, Σbuy − Σsell)` of the position's transactions in exact decimal arithmetic, as
/// canonical decimal text; `None` when a quantity or type is unreadable or the sum overflows.
fn unchecked_net_quantity(
    conn: &Connection,
    positions: &Positions,
    id: &str,
) -> Result<Option<String>> {
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT type, quantity FROM {} WHERE {} = ?1 ORDER BY id",
            positions.transactions, positions.owner
        ))?;
        let rows = stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    Ok(net_quantity(&rows))
}

/// See [`unchecked_net_quantity`]; pure.
fn net_quantity(rows: &[(String, String)]) -> Option<String> {
    let mut parsed = Vec::with_capacity(rows.len());
    for (kind, quantity) in rows {
        let sign: i128 = match kind.to_ascii_lowercase().as_str() {
            "buy" => 1,
            "sell" => -1,
            _ => return None,
        };
        let canonical = dedup::canonical_amount(quantity);
        let (integer, fraction) = canonical.split_once('.').unwrap_or((&canonical, ""));
        if integer.is_empty()
            || !integer
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return None;
        }
        parsed.push((sign, format!("{integer}{fraction}"), fraction.len()));
    }
    let scale = parsed.iter().map(|(_, _, s)| *s).max().unwrap_or(0);
    let mut total: i128 = 0;
    for (sign, digits, fraction) in parsed {
        let value: i128 = digits.parse().ok()?;
        let factor = 10_i128.checked_pow(u32::try_from(scale - fraction).ok()?)?;
        total = total.checked_add(sign.checked_mul(value.checked_mul(factor)?)?)?;
    }
    if total <= 0 {
        return Some("0".to_string());
    }
    let mut text = total.to_string();
    if scale > 0 {
        if text.len() <= scale {
            text = format!("{}{text}", "0".repeat(scale + 1 - text.len()));
        }
        text.insert(text.len() - scale, '.');
    }
    Some(dedup::canonical_amount(&text))
}

/// The touched owners of `table` that still exist and have at least one row in `rows_table`.
fn with_rows(
    conn: &Connection,
    touched: &Touched,
    table: &str,
    rows_table: &str,
    owner: &str,
) -> Result<Vec<String>> {
    let ids = existing(conn, table, touched.owners(table))?;
    only_with_rows(conn, ids, rows_table, owner)
}

fn existing(conn: &Connection, table: &str, ids: BTreeSet<String>) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"
    ))?;
    let mut out = Vec::new();
    for id in ids {
        if stmt.query_row([&id], |r| r.get::<_, bool>(0))? {
            out.push(id);
        }
    }
    Ok(out)
}

fn only_with_rows(
    conn: &Connection,
    ids: Vec<String>,
    rows_table: &str,
    owner: &str,
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT EXISTS (SELECT 1 FROM {rows_table} WHERE {owner} = ?1)"
    ))?;
    let mut out = Vec::new();
    for id in ids {
        if stmt.query_row([&id], |r| r.get::<_, bool>(0))? {
            out.push(id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::net_quantity;

    fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, q)| (k.to_string(), q.to_string()))
            .collect()
    }

    #[test]
    fn net_quantity_is_exact_and_never_negative() {
        assert_eq!(
            net_quantity(&rows(&[("buy", "15"), ("sell", "10"), ("sell", "10")])),
            Some("0".into())
        );
        assert_eq!(
            net_quantity(&rows(&[("buy", "0.1"), ("buy", "0.2"), ("sell", "0.05")])),
            Some("0.25".into())
        );
        assert_eq!(
            net_quantity(&rows(&[("BUY", "1e2"), ("sell", "0.5")])),
            Some("99.5".into())
        );
        assert_eq!(net_quantity(&rows(&[("buy", "abc")])), None);
        assert_eq!(net_quantity(&rows(&[("gift", "1")])), None);
    }
}
