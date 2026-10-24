//! The atomic import of one file as a recorded batch.
//!
//! The import re-runs the preview's simulation on the same file and
//! configuration (so it excludes exactly the rows the preview excluded) and
//! writes the rows it accepts through the shared bulk writer
//! (`services::investments::bulk_create_stock_transactions`, ADR 0007). One SQL
//! transaction holds the split records and the rows; a rule failure still found
//! while writing aborts the whole import with that row's message.
//!
//! Splits (spec D14): the file's action (`convert`/`keep`) and the stored
//! rows' switch (`apply_stored`) are two decisions. A split chosen
//! `apply_stored` that Moony holds as `pending` or does not know is applied to
//! the stored rows (D10) **before** the file's rows are written: the stored
//! rows are in pre-split units, the file's rows already in today's (the
//! preview scaled both the same way). A split Moony has no row for is
//! recorded (source `import`) as `applied` by that apply, else as `pending`
//! when the ticker still has rows dated before it (the user left them as they
//! are: the position page is where the split waits), else as `dismissed` for
//! `keep` or `applied` for `convert` (the file's rows arrive in today's units
//! and nothing waits). Existing `applied` and `dismissed` rows stay, and so
//! does a pending one that is not applied. Undo of the batch deletes its rows
//! and leaves the split records.

use rusqlite::Connection;

use super::simulate::{simulate, ticker_split_choices, SimulatedTrade, TradeOutcome};
use super::types::{
    ParsedFile, StockImportConfig, StockImportInstrument, StockImportResult, StockInstrumentStatus,
    StockRowMessage, StockSplitAction, StockSplitOverride,
};
use crate::error::{AppError, Result};
use crate::services::investments::{
    bulk_create_stock_transactions, BulkStockRow, BulkWriteReport, NewStockImportBatch,
};
use crate::services::stock_splits::{self, StockSplitApplyResult, StockSplitStatus};

/// Import the rows the preview would import, in one SQL transaction, as a
/// recorded batch (none when nothing is imported), together with the file's
/// split choices (D14).
pub fn import(
    conn: &mut Connection,
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
        converted: 0,
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

    let choices = split_choices(&simulation.instruments, config);
    let rows: Vec<BulkStockRow> = accepted.iter().map(|(_, row)| row.clone()).collect();
    let batch = NewStockImportBatch {
        file_name: file_name.to_string(),
        source: config.source.clone(),
    };
    let (report, applied) = in_savepoint(conn, |conn| {
        // The stored rows first, while they are the only ones dated before the splits.
        let applied = record_splits(conn, &choices)?;
        let report = bulk_create_stock_transactions(conn, &rows, Some(&batch))?;
        if let Some(issue) = report.errors.first() {
            let line = accepted.get(issue.index).map_or(0, |(trade, _)| trade.line);
            return Err(row_failure(line, &issue.message));
        }
        Ok((report, applied))
    })?;

    let BulkWriteReport {
        batch_id,
        created,
        created_positions,
        written_tickers,
        ..
    } = report;
    result.batch_id = batch_id;
    result.imported = created;
    result.converted = accepted.iter().filter(|(trade, _)| trade.converted).count();
    // A ticker whose stored rows a split rewrote changed too, imported rows or not.
    let rewritten: Vec<&StockSplitApplyResult> =
        applied.iter().filter(|a| a.rewritten > 0).collect();
    for ticker in written_tickers
        .into_iter()
        .chain(rewritten.iter().map(|a| a.split.ticker.clone()))
    {
        if !created_positions.contains(&ticker) && !result.updated_positions.contains(&ticker) {
            result.updated_positions.push(ticker);
        }
    }
    result.new_positions = created_positions;
    // The history rebuild has to cover the rewritten rows as well as the imported ones.
    result.earliest_day = accepted
        .iter()
        .map(|(trade, _)| trade.day)
        .chain(rewritten.iter().filter_map(|a| a.earliest_day))
        .min();
    Ok(result)
}

