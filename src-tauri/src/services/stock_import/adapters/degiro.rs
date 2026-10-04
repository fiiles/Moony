//! Degiro: the Transactions export (`dd-MM-yyyy`, English UI or another).
//!
//! `Date,Time,Product,ISIN,Reference exchange,Venue,Quantity,Price,,Local
//! value,,Value,,Exchange rate,[AutoFX Fee,]Transaction and/or third party
//! fees,,Total,,Order ID`: the headers are translated with the UI language but
//! the layout is not, and the currency of an amount sits in the unnamed column
//! after it. Sells have a negative quantity; some locales write decimal commas
//! and quote the numbers. There is no ticker, only the ISIN. (The account
//! statement is a different file.)
//!
//! The export is detected by its structure, not by header words: at least 17
//! columns, `ISIN` at index 3, an unnamed column right after the price,
//! ISIN-shaped values and `dd-MM-yyyy` dates in the rows.

use crate::services::csv_import::columns::normalize_header;
use crate::services::stock_import::detect::is_isin;
use crate::services::stock_import::types::{CurrencyMode, DirectionMode, SOURCE_DEGIRO};

use super::{base_config, decimal_of, AdapterConfig, StockAdapter};

pub const ADAPTER: StockAdapter = StockAdapter {
    source: SOURCE_DEGIRO,
    detect,
    config,
};

const MIN_COLUMNS: usize = 17;
const DATE_COLUMN: usize = 0;
const NAME_COLUMN: usize = 2;
const ISIN_COLUMN: usize = 3;
const QUANTITY_COLUMN: usize = 6;
const PRICE_COLUMN: usize = 7;
/// The unnamed column after the price holds its currency.
const CURRENCY_COLUMN: usize = PRICE_COLUMN + 1;

/// The shape of the header row.
fn has_structure(headers: &[String]) -> bool {
    headers.len() >= MIN_COLUMNS
        && normalize_header(&headers[ISIN_COLUMN]) == "isin"
        && headers[CURRENCY_COLUMN].trim().is_empty()
}

/// `15-01-2024`, `5-1-2024`.
fn is_dd_mm_yyyy(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 3
        && (1..=2).contains(&parts[0].len())
        && (1..=2).contains(&parts[1].len())
        && parts[2].len() == 4
        && parts
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
}

/// The rows say the same: ISINs where the ISIN is, Degiro's dates. A file
/// without rows has nothing to contradict the header.
fn rows_fit(rows: &[Vec<String>]) -> bool {
    if rows.is_empty() {
        return true;
    }
    let cells = |column: usize| -> Vec<&str> {
        rows.iter()
            .filter_map(|row| row.get(column))
            .map(|cell| cell.trim())
            .filter(|cell| !cell.is_empty())
            .collect()
    };
    let isins = cells(ISIN_COLUMN);
    let dates = cells(DATE_COLUMN);
    !isins.is_empty()
        && isins.iter().all(|v| is_isin(v))
        && !dates.is_empty()
        && dates.iter().all(|d| is_dd_mm_yyyy(d))
}

pub fn detect(headers: &[String], rows: &[Vec<String>]) -> bool {
    has_structure(headers) && rows_fit(rows)
}

