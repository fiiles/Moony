//! Received stock dividends (spec 2026-10-30-stock-dividends-design, ADR 0022).
//!
//! The one writer of `investment_dividends` for the detail page, the CSV import and MCP
//! (ADR 0007). A dividend is a payout of a position: its gross total, the tax withheld from it
//! and their currency on the pay day. It never touches `investment_transactions`, so the
//! quantity, cost basis, gains and history of the position stay what its buys and sells make
//! them.
//!
//! Duplicates (import and MCP): rule 1, a stored dividend with the same broker id (`external_id`)
//! is the same payout, certainly; rule 2, a stored dividend of the same position, pay day,
//! numeric gross and currency is possibly the same payout. Each stored dividend absorbs at most
//! one incoming row, so a file with two equal payouts imports the second after the first was
//! matched.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::stock_dividends::amount_text;
use crate::models::{InsertStockDividend, StockDividend, StockDividendList, ValidDividend};
use crate::services::dedup::canonical_amount;
use crate::services::fx::{History, Quality, Quotes};
use crate::services::investments::{BulkRowIssue, BulkWriteReport, NewStockImportBatch};
use crate::services::reporting::money_in_currency;

const COLUMNS: &str =
    "id, investment_id, paid_date, gross_amount, withholding_tax, currency, note, created_at";

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockDividend> {
    let gross: String = row.get(3)?;
    let tax: String = row.get(4)?;
    let net = match (gross.parse::<f64>(), tax.parse::<f64>()) {
        (Ok(g), Ok(t)) => amount_text(g - t),
        _ => gross.clone(),
    };
    Ok(StockDividend {
        id: row.get(0)?,
        investment_id: row.get(1)?,
        paid_date: row.get(2)?,
        gross_amount: gross,
        withholding_tax: tax,
        net_amount: net,
        currency: row.get(5)?,
        note: row.get(6)?,
        created_at: row.get(7)?,
    })
}

/// One dividend by id.
pub fn get(conn: &Connection, id: &str) -> Result<StockDividend> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM investment_dividends WHERE id = ?1"),
        [id],
        read_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Dividend not found".into()))
}

