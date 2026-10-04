//! The atomic import of one file as a recorded batch.
//!
//! The import re-runs the preview's simulation on the same file and
//! configuration (so it excludes exactly the rows the preview excluded) and
//! writes the rows it accepts through the shared bulk writer
//! (`services::investments::bulk_create_stock_transactions`, ADR 0007): one SQL
//! transaction that also records the batch. A rule failure still found while
//! writing aborts the whole import with that row's message.

use super::simulate::{simulate, SimulatedTrade, TradeOutcome};
use super::types::{ParsedFile, StockImportConfig, StockImportResult, StockRowMessage};
use crate::error::{AppError, Result};
use crate::services::investments::{
    bulk_create_stock_transactions, BulkStockRow, NewStockImportBatch,
};

/// Import the rows the preview would import, in one SQL transaction, as a
/// recorded batch (none when nothing is imported).
pub fn import(
    conn: &mut rusqlite::Connection,
    parsed: &ParsedFile,
    config: &StockImportConfig,
    file_name: &str,
) -> Result<StockImportResult> {
    config.validate()?;
    let simulation = simulate(conn, parsed, config)?;

    // The simulation decided duplicates (numerically, and by broker id), so
    // every row it accepted is written even when it looks identical in text to
    // another one.
    let accepted: Vec<(&SimulatedTrade, BulkStockRow)> = simulation
        .trades
        .iter()
        .filter(|trade| trade.will_import())
        .filter_map(|trade| {
            Some((
                trade,
                BulkStockRow {
                    ticker: trade.ticker.clone()?,
                    company_name: trade.company_name.clone(),
                    tx_type: trade.direction.as_str().to_string(),
                    quantity: trade.quantity_text(),
                    price_per_unit: trade.price_text(),
                    currency: trade.currency.clone()?,
                    transaction_date: trade.day,
                    external_id: trade.external_id.clone(),
                    allow_duplicate: true,
                },
            ))
        })
        .collect();

    let mut messages: Vec<StockRowMessage> = parsed
        .skipped
        .iter()
        .chain(&parsed.errors)
        .cloned()
        .chain(
            simulation
                .trades
                .iter()
                .filter_map(|t| t.left_out_message()),
        )
        .collect();
    messages.sort_by_key(|m| m.line);

    let mut result = StockImportResult {
        batch_id: None,
        imported: 0,
        duplicates: 0,
        skipped: parsed.skipped.len(),
        errors: parsed.errors.len(),
        new_positions: Vec::new(),
        updated_positions: Vec::new(),
        messages,
        earliest_day: None,
    };
    for trade in &simulation.trades {
        match &trade.outcome {
            TradeOutcome::Duplicate { forced: false, .. } => result.duplicates += 1,
            TradeOutcome::Skipped(_) => result.skipped += 1,
            TradeOutcome::Error(_) => result.errors += 1,
            TradeOutcome::New | TradeOutcome::Duplicate { forced: true, .. } => {}
        }
    }
    if accepted.is_empty() {
        return Ok(result);
    }

    let rows: Vec<BulkStockRow> = accepted.iter().map(|(_, row)| row.clone()).collect();
    let batch = NewStockImportBatch {
        file_name: file_name.to_string(),
        source: config.source.clone(),
    };
    let report = bulk_create_stock_transactions(conn, &rows, Some(&batch))?;
    if let Some(issue) = report.errors.first() {
        let line = accepted.get(issue.index).map_or(0, |(trade, _)| trade.line);
        let reason = issue
            .message
            .strip_prefix("Validation error: ")
            .unwrap_or(&issue.message);
        return Err(AppError::Validation(format!("line {line}: {reason}")));
    }

    result.batch_id = report.batch_id;
    result.imported = report.created;
    result.updated_positions = report
        .written_tickers
        .iter()
        .filter(|ticker| !report.created_positions.contains(ticker))
        .cloned()
        .collect();
    result.new_positions = report.created_positions;
    result.earliest_day = accepted.iter().map(|(trade, _)| trade.day).min();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::preview::preview;
    use super::super::simulate::test_db::*;
    use super::super::simulate::{
        KEY_DUPLICATE, KEY_INSTRUMENT_SKIPPED, KEY_SELL_EXCEEDS_HOLDINGS,
    };
    use super::super::types::{
        ParsedTrade, StockInstrumentOverride, StockRowMessage, StockRowStatus,
    };
    use super::*;

    fn message(line: usize, key: &str) -> StockRowMessage {
        StockRowMessage {
            line,
            key: key.to_string(),
            detail: None,
        }
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// `(ticker, type, quantity, price, currency, day, batch id, external id)`.
    type StoredRow = (
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<String>,
        Option<String>,
    );

    fn stored(conn: &Connection) -> Vec<StoredRow> {
        let mut stmt = conn
            .prepare(
                "SELECT ticker, type, quantity, price_per_unit, currency, transaction_date,
                        import_batch_id, external_id
                 FROM investment_transactions ORDER BY rowid",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn position(conn: &Connection, ticker: &str) -> Option<(String, String, String)> {
        conn.query_row(
            "SELECT company_name, quantity, currency FROM stock_investments WHERE ticker = ?1",
            [ticker],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()
    }

    fn file(trades: Vec<ParsedTrade>) -> ParsedFile {
        parsed(trades)
    }

    #[test]
    fn writes_the_importable_rows_as_one_recorded_batch() {
        let mut conn = db();
        let mut a = with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "111");
        a.name = Some("Apple Inc.".into());
        let b = with_id(buy(3, day(1), "AAPL", 5.0, 110.5), "112");
        let c = with_id(sell(4, day(2), "AAPL", 4.0, 120.0), "113");
        let d = with_id(buy(6, day(1), "MSFT", 2.0, 300.0), "114");

        let result = import(
            &mut conn,
            &file(vec![a, b, c, d]),
            &config("xtb"),
            "xtb.csv",
        )
        .unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(
            (result.duplicates, result.skipped, result.errors),
            (0, 0, 0)
        );
        assert!(result.messages.is_empty());
        assert_eq!(result.new_positions, vec!["AAPL", "MSFT"]);
        assert!(result.updated_positions.is_empty());
        assert_eq!(result.earliest_day, Some(day(0)));
        let batch_id = result.batch_id.clone().expect("a batch");

        let rows = stored(&conn);
        assert_eq!(rows.len(), 4);
        assert!(rows
            .iter()
            .all(|r| r.6.as_deref() == Some(batch_id.as_str())));
        let ids: Vec<&str> = rows.iter().filter_map(|r| r.7.as_deref()).collect();
        assert_eq!(ids.len(), 4, "every row keeps its broker id");
        assert!(ids.contains(&"xtb:111") && ids.contains(&"xtb:114"));
        assert!(
            rows.iter().any(|r| r.2 == "5" && r.3 == "110.5"),
            "{rows:?}"
        );

        let (file_name, source, trades): (String, String, i64) = conn
            .query_row(
                "SELECT file_name, source, trade_count FROM stock_import_batches WHERE id = ?1",
                [&batch_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (file_name.as_str(), source.as_str(), trades),
            ("xtb.csv", "xtb", 4)
        );

        // 10 + 5 - 4 held; the position carries the name from the file.
        let aapl = position(&conn, "AAPL").unwrap();
        assert_eq!(aapl.0, "Apple Inc.");
        assert_eq!(aapl.1, "11");
        assert_eq!(aapl.2, "USD");
        assert_eq!(
            position(&conn, "MSFT").unwrap().0,
            "MSFT",
            "no name: the ticker"
        );
    }

    #[test]
    fn rows_that_are_not_imported_are_reported_in_file_order() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            skip: true,
            ..over("symbol:OPT")
        }];
        let mut f = file(vec![
            buy(2, day(0), "AAPL", 10.0, 100.0),  // duplicate
            buy(3, day(1), "AAPL", 1.0, 110.0),   // imported
            buy(5, day(1), "OPT", 1.0, 1.0),      // skipped instrument
            sell(7, day(2), "AAPL", 50.0, 100.0), // too much
        ]);
        f.skipped = vec![message(4, "importWizard.row.notATrade")];
        f.errors = vec![message(6, "importWizard.row.dateUnparseable")];
        f.total_rows = 6;

        let result = import(&mut conn, &f, &cfg, "file.csv").unwrap();

        assert_eq!(result.imported, 1);
        assert_eq!(result.duplicates, 1);
        assert_eq!(result.skipped, 2, "one parsed, one skipped instrument");
        assert_eq!(result.errors, 2, "one parsed, one sell above the holding");
        let keys: Vec<(usize, &str)> = result
            .messages
            .iter()
            .map(|m| (m.line, m.key.as_str()))
            .collect();
        assert_eq!(
            keys,
            vec![
                (2, KEY_DUPLICATE),
                (4, "importWizard.row.notATrade"),
                (5, KEY_INSTRUMENT_SKIPPED),
                (6, "importWizard.row.dateUnparseable"),
                (7, KEY_SELL_EXCEEDS_HOLDINGS),
            ]
        );
        assert_eq!(result.updated_positions, vec!["AAPL"]);
        assert!(result.new_positions.is_empty());
        assert_eq!(count(&conn, "investment_transactions"), 2);
        assert!(position(&conn, "OPT").is_none());
    }

    #[test]
    fn a_trade_added_to_an_existing_position_carries_its_name_and_updates_its_quantity() {
        let mut conn = db();
        add_position(&conn, "AAPL", "Apple Inc.", "USD");
        add_stored(&conn, "AAPL", "buy", "10", "100", day(-5), None);
        let mut t = buy(2, day(0), "AAPL", 3.0, 120.0);
        t.name = Some("APPLE INC".into());

        let result = import(&mut conn, &file(vec![t]), &config("custom"), "f.csv").unwrap();

        assert_eq!(result.updated_positions, vec!["AAPL"]);
        assert!(result.new_positions.is_empty());
        let name: String = conn
            .query_row(
                "SELECT company_name FROM investment_transactions WHERE import_batch_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "Apple Inc.");
        assert_eq!(position(&conn, "AAPL").unwrap().1, "13");
    }

    #[test]
    fn importing_the_same_file_again_finds_only_duplicates_and_records_no_batch() {
        let mut conn = db();
        let rows = || {
            vec![
                with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "1"),
                with_id(buy(3, day(0), "AAPL", 10.0, 100.0), "2"), // alike, another trade
                with_id(sell(4, day(1), "AAPL", 5.0, 110.0), "3"),
            ]
        };
        let first = import(&mut conn, &file(rows()), &config("trading212"), "t212.csv").unwrap();
        assert_eq!(first.imported, 3);

        let again = import(&mut conn, &file(rows()), &config("trading212"), "t212.csv").unwrap();

        assert_eq!(again.imported, 0);
        assert_eq!(again.duplicates, 3);
        assert_eq!(again.batch_id, None);
        assert!(again.new_positions.is_empty() && again.updated_positions.is_empty());
        assert_eq!(again.earliest_day, None);
        assert_eq!(count(&conn, "investment_transactions"), 3);
        assert_eq!(
            count(&conn, "stock_import_batches"),
            1,
            "no batch for a no-op"
        );
        assert_eq!(position(&conn, "AAPL").unwrap().1, "15");
    }

    #[test]
    fn nothing_to_import_changes_nothing() {
        let mut conn = db();
        let mut f = file(vec![]);
        f.skipped = vec![message(2, "importWizard.row.notATrade")];
        f.total_rows = 1;
        let result = import(&mut conn, &f, &config("custom"), "empty.csv").unwrap();
        assert_eq!(result.batch_id, None);
        assert_eq!((result.imported, result.skipped), (0, 1));
        assert_eq!(result.messages.len(), 1);
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn a_duplicate_imported_anyway_is_written() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let mut cfg = config("custom");
        cfg.import_anyway_lines = vec![2];
        let f = file(vec![
            buy(2, day(0), "AAPL", 10.0, 100.0), // forced
            buy(3, day(0), "AAPL", 10.0, 100.0), // left out
        ]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!((result.imported, result.duplicates), (1, 1));
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].line, 3);
        assert_eq!(count(&conn, "investment_transactions"), 2);
    }

    #[test]
    fn identical_looking_rows_with_different_broker_ids_are_both_written() {
        let mut conn = db();
        let f = file(vec![
            with_id(buy(2, day(0), "AAPL", 5.0, 100.0), "1"),
            with_id(buy(3, day(0), "AAPL", 5.0, 100.0), "2"),
        ]);
        let result = import(&mut conn, &f, &config("xtb"), "f.csv").unwrap();
        assert_eq!(
            result.imported, 2,
            "the writer's own exact-text check stays out of it"
        );
        assert_eq!(position(&conn, "AAPL").unwrap().1, "10");
    }

    #[test]
    fn overrides_decide_the_ticker_name_and_currency_of_a_new_position() {
        let mut conn = db();
        let mut cfg = config("degiro");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            key: "isin:IE00B3RBWM25".into(),
            ticker: Some("vwrl.as".into()),
            name: Some("Vanguard FTSE All-World".into()),
            currency: Some("eur".into()),
            skip: false,
        }];
        let mut t = buy(2, day(0), "X", 3.0, 100.0);
        t.instrument_key = "isin:IE00B3RBWM25".into();
        t.symbol = None;
        t.isin = Some("IE00B3RBWM25".into());
        t.currency = Some("USD".into()); // overridden

        let result = import(&mut conn, &file(vec![t]), &cfg, "degiro.csv").unwrap();

        assert_eq!(result.new_positions, vec!["VWRL.AS"]);
        let p = position(&conn, "VWRL.AS").unwrap();
        assert_eq!(
            (p.0.as_str(), p.2.as_str()),
            ("Vanguard FTSE All-World", "EUR")
        );
    }

    #[test]
    fn a_failure_while_writing_rolls_the_whole_import_back() {
        let mut conn = db();
        // The data "changed in between": the database refuses one of the rows.
        conn.execute_batch(
            "CREATE TRIGGER refuse_bad BEFORE INSERT ON investment_transactions
             WHEN NEW.ticker = 'BAD'
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
        let f = file(vec![
            buy(2, day(0), "AAPL", 1.0, 1.0),
            buy(3, day(1), "BAD", 1.0, 1.0),
            buy(4, day(2), "MSFT", 1.0, 1.0),
        ]);

        let err = import(&mut conn, &f, &config("custom"), "f.csv").unwrap_err();

        let text = err.to_string();
        assert!(text.contains("line 3"), "names the file line: {text}");
        assert!(text.contains("refused"), "{text}");
        assert_eq!(count(&conn, "stock_investments"), 0);
        assert_eq!(count(&conn, "investment_transactions"), 0);
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn an_incomplete_configuration_is_refused_before_anything_is_written() {
        let mut conn = db();
        let mut cfg = config("custom");
        cfg.symbol_column = None;
        let err = import(
            &mut conn,
            &file(vec![buy(2, day(0), "AAPL", 1.0, 1.0)]),
            &cfg,
            "f.csv",
        )
        .unwrap_err();
        assert!(
            matches!(err, AppError::Validation(k) if k == "validation.stockImportSymbolRequired")
        );
        assert_eq!(count(&conn, "stock_investments"), 0);
    }

    #[test]
    fn the_import_writes_exactly_what_the_preview_promised() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(-3), None);
        let mut f = file(vec![
            sell(2, day(1), "AAPL", 4.0, 1.0),
            buy(3, day(0), "AAPL", 10.0, 100.0),
            buy(4, day(0), "MSFT", 1.0, 1.0),
            sell(5, day(0), "MSFT", 2.0, 1.0), // more than the 1 held
            buy(6, day(-3), "AAPL", 10.0, 100.0), // duplicate of the stored buy
        ]);
        f.total_rows = 5;
        let cfg = config("custom");

        let preview = preview(&conn, &f, &cfg).unwrap();
        let importable: Vec<usize> = preview
            .rows
            .iter()
            .filter(|r| r.status == StockRowStatus::New)
            .map(|r| r.line)
            .collect();
        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!(result.imported, preview.counts.will_import);
        assert_eq!(result.duplicates, preview.counts.duplicates);
        assert_eq!(result.errors, preview.counts.errors);
        assert_eq!(importable, vec![2, 3, 4]);
        let mut written: Vec<(String, String)> = stored(&conn)
            .into_iter()
            .filter(|r| r.6.is_some())
            .map(|r| (r.0, r.2))
            .collect();
        written.sort();
        assert_eq!(
            written,
            vec![
                ("AAPL".to_string(), "10".to_string()),
                ("AAPL".to_string(), "4".to_string()),
                ("MSFT".to_string(), "1".to_string()),
            ]
        );
    }
}