/// The import's error for a row the writer refused: the file line and a key.
/// The technical text stays in the log.
fn row_failure(line: usize, message: &str) -> AppError {
    log::debug!("[STOCK IMPORT] line {line}: {message}");
    let reason = message
        .strip_prefix("Validation error: ")
        .filter(|message| message.starts_with("validation."))
        .unwrap_or("validation.stockImportRowFailed");
    AppError::validation_with(
        "validation.stockImportLine",
        serde_json::json!({ "line": line, "reason": reason }),
    )
}

/// The split choices the import acts on, `(ticker, split)` per ticker of a
/// non-skipped instrument in file order, one per split day exactly as the
/// preview scales the stored rows with them (`ticker_split_choices`: a day any
/// instrument of the ticker converts or applies is converted or applied, the
/// first entry counts otherwise).
fn split_choices(
    instruments: &[StockImportInstrument],
    config: &StockImportConfig,
) -> Vec<(String, StockSplitOverride)> {
    let mut tickers: Vec<&str> = Vec::new();
    for instrument in instruments {
        if let Some(ticker) = instrument.ticker.as_deref() {
            if instrument.status != StockInstrumentStatus::Skipped && !tickers.contains(&ticker) {
                tickers.push(ticker);
            }
        }
    }
    let mut choices = Vec::new();
    for ticker in tickers {
        let chosen = ticker_split_choices(
            instruments
                .iter()
                .filter(|i| {
                    i.status != StockInstrumentStatus::Skipped
                        && i.ticker.as_deref() == Some(ticker)
                })
                .filter_map(|i| config.override_for(&i.key))
                .flat_map(|o| &o.splits),
        );
        choices.extend(chosen.into_iter().map(|split| (ticker.to_string(), split)));
    }
    choices
}

/// Records the split choices and applies the `apply_stored` ones the stored
/// rows have not had (D14, see the module docs); returns what each apply did.
/// Runs before the file's rows are written. An apply that would oversell the
/// stored history fails with `validation.stockSplitOversold`.
fn record_splits(
    conn: &Connection,
    choices: &[(String, StockSplitOverride)],
) -> Result<Vec<StockSplitApplyResult>> {
    let mut applied = Vec::new();
    for (ticker, split) in choices {
        match stock_splits::find(conn, ticker, split.effective_day)? {
            None => {
                let waiting = stock_splits::pre_split_count(conn, ticker, split.effective_day)? > 0;
                let converts = split.action == StockSplitAction::Convert;
                if (split.apply_stored && waiting) || (converts && !waiting) {
                    // Rewrite the stored rows (none, for a convert without any: the record
                    // still says the rows before the split are in today's units).
                    let row = stock_splits::insert_from_import(
                        conn,
                        ticker,
                        split,
                        StockSplitStatus::Pending,
                    )?;
                    applied.push(stock_splits::apply(conn, &row.id)?);
                } else {
                    // Stored rows left as they are still wait for the split; none, and a kept
                    // file, is a split with nothing to do.
                    let status = if waiting {
                        StockSplitStatus::Pending
                    } else {
                        StockSplitStatus::Dismissed
                    };
                    stock_splits::insert_from_import(conn, ticker, split, status)?;
                }
            }
            Some(row) if row.status == StockSplitStatus::Pending && split.apply_stored => {
                applied.push(stock_splits::apply(conn, &row.id)?);
            }
            // Applied and dismissed rows stay; a pending split not applied stays pending.
            Some(_) => {}
        }
    }
    Ok(applied)
}

