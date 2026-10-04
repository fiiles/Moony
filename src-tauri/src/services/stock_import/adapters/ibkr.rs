//! Interactive Brokers: the Flex Query, Trades section (CSV).
//!
//! The user ticks the fields; the ones that matter have stable names:
//! `Symbol, ISIN, TradeDate, Buy/Sell, Quantity, TradePrice, CurrencyPrimary,
//! AssetClass, TradeID` (in any order; `ISIN`, `Buy/Sell`, `AssetClass` and
//! `TradeID` are optional, as is `Symbol` when there is an ISIN). Sells have a
//! negative quantity, the date is `yyyyMMdd` or another format chosen in the
//! query, and options and currency pairs come in the same rows, so rows whose
//! asset class is not `STK` are skipped.

use crate::services::stock_import::types::{CurrencyMode, DirectionMode, SOURCE_IBKR};

use super::{base_config, date_format_of, decimal_of, find_header, AdapterConfig, StockAdapter};

pub const ADAPTER: StockAdapter = StockAdapter {
    source: SOURCE_IBKR,
    detect,
    config,
};

// Normalised field names.
const DATE: &[&str] = &["tradedate"];
const QUANTITY: &[&str] = &["quantity"];
const PRICE: &[&str] = &["tradeprice"];
const CURRENCY: &[&str] = &["currencyprimary"];
const SYMBOL: &[&str] = &["symbol"];
const ISIN: &[&str] = &["isin"];
const DIRECTION: &[&str] = &["buy sell", "buysell"];
const ASSET_CLASS: &[&str] = &["assetclass"];
const TRADE_ID: &[&str] = &["tradeid"];
const NAME: &[&str] = &["description"];

/// The asset class of a share (and an ETF).
const STOCK: &str = "STK";

/// Positions of the fields the adapter reads.
struct Columns {
    date: usize,
    quantity: usize,
    price: usize,
    currency: usize,
    symbol: Option<usize>,
    isin: Option<usize>,
    direction: Option<usize>,
    asset_class: Option<usize>,
    trade_id: Option<usize>,
    name: Option<usize>,
}

/// The fields a trade cannot do without; a symbol or an ISIN says what it is.
fn columns(headers: &[String]) -> Option<Columns> {
    let symbol = find_header(headers, SYMBOL);
    let isin = find_header(headers, ISIN);
    if symbol.is_none() && isin.is_none() {
        return None;
    }
    Some(Columns {
        date: find_header(headers, DATE)?,
        quantity: find_header(headers, QUANTITY)?,
        price: find_header(headers, PRICE)?,
        currency: find_header(headers, CURRENCY)?,
        symbol,
        isin,
        direction: find_header(headers, DIRECTION),
        asset_class: find_header(headers, ASSET_CLASS),
        trade_id: find_header(headers, TRADE_ID),
        name: find_header(headers, NAME),
    })
}

/// A Flex Query with the trade fields: `TradeDate`, `Quantity`, `TradePrice`,
/// `CurrencyPrimary` and a symbol or an ISIN.
pub fn detect(headers: &[String], _rows: &[Vec<String>]) -> bool {
    columns(headers).is_some()
}

