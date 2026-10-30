//! Built-in sources: header signatures, the configuration each produces for a
//! file and the transforms it needs.
//!
//! One module per source ([`xtb`], [`trading212`], [`degiro`], [`ibkr`],
//! [`moony`]). Each describes itself with a [`StockAdapter`]: `detect` says
//! whether a file is of that source (from its header cells and the first rows),
//! `config` builds the [`StockImportConfig`] with the columns resolved to the
//! file's own positions. Adding a broker = one module here, a fixture in
//! `tests/fixtures/csv/`, a line in [`ADAPTERS`] and the guide text.
//!
//! An adapter does not know the layout of the file (delimiter, encoding,
//! header row, skipped rows) nor the values of the type column: `inspect`
//! fills those in.

use crate::services::csv_import::amounts::detect_decimal_separator;
use crate::services::csv_import::columns::normalize_header;
use crate::services::date_parser;

use super::detect::date_part;
use super::types::{CurrencyMode, DirectionMode, StockImportConfig, StockImportTransforms};

pub mod degiro;
pub mod ibkr;
pub mod moony;
pub mod trading212;
pub mod xtb;

#[cfg(test)]
pub(crate) mod test_support;

/// What an adapter makes of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct AdapterConfig {
    pub config: StockImportConfig,
    /// The day/month order could not be decided from the file (only for
    /// adapters that read the date format from the data).
    pub date_ambiguous: bool,
}

/// One built-in source.
pub struct StockAdapter {
    /// `types::SOURCE_*`.
    pub source: &'static str,
    /// Is the file of this source? `headers` are the trimmed header cells,
    /// `rows` the first data rows (each as wide as the headers).
    pub detect: fn(headers: &[String], rows: &[Vec<String>]) -> bool,
    /// The configuration for the file, or `None` when it lacks a column the
    /// source cannot do without. Lenient on purpose: it needs the columns, not
    /// the proof `detect` asks for.
    pub config: fn(headers: &[String], rows: &[Vec<String>]) -> Option<AdapterConfig>,
}

/// The built-in sources in detection order: the specific before the generic.
/// The first adapter that claims a file is its source.
pub static ADAPTERS: [StockAdapter; 5] = [
    xtb::ADAPTER,
    trading212::ADAPTER,
    degiro::ADAPTER,
    ibkr::ADAPTER,
    moony::ADAPTER,
];

/// The adapter that claims the file, if any.
pub fn detect(headers: &[String], rows: &[Vec<String>]) -> Option<&'static StockAdapter> {
    ADAPTERS.iter().find(|a| (a.detect)(headers, rows))
}

/// The built-in adapter with this source id.
pub fn by_source(source: &str) -> Option<&'static StockAdapter> {
    ADAPTERS.iter().find(|a| a.source == source)
}

/// A configuration to start from: the layout fields are placeholders for
/// `inspect`, the rest is the plainest reading (comma decimals and the like
/// are for the adapter to say).
pub(super) fn base_config(source: &str) -> StockImportConfig {
    StockImportConfig {
        source: source.to_string(),
        delimiter: ",".to_string(),
        encoding: "UTF-8".to_string(),
        header_row: 0,
        skip_rows: 0,
        date_column: 0,
        date_format: "%Y-%m-%d".to_string(),
        symbol_column: None,
        isin_column: None,
        name_column: None,
        quantity_column: 0,
        price_column: 0,
        currency_mode: CurrencyMode::Column,
        currency_column: None,
        fixed_currency: None,
        direction_mode: DirectionMode::TypeColumn,
        type_column: None,
        type_values: Vec::new(),
        decimal_separator: ".".to_string(),
        external_id_column: None,
        transforms: StockImportTransforms::default(),
        instrument_overrides: Vec::new(),
        import_anyway_lines: Vec::new(),
        amount_column: None,
        tax_column: None,
        tax_currency_column: None,
        cash_currency: None,
    }
}

/// Index of the first header whose normalised text is one of `names`.
pub(super) fn find_header(headers: &[String], names: &[&str]) -> Option<usize> {
    headers
        .iter()
        .position(|h| names.contains(&normalize_header(h).as_str()))
}

/// Index of the first header equal to `name` ignoring case and surrounding
/// spaces. For names whose brackets matter ("Currency (Price / share)").
pub(super) fn find_header_exact(headers: &[String], name: &str) -> Option<usize> {
    let name = name.trim();
    headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case(name))
}

/// The decimal separator of the cells of `columns` in the sample rows.
pub(super) fn decimal_of(rows: &[Vec<String>], columns: &[usize]) -> String {
    let cells: Vec<&str> = rows
        .iter()
        .flat_map(|row| columns.iter().filter_map(|&c| row.get(c)))
        .map(String::as_str)
        .collect();
    detect_decimal_separator(&cells).to_string()
}

/// The date format of one column of the sample rows and whether day and month
/// could be told apart; `None` when no sample reads as a date.
pub(super) fn detect_column_date(rows: &[Vec<String>], column: usize) -> Option<(String, bool)> {
    let samples: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get(column))
        .map(|cell| date_part(cell))
        .filter(|cell| !cell.is_empty())
        .collect();
    let detected = date_parser::detect_date_format(&samples);
    let backed = samples
        .iter()
        .any(|s| date_parser::parse_date_strict(s, &detected.format).is_some());
    backed.then_some((detected.format, detected.ambiguous))
}

