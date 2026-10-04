//! Moony's own table: the export of the app and the hand-made file of a user
//! whose broker has no preset.
//!
//! `Date, Type, Ticker|Symbol, Name, Quantity, Price, Currency` in English or
//! `Datum, Typ, Symbol|Ticker, Název, Počet, Cena, Měna` in Czech, in any
//! order and any mix of the two, `Name` optional. The type words are
//! buy/sell/nákup/prodej/koupě (case and diacritics do not matter). Spreadsheets
//! save such a file in any locale: any delimiter, encoding and decimal mark,
//! dates `dd.MM.yyyy`, `d.M.yyyy` or `yyyy-MM-dd`.

use crate::services::stock_import::types::{CurrencyMode, SOURCE_MOONY};

use super::{base_config, date_format_of, decimal_of, find_header, AdapterConfig, StockAdapter};

pub const ADAPTER: StockAdapter = StockAdapter {
    source: SOURCE_MOONY,
    detect,
    config,
};

// Normalised header names.
const DATE: &[&str] = &["date", "datum"];
const TYPE: &[&str] = &["type", "typ"];
const SYMBOL: &[&str] = &["ticker", "symbol"];
const NAME: &[&str] = &["name", "nazev"];
const QUANTITY: &[&str] = &["quantity", "pocet", "pocet kusu"];
const PRICE: &[&str] = &["price", "cena", "cena za kus"];
const CURRENCY: &[&str] = &["currency", "mena"];
const ISIN: &[&str] = &["isin"];

/// Positions of the columns the adapter reads.
struct Columns {
    date: usize,
    kind: usize,
    symbol: usize,
    quantity: usize,
    price: usize,
    currency: usize,
    name: Option<usize>,
    isin: Option<usize>,
}

/// Six required columns; the name and an ISIN are optional, anything else is
/// ignored.
fn columns(headers: &[String]) -> Option<Columns> {
    Some(Columns {
        date: find_header(headers, DATE)?,
        kind: find_header(headers, TYPE)?,
        symbol: find_header(headers, SYMBOL)?,
        quantity: find_header(headers, QUANTITY)?,
        price: find_header(headers, PRICE)?,
        currency: find_header(headers, CURRENCY)?,
        name: find_header(headers, NAME),
        isin: find_header(headers, ISIN),
    })
}

/// The table: all six required columns are there.
pub fn detect(headers: &[String], _rows: &[Vec<String>]) -> bool {
    columns(headers).is_some()
}