/// The dividends of a position, newest pay day first.
pub fn for_investment(conn: &Connection, investment_id: &str) -> Result<Vec<StockDividend>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM investment_dividends WHERE investment_id = ?1
         ORDER BY paid_date DESC, created_at DESC, rowid DESC"
    ))?;
    let rows = stmt.query_map([investment_id], read_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every dividend, newest pay day first, optionally of one ticker only, at most `limit`.
pub fn all(
    conn: &Connection,
    ticker: Option<&str>,
    limit: i64,
) -> Result<Vec<(String, StockDividend)>> {
    let sql = format!(
        "SELECT i.ticker, {} FROM investment_dividends d
         JOIN stock_investments i ON i.id = d.investment_id
         WHERE ?1 IS NULL OR i.ticker = ?1
         ORDER BY d.paid_date DESC, d.created_at DESC, d.rowid DESC LIMIT ?2",
        COLUMNS
            .split(", ")
            .map(|c| format!("d.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let ticker = ticker.map(str::to_uppercase);
    let rows = stmt.query_map(rusqlite::params![ticker, limit], |row| {
        let ticker: String = row.get(0)?;
        let gross: String = row.get(4)?;
        let tax: String = row.get(5)?;
        let net = match (gross.parse::<f64>(), tax.parse::<f64>()) {
            (Ok(g), Ok(t)) => amount_text(g - t),
            _ => gross.clone(),
        };
        Ok((
            ticker,
            StockDividend {
                id: row.get(1)?,
                investment_id: row.get(2)?,
                paid_date: row.get(3)?,
                gross_amount: gross,
                withholding_tax: tax,
                net_amount: net,
                currency: row.get(6)?,
                note: row.get(7)?,
                created_at: row.get(8)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The dividends of a position with their gross and net sums in `report_currency`: every
/// payout converted at its pay day's rate (ten-day walk-back; day quotes only up to `today`),
/// else at `current` (flagged estimated), else left out and named in the quality. Never 1:1.
pub fn list(
    conn: &Connection,
    investment_id: &str,
    report_currency: &str,
    current: &Quotes,
    today: i64,
) -> Result<StockDividendList> {
    let report = report_currency.trim().to_uppercase();
    let items = for_investment(conn, investment_id)?;
    let mut quality = Quality::default();
    let (mut gross_sum, mut net_sum, mut converted) = (0.0, 0.0, 0usize);
    if let (Some(first), Some(last)) = (
        items.iter().map(|d| d.paid_date).min(),
        items.iter().map(|d| d.paid_date).max(),
    ) {
        let history = History::load(conn, first.min(today), last.min(today))?;
        for item in &items {
            let (Ok(gross), Ok(tax)) = (
                item.gross_amount.parse::<f64>(),
                item.withholding_tax.parse::<f64>(),
            ) else {
                quality.record_missing_conversion([item.currency.as_str()]);
                continue;
            };
            // A day without its own quotes yet (today, a future pay day) uses the current ones.
            let rate_on_day = (item.paid_date <= today)
                .then(|| history.convert_on(1.0, &item.currency, &report, item.paid_date))
                .flatten();
            let rate = match rate_on_day {
                Some(rate) => Some(rate),
                None => {
                    let rate = current.convert(1.0, &item.currency, &report);
                    if rate.is_some() {
                        quality.mark_estimated();
                    }
                    rate
                }
            };
            match rate {
                Some(rate) => {
                    gross_sum += gross * rate;
                    net_sum += (gross - tax) * rate;
                    converted += 1;
                }
                None => {
                    quality.track_conversion(current, &item.currency, &report, None);
                }
            }
        }
    }
    let total =
        |sum: f64| (items.is_empty() || converted > 0).then(|| money_in_currency(sum, &report));
    Ok(StockDividendList {
        total_gross: total(gross_sum),
        total_net: total(net_sum),
        currency: report,
        fx_quality: ConversionQuality::from(&quality),
        items,
    })
}

fn investment_exists(conn: &Connection, investment_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM stock_investments WHERE id = ?1)",
        [investment_id],
        |row| row.get(0),
    )?)
}

/// Where a written dividend came from: the import batch and the broker's id, both optional.
#[derive(Debug, Clone, Copy, Default)]
pub struct DividendOrigin<'a> {
    pub import_batch_id: Option<&'a str>,
    pub external_id: Option<&'a str>,
}

/// Write a validated dividend of an existing position.
pub fn insert(
    conn: &Connection,
    investment_id: &str,
    dividend: &ValidDividend,
    origin: DividendOrigin<'_>,
) -> Result<StockDividend> {
    if !investment_exists(conn, investment_id)? {
        return Err(AppError::NotFound("Investment not found".into()));
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO investment_dividends
            (id, investment_id, paid_date, gross_amount, withholding_tax, currency, note,
             import_batch_id, external_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            id,
            investment_id,
            dividend.paid_date,
            dividend.gross_amount,
            dividend.withholding_tax,
            dividend.currency,
            dividend.note,
            origin.import_batch_id,
            origin
                .external_id
                .map(str::trim)
                .filter(|external| !external.is_empty()),
        ],
    )?;
    get(conn, &id)
}

/// Record a dividend the user entered (detail page).
pub fn create(
    conn: &Connection,
    investment_id: &str,
    data: &InsertStockDividend,
) -> Result<StockDividend> {
    let valid = data.validate()?;
    insert(conn, investment_id, &valid, DividendOrigin::default())
}

/// Replace the values of a dividend; its position, batch and broker id stay.
pub fn update(conn: &Connection, id: &str, data: &InsertStockDividend) -> Result<StockDividend> {
    let valid = data.validate()?;
    let changed = conn.execute(
        "UPDATE investment_dividends
         SET paid_date = ?1, gross_amount = ?2, withholding_tax = ?3, currency = ?4, note = ?5
         WHERE id = ?6",
        rusqlite::params![
            valid.paid_date,
            valid.gross_amount,
            valid.withholding_tax,
            valid.currency,
            valid.note,
            id
        ],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound("Dividend not found".into()));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let changed = conn.execute("DELETE FROM investment_dividends WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(AppError::NotFound("Dividend not found".into()));
    }
    Ok(())
}

/// How an incoming dividend matches a stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DividendDuplicate {
    /// The same broker id: the same payout (rule 1; cannot be imported anyway).
    SameId,
    /// The same position, pay day, numeric gross and currency (rule 2; may be imported anyway).
    SameValues,
}

#[derive(Debug)]
struct StoredDividend {
    investment_id: String,
    paid_date: i64,
    gross: String,
    currency: String,
    external_id: Option<String>,
    consumed: bool,
}

/// The stored dividends an import compares its rows with; a matched one is consumed.
#[derive(Debug, Default)]
pub struct DividendDuplicates {
    stored: Vec<StoredDividend>,
}

impl DividendDuplicates {
    pub fn load(conn: &Connection) -> Result<DividendDuplicates> {
        let mut stmt = conn.prepare(
            "SELECT investment_id, paid_date, gross_amount, currency, external_id
             FROM investment_dividends ORDER BY rowid",
        )?;
        let stored = stmt
            .query_map([], |row| {
                Ok(StoredDividend {
                    investment_id: row.get(0)?,
                    paid_date: row.get(1)?,
                    gross: canonical_amount(&row.get::<_, String>(2)?),
                    currency: row.get::<_, String>(3)?.to_uppercase(),
                    external_id: row.get(4)?,
                    consumed: false,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(DividendDuplicates { stored })
    }

    /// Whether the row repeats a stored dividend; the matched dividend is consumed. An id match
    /// wins over a value match. With `allow_values` a value match is not reported (and nothing is
    /// consumed by it), an id match still is.
    pub fn check(
        &mut self,
        investment_id: Option<&str>,
        dividend: &ValidDividend,
        external_id: Option<&str>,
        allow_values: bool,
    ) -> Option<DividendDuplicate> {
        if let Some(external_id) = external_id.map(str::trim).filter(|id| !id.is_empty()) {
            if let Some(stored) = self
                .stored
                .iter_mut()
                .find(|s| !s.consumed && s.external_id.as_deref() == Some(external_id))
            {
                stored.consumed = true;
                return Some(DividendDuplicate::SameId);
            }
        }
        if allow_values {
            return None;
        }
        let investment_id = investment_id?;
        let gross = canonical_amount(&dividend.gross_amount);
        let stored = self.stored.iter_mut().find(|s| {
            !s.consumed
                && s.investment_id == investment_id
                && s.paid_date == dividend.paid_date
                && s.gross == gross
                && s.currency == dividend.currency
        })?;
        stored.consumed = true;
        Some(DividendDuplicate::SameValues)
    }
}

/// One dividend of a bulk write (MCP): the position by ticker.
#[derive(Debug, Clone)]
pub struct BulkDividendRow {
    pub ticker: String,
    pub dividend: InsertStockDividend,
    /// `<source>:<broker id>`.
    pub external_id: Option<String>,
    /// Write it even when a stored dividend has the same values.
    pub allow_duplicate: bool,
}

/// The bulk writer of dividends (MCP `stock_dividends_create`). Phase 1: every row validates
/// and names an existing position, else nothing is written and every failure is reported.
/// Phase 2: one savepoint; duplicates are skipped and reported; with a `batch` its row is
/// written too (only when a dividend is written) and stamps every dividend.
pub fn bulk_create(
    conn: &mut Connection,
    rows: &[BulkDividendRow],
    batch: Option<&NewStockImportBatch>,
) -> Result<BulkWriteReport> {
    let mut positions: HashMap<String, String> = HashMap::new();
    let mut valid: Vec<ValidDividend> = Vec::with_capacity(rows.len());
    let mut errors: Vec<BulkRowIssue> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let ticker = row.ticker.trim().to_uppercase();
        let position: Option<String> = conn
            .query_row(
                "SELECT id FROM stock_investments WHERE ticker = ?1",
                [&ticker],
                |r| r.get(0),
            )
            .optional()?;
        let checked = row.dividend.validate().and_then(|v| match position {
            Some(id) => {
                positions.insert(ticker.clone(), id);
                Ok(v)
            }
            None => Err(AppError::validation_with(
                "validation.dividendWithoutPosition",
                serde_json::json!({ "ticker": ticker }),
            )),
        });
        match checked {
            Ok(v) => valid.push(v),
            Err(e) => errors.push(BulkRowIssue {
                index,
                message: e.to_string(),
                possible_duplicate: false,
            }),
        }
    }
    if !errors.is_empty() {
        return Ok(BulkWriteReport {
            errors,
            ..BulkWriteReport::default()
        });
    }

    let mut duplicates = DividendDuplicates::load(conn)?;
    let tx = conn.savepoint()?;
    let batch_id = match batch {
        Some(batch) if !rows.is_empty() => {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO stock_import_batches (id, file_name, source, trade_count)
                 VALUES (?1, ?2, ?3, 0)",
                rusqlite::params![id, batch.file_name, batch.source],
            )?;
            Some(id)
        }
        _ => None,
    };
    let mut report = BulkWriteReport::default();
    for (index, (row, dividend)) in rows.iter().zip(&valid).enumerate() {
        let ticker = row.ticker.trim().to_uppercase();
        let position = &positions[&ticker];
        if let Some(kind) = duplicates.check(
            Some(position),
            dividend,
            row.external_id.as_deref(),
            row.allow_duplicate,
        ) {
            report.skipped_duplicates.push(BulkRowIssue {
                index,
                message: match kind {
                    DividendDuplicate::SameId => "externalId already imported".into(),
                    DividendDuplicate::SameValues => {
                        "A dividend with the same pay date, gross amount and currency already exists"
                            .into()
                    }
                },
                possible_duplicate: kind == DividendDuplicate::SameValues,
            });
            continue;
        }
        insert(
            &tx,
            position,
            dividend,
            DividendOrigin {
                import_batch_id: batch_id.as_deref(),
                external_id: row.external_id.as_deref(),
            },
        )?;
        report.created += 1;
        if !report.written_tickers.contains(&ticker) {
            report.written_tickers.push(ticker);
        }
    }
    if let Some(id) = batch_id {
        if report.created == 0 {
            tx.execute("DELETE FROM stock_import_batches WHERE id = ?1", [&id])?;
        } else {
            tx.execute(
                "UPDATE stock_import_batches SET trade_count = ?1 WHERE id = ?2",
                rusqlite::params![report.created as i64, id],
            )?;
            report.batch_id = Some(id);
        }
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::investments::recalculate_investment_metrics;

    const DAY: i64 = 86_400;
    const D1: i64 = 20_000 * DAY;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL UNIQUE, company_name TEXT NOT NULL,
                quantity TEXT NOT NULL, currency TEXT NOT NULL DEFAULT 'USD'
            );
            CREATE TABLE stock_import_batches (
                id TEXT PRIMARY KEY, file_name TEXT NOT NULL, source TEXT NOT NULL,
                trade_count INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                type TEXT NOT NULL, ticker TEXT NOT NULL, company_name TEXT NOT NULL,
                quantity TEXT NOT NULL, price_per_unit TEXT NOT NULL, currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                import_batch_id TEXT, external_id TEXT
            );
            CREATE TABLE investment_dividends (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                paid_date INTEGER NOT NULL,
                gross_amount TEXT NOT NULL,
                withholding_tax TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL,
                note TEXT,
                import_batch_id TEXT REFERENCES stock_import_batches(id) ON DELETE SET NULL,
                external_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL
            );
            INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                VALUES ('inv-aapl', 'AAPL', 'Apple', '10', 'USD');
            INSERT INTO investment_transactions
                (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency,
                 transaction_date)
                VALUES ('t1', 'inv-aapl', 'buy', 'AAPL', 'Apple', '10', '150', 'USD', 19000 * 86400);
            "#,
        )
        .expect("schema");
        conn
    }

    fn input(day: i64, gross: &str, tax: Option<&str>, currency: &str) -> InsertStockDividend {
        InsertStockDividend {
            paid_date: day,
            gross_amount: gross.into(),
            withholding_tax: tax.map(str::to_string),
            currency: currency.into(),
            note: None,
        }
    }

    fn eur_quotes(czk: f64, usd: f64) -> Quotes {
        let mut quotes = Quotes::new("EUR");
        quotes.insert("CZK", czk);
        quotes.insert("USD", usd);
        quotes
    }

    fn quantity(conn: &Connection) -> String {
        conn.query_row(
            "SELECT quantity FROM stock_investments WHERE id = 'inv-aapl'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn create_update_and_delete_a_dividend() {
        let conn = setup_test_db();
        let created = create(
            &conn,
            "inv-aapl",
            &input(D1 + 600, "24", Some("3.6"), "usd"),
        )
        .unwrap();
        assert_eq!(created.paid_date, D1);
        assert_eq!(
            (
                created.gross_amount.as_str(),
                created.withholding_tax.as_str(),
                created.net_amount.as_str()
            ),
            ("24", "3.6", "20.4")
        );
        assert_eq!(created.currency, "USD");

        let mut changed = input(D1 + DAY, "30", None, "EUR");
        changed.note = Some("special".into());
        let updated = update(&conn, &created.id, &changed).unwrap();
        assert_eq!(
            (
                updated.paid_date,
                updated.gross_amount.as_str(),
                updated.withholding_tax.as_str()
            ),
            (D1 + DAY, "30", "0")
        );
        assert_eq!(updated.note.as_deref(), Some("special"));
        assert_eq!(updated.investment_id, "inv-aapl");

        delete(&conn, &created.id).unwrap();
        assert!(for_investment(&conn, "inv-aapl").unwrap().is_empty());
        assert!(matches!(
            delete(&conn, &created.id),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            update(&conn, "missing", &changed),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn a_dividend_needs_an_existing_position_and_valid_values() {
        let conn = setup_test_db();
        assert!(matches!(
            create(&conn, "missing", &input(D1, "1", None, "USD")),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            create(&conn, "inv-aapl", &input(D1, "1", Some("2"), "USD")),
            Err(AppError::Validation(_))
        ));
    }

    /// The guard of ADR 0022: a dividend is not a trade, whatever recomputes the position.
    #[test]
    fn a_dividend_never_changes_the_position_quantity() {
        let conn = setup_test_db();
        create(&conn, "inv-aapl", &input(D1, "24", Some("3.6"), "USD")).unwrap();
        recalculate_investment_metrics(&conn, "inv-aapl").unwrap();
        assert_eq!(quantity(&conn), "10");
        let replayed = crate::services::cost_basis::replayed_quantities(
            &conn,
            crate::services::cost_basis::TxTable::Stocks,
        )
        .unwrap();
        assert_eq!(replayed.get("inv-aapl").copied(), Some(10.0));
    }

    #[test]
    fn deleting_the_position_deletes_its_dividends() {
        let conn = setup_test_db();
        create(&conn, "inv-aapl", &input(D1, "24", None, "USD")).unwrap();
        conn.execute("DELETE FROM stock_investments WHERE id = 'inv-aapl'", [])
            .unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM investment_dividends", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn totals_convert_every_payout_at_its_pay_day() {
        let conn = setup_test_db();
        // 1 EUR = 25 CZK = 1.25 USD on D1 (20 CZK per USD); 1 EUR = 24 CZK = 1.2 USD on D1 + 1.
        conn.execute_batch(&format!(
            "INSERT INTO exchange_rate_history VALUES ({D1}, 'CZK', 25.0), ({D1}, 'USD', 1.25),
                ({d2}, 'CZK', 24.0), ({d2}, 'USD', 1.2);",
            d2 = D1 + DAY
        ))
        .unwrap();
        create(&conn, "inv-aapl", &input(D1, "10", Some("1"), "USD")).unwrap();
        create(&conn, "inv-aapl", &input(D1 + DAY, "100", None, "CZK")).unwrap();
        let report = list(
            &conn,
            "inv-aapl",
            "CZK",
            &eur_quotes(30.0, 1.0),
            D1 + 10 * DAY,
        )
        .unwrap();
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].paid_date, D1 + DAY, "newest first");
        // 10 USD × 20 = 200 CZK, + 100 CZK; net: 9 × 20 + 100.
        assert_eq!(report.total_gross.as_deref(), Some("300.00"));
        assert_eq!(report.total_net.as_deref(), Some("280.00"));
        assert!(report.fx_quality.missing_currencies.is_empty());
        assert!(!report.fx_quality.estimated);
    }

    #[test]
    fn a_day_without_quotes_uses_todays_and_says_so() {
        let conn = setup_test_db();
        create(&conn, "inv-aapl", &input(D1, "10", None, "USD")).unwrap();
        let report = list(&conn, "inv-aapl", "CZK", &eur_quotes(25.0, 1.25), D1 + DAY).unwrap();
        assert_eq!(report.total_gross.as_deref(), Some("200.00"));
        assert!(report.fx_quality.estimated);
    }

    #[test]
    fn an_unconvertible_payout_is_left_out_and_named() {
        let conn = setup_test_db();
        create(&conn, "inv-aapl", &input(D1, "10", None, "USD")).unwrap();
        create(&conn, "inv-aapl", &input(D1, "5", None, "GBP")).unwrap();
        let report = list(&conn, "inv-aapl", "CZK", &eur_quotes(25.0, 1.25), D1 + DAY).unwrap();
        assert_eq!(report.total_gross.as_deref(), Some("200.00"));
        assert_eq!(
            report.fx_quality.missing_currencies,
            vec!["GBP".to_string()]
        );

        let conn = setup_test_db();
        create(&conn, "inv-aapl", &input(D1, "5", None, "GBP")).unwrap();
        let report = list(&conn, "inv-aapl", "CZK", &eur_quotes(25.0, 1.25), D1 + DAY).unwrap();
        assert_eq!(report.total_gross, None, "payouts exist, none converted");
    }

    #[test]
    fn no_dividends_sum_to_zero() {
        let conn = setup_test_db();
        let report = list(&conn, "inv-aapl", "usd", &Quotes::new("EUR"), D1).unwrap();
        assert_eq!(report.currency, "USD");
        assert_eq!(report.total_gross.as_deref(), Some("0.00"));
    }

    #[test]
    fn duplicates_match_by_id_then_by_values_one_to_one() {
        let conn = setup_test_db();
        let valid = input(D1, "24.0", None, "USD").validate().unwrap();
        insert(
            &conn,
            "inv-aapl",
            &valid,
            DividendOrigin {
                import_batch_id: None,
                external_id: Some("ibkr:1"),
            },
        )
        .unwrap();
        insert(&conn, "inv-aapl", &valid, DividendOrigin::default()).unwrap();

        let mut dups = DividendDuplicates::load(&conn).unwrap();
        let again = input(D1, "24", None, "USD").validate().unwrap();
        assert_eq!(
            dups.check(Some("inv-aapl"), &again, Some("ibkr:1"), false),
            Some(DividendDuplicate::SameId)
        );
        assert_eq!(
            dups.check(Some("inv-aapl"), &again, None, false),
            Some(DividendDuplicate::SameValues),
            "24 and 24.0 are the same gross"
        );
        assert_eq!(
            dups.check(Some("inv-aapl"), &again, None, false),
            None,
            "both stored dividends are consumed"
        );

        let mut dups = DividendDuplicates::load(&conn).unwrap();
        assert_eq!(
            dups.check(Some("inv-aapl"), &again, None, true),
            None,
            "import anyway"
        );
        let other_day = input(D1 + DAY, "24", None, "USD").validate().unwrap();
        assert_eq!(dups.check(Some("inv-aapl"), &other_day, None, false), None);
        let other_currency = input(D1, "24", None, "EUR").validate().unwrap();
        assert_eq!(
            dups.check(Some("inv-aapl"), &other_currency, None, false),
            None
        );
    }

    fn bulk_row(ticker: &str, gross: &str, external_id: Option<&str>) -> BulkDividendRow {
        BulkDividendRow {
            ticker: ticker.into(),
            dividend: input(D1, gross, Some("1"), "USD"),
            external_id: external_id.map(str::to_string),
            allow_duplicate: false,
        }
    }

    #[test]
    fn bulk_create_writes_a_batch_and_skips_repeats() {
        let mut conn = setup_test_db();
        let batch = NewStockImportBatch {
            file_name: "dividends.csv".into(),
            source: "mcp".into(),
        };
        let rows = vec![
            bulk_row("aapl", "10", Some("mcp:a")),
            bulk_row("AAPL", "12", None),
        ];
        let report = bulk_create(&mut conn, &rows, Some(&batch)).unwrap();
        assert_eq!(report.created, 2);
        assert!(report.errors.is_empty());
        let batch_id = report.batch_id.expect("batch recorded");
        let (count, stamped): (i64, i64) = conn
            .query_row(
                "SELECT (SELECT trade_count FROM stock_import_batches WHERE id = ?1),
                        (SELECT COUNT(*) FROM investment_dividends WHERE import_batch_id = ?1)",
                [&batch_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((count, stamped), (2, 2));

        let again = bulk_create(&mut conn, &rows, Some(&batch)).unwrap();
        assert_eq!(again.created, 0);
        assert_eq!(again.skipped_duplicates.len(), 2);
        assert!(
            !again.skipped_duplicates[0].possible_duplicate,
            "same id is certain"
        );
        assert!(again.skipped_duplicates[1].possible_duplicate);
        assert_eq!(again.batch_id, None, "nothing written, no batch");
        assert_eq!(quantity(&conn), "10");
    }

    #[test]
    fn bulk_create_is_all_or_nothing_on_invalid_rows() {
        let mut conn = setup_test_db();
        let rows = vec![
            bulk_row("AAPL", "10", None),
            bulk_row("MSFT", "5", None),
            bulk_row("AAPL", "-1", None),
        ];
        let report = bulk_create(&mut conn, &rows, None).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(
            report.errors.iter().map(|e| e.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let stored: i64 = conn
            .query_row("SELECT COUNT(*) FROM investment_dividends", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, 0);
    }
}
