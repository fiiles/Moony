//! Trading 212: the History export (`,`, `yyyy-MM-dd HH:mm:ss(.SSS)`).
//!
//! `Action,Time,ISIN,Ticker,Name,No. of shares,Price / share,Currency (Price /
//! share),Exchange rate,Result,…,ID,…`: trades (`Market buy`, `Limit sell`, …)
//! mixed with deposits, dividends (`Dividend (Ordinary)`), interest and
//! currency conversions. The tickers carry no exchange, so the ISIN says what
//! the instrument is; prices of London listings come in `GBX`. The export has
//! changed its column order over time, so the columns are found by their
//! English names, in any position (the names have brackets, which is why the
//! plain header comparison is not used).

use crate::services::stock_import::types::{CurrencyMode, SOURCE_TRADING212};

use super::{base_config, decimal_of, find_header_exact, AdapterConfig, StockAdapter};

pub const ADAPTER: StockAdapter = StockAdapter {
    source: SOURCE_TRADING212,
    detect,
    config,
};

/// Positions of the columns the adapter reads.
struct Columns {
    action: usize,
    time: usize,
    isin: usize,
    ticker: usize,
    name: usize,
    shares: usize,
    price: usize,
    currency: usize,
}

/// The eight columns every History export has, wherever they are.
fn columns(headers: &[String]) -> Option<Columns> {
    Some(Columns {
        action: find_header_exact(headers, "Action")?,
        time: find_header_exact(headers, "Time")?,
        isin: find_header_exact(headers, "ISIN")?,
        ticker: find_header_exact(headers, "Ticker")?,
        name: find_header_exact(headers, "Name")?,
        shares: find_header_exact(headers, "No. of shares")?,
        price: find_header_exact(headers, "Price / share")?,
        currency: find_header_exact(headers, "Currency (Price / share)")?,
    })
}

/// A Trading 212 History export: all eight columns are there.
pub fn detect(headers: &[String], _rows: &[Vec<String>]) -> bool {
    columns(headers).is_some()
}

pub fn config(headers: &[String], rows: &[Vec<String>]) -> Option<AdapterConfig> {
    let cols = columns(headers)?;
    let mut config = base_config(SOURCE_TRADING212);
    config.date_column = cols.time;
    config.date_format = "%Y-%m-%d".to_string();
    config.isin_column = Some(cols.isin);
    config.symbol_column = Some(cols.ticker);
    config.name_column = Some(cols.name);
    config.quantity_column = cols.shares;
    config.price_column = cols.price;
    config.currency_mode = CurrencyMode::Column;
    config.currency_column = Some(cols.currency);
    config.type_column = Some(cols.action);
    config.external_id_column = find_header_exact(headers, "ID");
    config.decimal_separator = decimal_of(rows, &[cols.shares, cols.price]);
    Some(AdapterConfig {
        config,
        date_ambiguous: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::test_support::*;
    use crate::services::stock_import::types::{CurrencyMode, DirectionMode};

    fn sample_rows() -> Vec<Vec<String>> {
        rows(&[
            &[
                "Market buy",
                "2023-12-18 14:30:03.613",
                "US17275R1023",
                "CSCO",
                "Cisco Systems",
                "0.0290530000",
                "49.96",
                "USD",
                "1.09303",
                "",
                "EUR",
                "1.33",
                "EUR",
                "",
                "",
                "",
                "EOF7504196256",
                "",
                "",
            ],
            &[
                "Deposit",
                "2023-12-18 11:45:06.326",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "31.00",
                "EUR",
                "",
                "",
                "Transaction ID: 1",
                "8d3b8c00-0000-4000-8000-000000000001",
                "",
                "",
            ],
        ])
    }

    /// The layout of 2024: notes and the id moved before the shares.
    const NEWER_HEADERS: [&str; 17] = [
        "Action",
        "Time",
        "ISIN",
        "Ticker",
        "Name",
        "Notes",
        "ID",
        "No. of shares",
        "Price / share",
        "Currency (Price / share)",
        "Exchange rate",
        "Result",
        "Currency (Result)",
        "Total",
        "Currency (Total)",
        "Withholding tax",
        "Currency (Withholding tax)",
    ];

    #[test]
    fn the_history_export_is_detected_by_its_columns() {
        assert!(detect(&headers(&TRADING212_HEADERS), &sample_rows()));
        assert!(detect(&headers(&TRADING212_HEADERS), &[]));
        assert!(detect(&headers(&NEWER_HEADERS), &[]));
        // Order and case do not matter.
        let mut shuffled: Vec<String> = headers(&TRADING212_HEADERS);
        shuffled.reverse();
        assert!(detect(&shuffled, &[]));
        let lower: Vec<String> = TRADING212_HEADERS
            .iter()
            .map(|h| h.to_lowercase())
            .collect();
        assert!(detect(&lower, &[]));
    }

    #[test]
    fn other_exports_and_partial_headers_are_not_trading212() {
        for list in [
            &XTB_HEADERS[..],
            &DEGIRO_HEADERS[..],
            &IBKR_HEADERS[..],
            &MOONY_HEADERS_EN[..],
        ] {
            assert!(!detect(&headers(list), &sample_rows()), "{list:?}");
        }
        // The price currency is what the price is in.
        let mut missing = headers(&TRADING212_HEADERS);
        missing.retain(|h| h != "Currency (Price / share)");
        assert!(!detect(&missing, &[]));
        assert!(config(&missing, &[]).is_none());
    }

    #[test]
    fn the_config_maps_the_history_columns() {
        let adapter = config(&headers(&TRADING212_HEADERS), &sample_rows()).expect("config");
        let c = adapter.config;
        assert_eq!(c.source, "trading212");
        assert_eq!((c.date_column, c.date_format.as_str()), (1, "%Y-%m-%d"));
        assert_eq!(c.type_column, Some(0));
        assert_eq!(c.direction_mode, DirectionMode::TypeColumn);
        assert_eq!(
            (c.isin_column, c.symbol_column, c.name_column),
            (Some(2), Some(3), Some(4))
        );
        assert_eq!((c.quantity_column, c.price_column), (5, 6));
        assert_eq!(c.currency_mode, CurrencyMode::Column);
        assert_eq!(c.currency_column, Some(7));
        assert_eq!(c.external_id_column, Some(16));
        assert_eq!(c.decimal_separator, ".");
        assert!(
            c.type_values.is_empty(),
            "inspect reads the values of the file"
        );
        assert!(!adapter.date_ambiguous);
    }

    #[test]
    fn the_newer_layout_is_mapped_by_name() {
        let c = config(&headers(&NEWER_HEADERS), &[])
            .expect("config")
            .config;
        assert_eq!(c.external_id_column, Some(6));
        assert_eq!((c.quantity_column, c.price_column), (7, 8));
        assert_eq!(
            c.currency_column,
            Some(9),
            "the price currency, not the result's"
        );
    }

    #[test]
    fn a_file_without_an_id_column_still_maps() {
        let mut h = headers(&TRADING212_HEADERS);
        h.retain(|c| c != "ID");
        let c = config(&h, &[]).expect("config").config;
        assert_eq!(c.external_id_column, None);
    }
}