pub fn config(headers: &[String], rows: &[Vec<String>]) -> Option<AdapterConfig> {
    let cols = columns(headers)?;
    // The format of Moony's own export when nothing in the rows says more.
    let (date_format, date_ambiguous) = date_format_of(rows, cols.date, "%Y-%m-%d");
    let mut config = base_config(SOURCE_MOONY);
    config.date_column = cols.date;
    config.date_format = date_format;
    config.symbol_column = Some(cols.symbol);
    config.isin_column = cols.isin;
    config.name_column = cols.name;
    config.quantity_column = cols.quantity;
    config.price_column = cols.price;
    config.currency_mode = CurrencyMode::Column;
    config.currency_column = Some(cols.currency);
    config.type_column = Some(cols.kind);
    config.decimal_separator = decimal_of(rows, &[cols.quantity, cols.price]);
    Some(AdapterConfig {
        config,
        date_ambiguous,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::test_support::*;
    use crate::services::stock_import::types::{CurrencyMode, DirectionMode};

    fn english_rows() -> Vec<Vec<String>> {
        rows(&[
            &[
                "2024-01-15",
                "buy",
                "AAPL",
                "Apple Inc.",
                "10",
                "185.5",
                "USD",
            ],
            &["2024-02-20", "sell", "VWCE.DE", "", "2.5", "112.34", "EUR"],
        ])
    }

    fn czech_rows() -> Vec<Vec<String>> {
        rows(&[
            &[
                "15.1.2024",
                "nákup",
                "AAPL",
                "Apple Inc.",
                "10",
                "185,5",
                "USD",
            ],
            &[
                "05.02.2024",
                "prodej",
                "CEZ.PR",
                "ČEZ",
                "20",
                "950,00",
                "CZK",
            ],
        ])
    }

    #[test]
    fn the_english_and_the_czech_table_are_detected() {
        assert!(detect(&headers(&MOONY_HEADERS_EN), &english_rows()));
        assert!(detect(&headers(&MOONY_HEADERS_CS), &czech_rows()));
        // A file without rows is still the table.
        assert!(detect(&headers(&MOONY_HEADERS_CS), &[]));
    }

    #[test]
    fn the_columns_come_in_any_order_and_any_mix_of_languages() {
        let h = headers(&["Měna", "Price", "Počet", "Symbol", "Datum", "Type"]);
        assert!(detect(&h, &[]));
        let c = config(&h, &[]).expect("config").config;
        assert_eq!(
            (c.currency_column, c.price_column, c.quantity_column),
            (Some(0), 1, 2)
        );
        assert_eq!(
            (c.symbol_column, c.date_column, c.type_column),
            (Some(3), 4, Some(5))
        );
        assert_eq!(c.name_column, None, "the name is optional");
    }

    #[test]
    fn symbol_or_ticker_name_or_nazev() {
        for symbol in ["Symbol", "Ticker"] {
            let h = headers(&["Date", "Type", symbol, "Quantity", "Price", "Currency"]);
            assert!(detect(&h, &[]), "{symbol}");
        }
        for name in ["Name", "Název", "Nazev"] {
            let h = headers(&[
                "Date", "Type", "Ticker", name, "Quantity", "Price", "Currency",
            ]);
            let c = config(&h, &[]).expect("config").config;
            assert_eq!(c.name_column, Some(3), "{name}");
        }
    }

    #[test]
    fn the_wording_of_the_guide_is_accepted() {
        let h = headers(&[
            "Datum",
            "Typ",
            "Symbol",
            "Počet kusů",
            "Cena za kus",
            "Měna",
        ]);
        assert!(detect(&h, &[]));
    }

    #[test]
    fn extra_columns_are_ignored() {
        let h = headers(&[
            "Datum",
            "Typ",
            "Symbol",
            "ISIN",
            "Počet",
            "Cena",
            "Měna",
            "Poznámka",
            "Poplatek",
        ]);
        assert!(detect(&h, &[]));
        let c = config(&h, &[]).expect("config").config;
        assert_eq!(
            c.isin_column,
            Some(3),
            "an ISIN column is used when it is there"
        );
        assert_eq!(c.quantity_column, 4);
    }

    #[test]
    fn a_required_column_is_needed() {
        for dropped in ["Date", "Type", "Ticker", "Quantity", "Price", "Currency"] {
            let mut h = headers(&MOONY_HEADERS_EN);
            h.retain(|c| c != dropped);
            assert!(!detect(&h, &english_rows()), "without {dropped}");
            assert!(config(&h, &english_rows()).is_none(), "without {dropped}");
        }
    }

    #[test]
    fn other_exports_are_not_the_moony_table() {
        for list in [
            &XTB_HEADERS[..],
            &TRADING212_HEADERS[..],
            &DEGIRO_HEADERS[..],
            &IBKR_HEADERS[..],
            &[
                "Trade Date",
                "Action",
                "Symbol",
                "Shares",
                "Unit Price",
                "CCY",
            ][..],
            &["Date", "Description", "Amount"][..],
        ] {
            assert!(!detect(&headers(list), &english_rows()), "{list:?}");
        }
    }

    #[test]
    fn the_english_config() {
        let adapter = config(&headers(&MOONY_HEADERS_EN), &english_rows()).expect("config");
        let c = adapter.config;
        assert_eq!(c.source, "moony");
        assert_eq!((c.date_column, c.date_format.as_str()), (0, "%Y-%m-%d"));
        assert_eq!(
            (c.type_column, c.direction_mode),
            (Some(1), DirectionMode::TypeColumn)
        );
        assert_eq!(
            (c.symbol_column, c.isin_column, c.name_column),
            (Some(2), None, Some(3))
        );
        assert_eq!((c.quantity_column, c.price_column), (4, 5));
        assert_eq!(
            (c.currency_mode, c.currency_column),
            (CurrencyMode::Column, Some(6))
        );
        assert_eq!(c.external_id_column, None);
        assert_eq!(c.decimal_separator, ".");
        assert!(
            c.type_values.is_empty(),
            "inspect reads the values of the file"
        );
        assert!(!adapter.date_ambiguous);
    }

    #[test]
    fn the_czech_config_reads_dates_and_decimals_from_the_data() {
        let adapter = config(&headers(&MOONY_HEADERS_CS), &czech_rows()).expect("config");
        // 15.1.2024 settles the order: day first, not padded.
        assert_eq!(adapter.config.date_format, "%d.%m.%Y");
        assert!(!adapter.date_ambiguous);
        assert_eq!(adapter.config.decimal_separator, ",");
    }

    #[test]
    fn an_unclear_day_month_order_is_flagged() {
        let data = rows(&[
            &["01.02.2024", "nákup", "AAPL", "", "1", "100,5", "USD"],
            &["03.04.2024", "nákup", "AAPL", "", "1", "101,5", "USD"],
        ]);
        let adapter = config(&headers(&MOONY_HEADERS_CS), &data).expect("config");
        assert_eq!(adapter.config.date_format, "%d.%m.%Y");
        assert!(adapter.date_ambiguous);
    }

    #[test]
    fn an_empty_table_gets_the_format_of_moonys_export() {
        let c = config(&headers(&MOONY_HEADERS_EN), &[])
            .expect("config")
            .config;
        assert_eq!(c.date_format, "%Y-%m-%d");
    }
}