pub fn config(headers: &[String], rows: &[Vec<String>]) -> Option<AdapterConfig> {
    if !has_structure(headers) {
        return None;
    }
    let mut config = base_config(SOURCE_DEGIRO);
    config.date_column = DATE_COLUMN;
    config.date_format = "%d-%m-%Y".to_string();
    config.name_column = Some(NAME_COLUMN);
    config.isin_column = Some(ISIN_COLUMN);
    config.quantity_column = QUANTITY_COLUMN;
    config.price_column = PRICE_COLUMN;
    config.currency_mode = CurrencyMode::Column;
    config.currency_column = Some(CURRENCY_COLUMN);
    // A sale has a negative quantity.
    config.direction_mode = DirectionMode::QuantitySign;
    // The order id closes the row (after the optional AutoFX column moved
    // everything else by one).
    config.external_id_column = headers.iter().rposition(|h| !h.trim().is_empty());
    // ...and is shared by the fills of the order.
    config.transforms.broker_id_per_order = true;
    config.decimal_separator = decimal_of(rows, &[QUANTITY_COLUMN, PRICE_COLUMN]);
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

    /// Degiro's order id is shared by the fills of an order; the flag says so
    /// and travels with a mapping the user remembers.
    #[test]
    fn the_order_id_is_marked_as_shared_by_the_fills() {
        let config = config(&headers(&DEGIRO_HEADERS), &sample_rows())
            .expect("degiro")
            .config;
        assert!(config.transforms.broker_id_per_order);
    }

    /// A purchase and a sale (negative quantity), decimal commas as in the
    /// Dutch and Czech exports.
    fn sample_rows() -> Vec<Vec<String>> {
        rows(&[
            &[
                "15-01-2024",
                "09:31",
                "VANGUARD S&P 500 UCITS ETF",
                "IE00B3XXRP09",
                "EAM",
                "XAMS",
                "5",
                "78,2200",
                "EUR",
                "-391,10",
                "EUR",
                "-391,10",
                "EUR",
                "",
                "-1,00",
                "EUR",
                "-392,10",
                "EUR",
                "6f1c2c9a-4a8e-4d5b-9c1e-2b7a1d9e0f11",
            ],
            &[
                "20-02-2024",
                "10:15",
                "VANGUARD S&P 500 UCITS ETF",
                "IE00B3XXRP09",
                "EAM",
                "XAMS",
                "-2",
                "79,1000",
                "EUR",
                "158,20",
                "EUR",
                "158,20",
                "EUR",
                "",
                "-1,00",
                "EUR",
                "157,20",
                "EUR",
                "a1b2c3d4-0000-4000-8000-000000000002",
            ],
        ])
    }

    #[test]
    fn the_transactions_export_is_detected_by_its_structure() {
        let data = sample_rows();
        for list in [
            &DEGIRO_HEADERS[..],
            &DEGIRO_AUTOFX_HEADERS[..],
            &DEGIRO_HEADERS_NL[..],
            &DEGIRO_HEADERS_CS[..],
        ] {
            assert!(detect(&headers(list), &data), "{list:?}");
        }
        // A file without rows has nothing to contradict the header.
        assert!(detect(&headers(&DEGIRO_HEADERS), &[]));
    }

    #[test]
    fn the_structure_has_to_fit() {
        let data = sample_rows();
        // Fewer than 17 columns.
        let short = headers(&DEGIRO_HEADERS[..16]);
        assert!(!detect(&short, &data));
        // No ISIN at index 3.
        let mut no_isin = headers(&DEGIRO_HEADERS);
        no_isin[3] = "Ticker".into();
        assert!(!detect(&no_isin, &data));
        // The price is not followed by an unnamed currency column.
        let mut named = headers(&DEGIRO_HEADERS);
        named[8] = "Currency".into();
        assert!(!detect(&named, &data));
    }

    #[test]
    fn the_values_have_to_fit() {
        let h = headers(&DEGIRO_HEADERS);
        // ISO dates are not Degiro's.
        let mut iso = sample_rows();
        iso[0][0] = "2024-01-15".into();
        assert!(!detect(&h, &iso));
        // A ticker where the ISIN is.
        let mut ticker = sample_rows();
        ticker[1][3] = "VWCE".into();
        assert!(!detect(&h, &ticker));
        // Rows without any ISIN prove nothing.
        let mut blank = sample_rows();
        blank[0][3].clear();
        blank[1][3].clear();
        assert!(!detect(&h, &blank));
    }

    #[test]
    fn other_exports_are_not_degiro() {
        let data = sample_rows();
        for list in [
            &XTB_HEADERS[..],
            &TRADING212_HEADERS[..],
            &IBKR_HEADERS[..],
            &MOONY_HEADERS_EN[..],
        ] {
            assert!(!detect(&headers(list), &data), "{list:?}");
        }
    }

    #[test]
    fn the_config_addresses_the_columns_by_position() {
        let adapter = config(&headers(&DEGIRO_HEADERS), &sample_rows()).expect("config");
        let c = adapter.config;
        assert_eq!(c.source, "degiro");
        assert_eq!((c.date_column, c.date_format.as_str()), (0, "%d-%m-%Y"));
        assert_eq!(
            (c.name_column, c.isin_column, c.symbol_column),
            (Some(2), Some(3), None)
        );
        assert_eq!((c.quantity_column, c.price_column), (6, 7));
        // The unnamed column after the price holds its currency.
        assert_eq!(c.currency_mode, CurrencyMode::Column);
        assert_eq!(c.currency_column, Some(8));
        assert_eq!(c.direction_mode, DirectionMode::QuantitySign);
        assert_eq!(c.type_column, None);
        assert_eq!(
            c.external_id_column,
            Some(18),
            "the order id closes the row"
        );
        assert_eq!(c.decimal_separator, ",");
        assert!(!adapter.date_ambiguous);
    }

    #[test]
    fn the_autofx_column_moves_the_order_id_only() {
        let c = config(&headers(&DEGIRO_AUTOFX_HEADERS), &[])
            .expect("config")
            .config;
        assert_eq!((c.quantity_column, c.price_column), (6, 7));
        assert_eq!(c.currency_column, Some(8));
        assert_eq!(c.external_id_column, Some(19));
    }

    #[test]
    fn a_decimal_point_locale_is_noticed() {
        let mut data = sample_rows();
        data[0][7] = "78.22".into();
        data[1][7] = "79.10".into();
        let c = config(&headers(&DEGIRO_HEADERS), &data)
            .expect("config")
            .config;
        assert_eq!(c.decimal_separator, ".");
    }

    #[test]
    fn trailing_blank_headers_do_not_hide_the_order_id() {
        let mut h = headers(&DEGIRO_HEADERS);
        h.push(String::new());
        let c = config(&h, &[]).expect("config").config;
        assert_eq!(c.external_id_column, Some(18));
    }

    #[test]
    fn config_needs_the_structure_but_not_the_proof() {
        // Rows that do not look like Degiro's: detect says no, the lenient
        // config still maps the columns (a forced source).
        let mut iso = sample_rows();
        iso[0][0] = "2024-01-15".into();
        assert!(config(&headers(&DEGIRO_HEADERS), &iso).is_some());
        assert!(config(&headers(&MOONY_HEADERS_EN), &iso).is_none());
    }
}
