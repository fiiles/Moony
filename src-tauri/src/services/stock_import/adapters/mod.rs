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
    use crate::services::stock_import::types::{
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
}
