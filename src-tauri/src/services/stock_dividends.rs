//! Received stock dividends (spec 2026-10-30-stock-dividends-design, ADR 0022).
//!
//! Yahoo Finance is the source. `stock_dividend_events` keeps a ticker's payouts per share by
//! ex-date (`price_api` records them); a position's payout is that amount × the shares it held at
//! the end of the day before the ex-date, derived on every read, so a backdated trade corrects
//! it. A payout the user enters by hand (`investment_dividends`) is only for a ticker Yahoo has no
//! dividends for: once Yahoo has any, the manual ones are shown as superseded and left out of
//! every sum, and no new one can be entered. Every amount is gross, in its own currency.
//!
//! Nothing here touches `investment_transactions`: the quantity, cost basis, gains and history
//! of a position stay what its buys and sells make them.

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::stock_dividends::amount_text;
use crate::models::{
    InsertStockDividend, StockDividend, StockDividendList, StockDividendSource, ValidDividend,
};
use crate::services::fx::{History, Quality, Quotes};
use crate::services::investments::held_quantity_at;
use crate::services::reporting::money_in_currency;

const DAY: i64 = 86_400;

/// One per-share payout Yahoo reported for a ticker.
#[derive(Debug, Clone, PartialEq)]
pub struct DividendEvent {
    /// UTC midnight of the ex-date (the exchange's calendar day).
    pub ex_date: i64,
    /// Per share, in `currency` (pence already converted to pounds).
    pub amount: f64,
    pub currency: String,
}

/// Store the events of one fetch: each `(ticker, ex_date)` is replaced by the newer answer, and
/// the coverage says the ticker's events are known from `covered_from` on.
pub fn record_events(
    conn: &Connection,
    ticker: &str,
    events: &[DividendEvent],
    covered_from: i64,
    now: i64,
) -> Result<()> {
    let ticker = ticker.trim().to_uppercase();
    let tx = conn.unchecked_transaction()?;
    for event in events {
        if !(event.amount.is_finite() && event.amount > 0.0) {
            continue;
        }
        tx.execute(
            "INSERT INTO stock_dividend_events (ticker, ex_date, amount, currency, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(ticker, ex_date) DO UPDATE SET
               amount = excluded.amount, currency = excluded.currency,
               fetched_at = excluded.fetched_at",
            rusqlite::params![
                ticker,
                event.ex_date.div_euclid(DAY) * DAY,
                amount_text(event.amount),
                event.currency,
                now
            ],
        )?;
    }
    tx.execute(
        "INSERT INTO stock_dividend_coverage (ticker, covered_from, fetched_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(ticker) DO UPDATE SET
           covered_from = MIN(covered_from, excluded.covered_from),
           fetched_at = excluded.fetched_at",
        rusqlite::params![ticker, covered_from.div_euclid(DAY) * DAY, now],
    )?;
    tx.commit()?;
    Ok(())
}

/// The day from which the ticker's Yahoo events must be fetched, or `None` when the stored ones
/// are fresh enough: never fetched, fetched more than `max_age` seconds ago, or covering less
/// than the history from `first_day` (a backdated buy).
pub fn events_fetch_from(
    conn: &Connection,
    ticker: &str,
    first_day: i64,
    now: i64,
    max_age: i64,
) -> Result<Option<i64>> {
    let first_day = first_day.div_euclid(DAY) * DAY;
    let coverage: Option<(i64, i64)> = conn
        .query_row(
            "SELECT covered_from, fetched_at FROM stock_dividend_coverage WHERE ticker = ?1",
            [ticker.trim().to_uppercase()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match coverage {
        Some((from, fetched_at)) if from <= first_day && now - fetched_at < max_age => None,
        Some((from, _)) => Some(from.min(first_day)),
        None => Some(first_day),
    })
}

/// Whether Yahoo Finance has dividends for the ticker.
pub fn has_yahoo_dividends(conn: &Connection, ticker: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM stock_dividend_events WHERE ticker = ?1)",
        [ticker.trim().to_uppercase()],
        |row| row.get(0),
    )?)
}