/// For a source with a known date format: `known` when every sample reads with
/// it (never ambiguous, however few days there are to tell), else what the
/// samples say, flagged when they cannot tell day from month. A locale variant
/// of an export must not be read in the wrong order.
pub(super) fn known_date_format(
    rows: &[Vec<String>],
    column: usize,
    known: &str,
) -> (String, bool) {
    let all_read = rows
        .iter()
        .filter_map(|row| row.get(column))
        .map(|cell| date_part(cell))
        .filter(|cell| !cell.is_empty())
        .all(|cell| date_parser::parse_date_strict(cell, known).is_some());
    if all_read {
        (known.to_string(), false)
    } else {
        date_format_of(rows, column, known)
    }
}

/// [`detect_column_date`], or `fallback` (not ambiguous) when nothing reads.
pub(super) fn date_format_of(
    rows: &[Vec<String>],
    column: usize,
    fallback: &str,
) -> (String, bool) {
    detect_column_date(rows, column).unwrap_or_else(|| (fallback.to_string(), false))
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::services::stock_import::inspect::inspect;
    use crate::services::stock_import::parse::parse_file;
    use crate::services::stock_import::types::{
        StockCsvInspectOptions, StockImportConfig, StockInstrumentOverride, TradeDirection,
        SOURCE_DEGIRO, SOURCE_IBKR, SOURCE_MOONY, SOURCE_TRADING212, SOURCE_XTB,
    };

    #[test]
    fn adapters_are_in_detection_order() {
        let sources: Vec<&str> = ADAPTERS.iter().map(|a| a.source).collect();
        assert_eq!(
            sources,
            [
                SOURCE_XTB,
                SOURCE_TRADING212,
                SOURCE_DEGIRO,
                SOURCE_IBKR,
                SOURCE_MOONY
            ]
        );
    }

    #[test]
    fn every_header_row_is_claimed_by_its_own_adapter_only() {
        struct Case {
            name: &'static str,
            headers: Vec<String>,
            rows: Vec<Vec<String>>,
            expected: &'static str,
        }
        let case = |name, list: &[&str], data: Vec<Vec<String>>, expected| Case {
            name,
            headers: headers(list),
            rows: data,
            expected,
        };
        // XTB is only claimed with a trade in the rows.
        let xtb_rows = rows(&[&[
            "1",
            "Stocks/ETF purchase",
            "12.04.2024 13:01:45",
            "A.US",
            "OPEN BUY 1 @ 2",
            "-2",
        ]]);
        let cases = vec![
            case("xtb", &XTB_HEADERS, xtb_rows, SOURCE_XTB),
            case("t212", &TRADING212_HEADERS, vec![], SOURCE_TRADING212),
            case("degiro", &DEGIRO_HEADERS, vec![], SOURCE_DEGIRO),
            case(
                "degiro autofx",
                &DEGIRO_AUTOFX_HEADERS,
                vec![],
                SOURCE_DEGIRO,
            ),
            case("degiro nl", &DEGIRO_HEADERS_NL, vec![], SOURCE_DEGIRO),
            case("degiro cs", &DEGIRO_HEADERS_CS, vec![], SOURCE_DEGIRO),
            case("ibkr", &IBKR_HEADERS, vec![], SOURCE_IBKR),
            case("moony en", &MOONY_HEADERS_EN, vec![], SOURCE_MOONY),
            case("moony cs", &MOONY_HEADERS_CS, vec![], SOURCE_MOONY),
        ];
        for c in cases {
            let claimed: Vec<&str> = ADAPTERS
                .iter()
                .filter(|a| (a.detect)(&c.headers, &c.rows))
                .map(|a| a.source)
                .collect();
            assert_eq!(claimed, [c.expected], "{}", c.name);
        }
    }

    #[test]
    fn a_built_in_source_is_found_by_its_id() {
        assert_eq!(by_source("degiro").map(|a| a.source), Some(SOURCE_DEGIRO));
        assert!(by_source("custom").is_none());
        assert!(by_source("format:abc").is_none());
        assert!(by_source("").is_none());
    }

    #[test]
    fn detection_returns_the_first_adapter_that_claims_the_file() {
        let t212 = detect(&headers(&TRADING212_HEADERS), &[]);
        assert_eq!(t212.map(|a| a.source), Some(SOURCE_TRADING212));
        let moony = detect(&headers(&MOONY_HEADERS_CS), &[]);
        assert_eq!(moony.map(|a| a.source), Some(SOURCE_MOONY));
        assert!(detect(&headers(&["Date", "Amount", "Description"]), &[]).is_none());
        assert!(detect(&[], &[]).is_none());
    }

    #[test]
    fn the_base_config_is_the_plainest_reading() {
        let config = base_config(SOURCE_IBKR);
        assert_eq!(config.source, "ibkr");
        assert_eq!(config.decimal_separator, ".");
        assert_eq!(config.direction_mode, DirectionMode::TypeColumn);
        assert_eq!(config.currency_mode, CurrencyMode::Column);
        assert!(config.type_values.is_empty() && config.instrument_overrides.is_empty());
    }

    #[test]
    fn headers_are_found_normalised_or_exact() {
        let h = headers(&["Čas", "Currency (Price / share)", "Currency (Total)"]);
        assert_eq!(find_header(&h, &["cas", "time"]), Some(0));
        // Brackets are dropped by the normalisation: both read "currency".
        assert_eq!(find_header(&h, &["currency"]), Some(1));
        assert_eq!(find_header(&h, &["price"]), None);
        assert_eq!(find_header_exact(&h, "currency (total)"), Some(2));
        assert_eq!(
            find_header_exact(&h, "  CURRENCY (PRICE / SHARE) "),
            Some(1)
        );
        assert_eq!(find_header_exact(&h, "currency"), None);
    }

    #[test]
    fn the_decimal_separator_comes_from_the_named_columns() {
        let data = rows(&[&["x", "5", "78,22"], &["y", "-3", "1.234,50"]]);
        assert_eq!(decimal_of(&data, &[1, 2]), ",");
        let data = rows(&[&["x", "0.029053", "49.96"]]);
        assert_eq!(decimal_of(&data, &[1, 2]), ".");
        // No samples: the default.
        assert_eq!(decimal_of(&[], &[1, 2]), ".");
        // A column that does not exist is ignored.
        assert_eq!(decimal_of(&data, &[9]), ".");
    }

    #[test]
    fn a_known_date_format_is_kept_while_the_samples_read_with_it() {
        // Every day is at most 12: detection would call this ambiguous, but
        // the source's own format is known.
        let few_days = rows(&[&["01.02.2024 10:00:00"], &["03.04.2024 11:00:00"]]);
        assert_eq!(
            known_date_format(&few_days, 0, "%d.%m.%Y"),
            ("%d.%m.%Y".into(), false)
        );
        // No samples: the known format.
        assert_eq!(
            known_date_format(&[], 0, "%d.%m.%Y"),
            ("%d.%m.%Y".into(), false)
        );
        // A variant of the export: what the samples say.
        let iso = rows(&[&["2024-02-01 10:00:00"], &["2024-04-03 11:00:00"]]);
        assert_eq!(
            known_date_format(&iso, 0, "%d.%m.%Y"),
            ("%Y-%m-%d".into(), false)
        );
        let slashes = rows(&[&["13/04/2024"], &["02/05/2024"]]);
        assert_eq!(
            known_date_format(&slashes, 0, "%d.%m.%Y"),
            ("%d/%m/%Y".into(), false)
        );
        // ... flagged when they cannot tell day from month.
        let unclear = rows(&[&["01/02/2024"], &["03/04/2024"]]);
        assert_eq!(
            known_date_format(&unclear, 0, "%d.%m.%Y"),
            ("%d/%m/%Y".into(), true)
        );
        // Unreadable samples: the known format again.
        let junk = rows(&[&["n/a"]]);
        assert_eq!(
            known_date_format(&junk, 0, "%d.%m.%Y"),
            ("%d.%m.%Y".into(), false)
        );
    }

    #[test]
    fn the_date_format_comes_from_the_samples_with_a_fallback() {
        let iso = rows(&[&["2024-01-15"], &["2024-02-20 10:00:00"]]);
        assert_eq!(
            date_format_of(&iso, 0, "%d.%m.%Y"),
            ("%Y-%m-%d".into(), false)
        );
        let czech = rows(&[&["15.1.2024"], &["20.02.2024"]]);
        assert_eq!(
            date_format_of(&czech, 0, "%Y-%m-%d"),
            ("%d.%m.%Y".into(), false)
        );
        // Every day and month fits both orders: the European guess, flagged.
        let unclear = rows(&[&["01.02.2024"], &["03.04.2024"]]);
        assert_eq!(
            date_format_of(&unclear, 0, "%Y-%m-%d"),
            ("%d.%m.%Y".into(), true)
        );
        // Interactive Brokers: separator-less, with an optional time.
        let compact = rows(&[&["20230522"], &["20230523;093000"]]);
        assert_eq!(
            date_format_of(&compact, 0, "%Y-%m-%d"),
            ("%Y%m%d".into(), false)
        );
        // Nothing readable: the fallback.
        let junk = rows(&[&["n/a"], &[""]]);
        assert_eq!(
            date_format_of(&junk, 0, "%d-%m-%Y"),
            ("%d-%m-%Y".into(), false)
        );
        assert_eq!(
            date_format_of(&[], 0, "%d-%m-%Y"),
            ("%d-%m-%Y".into(), false)
        );
    }

    // ===================================================================
    // Fixtures: every adapter detects its own file and nobody else's
    // ===================================================================

    /// What the first trade of a fixture must be.
    struct Trade {
        line: usize,
        day: i64,
        direction: TradeDirection,
        key: &'static str,
        symbol: Option<&'static str>,
        isin: Option<&'static str>,
        name: Option<&'static str>,
        quantity: f64,
        price: f64,
        currency: Option<&'static str>,
        external_id: Option<&'static str>,
    }

    struct Fixture {
        file: &'static str,
        source: &'static str,
        /// Data rows of the file.
        rows: usize,
        buys: usize,
        sells: usize,
        /// Dividend rows that read (their taxes end skipped, paired).
        dividends: usize,
        skipped: usize,
        /// The account currency the user names in the review step (XTB's file has none).
        cash_currency: Option<&'static str>,
        first: Trade,
    }

    /// One entry per `stocks-*.csv` fixture of a source. Adding a broker adds a
    /// line here (the guard below fails for a fixture without one).
    fn fixtures() -> Vec<Fixture> {
        use TradeDirection::Buy;
        vec![
            Fixture {
                file: "stocks-xtb.csv",
                source: SOURCE_XTB,
                rows: 11,
                buys: 4,
                sells: 2,
                dividends: 1,
                skipped: 4,
                cash_currency: Some("USD"),
                first: Trade {
                    line: 3,
                    day: 1704326400, // 04.01.2024
                    direction: Buy,
                    key: "symbol:AAPL",
                    symbol: Some("AAPL"),
                    isin: None,
                    name: None,
                    quantity: 3.0,
                    price: 185.5,
                    currency: None,
                    external_id: Some("610000002"),
                },
            },
            Fixture {
                file: "stocks-trading212.csv",
                source: SOURCE_TRADING212,
                rows: 12,
                buys: 4,
                sells: 3,
                dividends: 1,
                skipped: 4,
                cash_currency: None,
                first: Trade {
                    line: 3,
                    day: 1702857600, // 2023-12-18
                    direction: Buy,
                    key: "isin:US17275R1023",
                    symbol: Some("CSCO"),
                    isin: Some("US17275R1023"),
                    name: Some("Cisco Systems"),
                    quantity: 0.029053,
                    price: 49.96,
                    currency: Some("USD"),
                    external_id: Some("EOF7504196256"),
                },
            },
            Fixture {
                file: "stocks-degiro.csv",
                source: SOURCE_DEGIRO,
                rows: 7,
                buys: 5,
                sells: 2,
                dividends: 0,
                skipped: 0,
                cash_currency: None,
                first: Trade {
                    line: 2,
                    day: 1705276800, // 15-01-2024
                    direction: Buy,
                    key: "isin:IE00B3XXRP09",
                    symbol: None,
                    isin: Some("IE00B3XXRP09"),
                    name: Some("VANGUARD S&P 500 UCITS ETF"),
                    quantity: 5.0,
                    price: 78.22,
                    currency: Some("EUR"),
                    external_id: Some("6f1c2c9a-4a8e-4d5b-9c1e-2b7a1d9e0f11"),
                },
            },
            Fixture {
                file: "stocks-ibkr.csv",
                source: SOURCE_IBKR,
                rows: 9,
                buys: 4,
                sells: 2,
                dividends: 0,
                skipped: 3,
                cash_currency: None,
                first: Trade {
                    line: 3,
                    day: 1684713600, // 20230522
                    direction: Buy,
                    key: "isin:US0378331005",
                    symbol: Some("AAPL"),
                    isin: Some("US0378331005"),
                    name: None,
                    quantity: 3.0,
                    price: 172.4,
                    currency: Some("USD"),
                    external_id: Some("3001001"),
                },
            },
            Fixture {
                file: "stocks-moony.csv",
                source: SOURCE_MOONY,
                rows: 7,
                buys: 4,
                sells: 2,
                dividends: 1,
                skipped: 0,
                cash_currency: None,
                first: Trade {
                    line: 2,
                    day: 1678752000, // 2023-03-14
                    direction: Buy,
                    key: "symbol:AAPL",
                    symbol: Some("AAPL"),
                    isin: None,
                    name: Some("Apple Inc."),
                    quantity: 10.0,
                    price: 152.8,
                    currency: Some("USD"),
                    external_id: None,
                },
            },
            Fixture {
                file: "stocks-handmade-cz.csv",
                source: SOURCE_MOONY,
                rows: 7,
                buys: 4,
                sells: 2,
                dividends: 1,
                skipped: 0,
                cash_currency: None,
                first: Trade {
                    line: 2,
                    day: 1705276800, // 15.1.2024
                    direction: Buy,
                    key: "symbol:AAPL",
                    symbol: Some("AAPL"),
                    isin: None,
                    name: Some("Apple Inc."),
                    quantity: 10.0,
                    price: 185.5,
                    currency: Some("USD"),
                    external_id: None,
                },
            },
        ]
    }

    /// Fixtures that are not the file of a built-in source: the generic
    /// mapping's (see the tests below).
    const GENERIC_FIXTURES: [&str; 2] = ["stocks-messy.csv", "stocks-template.csv"];

    #[test]
    fn all_stock_adapters_detect_their_fixture() {
        for fixture in fixtures() {
            let name = fixture.file;
            let bytes = fixture_bytes(name);
            let inspection = inspect(&bytes, name, &StockCsvInspectOptions::default(), &[])
                .unwrap_or_else(|e| panic!("{name}: {e}"));

            // Detected as its own source ...
            assert_eq!(
                inspection.detected_source.as_deref(),
                Some(fixture.source),
                "{name}"
            );
            // ... and claimed by no other adapter.
            for adapter in &ADAPTERS {
                assert_eq!(
                    (adapter.detect)(&inspection.headers, &inspection.sample_rows),
                    adapter.source == fixture.source,
                    "{name}: {}",
                    adapter.source
                );
            }

            // The ready configuration reads the whole file without an error.
            let mut config = inspection
                .config
                .clone()
                .unwrap_or_else(|| panic!("{name}: no ready config"));
            config.cash_currency = fixture.cash_currency.map(str::to_string);
            let config = &config;
            assert_eq!(config.source, fixture.source, "{name}");
            config.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                !inspection.date_format_ambiguous,
                "{name}: the format is known"
            );
            // It crosses the wire unchanged.
            let json = serde_json::to_string(config).unwrap_or_else(|e| panic!("{name}: {e}"));
            let back: StockImportConfig =
                serde_json::from_str(&json).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(&back, config, "{name}");
            assert_eq!(
                inspection.config.as_ref().map(|c| c.source.as_str()),
                Some(fixture.source),
                "{name}"
            );

            let parsed = parse_file(&bytes, config).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(parsed.errors.is_empty(), "{name}: {:?}", parsed.errors);
            assert_eq!(inspection.row_count, fixture.rows, "{name}");
            assert_eq!(parsed.total_rows, fixture.rows, "{name}");
            assert_eq!(
                parsed.skipped.len(),
                fixture.skipped,
                "{name}: {:?}",
                parsed.skipped
            );
            let buys = parsed
                .trades
                .iter()
                .filter(|t| t.direction == TradeDirection::Buy)
                .count();
            assert_eq!(buys, fixture.buys, "{name}: purchases");
            assert_eq!(parsed.trades.len() - buys, fixture.sells, "{name}: sales");
            assert_eq!(
                parsed.dividends.len(),
                fixture.dividends,
                "{name}: dividends"
            );
            assert_eq!(
                parsed.trades.len() + parsed.dividends.len() + parsed.skipped.len(),
                fixture.rows,
                "{name}: every row is a trade, a dividend or skipped"
            );

            let want = &fixture.first;
            let got = &parsed.trades[0];
            assert_eq!(got.line, want.line, "{name}");
            assert_eq!(got.day, want.day, "{name}");
            assert_eq!(got.direction, want.direction, "{name}");
            assert_eq!(got.instrument_key, want.key, "{name}");
            assert_eq!(got.symbol.as_deref(), want.symbol, "{name}");
            assert_eq!(got.isin.as_deref(), want.isin, "{name}");
            assert_eq!(got.name.as_deref(), want.name, "{name}");
            assert_eq!(
                (got.quantity, got.price),
                (want.quantity, want.price),
                "{name}"
            );
            assert_eq!(got.currency.as_deref(), want.currency, "{name}");
            assert_eq!(got.external_id.as_deref(), want.external_id, "{name}");
        }
    }

    #[test]
    fn every_stock_fixture_has_a_guard_entry() {
        let mut on_disk: Vec<String> = std::fs::read_dir(FIXTURE_DIR)
            .expect("fixture directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("stocks-") && name.ends_with(".csv"))
            .collect();
        on_disk.sort();
        let mut guarded: Vec<String> = fixtures()
            .iter()
            .map(|f| f.file)
            .chain(GENERIC_FIXTURES)
            .map(str::to_string)
            .collect();
        guarded.sort();
        assert_eq!(
            on_disk, guarded,
            "a stocks-*.csv fixture without a guard entry"
        );
    }

    #[test]
    fn the_excel_table_is_really_windows_1250_with_windows_line_ends() {
        let bytes = fixture_bytes("stocks-handmade-cz.csv");
        assert!(String::from_utf8(bytes.clone()).is_err(), "not UTF-8");
        assert!(bytes.windows(2).any(|w| w == b"\r\n"));
        let i = inspect(&bytes, "t.csv", &StockCsvInspectOptions::default(), &[]).expect("inspect");
        assert_eq!(i.encoding, "windows-1250");
        assert_eq!(
            (i.delimiter.as_str(), i.decimal_separator.as_str()),
            (";", ",")
        );
        assert_eq!(i.headers[3], "Název");
        let config = i.config.expect("config");
        let parsed = parse_file(&bytes, &config).expect("parse");
        // A non-breaking space between thousands, the type words in Czech.
        let kb = parsed
            .trades
            .iter()
            .find(|t| t.symbol.as_deref() == Some("KOMB.PR"))
            .expect("Komerční banka");
        assert_eq!((kb.quantity, kb.price), (50.0, 1056.0));
        assert_eq!(kb.name.as_deref(), Some("Komerční banka"));
        assert_eq!(kb.direction, TradeDirection::Buy, "Nákup");
        let vwce = parsed
            .trades
            .iter()
            .find(|t| t.symbol.as_deref() == Some("VWCE.DE"));
        assert_eq!(
            vwce.map(|t| t.direction),
            Some(TradeDirection::Buy),
            "koupě"
        );
    }

    #[test]
    fn london_prices_in_pence_are_pounds() {
        for (file, line) in [("stocks-trading212.csv", 6), ("stocks-degiro.csv", 7)] {
            let bytes = fixture_bytes(file);
            let config = inspect(&bytes, file, &StockCsvInspectOptions::default(), &[])
                .expect("inspect")
                .config
                .expect("config");
            let parsed = parse_file(&bytes, &config).expect("parse");
            let vodafone = parsed.trades.iter().find(|t| t.line == line).expect(file);
            assert_eq!(vodafone.isin.as_deref(), Some("GB00BH4HKS39"), "{file}");
            assert_eq!(vodafone.currency.as_deref(), Some("GBP"), "{file}");
            assert_eq!(vodafone.price, 0.6852, "{file}: 68.52 pence");
        }
    }

    #[test]
    fn xtb_symbols_are_yahoo_symbols_and_prices_have_no_currency() {
        let bytes = fixture_bytes("stocks-xtb.csv");
        let config = inspect(&bytes, "x.csv", &StockCsvInspectOptions::default(), &[])
            .expect("inspect")
            .config
            .expect("config");
        let parsed = parse_file(&bytes, &config).expect("parse");
        let symbols: Vec<&str> = parsed
            .trades
            .iter()
            .filter_map(|t| t.symbol.as_deref())
            .collect();
        assert_eq!(
            symbols,
            ["AAPL", "VUSA.L", "AAPL", "SPYL.DE", "CEZ.PR", "VUSA.L"]
        );
        assert!(parsed.trades.iter().all(|t| t.currency.is_none()));
        let directions: Vec<TradeDirection> = parsed.trades.iter().map(|t| t.direction).collect();
        use TradeDirection::{Buy, Sell};
        assert_eq!(directions, [Buy, Buy, Sell, Buy, Buy, Sell]);
    }

    /// An XTB export of a London share in pence (BARC.UK: XTB's prices follow the exchange line's
    /// quote unit) and of one in pounds, read the way the wizard reads it end to end.
    const XTB_PENCE_FILE: &str = "ID;Type;Time;Symbol;Comment;Amount\n\
        1;Stocks/ETF purchase;04.01.2024 15:31:18;BARC.UK;OPEN BUY 100 @ 443.6500;-443.65\n\
        2;Stocks/ETF sale;15.04.2024 09:30:10;BARC.UK;CLOSE BUY 40 @ 491.2350;196.49\n\
        3;Stocks/ETF purchase;04.01.2024 15:32:00;VUSA.UK;OPEN BUY 10 @ 78.2200;-782.20\n";

    #[test]
    fn an_xtb_file_in_pence_is_stored_in_pounds_when_the_listing_says_gbx() {
        use crate::services::stock_import::simulate::test_db::db;
        use crate::services::stock_import::types::CurrencyMode;
        use crate::services::stock_import::{import, preview};

        let bytes = XTB_PENCE_FILE.as_bytes();
        let mut config = inspect(bytes, "x.csv", &StockCsvInspectOptions::default(), &[])
            .expect("inspect")
            .config
            .expect("config");
        assert_eq!(config.currency_mode, CurrencyMode::Instrument);
        // What the wizard sets from the lookup: BARC.L is quoted in pence, VUSA.L in pounds.
        config.instrument_overrides = vec![
            StockInstrumentOverride {
                key: "symbol:BARC.L".into(),
                ticker: None,
                name: None,
                currency: Some("GBX".into()),
                skip: false,
                splits: vec![],
            },
            StockInstrumentOverride {
                key: "symbol:VUSA.L".into(),
                ticker: None,
                name: None,
                currency: Some("GBP".into()),
                skip: false,
                splits: vec![],
            },
        ];
        let parsed = parse_file(bytes, &config).expect("parse");
        let mut conn = db();

        let review = preview::preview(&conn, &parsed, &config).expect("preview");
        let rows: Vec<(usize, &str, &str)> = review
            .rows
            .iter()
            .map(|r| {
                (
                    r.line,
                    r.price.as_deref().unwrap_or("-"),
                    r.currency.as_deref().unwrap_or("-"),
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                (2, "4.4365", "GBP"),
                (3, "4.91235", "GBP"),
                (4, "78.22", "GBP")
            ]
        );
        assert_eq!(review.counts.will_import, 3);

        let first = import::import(&mut conn, &parsed, &config, "x.csv").expect("import");
        assert_eq!(first.imported, 3);
        let again = import::import(&mut conn, &parsed, &config, "x.csv").expect("again");
        assert_eq!((again.imported, again.duplicates), (0, 3));
    }

    #[test]
    fn without_the_gbx_override_the_pence_stay_what_the_file_says() {
        // The suffix guess knows nothing of pence: this is what the lookup's override prevents.
        use crate::services::stock_import::preview;
        use crate::services::stock_import::simulate::test_db::db;

        let bytes = XTB_PENCE_FILE.as_bytes();
        let config = inspect(bytes, "x.csv", &StockCsvInspectOptions::default(), &[])
            .expect("inspect")
            .config
            .expect("config");
        let parsed = parse_file(bytes, &config).expect("parse");
        let review = preview::preview(&db(), &parsed, &config).expect("preview");
        assert_eq!(review.rows[0].price.as_deref(), Some("443.65"));
        assert_eq!(review.rows[0].currency.as_deref(), Some("GBP"));
    }

    #[test]
    fn the_ibkr_trailer_and_non_stock_rows_are_skipped_not_errors() {
        let bytes = fixture_bytes("stocks-ibkr.csv");
        let i = inspect(&bytes, "i.csv", &StockCsvInspectOptions::default(), &[]).expect("inspect");
        assert_eq!(i.header_row, 1, "the BOF record is no header");
        let config = i.config.expect("config");
        let parsed = parse_file(&bytes, &config).expect("parse");
        let skipped: Vec<(usize, &str, Option<&str>)> = parsed
            .skipped
            .iter()
            .map(|m| (m.line, m.key.as_str(), m.detail.as_deref()))
            .collect();
        assert_eq!(
            skipped,
            [
                (7, "importWizard.row.assetClass", Some("OPT")),
                (8, "importWizard.row.assetClass", Some("CASH")),
                (11, "importWizard.row.notATrade", None),
            ]
        );
    }

    #[test]
    fn the_old_template_is_the_moony_table_without_a_name() {
        let bytes = fixture_bytes("stocks-template.csv");
        let i = inspect(&bytes, "t.csv", &StockCsvInspectOptions::default(), &[]).expect("inspect");
        assert_eq!(i.detected_source.as_deref(), Some(SOURCE_MOONY));
        let config = i.config.expect("config");
        assert_eq!(config.name_column, None);
        let parsed = parse_file(&bytes, &config).expect("parse");
        assert_eq!(
            (
                parsed.trades.len(),
                parsed.skipped.len(),
                parsed.errors.len()
            ),
            (5, 0, 0)
        );
        assert_eq!(parsed.trades[3].symbol.as_deref(), Some("EUNL.DE"));
        assert_eq!(parsed.trades[2].direction, TradeDirection::Sell);
    }

    #[test]
    fn a_file_of_another_broker_is_mapped_generically() {
        let bytes = fixture_bytes("stocks-messy.csv");
        let i = inspect(&bytes, "m.csv", &StockCsvInspectOptions::default(), &[]).expect("inspect");
        assert_eq!(i.detected_source, None);
        let config = i.config.expect("a complete suggestion from the headers");
        assert_eq!(config.source, "custom");
        assert_eq!(
            (
                config.decimal_separator.as_str(),
                config.date_format.as_str()
            ),
            (",", "%d.%m.%Y")
        );
        let parsed = parse_file(&bytes, &config).expect("parse");
        assert_eq!(parsed.errors, []);
        let symbols: Vec<&str> = parsed
            .trades
            .iter()
            .filter_map(|t| t.symbol.as_deref())
            .collect();
        assert_eq!(symbols, ["AAPL", "AAPL", "NOTATICKER123"]);
        assert_eq!(parsed.trades[1].direction, TradeDirection::Sell);
        assert_eq!(parsed.skipped.len(), 1, "the dividend");
    }

    // ---- dividends -----------------------------------------------------------

    fn inspect_text(text: &str) -> crate::services::stock_import::types::StockCsvInspection {
        inspect(
            text.as_bytes(),
            "d.csv",
            &StockCsvInspectOptions::default(),
            &[],
        )
        .expect("inspect")
    }

    #[test]
    fn trading212_dividends_are_shares_times_the_gross_per_share_with_the_tax_of_the_row() {
        let bytes = fixture_bytes("stocks-trading212.csv");
        let i = inspect(&bytes, "t.csv", &StockCsvInspectOptions::default(), &[]).expect("inspect");
        let config = i.config.expect("config");
        // `Total` is the net amount in the account currency: the amount column stays unset.
        assert_eq!(config.amount_column, None);
        assert_eq!(config.tax_column, Some(13));
        assert_eq!(config.tax_currency_column, Some(14));
        let parsed = parse_file(&bytes, &config).expect("parse");
        assert_eq!(parsed.dividends.len(), 1);
        let d = &parsed.dividends[0];
        assert_eq!(
            (
                d.line,
                d.day,
                d.instrument_key.as_str(),
                d.currency.as_str()
            ),
            (8, 1705276800, "isin:US0378331005", "USD")
        );
        assert_eq!(
            (d.gross, d.tax),
            (0.48, 0.07),
            "2 shares at 0.24; tax in USD"
        );
        assert_eq!(parsed.unpaired_taxes, 0);
    }

    #[test]
    fn xtb_dividends_use_the_amount_column_and_the_account_currency_the_user_names() {
        let bytes = fixture_bytes("stocks-xtb.csv");
        let i = inspect(&bytes, "x.csv", &StockCsvInspectOptions::default(), &[]).expect("inspect");
        let mut config = i.config.expect("config");
        assert_eq!(config.amount_column, Some(5));
        assert_eq!(config.cash_currency, None);

        // Without it the dividend and its tax row ask for the account currency.
        let parsed = parse_file(&bytes, &config).expect("parse");
        assert!(parsed.needs_cash_currency);
        assert!(parsed.dividends.is_empty());
        assert_eq!(parsed.errors.len(), 2);

        config.cash_currency = Some("USD".into());
        let parsed = parse_file(&bytes, &config).expect("parse");
        assert!(!parsed.needs_cash_currency && parsed.errors.is_empty());
        let d = &parsed.dividends[0];
        assert_eq!(
            (
                d.line,
                d.symbol.as_deref(),
                d.gross,
                d.tax,
                d.currency.as_str()
            ),
            (6, Some("AAPL"), 0.72, 0.11, "USD")
        );
        assert_eq!(d.external_id.as_deref(), Some("610000005"));
        assert!(parsed
            .skipped
            .iter()
            .any(|m| m.line == 7 && m.key == "importWizard.row.taxPaired"));
    }

    const IBKR_CASH: &str = "\
\"Symbol\",\"ISIN\",\"Date/Time\",\"Type\",\"Amount\",\"CurrencyPrimary\",\"TransactionID\",\"Description\"
\"AAPL\",\"US0378331005\",\"20230810;093000\",\"Dividends\",\"12.00\",\"USD\",\"8001\",\"AAPL(US0378331005) CASH DIVIDEND USD 0.24 PER SHARE\"
\"AAPL\",\"US0378331005\",\"20230810;093000\",\"Withholding Tax\",\"-1.80\",\"USD\",\"8002\",\"AAPL(US0378331005) CASH DIVIDEND USD 0.24 PER SHARE - US TAX\"
\"AAPL\",\"US0378331005\",\"20230811;093000\",\"Payment In Lieu Of Dividends\",\"3.00\",\"USD\",\"8003\",\"PAYMENT IN LIEU\"
\"AAPL\",\"US0378331005\",\"20230811;093000\",\"Withholding Tax\",\"0.30\",\"USD\",\"8004\",\"REVERSAL\"
\"\",\"\",\"20230831;000000\",\"Broker Interest Received\",\"0.50\",\"USD\",\"8005\",\"USD CREDIT INT\"
";

    #[test]
    fn an_ibkr_cash_transactions_section_is_read_for_its_dividends_and_taxes() {
        let i = inspect_text(IBKR_CASH);
        assert_eq!(i.detected_source.as_deref(), Some(SOURCE_IBKR));
        let config = i.config.expect("config");
        assert_eq!(config.source, SOURCE_IBKR);
        assert_eq!(
            (
                config.date_column,
                config.amount_column,
                config.external_id_column
            ),
            (2, Some(4), Some(6))
        );
        assert_eq!(config.date_format, "%Y%m%d");
        let parsed = parse_file(IBKR_CASH.as_bytes(), &config).expect("parse");
        assert_eq!(parsed.errors, []);
        assert_eq!(parsed.dividends.len(), 2);
        assert_eq!(
            (parsed.dividends[0].gross, parsed.dividends[0].tax),
            (12.0, 1.8)
        );
        assert_eq!(parsed.dividends[0].day, 1691625600);
        assert_eq!(parsed.dividends[0].external_id.as_deref(), Some("8001"));
        // A reversal row (positive) cancels the charge of its day.
        assert_eq!(
            (parsed.dividends[1].gross, parsed.dividends[1].tax),
            (3.0, 0.3)
        );
        assert_eq!(
            parsed
                .skipped
                .iter()
                .map(|m| (m.line, m.key.as_str()))
                .collect::<Vec<_>>(),
            [
                (3, "importWizard.row.taxPaired"),
                (5, "importWizard.row.taxPaired"),
                (6, "importWizard.row.notATrade"),
            ]
        );
        assert_eq!(parsed.total_rows, 5);
    }

    #[test]
    fn ibkr_date_times_with_a_comma_read_and_the_trades_section_is_still_the_trades_section() {
        let text = IBKR_CASH
            .replace("20230810;093000", "2023-08-10, 09:30:00")
            .replace("20230811;093000", "2023-08-11, 09:30:00")
            .replace("20230831;000000", "2023-08-31, 00:00:00");
        let i = inspect_text(&text);
        let config = i.config.expect("config");
        assert_eq!(config.date_format, "%Y-%m-%d");
        let parsed = parse_file(text.as_bytes(), &config).expect("parse");
        assert_eq!(parsed.errors, []);
        assert_eq!(parsed.dividends[0].day, 1691625600);

        // Settle or report dates work as well, without a time.
        let settle = IBKR_CASH.replace("Date/Time", "SettleDate");
        assert_eq!(
            inspect_text(&settle).detected_source.as_deref(),
            Some(SOURCE_IBKR)
        );
        // With a TradePrice it is the Trades section whatever else it has.
        let trades = fixture_bytes("stocks-ibkr.csv");
        let i = inspect(&trades, "i.csv", &StockCsvInspectOptions::default(), &[]).unwrap();
        assert_eq!(i.config.expect("config").amount_column, None);
    }

    const DEGIRO_STATEMENT: &str = "\
Date,Time,Value date,Product,ISIN,Description,FX,Change,,Balance,,Order Id
15-03-2024,06:10,15-03-2024,APPLE INC,US0378331005,Dividend,,USD,12.00,USD,112.00,
15-03-2024,06:10,15-03-2024,APPLE INC,US0378331005,Dividend Tax,,USD,-1.80,USD,110.20,
14-03-2024,09:31,14-03-2024,APPLE INC,US0378331005,Koop 5 @ 170 USD,,USD,-850.00,USD,100.00,6f1c2c9a-4a8e-4d5b-9c1e-2b7a1d9e0f11
13-03-2024,08:00,13-03-2024,,,Deposit,,EUR,500.00,EUR,950.00,
";

    #[test]
    fn a_degiro_account_statement_is_read_for_its_dividends_only() {
        let i = inspect_text(DEGIRO_STATEMENT);
        assert_eq!(i.detected_source.as_deref(), Some(SOURCE_DEGIRO));
        let config = i.config.expect("config");
        assert!(config.transforms.cash_rows_only);
        assert_eq!(
            (
                config.amount_column,
                config.currency_column,
                config.type_column
            ),
            (Some(8), Some(7), Some(5))
        );
        assert_eq!(config.date_format, "%d-%m-%Y");
        let parsed = parse_file(DEGIRO_STATEMENT.as_bytes(), &config).expect("parse");
        assert_eq!(parsed.errors, []);
        assert_eq!(
            parsed.trades.len(),
            0,
            "its trades come from the Transactions export"
        );
        assert_eq!(parsed.dividends.len(), 1);
        let d = &parsed.dividends[0];
        assert_eq!(
            (
                d.instrument_key.as_str(),
                d.gross,
                d.tax,
                d.currency.as_str()
            ),
            ("isin:US0378331005", 12.0, 1.8, "USD")
        );
        assert_eq!(parsed.skipped.len(), 3);
        assert_eq!(parsed.total_rows, 4);
        // The Transactions export is still its own thing.
        let bytes = fixture_bytes("stocks-degiro.csv");
        let i = inspect(&bytes, "d.csv", &StockCsvInspectOptions::default(), &[]).unwrap();
        assert!(!i.config.expect("config").transforms.cash_rows_only);
    }

    #[test]
    fn the_moony_table_reads_dividends_in_english_and_czech() {
        let en = "Date,Type,Ticker,Name,Quantity,Price,Currency,Amount,Withholding tax\n\
2024-01-10,buy,AAPL,Apple,10,150,USD,,\n\
2024-03-15,dividend,AAPL,Apple,,,USD,12.5,1.5\n";
        let config = inspect_text(en).config.expect("config");
        let parsed = parse_file(en.as_bytes(), &config).expect("parse");
        assert_eq!(parsed.errors, []);
        assert_eq!(
            (parsed.dividends[0].gross, parsed.dividends[0].tax),
            (12.5, 1.5)
        );

        let cs = "Datum;Typ;Symbol;Název;Počet;Cena;Měna;Částka;Srážková daň\n\
10.01.2024;nákup;AAPL;Apple;10;150;USD;;\n\
15.03.2024;dividenda;AAPL;Apple;;;USD;12,5;1,5\n";
        let config = inspect_text(cs).config.expect("config");
        assert_eq!(config.decimal_separator, ",");
        let parsed = parse_file(cs.as_bytes(), &config).expect("parse");
        assert_eq!(parsed.errors, []);
        assert_eq!(
            (parsed.dividends[0].gross, parsed.dividends[0].tax),
            (12.5, 1.5)
        );
    }
}
