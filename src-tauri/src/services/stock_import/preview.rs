//! Dry run against the database: what an import of a parsed file would do,
//! without writing anything. The decisions come from [`simulate`], the same
//! replay the import runs, so the preview is what the import does.

use super::simulate::{simulate, SimulatedTrade, Simulation, TradeOutcome, KEY_INSTRUMENT_SKIPPED};
use super::types::{
    ParsedFile, StockImportConfig, StockImportCounts, StockImportInstrument, StockImportPreview,
    StockInstrumentStatus, StockPreviewRow, StockRowMessage, StockRowStatus, PREVIEW_ROWS,
};
use crate::error::Result;
use crate::services::csv_import::CsvDateRange;

/// Instruments, statuses and counts of an import without writing anything.
///
/// Rows are the first [`PREVIEW_ROWS`] of the file in file order, rows the
/// parser skipped or could not read included; the counts cover the whole file.
/// `counts.will_import` includes duplicates the user chose to import anyway
/// (they stay `duplicate` rows), so `will_import + duplicates + skipped +
/// errors` is `total`.
pub fn preview(
    conn: &rusqlite::Connection,
    parsed: &ParsedFile,
    config: &StockImportConfig,
) -> Result<StockImportPreview> {
    let simulation = simulate(conn, parsed, config)?;

    let mut instruments = simulation.instruments.clone();
    instruments.extend(instruments_skipped_by_the_parser(
        parsed,
        config,
        &simulation,
    ));

    Ok(StockImportPreview {
        instruments,
        rows: preview_rows(parsed, &simulation),
        counts: counts(parsed, &simulation),
        date_range: date_range(&simulation),
        has_fee_column: parsed.has_fee_column,
    })
}

fn counts(parsed: &ParsedFile, simulation: &Simulation) -> StockImportCounts {
    let mut counts = StockImportCounts {
        total: parsed.total_rows,
        skipped: parsed.skipped.len(),
        errors: parsed.errors.len(),
        ..StockImportCounts::default()
    };
    for trade in &simulation.trades {
        match &trade.outcome {
            _ if trade.will_import() => counts.will_import += 1,
            TradeOutcome::Duplicate { .. } => counts.duplicates += 1,
            TradeOutcome::Skipped(_) => counts.skipped += 1,
            TradeOutcome::Error(_) => counts.errors += 1,
            TradeOutcome::New => {}
        }
    }
    counts
}

/// Trade days of the rows that will be imported.
fn date_range(simulation: &Simulation) -> Option<CsvDateRange> {
    let mut days = simulation
        .trades
        .iter()
        .filter(|trade| trade.will_import())
        .map(|trade| trade.day);
    let first = days.next()?;
    let (from, to) = days.fold((first, first), |(from, to), day| {
        (from.min(day), to.max(day))
    });
    Some(CsvDateRange { from, to })
}