fn ticker_of(conn: &Connection, investment_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT ticker FROM stock_investments WHERE id = ?1",
        [investment_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Investment not found".into()))
}

/// The Yahoo payouts of a position: every event after its first transaction up to `today`
/// with shares held the day before; newest first.
fn yahoo_payouts(
    conn: &Connection,
    investment_id: &str,
    ticker: &str,
    today: i64,
) -> Result<Vec<StockDividend>> {
    let first_day: Option<i64> = conn.query_row(
        "SELECT MIN(transaction_date) FROM investment_transactions WHERE investment_id = ?1",
        [investment_id],
        |row| row.get(0),
    )?;
    let Some(first_day) = first_day else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT ex_date, amount, currency FROM stock_dividend_events
         WHERE ticker = ?1 AND ex_date > ?2 AND ex_date <= ?3
         ORDER BY ex_date DESC",
    )?;
    let events = stmt
        .query_map(
            rusqlite::params![ticker, first_day.div_euclid(DAY) * DAY, today],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut payouts = Vec::new();
    for (ex_date, amount, currency) in events {
        let Ok(per_share) = amount.parse::<f64>() else {
            continue;
        };
        let shares = held_quantity_at(
            conn,
            "investment_transactions",
            investment_id,
            ex_date - DAY,
        )?;
        if shares <= 0.0 {
            continue;
        }
        payouts.push(StockDividend {
            id: format!("yahoo:{ticker}:{ex_date}"),
            investment_id: investment_id.to_string(),
            source: StockDividendSource::Yahoo,
            ex_date,
            amount_per_share: Some(amount),
            shares: Some(amount_text(shares)),
            gross_amount: amount_text(per_share * shares),
            currency,
            note: None,
            superseded: false,
        });
    }
    Ok(payouts)
}

const MANUAL_COLUMNS: &str = "id, investment_id, ex_date, gross_amount, currency, note";

fn read_manual(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockDividend> {
    Ok(StockDividend {
        id: row.get(0)?,
        investment_id: row.get(1)?,
        source: StockDividendSource::Manual,
        ex_date: row.get(2)?,
        amount_per_share: None,
        shares: None,
        gross_amount: row.get(3)?,
        currency: row.get(4)?,
        note: row.get(5)?,
        superseded: false,
    })
}

fn manual_dividends(conn: &Connection, investment_id: &str) -> Result<Vec<StockDividend>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MANUAL_COLUMNS} FROM investment_dividends WHERE investment_id = ?1
         ORDER BY ex_date DESC, created_at DESC, rowid DESC"
    ))?;
    let rows = stmt.query_map([investment_id], read_manual)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One manual dividend by id.
