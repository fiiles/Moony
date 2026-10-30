//! The Moony table as the app exports it: the file the Moony adapter reads back.
//!
//! `Date,Type,Ticker,Name,Quantity,Price,Currency,Amount,Withholding tax`, newest first. A
//! trade fills quantity and price and leaves the two last cells empty; a dividend (spec D15) has
//! `Type = dividend`, empty quantity and price, its gross in `Amount` and its tax in
//! `Withholding tax`. Exporting and importing the file again yields the same rows, and importing
//! it a second time reports them as duplicates.

use chrono::{TimeZone, Utc};
use rusqlite::Connection;

use crate::error::Result;

pub const HEADER: &str = "Date,Type,Ticker,Name,Quantity,Price,Currency,Amount,Withholding tax\n";

fn escape_csv(s: &str) -> String {
    s.replace('"', "\"\"")
}

fn day_text(timestamp: i64) -> String {
    Utc.timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| timestamp.to_string())
}

/// The export of every stock trade and dividend, and how many rows it has.
pub fn stock_csv(conn: &Connection) -> Result<(String, usize)> {
    // (date, line)
    let mut lines: Vec<(i64, String)> = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT t.type, i.ticker, i.company_name, t.quantity, t.price_per_unit, t.currency,
                t.transaction_date
         FROM investment_transactions t
         JOIN stock_investments i ON t.investment_id = i.id
         ORDER BY t.transaction_date DESC",
    )?;
    let trades = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;
    for row in trades {
        let (tx_type, ticker, name, quantity, price, currency, date) = row?;
        lines.push((
            date,
            format!(
                "{},{},{},\"{}\",{},{},{},,\n",
                day_text(date),
                tx_type,
                ticker,
                escape_csv(&name),
                quantity,
                price,
                currency
            ),
        ));
    }

    let mut stmt = conn.prepare(
        "SELECT i.ticker, i.company_name, d.gross_amount, d.withholding_tax, d.currency,
                d.paid_date
         FROM investment_dividends d
         JOIN stock_investments i ON d.investment_id = i.id
         ORDER BY d.paid_date DESC",
    )?;
    let dividends = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    for row in dividends {
        let (ticker, name, gross, tax, currency, date) = row?;
        lines.push((
            date,
            format!(
                "{},dividend,{},\"{}\",,,{},{},{}\n",
                day_text(date),
                ticker,
                escape_csv(&name),
                currency,
                gross,
                tax
            ),
        ));
    }

    // Newest first; a stable sort keeps the trades before the dividends of a day.
    lines.sort_by_key(|(date, _)| std::cmp::Reverse(*date));
    let mut csv = String::from(HEADER);
    for (_, line) in &lines {
        csv.push_str(line);
    }
    Ok((csv, lines.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::test_support::*;
    use crate::services::stock_import::import::import;
    use crate::services::stock_import::inspect::inspect;
    use crate::services::stock_import::parse::parse_file;
    use crate::services::stock_import::preview::preview;
    use crate::services::stock_import::simulate::test_db::*;
    use crate::services::stock_import::types::{StockCsvInspectOptions, StockRowStatus};

    fn seed(conn: &Connection) {
        add_position(conn, "AAPL", "Apple Inc.", "USD");
        add_stored(conn, "AAPL", "buy", "10", "150", day(0), None);
        add_stored(conn, "AAPL", "sell", "2", "180", day(40), None);
        let id = "pos-AAPL";
        for (paid, gross, tax) in [(day(20), "2.4", "0.36"), (day(60), "2.5", "0")] {
            conn.execute(
                "INSERT INTO investment_dividends
                    (id, investment_id, paid_date, gross_amount, withholding_tax, currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'USD')",
                rusqlite::params![format!("d{paid}"), id, paid, gross, tax],
            )
            .unwrap();
        }
    }

    #[test]
    fn dividends_are_exported_newest_first_after_the_new_header() {
        let conn = db();
        seed(&conn);
        let (csv, count) = stock_csv(&conn).unwrap();
        assert_eq!(count, 4);
        assert_eq!(
            csv,
            "Date,Type,Ticker,Name,Quantity,Price,Currency,Amount,Withholding tax\n\
2024-03-01,dividend,AAPL,\"Apple Inc.\",,,USD,2.5,0\n\
2024-02-10,sell,AAPL,\"Apple Inc.\",2,180,USD,,\n\
2024-01-21,dividend,AAPL,\"Apple Inc.\",,,USD,2.4,0.36\n\
2024-01-01,buy,AAPL,\"Apple Inc.\",10,150,USD,,\n"
        );
    }

    /// Export → import into an empty database gives the same dividends; the same file imported
    /// again reports them as duplicates.
    #[test]
    fn an_export_imports_back_and_a_second_import_finds_the_duplicates() {
        let source = db();
        seed(&source);
        let (csv, _) = stock_csv(&source).unwrap();

        let inspection = inspect(
            csv.as_bytes(),
            "stock_transactions.csv",
            &StockCsvInspectOptions::default(),
            &[],
        )
        .unwrap();
        assert_eq!(inspection.detected_source.as_deref(), Some("moony"));
        let config = inspection.config.expect("config");
        assert_eq!(config.amount_column, Some(7));
        assert_eq!(config.tax_column, Some(8));
        let parsed = parse_file(csv.as_bytes(), &config).unwrap();
        assert_eq!(parsed.errors, []);
        assert_eq!(parsed.dividends.len(), 2);
        assert_eq!(parsed.trades.len(), 2);

        let mut target = db();
        let result = import(&mut target, &parsed, &config, "stock_transactions.csv").unwrap();
        assert_eq!((result.imported, result.dividends), (4, 2));
        let dividends = |conn: &Connection| -> Vec<(String, String, String, String)> {
            let mut stmt = conn
                .prepare(
                    "SELECT paid_date || '', gross_amount, withholding_tax, currency
                     FROM investment_dividends ORDER BY paid_date",
                )
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(dividends(&target), dividends(&source));

        let again = preview(&target, &parsed, &config).unwrap();
        assert_eq!(again.counts.duplicates, 4);
        assert_eq!(again.counts.will_import, 0);
        assert!(again
            .rows
            .iter()
            .all(|row| row.status == StockRowStatus::Duplicate));
    }

    #[test]
    fn the_czech_headers_read_the_amount_and_the_tax() {
        let headers = headers(&[
            "Datum",
            "Typ",
            "Symbol",
            "Název",
            "Počet",
            "Cena",
            "Měna",
            "Částka",
            "Srážková daň",
        ]);
        let adapter = crate::services::stock_import::adapters::by_source("moony").unwrap();
        let config = (adapter.config)(&headers, &[]).unwrap().config;
        assert_eq!(
            (config.amount_column, config.tax_column),
            (Some(7), Some(8))
        );
    }
}