/// Runs `work` inside `SAVEPOINT stock_import` (a transaction of its own here),
/// rolled back when it fails.
fn in_savepoint<T>(
    conn: &mut Connection,
    work: impl FnOnce(&mut Connection) -> Result<T>,
) -> Result<T> {
    conn.execute_batch("SAVEPOINT stock_import")?;
    match work(conn) {
        Ok(value) => {
            conn.execute_batch("RELEASE stock_import")?;
            Ok(value)
        }
        Err(error) => {
            conn.execute_batch("ROLLBACK TO stock_import; RELEASE stock_import")?;
            Err(error)
        }
    }
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
        CurrencyMode, ParsedTrade, StockInstrumentOverride, StockRowMessage, StockRowStatus,
        StockSplitAction, StockSplitOverride,
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
            splits: vec![],
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

    fn isin_trade() -> ParsedTrade {
        let mut t = buy(2, day(0), "X", 3.0, 100.0);
        t.instrument_key = "isin:IE00B3RBWM25".into();
        t.symbol = None;
        t.isin = Some("IE00B3RBWM25".into());
        t
    }

    fn vwrl_override() -> StockInstrumentOverride {
        StockInstrumentOverride {
            key: "isin:IE00B3RBWM25".into(),
            ticker: Some("vwrl.as".into()),
            name: Some("Vanguard FTSE All-World".into()),
            currency: Some("eur".into()),
            skip: false,
            splits: vec![],
        }
    }

    #[test]
    fn overrides_decide_the_ticker_name_and_currency_of_a_new_position() {
        // XTB-style file: the rows name no currency, so the user's choice (or the lookup's) is it.
        let mut conn = db();
        let mut cfg = config("xtb");
        cfg.currency_mode = CurrencyMode::Instrument;
        cfg.currency_column = None;
        cfg.instrument_overrides = vec![vwrl_override()];
        let mut t = isin_trade();
        t.currency = None;

        let result = import(&mut conn, &file(vec![t]), &cfg, "xtb.csv").unwrap();

        assert_eq!(result.new_positions, vec!["VWRL.AS"]);
        let p = position(&conn, "VWRL.AS").unwrap();
        assert_eq!(
            (p.0.as_str(), p.2.as_str()),
            ("Vanguard FTSE All-World", "EUR")
        );
    }

    #[test]
    fn an_override_relabels_nothing_the_file_states_in_its_own_currency() {
        // Degiro-style file: the cell says USD; the override only decides ticker and name.
        let mut conn = db();
        let mut cfg = config("degiro");
        cfg.instrument_overrides = vec![vwrl_override()];
        let mut t = isin_trade();
        t.currency = Some("USD".into());

        import(&mut conn, &file(vec![t]), &cfg, "degiro.csv").unwrap();

        let p = position(&conn, "VWRL.AS").unwrap();
        assert_eq!(
            (p.0.as_str(), p.2.as_str()),
            ("Vanguard FTSE All-World", "USD")
        );
        let rows = stored(&conn);
        assert_eq!((rows[0].3.as_str(), rows[0].4.as_str()), ("100", "USD"));
    }

    #[test]
    fn a_pence_file_is_written_in_pounds_and_a_second_import_finds_all_of_it_again() {
        let mut conn = db();
        let mut cfg = config("xtb");
        cfg.currency_mode = CurrencyMode::Instrument;
        cfg.currency_column = None;
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            currency: Some("GBX".into()),
            ..over("symbol:BARC.L")
        }];
        let pence_file = || {
            let mut rows = vec![
                buy(2, day(0), "BARC.L", 10.0, 443.65),
                sell(3, day(1), "BARC.L", 4.0, 4912.35),
            ];
            for t in &mut rows {
                t.currency = None;
            }
            file(rows)
        };

        let first = import(&mut conn, &pence_file(), &cfg, "xtb.csv").unwrap();
        assert_eq!((first.imported, first.duplicates), (2, 0));
        let rows = stored(&conn);
        let written: Vec<(&str, &str, &str)> = rows
            .iter()
            .map(|r| (r.1.as_str(), r.3.as_str(), r.4.as_str()))
            .collect();
        assert_eq!(
            written,
            vec![("buy", "4.4365", "GBP"), ("sell", "49.1235", "GBP")],
            "pounds, as the quote of the position is"
        );
        assert_eq!(position(&conn, "BARC.L").unwrap().2, "GBP");

        let again = import(&mut conn, &pence_file(), &cfg, "xtb.csv").unwrap();
        assert_eq!((again.imported, again.duplicates), (0, 2));
        assert_eq!(count(&conn, "investment_transactions"), 2);
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
        assert!(
            text.starts_with("Validation error: validation.stockImportLine|"),
            "{text}"
        );
        assert!(text.contains(r#""line":3"#), "names the file line: {text}");
        assert!(text.contains("validation.stockImportRowFailed"), "{text}");
        assert!(
            !text.contains("refused"),
            "technical text stays out of the message: {text}"
        );
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

    // ---- splits (D14) ---------------------------------------------------------

    /// A 10:1 split of `effective_day`, the stored rows left alone.
    fn split(effective_day: i64, action: StockSplitAction) -> StockSplitOverride {
        StockSplitOverride {
            effective_day,
            numerator: "10".into(),
            denominator: "1".into(),
            action,
            apply_stored: false,
        }
    }

    /// `split` that also applies to the stored rows.
    fn with_stored(split: StockSplitOverride) -> StockSplitOverride {
        StockSplitOverride {
            apply_stored: true,
            ..split
        }
    }

    /// `custom` config whose AAPL instrument carries `splits`.
    fn with_splits(splits: Vec<StockSplitOverride>) -> StockImportConfig {
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            splits,
            ..over("symbol:AAPL")
        }];
        cfg
    }

    /// A 10:1 split of `ticker` Yahoo reported, with `status` (resolved at 7 unless pending).
    fn record_split(conn: &Connection, ticker: &str, effective_day: i64, status: &str) {
        conn.execute(
            "INSERT INTO stock_splits
                 (id, ticker, effective_day, numerator, denominator, source, status, resolved_at)
             VALUES (?1, ?2, ?3, '10', '1', 'yahoo', ?4,
                     CASE WHEN ?4 = 'pending' THEN NULL ELSE 7 END)",
            rusqlite::params![format!("split-{ticker}"), ticker, effective_day, status],
        )
        .unwrap();
    }

    /// `(source, status, resolved, applied_count)` of the split recorded for `(ticker, day)`.
    fn split_row(
        conn: &Connection,
        ticker: &str,
        effective_day: i64,
    ) -> Option<(String, String, Option<i64>, Option<i64>)> {
        conn.query_row(
            "SELECT source, status, resolved_at, applied_count FROM stock_splits
             WHERE ticker = ?1 AND effective_day = ?2",
            rusqlite::params![ticker, effective_day],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
    }

    /// `(quantity, price)` of the stored rows of `ticker`, in insertion order.
    fn amounts(conn: &Connection, ticker: &str) -> Vec<(String, String)> {
        stored(conn)
            .into_iter()
            .filter(|r| r.0 == ticker)
            .map(|r| (r.2, r.3))
            .collect()
    }

    fn pair(quantity: &str, price: &str) -> (String, String) {
        (quantity.to_string(), price.to_string())
    }

    #[test]
    fn convert_records_an_applied_split() {
        let mut conn = db();
        let f = file(vec![
            buy(2, day(100), "AAPL", 2.0, 1000.0),
            buy(3, day(300), "AAPL", 1.0, 100.0),
        ]);
        let cfg = with_splits(vec![split(day(200), StockSplitAction::Convert)]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!(result.imported, 2);
        assert_eq!(
            amounts(&conn, "AAPL"),
            vec![pair("20", "100"), pair("1", "100")]
        );
        let (source, status, resolved, applied_count) =
            split_row(&conn, "AAPL", day(200)).expect("recorded");
        assert_eq!((source.as_str(), status.as_str()), ("import", "applied"));
        assert!(resolved.is_some());
        assert_eq!(applied_count, Some(0), "no stored rows to rewrite");
    }

    #[test]
    fn keep_records_a_dismissed_split() {
        let mut conn = db();
        let f = file(vec![
            buy(2, day(100), "AAPL", 2.0, 1000.0),
            buy(3, day(300), "AAPL", 1.0, 100.0),
        ]);
        let cfg = with_splits(vec![split(day(200), StockSplitAction::Keep)]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!((result.imported, result.converted), (2, 0));
        assert_eq!(
            amounts(&conn, "AAPL"),
            vec![pair("2", "1000"), pair("1", "100")]
        );
        let (source, status, resolved, applied_count) =
            split_row(&conn, "AAPL", day(200)).expect("recorded");
        assert_eq!((source.as_str(), status.as_str()), ("import", "dismissed"));
        assert!(resolved.is_some());
        assert_eq!(applied_count, None);
    }

    #[test]
    fn apply_stored_applies_a_pending_split_to_stored_rows() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
        record_split(&conn, "AAPL", day(200), "pending");
        let f = file(vec![sell(2, day(300), "AAPL", 25.0, 120.0)]);
        let cfg = with_splits(vec![with_stored(split(
            day(200),
            StockSplitAction::Convert,
        ))]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!((result.imported, result.converted), (1, 0));
        assert_eq!(
            amounts(&conn, "AAPL"),
            vec![pair("30", "95"), pair("25", "120")]
        );
        let (source, status, _, applied_count) = split_row(&conn, "AAPL", day(200)).unwrap();
        assert_eq!(
            (source.as_str(), status.as_str()),
            ("yahoo", "applied"),
            "the existing row is applied and keeps its source"
        );
        assert_eq!(applied_count, Some(1));
        assert_eq!(position(&conn, "AAPL").unwrap().1, "5");
    }

    #[test]
    fn apply_stored_with_an_absent_row_rewrites_stored_rows_and_records_applied() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
        let f = file(vec![sell(2, day(300), "AAPL", 25.0, 120.0)]);
        let cfg = with_splits(vec![with_stored(split(
            day(200),
            StockSplitAction::Convert,
        ))]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!(result.imported, 1);
        assert_eq!(
            amounts(&conn, "AAPL"),
            vec![pair("30", "95"), pair("25", "120")]
        );
        let (source, status, _, applied_count) = split_row(&conn, "AAPL", day(200)).unwrap();
        assert_eq!((source.as_str(), status.as_str()), ("import", "applied"));
        assert_eq!(applied_count, Some(1));
        assert_eq!(position(&conn, "AAPL").unwrap().1, "5");
    }

    #[test]
    fn the_stored_rows_left_alone_leave_a_pending_split_pending() {
        // Whatever the file's action: the stored switch is the stored rows' decision.
        for action in [StockSplitAction::Convert, StockSplitAction::Keep] {
            let mut conn = db();
            add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
            record_split(&conn, "AAPL", day(200), "pending");
            let f = file(vec![buy(2, day(300), "AAPL", 2.0, 100.0)]);
            let cfg = with_splits(vec![split(day(200), action)]);

            let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

            assert_eq!(result.imported, 1, "{action:?}");
            assert_eq!(
                amounts(&conn, "AAPL"),
                vec![pair("3", "950"), pair("2", "100")],
                "{action:?}"
            );
            let (source, status, resolved, _) = split_row(&conn, "AAPL", day(200)).unwrap();
            assert_eq!((source.as_str(), status.as_str()), ("yahoo", "pending"));
            assert_eq!(resolved, None);
            assert_eq!(position(&conn, "AAPL").unwrap().1, "5");
            assert!(result.updated_positions.contains(&"AAPL".to_string()));
        }
    }

    #[test]
    fn the_stored_rows_left_alone_record_an_absent_split_as_pending() {
        for action in [StockSplitAction::Convert, StockSplitAction::Keep] {
            let mut conn = db();
            add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
            let f = file(vec![buy(2, day(300), "AAPL", 2.0, 100.0)]);
            let cfg = with_splits(vec![split(day(200), action)]);

            let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

            assert_eq!(result.imported, 1, "{action:?}");
            assert_eq!(
                amounts(&conn, "AAPL"),
                vec![pair("3", "950"), pair("2", "100")],
                "{action:?}"
            );
            let (source, status, resolved, applied_count) =
                split_row(&conn, "AAPL", day(200)).expect("recorded");
            assert_eq!(
                (source.as_str(), status.as_str()),
                ("import", "pending"),
                "{action:?}: the position page is where the split waits"
            );
            assert_eq!((resolved, applied_count), (None, None));
        }
    }

    #[test]
    fn keep_with_apply_stored_rewrites_the_stored_rows_and_keeps_the_file_rows() {
        // The headline case: a pre-split position, a file the broker already adjusted.
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
        record_split(&conn, "AAPL", day(200), "pending");
        let f = file(vec![
            buy(2, day(150), "AAPL", 10.0, 90.0),
            sell(3, day(300), "AAPL", 35.0, 120.0),
        ]);
        let cfg = with_splits(vec![with_stored(split(day(200), StockSplitAction::Keep))]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!((result.imported, result.converted), (2, 0));
        assert_eq!(
            amounts(&conn, "AAPL"),
            vec![pair("30", "95"), pair("10", "90"), pair("35", "120")],
            "the stored row rewritten, the file's rows as they came"
        );
        let (source, status, _, applied_count) = split_row(&conn, "AAPL", day(200)).unwrap();
        assert_eq!((source.as_str(), status.as_str()), ("yahoo", "applied"));
        assert_eq!(applied_count, Some(1));
        assert_eq!(position(&conn, "AAPL").unwrap().1, "5");
        assert_eq!(result.updated_positions, vec!["AAPL"]);
        assert_eq!(result.earliest_day, Some(day(100)));
    }

    #[test]
    fn existing_applied_row_is_left_alone() {
        for status in ["applied", "dismissed"] {
            for action in [StockSplitAction::Convert, StockSplitAction::Keep] {
                for apply_stored in [false, true] {
                    let mut conn = db();
                    add_stored(&conn, "AAPL", "buy", "30", "95", day(100), None);
                    record_split(&conn, "AAPL", day(200), status);
                    let f = file(vec![buy(2, day(300), "AAPL", 1.0, 100.0)]);
                    let mut choice = split(day(200), action);
                    choice.apply_stored = apply_stored;

                    let result =
                        import(&mut conn, &f, &with_splits(vec![choice]), "f.csv").unwrap();

                    assert_eq!(result.imported, 1, "{status} {action:?} {apply_stored}");
                    assert_eq!(
                        amounts(&conn, "AAPL"),
                        vec![pair("30", "95"), pair("1", "100")],
                        "{status} {action:?} {apply_stored}"
                    );
                    let row = split_row(&conn, "AAPL", day(200)).unwrap();
                    assert_eq!(
                        row,
                        ("yahoo".to_string(), status.to_string(), Some(7), None),
                        "{action:?} {apply_stored}"
                    );
                }
            }
        }
    }

    #[test]
    fn converted_count_in_result() {
        let mut conn = db();
        let f = file(vec![
            buy(2, day(100), "AAPL", 2.0, 1000.0),
            buy(3, day(150), "AAPL", 3.0, 500.0),
            buy(4, day(300), "AAPL", 1.0, 100.0),
        ]);
        let cfg = with_splits(vec![split(day(200), StockSplitAction::Convert)]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!((result.imported, result.converted), (3, 2));
    }

    #[test]
    fn earliest_day_covers_rewritten_rows() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(50), None);
        record_split(&conn, "AAPL", day(200), "pending");
        let f = file(vec![buy(2, day(300), "AAPL", 1.0, 100.0)]);
        let cfg = with_splits(vec![with_stored(split(
            day(200),
            StockSplitAction::Convert,
        ))]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!(
            result.earliest_day,
            Some(day(50)),
            "the history rebuild starts at the first rewritten row"
        );
    }

    #[test]
    fn a_ticker_whose_stored_rows_were_rewritten_is_reported_as_updated() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
        record_split(&conn, "AAPL", day(200), "pending");
        let f = file(vec![
            buy(2, day(100), "AAPL", 3.0, 950.0), // converted: the stored row again
            buy(3, day(300), "MSFT", 1.0, 300.0),
        ]);
        let cfg = with_splits(vec![with_stored(split(
            day(200),
            StockSplitAction::Convert,
        ))]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!((result.imported, result.duplicates), (1, 1));
        assert_eq!(result.new_positions, vec!["MSFT"]);
        assert_eq!(
            result.updated_positions,
            vec!["AAPL"],
            "its history is rebuilt"
        );
        assert_eq!(result.earliest_day, Some(day(100)));
        assert_eq!(amounts(&conn, "AAPL"), vec![pair("30", "95")]);
    }

    #[test]
    fn splits_of_a_skipped_instrument_are_ignored() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
        let mut cfg = with_splits(vec![with_stored(split(
            day(200),
            StockSplitAction::Convert,
        ))]);
        cfg.instrument_overrides[0].skip = true;
        let f = file(vec![
            buy(2, day(300), "AAPL", 1.0, 100.0),
            buy(3, day(300), "MSFT", 1.0, 300.0),
        ]);

        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!(result.imported, 1);
        assert_eq!(amounts(&conn, "AAPL"), vec![pair("3", "950")]);
        assert_eq!(split_row(&conn, "AAPL", day(200)), None);
        assert_eq!(count(&conn, "stock_splits"), 0);
    }

    #[test]
    fn an_apply_stored_of_any_instrument_of_the_ticker_is_applied_as_the_preview_scales() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
        let mut by_isin = isin_trade();
        by_isin.day = day(300);
        let f = file(vec![by_isin, sell(3, day(300), "AAPL", 25.0, 120.0)]);
        let mut cfg = with_splits(vec![with_stored(split(
            day(200),
            StockSplitAction::Convert,
        ))]);
        cfg.instrument_overrides.insert(
            0,
            StockInstrumentOverride {
                ticker: Some("AAPL".into()),
                splits: vec![split(day(200), StockSplitAction::Keep)],
                ..over("isin:IE00B3RBWM25")
            },
        );

        let preview = preview(&conn, &f, &cfg).unwrap();
        assert_eq!(preview.counts.will_import, 2);
        let result = import(&mut conn, &f, &cfg, "f.csv").unwrap();

        assert_eq!(result.imported, 2);
        assert_eq!(amounts(&conn, "AAPL")[0], pair("30", "95"));
        let (_, status, _, _) = split_row(&conn, "AAPL", day(200)).unwrap();
        assert_eq!(status, "applied");
        assert_eq!(position(&conn, "AAPL").unwrap().1, "8");
    }

    #[test]
    fn a_failure_while_writing_rolls_the_applied_split_back_too() {
        let mut conn = db();
        add_stored(&conn, "AAPL", "buy", "3", "950", day(100), None);
        record_split(&conn, "AAPL", day(200), "pending");
        conn.execute_batch(
            "CREATE TRIGGER refuse_bad BEFORE INSERT ON investment_transactions
             WHEN NEW.ticker = 'BAD'
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
        let f = file(vec![
            sell(2, day(300), "AAPL", 25.0, 120.0),
            buy(3, day(300), "BAD", 1.0, 1.0),
        ]);
        let mut cfg = with_splits(vec![
            with_stored(split(day(200), StockSplitAction::Convert)),
            split(day(250), StockSplitAction::Keep),
        ]);
        cfg.instrument_overrides[0].splits[1].numerator = "2".into();

        import(&mut conn, &f, &cfg, "f.csv").unwrap_err();

        assert_eq!(amounts(&conn, "AAPL"), vec![pair("3", "950")]);
        let (_, status, _, _) = split_row(&conn, "AAPL", day(200)).unwrap();
        assert_eq!(status, "pending");
        assert_eq!(split_row(&conn, "AAPL", day(250)), None);
        assert_eq!(count(&conn, "stock_import_batches"), 0);
        // The connection is usable again: no savepoint left open.
        assert!(conn.is_autocommit());
    }
}