pub fn get(conn: &Connection, id: &str) -> Result<StockDividend> {
    conn.query_row(
        &format!("SELECT {MANUAL_COLUMNS} FROM investment_dividends WHERE id = ?1"),
        [id],
        read_manual,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Dividend not found".into()))
}

/// Every dividend of a position, newest ex-date first: Yahoo's payouts when Yahoo has dividends
/// for the ticker (the manual ones then follow as superseded), else the manual ones.
pub fn for_investment(
    conn: &Connection,
    investment_id: &str,
    today: i64,
) -> Result<(Vec<StockDividend>, bool)> {
    let ticker = ticker_of(conn, investment_id)?;
    let from_yahoo = has_yahoo_dividends(conn, &ticker)?;
    let mut items = if from_yahoo {
        yahoo_payouts(conn, investment_id, &ticker, today)?
    } else {
        Vec::new()
    };
    let mut manual = manual_dividends(conn, investment_id)?;
    for dividend in &mut manual {
        dividend.superseded = from_yahoo;
    }
    items.extend(manual);
    items.sort_by_key(|d| std::cmp::Reverse(d.ex_date));
    Ok((items, from_yahoo))
}

/// A counted payout of a position, for reports (the GIPS return engine): never a superseded
/// manual one.
#[derive(Debug, Clone, PartialEq)]
pub struct Payout {
    pub investment_id: String,
    pub ticker: String,
    pub ex_date: i64,
    /// Gross total in `currency`.
    pub gross: f64,
    pub currency: String,
    pub source: StockDividendSource,
}

/// Every counted payout of every stock position up to `today`, oldest first.
pub fn payouts(conn: &Connection, today: i64) -> Result<Vec<Payout>> {
    let mut stmt = conn.prepare("SELECT id, ticker FROM stock_investments")?;
    let positions = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut all = Vec::new();
    for (investment_id, ticker) in positions {
        let (items, _) = for_investment(conn, &investment_id, today)?;
        all.extend(
            items
                .into_iter()
                .filter(|d| !d.superseded && d.ex_date <= today)
                .filter_map(|d| {
                    Some(Payout {
                        gross: d.gross_amount.parse().ok()?,
                        investment_id: d.investment_id,
                        ticker: ticker.clone(),
                        ex_date: d.ex_date,
                        currency: d.currency,
                        source: d.source,
                    })
                }),
        );
    }
    all.sort_by_key(|p| p.ex_date);
    Ok(all)
}

/// The dividends of a position with the gross sum of the counted ones in `report_currency`:
/// every payout converted at its ex-date's rate (ten-day walk-back; day quotes only up to
/// `today`), else at `current` (flagged estimated), else left out and named in the quality.
/// Never 1:1.
pub fn list(
    conn: &Connection,
    investment_id: &str,
    report_currency: &str,
    current: &Quotes,
    today: i64,
) -> Result<StockDividendList> {
    let report = report_currency.trim().to_uppercase();
    let (items, from_yahoo) = for_investment(conn, investment_id, today)?;
    let counted: Vec<&StockDividend> = items.iter().filter(|d| !d.superseded).collect();
    let mut quality = Quality::default();
    let (mut sum, mut converted) = (0.0, 0usize);
    if let (Some(first), Some(last)) = (
        counted.iter().map(|d| d.ex_date).min(),
        counted.iter().map(|d| d.ex_date).max(),
    ) {
        let history = History::load(conn, first.min(today), last.min(today))?;
        for item in &counted {
            let Ok(gross) = item.gross_amount.parse::<f64>() else {
                quality.record_missing_conversion([item.currency.as_str()]);
                continue;
            };
            // A day without its own quotes yet (today, a future day) uses the current ones.
            let on_day = (item.ex_date <= today)
                .then(|| history.convert_on(gross, &item.currency, &report, item.ex_date))
                .flatten();
            let value = match on_day {
                Some(value) => Some(value),
                None => {
                    let value = current.convert(gross, &item.currency, &report);
                    if value.is_some() {
                        quality.mark_estimated();
                    }
                    value
                }
            };
            match value {
                Some(value) => {
                    sum += value;
                    converted += 1;
                }
                None => {
                    quality.track_conversion(current, &item.currency, &report, None);
                }
            }
        }
    }
    Ok(StockDividendList {
        total_gross: (counted.is_empty() || converted > 0).then(|| money_in_currency(sum, &report)),
        currency: report,
        fx_quality: ConversionQuality::from(&quality),
        items,
        from_yahoo,
    })
}

fn refuse_when_yahoo_has_dividends(conn: &Connection, investment_id: &str) -> Result<()> {
    let ticker = ticker_of(conn, investment_id)?;
    if has_yahoo_dividends(conn, &ticker)? {
        return Err(AppError::validation_with(
            "validation.dividendFromYahoo",
            serde_json::json!({ "ticker": ticker }),
        ));
    }
    Ok(())
}

/// Enter a dividend by hand (detail page). Refused for a ticker Yahoo has dividends for.
pub fn create(
    conn: &Connection,
    investment_id: &str,
    data: &InsertStockDividend,
) -> Result<StockDividend> {
    let valid: ValidDividend = data.validate()?;
    refuse_when_yahoo_has_dividends(conn, investment_id)?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO investment_dividends (id, investment_id, ex_date, gross_amount, currency, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            id,
            investment_id,
            valid.ex_date,
            valid.gross_amount,
            valid.currency,
            valid.note
        ],
    )?;
    get(conn, &id)
}