/// A row of the file that is listed.
enum Listed<'a> {
    Trade(&'a SimulatedTrade),
    /// A row the parser skipped or could not read.
    Parsed(&'a StockRowMessage, StockRowStatus),
}

fn preview_rows(parsed: &ParsedFile, simulation: &Simulation) -> Vec<StockPreviewRow> {
    let mut listed: Vec<(usize, Listed)> = simulation
        .trades
        .iter()
        .map(|trade| (trade.line, Listed::Trade(trade)))
        .chain(
            parsed
                .skipped
                .iter()
                .map(|m| (m.line, Listed::Parsed(m, StockRowStatus::Skipped))),
        )
        .chain(
            parsed
                .errors
                .iter()
                .map(|m| (m.line, Listed::Parsed(m, StockRowStatus::Error))),
        )
        .collect();
    // Stable: rows of one line keep trades, skips, errors order.
    listed.sort_by_key(|(line, _)| *line);
    listed
        .into_iter()
        .take(PREVIEW_ROWS)
        .map(|(_, row)| match row {
            Listed::Trade(trade) => trade_row(trade),
            Listed::Parsed(message, status) => StockPreviewRow {
                line: message.line,
                day: None,
                direction: None,
                instrument_key: None,
                ticker: None,
                quantity: None,
                price: None,
                currency: None,
                status,
                message: Some(message.clone()),
            },
        })
        .collect()
}

fn trade_row(trade: &SimulatedTrade) -> StockPreviewRow {
    StockPreviewRow {
        line: trade.line,
        day: Some(trade.day),
        direction: Some(trade.direction),
        instrument_key: Some(trade.instrument_key.clone()),
        ticker: trade.ticker.clone(),
        quantity: Some(trade.quantity_text()),
        price: Some(trade.price_text()),
        currency: trade.currency.clone(),
        status: match &trade.outcome {
            TradeOutcome::New => StockRowStatus::New,
            TradeOutcome::Duplicate { .. } => StockRowStatus::Duplicate,
            TradeOutcome::Skipped(_) => StockRowStatus::Skipped,
            TradeOutcome::Error(_) => StockRowStatus::Error,
        },
        message: trade.message(),
    }
}

/// The parser drops the rows of a skipped instrument (`instrumentSkipped`), so
/// the instruments seen in the trades no longer include it. Every skipping
/// override that matches none of them is listed anyway, as `skipped`, so the
/// user can bring the instrument back; its trade count is unknown (0) and its
/// symbol or ISIN comes from the key. Nothing is added when the parser skipped
/// no instrument rows: the override then belongs to another file.
fn instruments_skipped_by_the_parser(
    parsed: &ParsedFile,
    config: &StockImportConfig,
    simulation: &Simulation,
) -> Vec<StockImportInstrument> {
    if !parsed
        .skipped
        .iter()
        .any(|m| m.key == KEY_INSTRUMENT_SKIPPED)
    {
        return Vec::new();
    }
    let mut listed: Vec<StockImportInstrument> = Vec::new();
    for over in config.instrument_overrides.iter().filter(|o| o.skip) {
        let known = simulation.instruments.iter().any(|i| i.key == over.key)
            || listed.iter().any(|i| i.key == over.key);
        if known {
            continue;
        }
        let (symbol, isin) = match over.key.split_once(':') {
            Some(("symbol", symbol)) => (Some(symbol.to_string()), None),
            Some(("isin", isin)) => (None, Some(isin.to_string())),
            _ => (None, None),
        };
        let text = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        listed.push(StockImportInstrument {
            key: over.key.clone(),
            symbol,
            isin,
            name: text(&over.name),
            currency: text(&over.currency).map(|c| c.to_uppercase()),
            trade_count: 0,
            ticker: text(&over.ticker).map(|t| t.to_uppercase()),
            status: StockInstrumentStatus::Skipped,
            position_currency: None,
        });
    }
    listed
}

#[cfg(test)]
mod tests {
    use super::super::simulate::test_db::*;
    use super::super::simulate::{
        KEY_DUPLICATE, KEY_INSTRUMENT_SKIPPED, KEY_SELL_EXCEEDS_HOLDINGS,
    };
    use super::super::types::{
        ParsedTrade, StockImportInstrument, StockInstrumentOverride, StockInstrumentStatus,
        StockPreviewRow, StockRowMessage, StockRowStatus, TradeDirection, PREVIEW_ROWS,
    };
    use super::*;

    fn message(line: usize, key: &str, detail: Option<&str>) -> StockRowMessage {
        StockRowMessage {
            line,
            key: key.to_string(),
            detail: detail.map(str::to_string),
        }
    }

    fn row_of(preview: &StockImportPreview, line: usize) -> &StockPreviewRow {
        preview
            .rows
            .iter()
            .find(|r| r.line == line)
            .unwrap_or_else(|| panic!("no row for line {line}"))
    }

    fn instrument<'a>(preview: &'a StockImportPreview, key: &str) -> &'a StockImportInstrument {
        preview
            .instruments
            .iter()
            .find(|i| i.key == key)
            .unwrap_or_else(|| panic!("no instrument {key}"))
    }

    #[test]
    fn counts_cover_every_row_of_the_file() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let mut file = parsed(vec![
            buy(2, day(0), "AAPL", 10.0, 100.0), // duplicate
            buy(3, day(1), "AAPL", 5.0, 110.0),  // new
            sell(4, day(2), "AAPL", 99.0, 1.0),  // sells too much
            buy(7, day(3), "MSFT", 1.0, 300.0),  // new
        ]);
        file.skipped = vec![message(5, "importWizard.row.notATrade", Some("Dividend"))];
        file.errors = vec![message(
            6,
            "importWizard.row.dateUnparseable",
            Some("31.02.2024"),
        )];
        file.total_rows = 6;

        let preview = preview(&conn, &file, &config("custom")).unwrap();

        assert_eq!(preview.counts.total, 6);
        assert_eq!(preview.counts.will_import, 2);
        assert_eq!(preview.counts.duplicates, 1);
        assert_eq!(preview.counts.skipped, 1);
        assert_eq!(preview.counts.errors, 2, "one parsed, one from the rules");
        assert_eq!(
            preview.counts.will_import
                + preview.counts.duplicates
                + preview.counts.skipped
                + preview.counts.errors,
            preview.counts.total
        );
    }

    #[test]
    fn rows_are_in_file_order_with_parsed_skips_and_errors_among_them() {
        let conn = db();
        let mut file = parsed(vec![
            buy(2, day(0), "AAPL", 10.5, 100.25),
            buy(5, day(1), "AAPL", 1.0, 1.0),
        ]);
        file.skipped = vec![message(3, "importWizard.row.notATrade", Some("Deposit"))];
        file.errors = vec![message(
            4,
            "importWizard.row.numberUnparseable",
            Some("abc"),
        )];
        file.total_rows = 4;

        let preview = preview(&conn, &file, &config("custom")).unwrap();

        let lines: Vec<usize> = preview.rows.iter().map(|r| r.line).collect();
        assert_eq!(lines, vec![2, 3, 4, 5]);
        let first = row_of(&preview, 2);
        assert_eq!(first.status, StockRowStatus::New);
        assert_eq!(first.day, Some(day(0)));
        assert_eq!(first.direction, Some(TradeDirection::Buy));
        assert_eq!(first.instrument_key.as_deref(), Some("symbol:AAPL"));
        assert_eq!(first.ticker.as_deref(), Some("AAPL"));
        assert_eq!(first.quantity.as_deref(), Some("10.5"));
        assert_eq!(first.price.as_deref(), Some("100.25"));
        assert_eq!(first.currency.as_deref(), Some("USD"));
        assert_eq!(first.message, None);

        let skipped = row_of(&preview, 3);
        assert_eq!(skipped.status, StockRowStatus::Skipped);
        assert_eq!(
            skipped.message.as_ref().map(|m| m.key.as_str()),
            Some("importWizard.row.notATrade")
        );
        assert_eq!(skipped.day, None);
        assert_eq!(skipped.quantity, None);
        let error = row_of(&preview, 4);
        assert_eq!(error.status, StockRowStatus::Error);
        assert_eq!(
            error.message.as_ref().and_then(|m| m.detail.as_deref()),
            Some("abc")
        );
    }

    #[test]
    fn only_the_first_rows_are_listed_but_the_counts_cover_the_file() {
        let conn = db();
        let total = PREVIEW_ROWS + 50;
        let trades: Vec<ParsedTrade> = (0..total)
            .map(|i| buy(i + 2, day(i as i64), "AAPL", 1.0, 1.0 + i as f64))
            .collect();
        let preview = preview(&conn, &parsed(trades), &config("custom")).unwrap();

        assert_eq!(preview.rows.len(), PREVIEW_ROWS);
        assert_eq!(preview.rows[0].line, 2);
        assert_eq!(preview.rows[PREVIEW_ROWS - 1].line, PREVIEW_ROWS + 1);
        assert_eq!(preview.counts.total, total);
        assert_eq!(preview.counts.will_import, total);
    }

    #[test]
    fn duplicates_and_errors_explain_themselves() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let file = parsed(vec![
            buy(2, day(0), "AAPL", 10.0, 100.0),
            sell(3, day(1), "AAPL", 50.0, 1.0),
        ]);
        let preview = preview(&conn, &file, &config("custom")).unwrap();

        let duplicate = row_of(&preview, 2);
        assert_eq!(duplicate.status, StockRowStatus::Duplicate);
        assert_eq!(
            duplicate.message.as_ref().map(|m| m.key.as_str()),
            Some(KEY_DUPLICATE)
        );
        let error = row_of(&preview, 3);
        assert_eq!(error.status, StockRowStatus::Error);
        assert_eq!(
            error.message,
            Some(message(3, KEY_SELL_EXCEEDS_HOLDINGS, Some("10")))
        );
    }

    #[test]
    fn a_duplicate_imported_anyway_stays_a_duplicate_row_but_counts_as_importable() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let mut cfg = config("custom");
        cfg.import_anyway_lines = vec![2];
        let file = parsed(vec![
            buy(2, day(0), "AAPL", 10.0, 100.0),
            buy(3, day(0), "AAPL", 10.0, 100.0),
        ]);
        let preview = preview(&conn, &file, &cfg).unwrap();

        assert_eq!(row_of(&preview, 2).status, StockRowStatus::Duplicate);
        assert!(row_of(&preview, 2).message.is_some());
        assert_eq!(preview.counts.will_import, 1);
        assert_eq!(preview.counts.duplicates, 1);
        assert_eq!(
            preview.date_range.map(|r| (r.from, r.to)),
            Some((day(0), day(0)))
        );
    }

    #[test]
    fn the_date_range_is_that_of_the_rows_that_will_be_imported() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(2), None);
        let file = parsed(vec![
            buy(2, day(5), "AAPL", 1.0, 1.0),
            buy(3, day(2), "AAPL", 10.0, 100.0), // duplicate
            sell(4, day(1), "AAPL", 1.0, 1.0),   // error: nothing held yet
            buy(5, day(9), "AAPL", 1.0, 2.0),
        ]);
        let preview = preview(&conn, &file, &config("custom")).unwrap();
        let range = preview.date_range.expect("range");
        assert_eq!((range.from, range.to), (day(5), day(9)));

        let nothing = ParsedFile::default();
        assert!(super::preview(&conn, &nothing, &config("custom"))
            .unwrap()
            .date_range
            .is_none());
    }

    #[test]
    fn the_fee_column_flag_is_passed_through() {
        let conn = db();
        let mut file = parsed(vec![buy(2, day(0), "AAPL", 1.0, 1.0)]);
        assert!(
            !preview(&conn, &file, &config("custom"))
                .unwrap()
                .has_fee_column
        );
        file.has_fee_column = true;
        assert!(
            preview(&conn, &file, &config("custom"))
                .unwrap()
                .has_fee_column
        );
    }

    #[test]
    fn instruments_report_status_ticker_and_position_currency() {
        let conn = db();
        add_position(&conn, "AAPL", "Apple Inc.", "USD");
        let mut by_isin = buy(4, day(0), "X", 1.0, 1.0);
        by_isin.instrument_key = "isin:IE00B3RBWM25".into();
        by_isin.symbol = None;
        by_isin.isin = Some("IE00B3RBWM25".into());
        by_isin.name = Some("Vanguard FTSE All-World".into());
        let mut cfg = config("degiro");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            skip: true,
            ..over("symbol:OPT")
        }];
        let file = parsed(vec![
            trade(2, day(0), TradeDirection::Buy, "AAPL", 1.0, 1.0, "EUR"), // existing, in EUR
            buy(3, day(0), "OPT", 1.0, 1.0),
            by_isin,
            buy(5, day(0), "NVDA", 1.0, 1.0),
        ]);
        let preview = preview(&conn, &file, &cfg).unwrap();

        let keys: Vec<&str> = preview.instruments.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "symbol:AAPL",
                "symbol:OPT",
                "isin:IE00B3RBWM25",
                "symbol:NVDA"
            ],
            "first appearance"
        );
        let aapl = instrument(&preview, "symbol:AAPL");
        assert_eq!(aapl.status, StockInstrumentStatus::Existing);
        assert_eq!(aapl.currency.as_deref(), Some("EUR"));
        assert_eq!(aapl.position_currency.as_deref(), Some("USD"));
        assert_eq!(
            instrument(&preview, "symbol:OPT").status,
            StockInstrumentStatus::Skipped
        );
        let etf = instrument(&preview, "isin:IE00B3RBWM25");
        assert_eq!(etf.status, StockInstrumentStatus::MissingSymbol);
        assert_eq!(etf.name.as_deref(), Some("Vanguard FTSE All-World"));
        assert_eq!(etf.ticker, None);
        assert_eq!(
            instrument(&preview, "symbol:NVDA").status,
            StockInstrumentStatus::New
        );
    }

    #[test]
    fn an_instrument_skipped_by_the_parser_is_still_listed() {
        // The parser drops the rows of a skipped instrument into `skipped`; the
        // override keeps a row in the list so the user can bring it back.
        let conn = db();
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![
            StockInstrumentOverride {
                skip: true,
                name: Some("Some option".into()),
                ..over("isin:US0378331005")
            },
            StockInstrumentOverride {
                skip: true,
                ..over("symbol:OPT")
            },
            over("symbol:AAPL"), // not skipped: not synthesized
        ];
        let mut file = parsed(vec![buy(2, day(0), "MSFT", 1.0, 1.0)]);
        file.skipped = vec![
            message(3, KEY_INSTRUMENT_SKIPPED, None),
            message(4, KEY_INSTRUMENT_SKIPPED, None),
        ];
        file.total_rows = 3;

        let preview = preview(&conn, &file, &cfg).unwrap();

        let keys: Vec<&str> = preview.instruments.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, vec!["symbol:MSFT", "isin:US0378331005", "symbol:OPT"]);
        let isin = instrument(&preview, "isin:US0378331005");
        assert_eq!(isin.status, StockInstrumentStatus::Skipped);
        assert_eq!(isin.isin.as_deref(), Some("US0378331005"));
        assert_eq!(isin.symbol, None);
        assert_eq!(isin.name.as_deref(), Some("Some option"));
        assert_eq!(
            isin.trade_count, 0,
            "unknown: its rows were dropped by the parser"
        );
        assert_eq!(
            instrument(&preview, "symbol:OPT").symbol.as_deref(),
            Some("OPT")
        );
        assert_eq!(preview.counts.skipped, 2);
    }

    #[test]
    fn a_skip_override_that_matched_nothing_adds_no_instrument() {
        let conn = db();
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            skip: true,
            ..over("symbol:GONE")
        }];
        let file = parsed(vec![buy(2, day(0), "MSFT", 1.0, 1.0)]);
        let preview = preview(&conn, &file, &cfg).unwrap();
        assert_eq!(preview.instruments.len(), 1);
    }

    #[test]
    fn nothing_is_written() {
        let conn = db();
        let file = parsed(vec![buy(2, day(0), "AAPL", 1.0, 1.0)]);
        preview(&conn, &file, &config("custom")).unwrap();
        let positions: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_investments", [], |r| r.get(0))
            .unwrap();
        let transactions: i64 = conn
            .query_row("SELECT COUNT(*) FROM investment_transactions", [], |r| {
                r.get(0)
            })
            .unwrap();
        let batches: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_import_batches", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!((positions, transactions, batches), (0, 0, 0));
    }
}