pub fn config(headers: &[String], rows: &[Vec<String>]) -> Option<AdapterConfig> {
    let cols = columns(headers)?;
    let (date_format, date_ambiguous) = date_format_of(rows, cols.date, "%Y%m%d");
    let mut config = base_config(SOURCE_IBKR);
    config.date_column = cols.date;
    config.date_format = date_format;
    config.symbol_column = cols.symbol;
    config.isin_column = cols.isin;
    config.name_column = cols.name;
    config.quantity_column = cols.quantity;
    config.price_column = cols.price;
    config.currency_mode = CurrencyMode::Column;
    config.currency_column = Some(cols.currency);
    // `Buy/Sell` when it is there, else the sign of the quantity.
    config.direction_mode = if cols.direction.is_some() {
        DirectionMode::TypeColumn
    } else {
        DirectionMode::QuantitySign
    };
    config.type_column = cols.direction;
    config.external_id_column = cols.trade_id;
    if let Some(column) = cols.asset_class {
        config.transforms.asset_class_column = Some(column);
        config.transforms.asset_class_allowed = vec![STOCK.to_string()];
    }
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

    fn sample_rows() -> Vec<Vec<String>> {
        rows(&[
            &[
                "AAPL",
                "US0378331005",
                "20230522",
                "BUY",
                "3",
                "172.4",
                "USD",
                "STK",
                "1001",
            ],
            &[
                "AAPL",
                "US0378331005",
                "20230630",
                "SELL",
                "-1",
                "189.5",
                "USD",
                "STK",
                "1002",
            ],
            &[
                "AAPL  250117C00200000",
                "",
                "20230701",
                "BUY",
                "1",
                "5.1",
                "USD",
                "OPT",
                "1003",
            ],
        ])
    }

    /// The shape of the example in the guide: no symbol, asset class or id.
    const MINIMAL: [&str; 9] = [
        "Buy/Sell",
        "TradeDate",
        "ISIN",
        "Quantity",
        "TradePrice",
        "TradeMoney",
        "CurrencyPrimary",
        "IBCommission",
        "IBCommissionCurrency",
    ];

    #[test]
    fn a_flex_query_with_the_trade_fields_is_detected() {
        assert!(detect(&headers(&IBKR_HEADERS), &sample_rows()));
        assert!(detect(&headers(&MINIMAL), &[]));
        // The order of the fields is free.
        let mut shuffled = headers(&IBKR_HEADERS);
        shuffled.reverse();
        assert!(detect(&shuffled, &[]));
        // A symbol is as good as an ISIN.
        let symbol_only = headers(&[
            "Symbol",
            "TradeDate",
            "Quantity",
            "TradePrice",
            "CurrencyPrimary",
        ]);
        assert!(detect(&symbol_only, &[]));
    }

    #[test]
    fn a_missing_required_field_is_not_detected() {
        for dropped in ["TradeDate", "Quantity", "TradePrice", "CurrencyPrimary"] {
            let mut h = headers(&IBKR_HEADERS);
            h.retain(|c| c != dropped);
            assert!(!detect(&h, &sample_rows()), "without {dropped}");
            assert!(config(&h, &sample_rows()).is_none(), "without {dropped}");
        }
        // Neither a symbol nor an ISIN.
        let mut h = headers(&IBKR_HEADERS);
        h.retain(|c| c != "Symbol" && c != "ISIN");
        assert!(!detect(&h, &sample_rows()));
    }

    #[test]
    fn other_exports_are_not_ibkr() {
        for list in [
            &XTB_HEADERS[..],
            &TRADING212_HEADERS[..],
            &DEGIRO_HEADERS[..],
            &MOONY_HEADERS_EN[..],
        ] {
            assert!(!detect(&headers(list), &sample_rows()), "{list:?}");
        }
    }

    #[test]
    fn the_config_maps_the_trade_fields() {
        let adapter = config(&headers(&IBKR_HEADERS), &sample_rows()).expect("config");
        let c = adapter.config;
        assert_eq!(c.source, "ibkr");
        assert_eq!((c.date_column, c.date_format.as_str()), (2, "%Y%m%d"));
        assert_eq!(
            (c.symbol_column, c.isin_column, c.name_column),
            (Some(0), Some(1), None)
        );
        assert_eq!((c.quantity_column, c.price_column), (4, 5));
        assert_eq!(c.currency_mode, CurrencyMode::Column);
        assert_eq!(c.currency_column, Some(6));
        assert_eq!(c.direction_mode, DirectionMode::TypeColumn);
        assert_eq!(c.type_column, Some(3));
        assert_eq!(c.external_id_column, Some(8));
        assert_eq!(c.transforms.asset_class_column, Some(7));
        assert_eq!(c.transforms.asset_class_allowed, vec!["STK"]);
        assert_eq!(c.decimal_separator, ".");
        assert!(!adapter.date_ambiguous);
    }

    #[test]
    fn without_buy_sell_the_sign_of_the_quantity_decides() {
        let c = config(
            &headers(&[
                "Symbol",
                "TradeDate",
                "Quantity",
                "TradePrice",
                "CurrencyPrimary",
            ]),
            &[],
        )
        .expect("config")
        .config;
        assert_eq!(c.direction_mode, DirectionMode::QuantitySign);
        assert_eq!(c.type_column, None);
        // BuySell without the slash is the same field.
        let c = config(
            &headers(&[
                "Symbol",
                "BuySell",
                "TradeDate",
                "Quantity",
                "TradePrice",
                "CurrencyPrimary",
            ]),
            &[],
        )
        .expect("config")
        .config;
        assert_eq!(
            (c.direction_mode, c.type_column),
            (DirectionMode::TypeColumn, Some(1))
        );
    }

    #[test]
    fn without_an_asset_class_nothing_is_filtered() {
        let c = config(&headers(&MINIMAL), &[]).expect("config").config;
        assert_eq!(c.transforms.asset_class_column, None);
        assert!(c.transforms.asset_class_allowed.is_empty());
        assert_eq!(c.external_id_column, None);
        assert_eq!((c.symbol_column, c.isin_column), (None, Some(2)));
    }

    #[test]
    fn the_description_is_the_name() {
        let h = headers(&[
            "Symbol",
            "Description",
            "TradeDate",
            "Quantity",
            "TradePrice",
            "CurrencyPrimary",
        ]);
        let c = config(&h, &[]).expect("config").config;
        assert_eq!(c.name_column, Some(1));
    }

    #[test]
    fn the_date_format_follows_the_file() {
        let h = headers(&IBKR_HEADERS);
        let with = |dates: &[&str]| {
            let data: Vec<Vec<String>> = dates
                .iter()
                .map(|d| {
                    let mut row = sample_rows()[0].clone();
                    row[2] = (*d).to_string();
                    row
                })
                .collect();
            let adapter = config(&h, &data).expect("config");
            (adapter.config.date_format, adapter.date_ambiguous)
        };
        assert_eq!(
            with(&["2023-05-22", "2023-06-30"]),
            ("%Y-%m-%d".into(), false)
        );
        assert_eq!(
            with(&["05/22/2023", "06/30/2023"]),
            ("%m/%d/%Y".into(), false)
        );
        assert_eq!(with(&["22/05/2023"]), ("%d/%m/%Y".into(), false));
        // Day and month cannot be told apart: flagged for the user.
        assert_eq!(
            with(&["05/06/2023", "01/02/2023"]),
            ("%d/%m/%Y".into(), true)
        );
        // A time after the date is no matter.
        assert_eq!(with(&["20230522;093000"]), ("%Y%m%d".into(), false));
        // No rows: the format IBKR writes by default.
        assert_eq!(
            config(&h, &[]).expect("config").config.date_format,
            "%Y%m%d"
        );
    }
}