/// Replace the values of a manual dividend. Refused once Yahoo has dividends for the ticker (the
/// superseded record can still be deleted).
pub fn update(conn: &Connection, id: &str, data: &InsertStockDividend) -> Result<StockDividend> {
    let valid = data.validate()?;
    let current = get(conn, id)?;
    refuse_when_yahoo_has_dividends(conn, &current.investment_id)?;
    conn.execute(
        "UPDATE investment_dividends SET ex_date = ?1, gross_amount = ?2, currency = ?3, note = ?4
         WHERE id = ?5",
        rusqlite::params![
            valid.ex_date,
            valid.gross_amount,
            valid.currency,
            valid.note,
            id
        ],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let changed = conn.execute("DELETE FROM investment_dividends WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(AppError::NotFound("Dividend not found".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::investments::recalculate_investment_metrics;

    const D1: i64 = 20_000 * DAY;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(&format!(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL UNIQUE, company_name TEXT NOT NULL,
                quantity TEXT NOT NULL, currency TEXT NOT NULL DEFAULT 'USD'
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
            CREATE TABLE stock_dividend_events (
                ticker TEXT NOT NULL, ex_date INTEGER NOT NULL, amount TEXT NOT NULL,
                currency TEXT NOT NULL, fetched_at INTEGER NOT NULL, PRIMARY KEY (ticker, ex_date)
            );
            CREATE TABLE stock_dividend_coverage (
                ticker TEXT PRIMARY KEY, covered_from INTEGER NOT NULL, fetched_at INTEGER NOT NULL
            );
            CREATE TABLE investment_dividends (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                ex_date INTEGER NOT NULL, gross_amount TEXT NOT NULL, currency TEXT NOT NULL,
                note TEXT, created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL
            );
            INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                VALUES ('inv-aapl', 'AAPL', 'Apple', '10', 'USD'),
                       ('inv-cez', 'CEZ.PR', 'ČEZ', '5', 'CZK');
            INSERT INTO investment_transactions
                (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency,
                 transaction_date)
                VALUES ('t1', 'inv-aapl', 'buy', 'AAPL', 'Apple', '10', '150', 'USD', {d1}),
                       ('t2', 'inv-cez', 'buy', 'CEZ.PR', 'ČEZ', '5', '900', 'CZK', {d1});
            "#,
            d1 = D1
        ))
        .expect("schema");
        conn
    }

    fn input(day: i64, gross: &str, currency: &str) -> InsertStockDividend {
        InsertStockDividend {
            ex_date: day,
            gross_amount: gross.into(),
            currency: currency.into(),
            note: None,
        }
    }

    fn event(day: i64, amount: f64) -> DividendEvent {
        DividendEvent {
            ex_date: day,
            amount,
            currency: "USD".into(),
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
    fn a_yahoo_payout_is_the_per_share_amount_times_the_shares_held_the_day_before() {
        let conn = setup_test_db();
        // 5 more shares bought on the ex-date itself: they get no dividend.
        conn.execute(
            "INSERT INTO investment_transactions
                (id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                 currency, transaction_date)
             VALUES ('t3', 'inv-aapl', 'buy', 'AAPL', 'Apple', '5', '160', 'USD', ?1)",
            [D1 + 30 * DAY],
        )
        .unwrap();
        record_events(
            &conn,
            "aapl",
            &[
                event(D1 - 30 * DAY, 0.24),
                event(D1 + 30 * DAY, 0.25),
                event(D1 + 120 * DAY, 0.26),
            ],
            D1,
            D1 + 200 * DAY,
        )
        .unwrap();
        let (items, from_yahoo) = for_investment(&conn, "inv-aapl", D1 + 100 * DAY).unwrap();
        assert!(from_yahoo);
        // Before the first buy: none; after today: none.
        assert_eq!(items.len(), 1);
        let payout = &items[0];
        assert_eq!(payout.source, StockDividendSource::Yahoo);
        assert_eq!(payout.ex_date, D1 + 30 * DAY);
        assert_eq!(payout.shares.as_deref(), Some("10"));
        assert_eq!(payout.amount_per_share.as_deref(), Some("0.25"));
        assert_eq!(payout.gross_amount, "2.5");
        assert_eq!(payout.id, format!("yahoo:AAPL:{}", D1 + 30 * DAY));
    }

    #[test]
    fn manual_dividends_count_only_without_yahoo_dividends() {
        let conn = setup_test_db();
        let manual = create(&conn, "inv-aapl", &input(D1 + 10 * DAY, "3", "USD")).unwrap();
        let (items, from_yahoo) = for_investment(&conn, "inv-aapl", D1 + 100 * DAY).unwrap();
        assert!(!from_yahoo);
        assert_eq!(items, vec![manual.clone()]);

        record_events(&conn, "AAPL", &[event(D1 + 30 * DAY, 0.25)], D1, D1).unwrap();
        let (items, from_yahoo) = for_investment(&conn, "inv-aapl", D1 + 100 * DAY).unwrap();
        assert!(from_yahoo);
        assert_eq!(items.len(), 2);
        let superseded = items.iter().find(|d| d.id == manual.id).unwrap();
        assert!(superseded.superseded);
        let counted = payouts(&conn, D1 + 100 * DAY).unwrap();
        assert_eq!(counted.len(), 1);
        assert_eq!(counted[0].source, StockDividendSource::Yahoo);
        assert_eq!(
            (counted[0].ticker.as_str(), counted[0].gross),
            ("AAPL", 2.5)
        );
    }

    #[test]
    fn no_manual_dividend_for_a_ticker_yahoo_has_dividends_for() {
        let conn = setup_test_db();
        let manual = create(&conn, "inv-aapl", &input(D1, "3", "USD")).unwrap();
        record_events(&conn, "AAPL", &[event(D1 + 30 * DAY, 0.25)], D1, D1).unwrap();
        let refused = |r: Result<StockDividend>| match r {
            Err(AppError::Validation(key)) => key.starts_with("validation.dividendFromYahoo"),
            _ => false,
        };
        assert!(refused(create(&conn, "inv-aapl", &input(D1, "3", "USD"))));
        assert!(refused(update(&conn, &manual.id, &input(D1, "4", "USD"))));
        // The superseded one can still go.
        delete(&conn, &manual.id).unwrap();
        // Another ticker without Yahoo dividends still takes one.
        assert!(create(&conn, "inv-cez", &input(D1, "120", "CZK")).is_ok());
    }

    #[test]
    fn create_update_and_delete_a_manual_dividend() {
        let conn = setup_test_db();
        let created = create(&conn, "inv-cez", &input(D1 + 600, "120", "czk")).unwrap();
        assert_eq!(
            (created.ex_date, created.gross_amount.as_str()),
            (D1, "120")
        );
        assert_eq!(
            (created.source, created.currency.as_str()),
            (StockDividendSource::Manual, "CZK")
        );
        let mut changed = input(D1 + DAY, "130", "CZK");
        changed.note = Some("mimořádná".into());
        let updated = update(&conn, &created.id, &changed).unwrap();
        assert_eq!(
            (updated.ex_date, updated.gross_amount.as_str()),
            (D1 + DAY, "130")
        );
        assert_eq!(updated.note.as_deref(), Some("mimořádná"));
        delete(&conn, &created.id).unwrap();
        assert!(matches!(
            delete(&conn, &created.id),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            update(&conn, "missing", &changed),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            create(&conn, "missing", &input(D1, "1", "USD")),
            Err(AppError::NotFound(_))
        ));
    }

    /// The guard of ADR 0022: a dividend is not a trade, whatever recomputes the position.
    #[test]
    fn a_dividend_never_changes_the_position_quantity() {
        let conn = setup_test_db();
        create(&conn, "inv-aapl", &input(D1, "24", "USD")).unwrap();
        record_events(&conn, "AAPL", &[event(D1 + DAY, 0.25)], D1, D1).unwrap();
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
    fn deleting_the_position_deletes_its_manual_dividends() {
        let conn = setup_test_db();
        create(&conn, "inv-cez", &input(D1, "24", "CZK")).unwrap();
        conn.execute("DELETE FROM stock_investments WHERE id = 'inv-cez'", [])
            .unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM investment_dividends", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn the_total_converts_every_payout_at_its_ex_date() {
        let conn = setup_test_db();
        // 1 EUR = 25 CZK = 1.25 USD on D1 + 1 (20 CZK per USD); 24 CZK = 1.2 USD on D1 + 2.
        conn.execute_batch(&format!(
            "INSERT INTO exchange_rate_history VALUES ({a}, 'CZK', 25.0), ({a}, 'USD', 1.25),
                ({b}, 'CZK', 24.0), ({b}, 'USD', 1.2);",
            a = D1 + DAY,
            b = D1 + 2 * DAY
        ))
        .unwrap();
        record_events(
            &conn,
            "AAPL",
            &[event(D1 + DAY, 1.0), event(D1 + 2 * DAY, 2.0)],
            D1,
            D1,
        )
        .unwrap();
        let report = list(
            &conn,
            "inv-aapl",
            "CZK",
            &eur_quotes(30.0, 1.0),
            D1 + 10 * DAY,
        )
        .unwrap();
        assert!(report.from_yahoo);
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].ex_date, D1 + 2 * DAY, "newest first");
        // 10 USD × 20 + 20 USD × 20.
        assert_eq!(report.total_gross.as_deref(), Some("600.00"));
        assert!(report.fx_quality.missing_currencies.is_empty());
        assert!(!report.fx_quality.estimated);
    }

    #[test]
    fn superseded_dividends_are_left_out_of_the_total() {
        let conn = setup_test_db();
        create(&conn, "inv-aapl", &input(D1 + DAY, "100", "USD")).unwrap();
        record_events(&conn, "AAPL", &[event(D1 + DAY, 1.0)], D1, D1).unwrap();
        let report = list(
            &conn,
            "inv-aapl",
            "USD",
            &eur_quotes(25.0, 1.25),
            D1 + 5 * DAY,
        )
        .unwrap();
        assert_eq!(report.total_gross.as_deref(), Some("10.00"));
    }

    #[test]
    fn a_day_without_quotes_uses_todays_and_an_unconvertible_one_is_named() {
        let conn = setup_test_db();
        create(&conn, "inv-cez", &input(D1, "100", "CZK")).unwrap();
        let report = list(&conn, "inv-cez", "USD", &eur_quotes(25.0, 1.25), D1 + DAY).unwrap();
        assert_eq!(report.total_gross.as_deref(), Some("5.00"));
        assert!(report.fx_quality.estimated);

        let report = list(&conn, "inv-cez", "USD", &Quotes::new("EUR"), D1 + DAY).unwrap();
        assert_eq!(report.total_gross, None, "payouts exist, none converted");
        assert!(!report.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn events_are_fetched_when_missing_stale_or_not_covering_the_first_trade() {
        let conn = setup_test_db();
        let now = D1 + 100 * DAY;
        assert_eq!(
            events_fetch_from(&conn, "AAPL", D1, now, DAY).unwrap(),
            Some(D1)
        );
        record_events(&conn, "AAPL", &[], D1, now).unwrap();
        assert_eq!(
            events_fetch_from(&conn, "AAPL", D1, now + 3600, DAY).unwrap(),
            None
        );
        assert_eq!(
            events_fetch_from(&conn, "AAPL", D1, now + DAY, DAY).unwrap(),
            Some(D1)
        );
        // A backdated buy before the covered range.
        assert_eq!(
            events_fetch_from(&conn, "AAPL", D1 - 50 * DAY, now + 3600, DAY).unwrap(),
            Some(D1 - 50 * DAY)
        );
        // A refetch keeps the widest range and replaces an amount.
        record_events(&conn, "AAPL", &[event(D1 + DAY, 0.2)], D1 + 10 * DAY, now).unwrap();
        record_events(&conn, "AAPL", &[event(D1 + DAY, 0.25)], D1 + 10 * DAY, now).unwrap();
        let from: i64 = conn
            .query_row(
                "SELECT covered_from FROM stock_dividend_coverage",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(from, D1);
        let amount: String = conn
            .query_row("SELECT amount FROM stock_dividend_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(amount, "0.25");
        assert!(!has_yahoo_dividends(&conn, "CEZ.PR").unwrap());
    }
}
