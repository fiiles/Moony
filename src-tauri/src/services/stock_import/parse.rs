//! Reading every row of a file with a configuration.
//!
//! Every data row ends in exactly one of `trades`, `skipped` (not a trade) and
//! `errors` (cannot be read), each with its 1-based file line and an i18n key
//! plus the raw value it is about; blank lines are not rows. Dates are read strictly with the file's one format, numbers with
//! the file's decimal separator (thousands separators and currency signs are
//! fine), quantity and price are absolute values (the direction carries the
//! sign), symbols, ISINs and currencies are uppercase and prices in pence
//! (`GBX`, `GBp`) are converted to pounds, whatever the source.
//!
//! What a row needs the database for (duplicates, holdings, currency of the
//! position) is not decided here, nor are the instrument overrides, `skip`
//! included: the rows of an instrument the user skipped stay trades, so the
//! preview still lists the instrument and the user can bring it back; the
//! simulation excludes them.

use std::collections::HashMap;

use csv::StringRecord;

use crate::error::{AppError, Result};
use crate::services::csv_import::amounts::{clean_and_parse_amount, AmountError};
use crate::services::csv_import::decode::{decode_csv_content, read_headers};
use crate::services::date_parser;

use super::adapters::xtb;
use super::columns::{fold_value, has_fee_column};
use super::detect::{data_records, date_part, delimiter_char, is_isin};
use super::types::{
    CurrencyMode, DirectionMode, ParsedFile, ParsedTrade, StockImportConfig, StockRowMessage,
    StockTypeValueMapping, TradeDirection, TypeValueAction,
};

/// Row messages (`stocks` namespace) and the one `common` key a row can carry.
pub const ROW_NOT_A_TRADE: &str = "importWizard.row.notATrade";
pub const ROW_ZERO_QUANTITY: &str = "importWizard.row.zeroQuantity";
pub const ROW_ASSET_CLASS: &str = "importWizard.row.assetClass";
pub const ROW_DATE_UNPARSEABLE: &str = "importWizard.row.dateUnparseable";
pub const ROW_NUMBER_UNPARSEABLE: &str = "importWizard.row.numberUnparseable";
pub const ROW_COMMENT_UNPARSEABLE: &str = "importWizard.row.commentUnparseable";
pub const ROW_SYMBOL_MISSING: &str = "importWizard.row.symbolMissing";
pub const ROW_CURRENCY_MISSING: &str = "importWizard.row.currencyMissing";
pub const ROW_PRICE_MISSING: &str = "importWizard.row.priceMissing";
/// A record the csv reader could not read at all.
pub const ROW_CANNOT_PARSE: &str = "importWizard.row.cannotParse";
pub const CURRENCY_INVALID: &str = "validation.currencyInvalid";

/// A row that is not a trade: why, and whether it is a skip or an error.
struct Note {
    skipped: bool,
    key: &'static str,
    detail: Option<String>,
}

fn skip(key: &'static str, detail: Option<&str>) -> Note {
    Note {
        skipped: true,
        key,
        detail: detail.map(str::to_string),
    }
}

fn fail(key: &'static str, detail: Option<&str>) -> Note {
    Note {
        skipped: false,
        key,
        detail: detail.map(str::to_string),
    }
}

/// `Some` unless empty.
fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

/// What each value of the type column means: matched exactly (trimmed) first,
/// then by folded text, so a saved format survives a change of case or
/// diacritics. A value not listed is not a trade.
struct TypeMap {
    exact: HashMap<String, TypeValueAction>,
    folded: HashMap<String, TypeValueAction>,
}

impl TypeMap {
    fn new(values: &[StockTypeValueMapping]) -> TypeMap {
        let mut exact = HashMap::new();
        let mut folded = HashMap::new();
        for mapping in values {
            exact
                .entry(mapping.value.trim().to_string())
                .or_insert(mapping.action);
            folded
                .entry(fold_value(&mapping.value))
                .or_insert(mapping.action);
        }
        TypeMap { exact, folded }
    }

    fn get(&self, value: &str) -> Option<TypeValueAction> {
        self.exact
            .get(value)
            .or_else(|| self.folded.get(&fold_value(value)))
            .copied()
    }
}

/// The currency of a cell and whether the price is in pence: `GBX` and `GBp`
/// are pence of the pound (`GBp` differs from `GBP` by one letter's case).
fn read_currency(raw: &str) -> std::result::Result<(String, bool), Note> {
    let raw = raw.trim();
    if raw == "GBp" || raw.eq_ignore_ascii_case("GBX") {
        return Ok(("GBP".to_string(), true));
    }
    if raw.len() == 3 && raw.bytes().all(|b| b.is_ascii_alphabetic()) {
        Ok((raw.to_ascii_uppercase(), false))
    } else {
        Err(fail(CURRENCY_INVALID, non_empty(raw)))
    }
}

/// Pence to pounds without the float noise of a plain division (49.1235, not
/// 49.12350000000001): the result is rounded to ten decimals.
fn pence_to_pounds(pence: f64) -> f64 {
    (pence / 100.0 * 1e10).round() / 1e10
}

/// A configuration prepared for reading rows.
struct RowReader<'a> {
    config: &'a StockImportConfig,
    decimal: char,
    types: TypeMap,
}

impl<'a> RowReader<'a> {
    fn new(config: &'a StockImportConfig, decimal: char) -> RowReader<'a> {
        RowReader {
            config,
            decimal,
            types: TypeMap::new(&config.type_values),
        }
    }

    /// The row as a trade, or why it is not one. The order of the checks is
    /// the order a user reads a row in: what kind of row, which instrument,
    /// when, how many, at what price, in what currency.
    fn read(&self, line: usize, record: &StringRecord) -> std::result::Result<ParsedTrade, Note> {
        let c = self.config;
        let cell = |column: usize| record.get(column).map_or("", str::trim);
        let optional = |column: Option<usize>| column.map(cell).and_then(non_empty);

        // What kind of row: a trade or not, by the type column.
        let mut direction = None;
        if c.direction_mode == DirectionMode::TypeColumn {
            let value = optional(c.type_column).unwrap_or("");
            match self.types.get(value) {
                Some(TypeValueAction::Buy) => direction = Some(TradeDirection::Buy),
                Some(TypeValueAction::Sell) => direction = Some(TradeDirection::Sell),
                Some(TypeValueAction::Skip) | None => {
                    return Err(skip(ROW_NOT_A_TRADE, non_empty(value)))
                }
            }
        }
        if let Some(column) = c.transforms.asset_class_column {
            let value = cell(column);
            if !c
                .transforms
                .asset_class_allowed
                .iter()
                .any(|allowed| allowed.trim().eq_ignore_ascii_case(value))
            {
                return Err(skip(ROW_ASSET_CLASS, non_empty(value)));
            }
        }

        // Which instrument: the ISIN when it is valid, else the symbol.
        let isin_cell = optional(c.isin_column);
        let isin = isin_cell
            .map(str::to_ascii_uppercase)
            .filter(|value| is_isin(value));
        let symbol = optional(c.symbol_column).map(|raw| {
            if c.transforms.xtb_symbols {
                xtb::yahoo_symbol(raw)
            } else {
                raw.to_uppercase()
            }
        });
        let instrument_key = match (&isin, &symbol) {
            (Some(isin), _) => format!("isin:{isin}"),
            (None, Some(symbol)) => format!("symbol:{symbol}"),
            (None, None) => return Err(fail(ROW_SYMBOL_MISSING, isin_cell)),
        };

        // When.
        let raw_date = cell(c.date_column);
        let Some(day) =
            date_parser::parse_date_to_timestamp_strict(date_part(raw_date), &c.date_format)
        else {
            return Err(fail(ROW_DATE_UNPARSEABLE, non_empty(raw_date)));
        };

        // How many: from the comment (XTB), else the column.
        let mut quantity;
        let mut price: Option<f64> = None;
        if c.transforms.xtb_comment {
            let comment = cell(c.quantity_column);
            let Some(trade) = xtb::parse_comment(comment, self.decimal) else {
                return Err(fail(ROW_COMMENT_UNPARSEABLE, non_empty(comment)));
            };
            direction = Some(trade.direction);
            quantity = trade.quantity;
            price = Some(trade.price);
        } else {
            let raw = cell(c.quantity_column);
            quantity = clean_and_parse_amount(raw, self.decimal)
                .map_err(|_| fail(ROW_NUMBER_UNPARSEABLE, non_empty(raw)))?;
            if c.direction_mode == DirectionMode::QuantitySign {
                direction = Some(if quantity < 0.0 {
                    TradeDirection::Sell
                } else {
                    TradeDirection::Buy
                });
            }
        }
        quantity = quantity.abs();
        if quantity == 0.0 {
            return Err(skip(ROW_ZERO_QUANTITY, None));
        }
        let Some(direction) = direction else {
            // A type column mode that listed no direction: not a trade.
            return Err(skip(ROW_NOT_A_TRADE, None));
        };

        // At what price: a zero price is missing, not free.
        let mut price = match price {
            Some(price) => price.abs(),
            None => {
                let raw = cell(c.price_column);
                match clean_and_parse_amount(raw, self.decimal) {
                    Ok(value) => value.abs(),
                    Err(AmountError::Empty) => return Err(fail(ROW_PRICE_MISSING, None)),
                    Err(AmountError::Invalid) => {
                        return Err(fail(ROW_NUMBER_UNPARSEABLE, non_empty(raw)))
                    }
                }
            }
        };
        if price == 0.0 {
            return Err(fail(ROW_PRICE_MISSING, None));
        }

        // In what currency.
        let (currency, pence) = match c.currency_mode {
            CurrencyMode::Column => {
                let raw =
                    optional(c.currency_column).ok_or_else(|| fail(ROW_CURRENCY_MISSING, None))?;
                let (code, pence) = read_currency(raw)?;
                (Some(code), pence)
            }
            CurrencyMode::Fixed => {
                let (code, pence) = read_currency(c.fixed_currency.as_deref().unwrap_or(""))?;
                (Some(code), pence)
            }
            CurrencyMode::Instrument => (None, false),
        };
        if pence {
            price = pence_to_pounds(price);
        }

        Ok(ParsedTrade {
            line,
            day,
            direction,
            instrument_key,
            symbol,
            isin,
            name: optional(c.name_column).map(str::to_string),
            quantity,
            price,
            currency,
            external_id: optional(c.external_id_column).map(str::to_string),
        })
    }
}

fn message(line: usize, note: Note) -> StockRowMessage {
    StockRowMessage {
        line,
        key: note.key.to_string(),
        detail: note.detail,
    }
}

/// Read every data row: each ends up as a trade, a skipped row or an error.
/// The config must have passed `validate()`; a delimiter or decimal separator
/// that did not is still refused here, and a file without a header row is
/// `validation.csvEmptyFile`.
pub fn parse_file(bytes: &[u8], config: &StockImportConfig) -> Result<ParsedFile> {
    let delimiter = delimiter_char(&config.delimiter)?;
    let decimal = match config.decimal_separator.as_str() {
        "," => ',',
        "." => '.',
        _ => {
            return Err(AppError::Validation(
                "validation.csvDecimalSeparatorInvalid".to_string(),
            ))
        }
    };
    let decoded = decode_csv_content(bytes, Some(config.encoding.as_str()));
    let content = decoded.text.as_str();
    let headers = read_headers(content, delimiter, config.header_row);
    if headers.iter().all(|h| h.is_empty()) {
        return Err(AppError::Validation("validation.csvEmptyFile".to_string()));
    }

    let reader = RowReader::new(config, decimal);
    let mut parsed = ParsedFile {
        has_fee_column: has_fee_column(&headers),
        ..ParsedFile::default()
    };
    for (line, result) in data_records(content, delimiter, config.header_row, config.skip_rows) {
        let record = match result {
            Ok(record) => record,
            Err(e) => {
                parsed.total_rows += 1;
                let detail = e.to_string();
                parsed
                    .errors
                    .push(message(line, fail(ROW_CANNOT_PARSE, Some(&detail))));
                continue;
            }
        };
        if record.iter().all(|cell| cell.trim().is_empty()) {
            continue;
        }
        parsed.total_rows += 1;
        match reader.read(line, &record) {
            Ok(trade) => parsed.trades.push(trade),
            Err(note) if note.skipped => parsed.skipped.push(message(line, note)),
            Err(note) => parsed.errors.push(message(line, note)),
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::test_support::*;
    use crate::services::stock_import::adapters::{base_config, xtb};
    use crate::services::stock_import::types::{
        CurrencyMode, DirectionMode, StockInstrumentOverride, StockTypeValueMapping,
        TradeDirection, TypeValueAction,
    };

    const HEADER: &str = "Datum;Typ;Symbol;Počet;Cena;Měna\n";

    fn mapping(value: &str, action: TypeValueAction) -> StockTypeValueMapping {
        StockTypeValueMapping {
            value: value.to_string(),
            action,
        }
    }

    /// The hand-made table: `Datum;Typ;Symbol;Počet;Cena;Měna`, decimal commas.
    fn config() -> StockImportConfig {
        let mut config = base_config("custom");
        config.delimiter = ";".into();
        config.date_format = "%d.%m.%Y".into();
        config.symbol_column = Some(2);
        config.quantity_column = 3;
        config.price_column = 4;
        config.currency_column = Some(5);
        config.type_column = Some(1);
        config.type_values = vec![
            mapping("nákup", TypeValueAction::Buy),
            mapping("prodej", TypeValueAction::Sell),
            mapping("dividenda", TypeValueAction::Skip),
        ];
        config.decimal_separator = ",".into();
        config
    }

    fn parse_with(rows: &str, config: &StockImportConfig) -> ParsedFile {
        parse_file(format!("{HEADER}{rows}").as_bytes(), config).expect("parse")
    }

    fn parse(rows: &str) -> ParsedFile {
        parse_with(rows, &config())
    }

    fn note(line: usize, key: &str, detail: Option<&str>) -> StockRowMessage {
        StockRowMessage {
            line,
            key: key.to_string(),
            detail: detail.map(str::to_string),
        }
    }

    /// The only trade of a one-row file.
    fn only_trade(rows: &str, config: &StockImportConfig) -> ParsedTrade {
        let parsed = parse_with(rows, config);
        assert_eq!(
            (
                parsed.trades.len(),
                parsed.skipped.len(),
                parsed.errors.len()
            ),
            (1, 0, 0),
            "{:?} {:?}",
            parsed.skipped,
            parsed.errors
        );
        parsed.trades[0].clone()
    }

    fn only_error(rows: &str, config: &StockImportConfig) -> StockRowMessage {
        let parsed = parse_with(rows, config);
        assert!(
            parsed.trades.is_empty() && parsed.skipped.is_empty(),
            "{parsed:?}"
        );
        assert_eq!(parsed.errors.len(), 1, "{parsed:?}");
        parsed.errors[0].clone()
    }

    fn only_skip(rows: &str, config: &StockImportConfig) -> StockRowMessage {
        let parsed = parse_with(rows, config);
        assert!(
            parsed.trades.is_empty() && parsed.errors.is_empty(),
            "{parsed:?}"
        );
        assert_eq!(parsed.skipped.len(), 1, "{parsed:?}");
        parsed.skipped[0].clone()
    }

    // ===================================================================
    // Rows
    // ===================================================================

    #[test]
    fn a_purchase_and_a_sale_become_trades() {
        let parsed =
            parse("15.01.2024;nákup;aapl;10;185,50;usd\n20.02.2024;prodej;AAPL;5;190,25;USD\n");
        assert_eq!(parsed.total_rows, 2);
        assert!(parsed.skipped.is_empty() && parsed.errors.is_empty());
        assert_eq!(
            parsed.trades[0],
            ParsedTrade {
                line: 2,
                day: 1705276800,
                direction: TradeDirection::Buy,
                instrument_key: "symbol:AAPL".into(),
                symbol: Some("AAPL".into()),
                isin: None,
                name: None,
                quantity: 10.0,
                price: 185.5,
                currency: Some("USD".into()),
                external_id: None,
            }
        );
        assert_eq!(parsed.trades[1].line, 3);
        assert_eq!(parsed.trades[1].day, 1708387200);
        assert_eq!(parsed.trades[1].direction, TradeDirection::Sell);
        assert_eq!(
            (parsed.trades[1].quantity, parsed.trades[1].price),
            (5.0, 190.25)
        );
    }

    #[test]
    fn every_row_ends_in_exactly_one_bucket_with_its_line() {
        let parsed = parse(
            "15.01.2024;nákup;AAPL;10;185,50;USD\n\
16.01.2024;dividenda;AAPL;0;1,20;USD\n\
17.01.2024;nákup;AAPL;0;185,50;USD\n\
18.01.2024;nákup;AAPL;abc;185,50;USD\n\
19.01.2024;nákup;AAPL;1;0;USD\n\
nedatum;nákup;AAPL;1;185,50;USD\n\
20.01.2024;nákup;;1;185,50;USD\n\
21.01.2024;nákup;AAPL;1;185,50;\n\
22.01.2024;nákup;AAPL;1;185,50;dolar\n\
\n\
23.01.2024;split;AAPL;1;1;USD\n",
        );
        assert_eq!(
            parsed.trades.iter().map(|t| t.line).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(
            parsed.skipped,
            [
                note(3, "importWizard.row.notATrade", Some("dividenda")),
                note(4, "importWizard.row.zeroQuantity", None),
                note(12, "importWizard.row.notATrade", Some("split")),
            ]
        );
        assert_eq!(
            parsed.errors,
            [
                note(5, "importWizard.row.numberUnparseable", Some("abc")),
                note(6, "importWizard.row.priceMissing", None),
                note(7, "importWizard.row.dateUnparseable", Some("nedatum")),
                note(8, "importWizard.row.symbolMissing", None),
                note(9, "importWizard.row.currencyMissing", None),
                note(10, "validation.currencyInvalid", Some("dolar")),
            ]
        );
        // The blank line is no row; the rest add up.
        assert_eq!(parsed.total_rows, 10);
        assert_eq!(
            parsed.trades.len() + parsed.skipped.len() + parsed.errors.len(),
            parsed.total_rows
        );
    }

    #[test]
    fn a_type_value_decides_what_a_row_is() {
        let c = config();
        // Listed as skip, not listed, and empty: not trades, with the value.
        assert_eq!(
            only_skip("15.01.2024;dividenda;AAPL;1;1;USD\n", &c),
            note(2, "importWizard.row.notATrade", Some("dividenda"))
        );
        assert_eq!(
            only_skip("15.01.2024;vklad;AAPL;1;1;USD\n", &c),
            note(2, "importWizard.row.notATrade", Some("vklad"))
        );
        assert_eq!(
            only_skip("15.01.2024;;AAPL;1;1;USD\n", &c),
            note(2, "importWizard.row.notATrade", None)
        );
    }

    #[test]
    fn type_values_match_exactly_first_then_by_folded_text() {
        let mut c = config();
        c.type_values = vec![
            mapping("Nákup", TypeValueAction::Buy),
            mapping("BUY", TypeValueAction::Sell),
            mapping("buy", TypeValueAction::Buy),
        ];
        // Folded: case and diacritics do not matter.
        assert_eq!(
            only_trade("15.01.2024;NAKUP;AAPL;1;1;USD\n", &c).direction,
            TradeDirection::Buy
        );
        // Exact beats folded: "BUY" is listed as a sale, whatever "buy" says.
        assert_eq!(
            only_trade("15.01.2024;BUY;AAPL;1;1;USD\n", &c).direction,
            TradeDirection::Sell
        );
        assert_eq!(
            only_trade("15.01.2024;buy;AAPL;1;1;USD\n", &c).direction,
            TradeDirection::Buy
        );
        // Cell spaces are trimmed.
        assert_eq!(
            only_trade("15.01.2024; Nákup ;AAPL;1;1;USD\n", &c).direction,
            TradeDirection::Buy
        );
    }

    // ===================================================================
    // Numbers
    // ===================================================================

    #[test]
    fn numbers_follow_the_decimal_separator_and_ignore_decorations() {
        let c = config();
        let t = only_trade("15.01.2024;nákup;AAPL;0,5;1 234,50 USD;USD\n", &c);
        assert_eq!((t.quantity, t.price), (0.5, 1234.5));
        let t = only_trade("15.01.2024;nákup;AAPL;1.000;12,5;USD\n", &c);
        assert_eq!(
            t.quantity, 1000.0,
            "a dot before three digits is a thousands mark here"
        );
        let mut point = config();
        point.decimal_separator = ".".into();
        let t = only_trade("15.01.2024;nákup;AAPL;1,000;1,234.50;USD\n", &point);
        assert_eq!((t.quantity, t.price), (1000.0, 1234.5));
        // Ten decimals, as Trading 212 writes fractional shares.
        let t = only_trade("15.01.2024;nákup;AAPL;0.0290530000;49.96;USD\n", &point);
        assert_eq!((t.quantity, t.price), (0.029053, 49.96));
    }

    #[test]
    fn currency_signs_and_codes_around_a_number_are_ignored() {
        let c = config();
        for (cell, want) in [
            ("€ 78,22", 78.22),
            ("78,22 €", 78.22),
            ("Kč 950,00", 950.0),
            ("950,00 Kč", 950.0),
            ("USD 185,50", 185.5),
            ("(12,50)", 12.5),
        ] {
            let t = only_trade(&format!("15.01.2024;nákup;AAPL;1;{cell};USD\n"), &c);
            assert_eq!(t.price, want, "{cell}");
        }
        let mut point = config();
        point.decimal_separator = ".".into();
        let t = only_trade("15.01.2024;nákup;AAPL;1;$1,234.50;USD\n", &point);
        assert_eq!(t.price, 1234.5);
        // Letters inside a number are not decoration.
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;1;12a5;USD\n", &c).key,
            "importWizard.row.numberUnparseable"
        );
    }

    #[test]
    fn czech_dates_written_with_spaces_after_the_dots() {
        let mut c = config();
        c.date_format = "%d. %m. %Y".into();
        assert_eq!(
            only_trade("15. 1. 2024;nákup;AAPL;1;1;USD\n", &c).day,
            1705276800
        );
        assert_eq!(
            only_trade("05. 02. 2024;nákup;AAPL;1;1;USD\n", &c).day,
            1707091200
        );
    }

    #[test]
    fn quantity_and_price_are_absolute_values() {
        let c = config();
        let t = only_trade("15.01.2024;prodej;AAPL;-5;-190,25;USD\n", &c);
        assert_eq!(
            (t.direction, t.quantity, t.price),
            (TradeDirection::Sell, 5.0, 190.25)
        );
    }

    #[test]
    fn unreadable_numbers_are_errors_with_the_cell() {
        let c = config();
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;abc;1;USD\n", &c),
            note(2, "importWizard.row.numberUnparseable", Some("abc"))
        );
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;1;12,5,5;USD\n", &c),
            note(2, "importWizard.row.numberUnparseable", Some("12,5,5"))
        );
        // An empty quantity is unreadable too; an empty price is missing.
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;;1;USD\n", &c),
            note(2, "importWizard.row.numberUnparseable", None)
        );
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;1;;USD\n", &c),
            note(2, "importWizard.row.priceMissing", None)
        );
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;1;0,00;USD\n", &c),
            note(2, "importWizard.row.priceMissing", None)
        );
        // A point where the file has decimal commas is not guessed.
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;1;12.5.1;USD\n", &c).key,
            "importWizard.row.numberUnparseable"
        );
    }

    #[test]
    fn a_zero_quantity_is_skipped_not_an_error() {
        let c = config();
        assert_eq!(
            only_skip("15.01.2024;nákup;AAPL;0;185,50;USD\n", &c),
            note(2, "importWizard.row.zeroQuantity", None)
        );
        // Before the price is looked at.
        assert_eq!(
            only_skip("15.01.2024;nákup;AAPL;0,0;;USD\n", &c).key,
            "importWizard.row.zeroQuantity"
        );
    }

    #[test]
    fn the_sign_of_the_quantity_decides_when_the_config_says_so() {
        let mut c = config();
        c.direction_mode = DirectionMode::QuantitySign;
        c.type_column = None;
        c.type_values = vec![];
        let sale = only_trade("15.01.2024;;AAPL;-2;79,10;EUR\n", &c);
        assert_eq!((sale.direction, sale.quantity), (TradeDirection::Sell, 2.0));
        let purchase = only_trade("15.01.2024;;AAPL;5;78,22;EUR\n", &c);
        assert_eq!(
            (purchase.direction, purchase.quantity),
            (TradeDirection::Buy, 5.0)
        );
        assert_eq!(
            only_skip("15.01.2024;;AAPL;0;78,22;EUR\n", &c).key,
            "importWizard.row.zeroQuantity"
        );
    }

    // ===================================================================
    // Dates
    // ===================================================================

    #[test]
    fn dates_are_read_with_the_format_of_the_file_and_nothing_else() {
        let c = config();
        // A time after the date is ignored.
        assert_eq!(
            only_trade("15.01.2024 13:45:00;nákup;AAPL;1;1;USD\n", &c).day,
            1705276800
        );
        assert_eq!(
            only_trade(" 15.01.2024 ;nákup;AAPL;1;1;USD\n", &c).day,
            1705276800
        );
        // Another layout, an impossible date, a US date: errors, never guesses.
        for cell in [
            "2024-01-15",
            "31.02.2024",
            "01/15/2024",
            "15.1.24",
            "January",
        ] {
            assert_eq!(
                only_error(&format!("{cell};nákup;AAPL;1;1;USD\n"), &c),
                note(2, "importWizard.row.dateUnparseable", Some(cell)),
                "{cell}"
            );
        }
        assert_eq!(
            only_error(";nákup;AAPL;1;1;USD\n", &c),
            note(2, "importWizard.row.dateUnparseable", None)
        );
    }

    #[test]
    fn the_day_is_midnight_utc() {
        let mut c = config();
        c.date_format = "%Y-%m-%d".into();
        let t = only_trade("2024-01-15T23:59:59Z;nákup;AAPL;1;1;USD\n", &c);
        assert_eq!(t.day, 1705276800);
        assert_eq!(t.day % 86400, 0);
    }

    #[test]
    fn separator_less_dates_with_an_optional_time() {
        let mut c = config();
        c.date_format = "%Y%m%d".into();
        assert_eq!(
            only_trade("20230522;nákup;AAPL;1;1;USD\n", &c).day,
            1684713600
        );
        // Interactive Brokers' DateTime: date;time (the cell is quoted).
        assert_eq!(
            only_trade("\"20230522;093000\";nákup;AAPL;1;1;USD\n", &c).day,
            1684713600
        );
        assert_eq!(
            only_error("2023052;nákup;AAPL;1;1;USD\n", &c).key,
            "importWizard.row.dateUnparseable"
        );
    }

    // ===================================================================
    // Instruments
    // ===================================================================

    #[test]
    fn symbols_and_isins_are_uppercase_and_an_isin_makes_the_key() {
        let mut c = config();
        c.isin_column = Some(6);
        c.name_column = Some(7);
        let t = only_trade(
            "15.01.2024;nákup;vwce.de;1;1;EUR;ie00bk5bqt80;  Vanguard FTSE All-World \n",
            &c,
        );
        assert_eq!(t.instrument_key, "isin:IE00BK5BQT80");
        assert_eq!(t.symbol.as_deref(), Some("VWCE.DE"));
        assert_eq!(t.isin.as_deref(), Some("IE00BK5BQT80"));
        assert_eq!(t.name.as_deref(), Some("Vanguard FTSE All-World"));
        // No ISIN: the symbol is the key; an empty name is none.
        let t = only_trade("15.01.2024;nákup;vwce.de;1;1;EUR;;\n", &c);
        assert_eq!(t.instrument_key, "symbol:VWCE.DE");
        assert_eq!((t.isin, t.name), (None, None));
    }

    #[test]
    fn an_invalid_isin_is_not_a_key() {
        let mut c = config();
        c.isin_column = Some(6);
        // With a symbol the symbol is used.
        let t = only_trade("15.01.2024;nákup;AAPL;1;1;USD;NOTANISIN\n", &c);
        assert_eq!((t.instrument_key.as_str(), t.isin), ("symbol:AAPL", None));
        // Without one nothing says what the instrument is.
        assert_eq!(
            only_error("15.01.2024;nákup;;1;1;USD;NOTANISIN\n", &c),
            note(2, "importWizard.row.symbolMissing", Some("NOTANISIN"))
        );
        // An ISIN is enough without a symbol column.
        c.symbol_column = None;
        let t = only_trade("15.01.2024;nákup;;1;1;USD;IE00B3XXRP09\n", &c);
        assert_eq!(
            (t.instrument_key.as_str(), t.symbol),
            ("isin:IE00B3XXRP09", None)
        );
    }

    #[test]
    fn instrument_overrides_are_left_to_the_simulation() {
        let mut c = config();
        c.instrument_overrides = vec![
            StockInstrumentOverride {
                key: "symbol:MSFT".into(),
                ticker: None,
                name: None,
                currency: None,
                skip: true,
            },
            StockInstrumentOverride {
                key: "symbol:AAPL".into(),
                ticker: Some("AAPL.US".into()),
                name: Some("Apple".into()),
                currency: Some("EUR".into()),
                skip: false,
            },
        ];
        let rows = "15.01.2024;nákup;msft;1;1;USD\n16.01.2024;nákup;AAPL;1;1;USD\n17.01.2024;prodej;MSFT;1;2;USD\n";
        let with_overrides = parse_with(rows, &c);
        // The rows of a skipped instrument stay trades: the preview lists the
        // instruments of the trades, and the user must be able to un-skip one.
        // Skip, ticker, name and currency are all applied later.
        assert_eq!(with_overrides, parse(rows));
        assert_eq!(with_overrides.trades.len(), 3);
        assert!(with_overrides.skipped.is_empty() && with_overrides.errors.is_empty());
        assert_eq!(with_overrides.trades[0].instrument_key, "symbol:MSFT");
        assert_eq!(with_overrides.trades[1].instrument_key, "symbol:AAPL");
        assert_eq!(with_overrides.trades[1].currency.as_deref(), Some("USD"));
        assert_eq!(with_overrides.total_rows, 3);
        // A row that cannot be read is an error, skipped instrument or not.
        let bad = parse_with("nedatum;nákup;MSFT;1;1;USD\n", &c);
        assert_eq!(
            bad.errors,
            [note(2, "importWizard.row.dateUnparseable", Some("nedatum"))]
        );
        assert!(bad.trades.is_empty() && bad.skipped.is_empty());
    }

    // ===================================================================
    // Currency
    // ===================================================================

    #[test]
    fn the_currency_comes_from_the_column_the_config_or_the_instrument() {
        let mut c = config();
        assert_eq!(
            only_trade("15.01.2024;nákup;AAPL;1;1;usd\n", &c)
                .currency
                .as_deref(),
            Some("USD")
        );
        c.currency_mode = CurrencyMode::Fixed;
        c.currency_column = None;
        c.fixed_currency = Some("czk".into());
        assert_eq!(
            only_trade("15.01.2024;nákup;AAPL;1;1;\n", &c)
                .currency
                .as_deref(),
            Some("CZK")
        );
        c.currency_mode = CurrencyMode::Instrument;
        assert_eq!(
            only_trade("15.01.2024;nákup;AAPL;1;1;USD\n", &c).currency,
            None
        );
    }

    #[test]
    fn a_currency_cell_that_is_not_a_code_is_an_error() {
        let c = config();
        for cell in ["dolar", "US", "U$D", "12E"] {
            assert_eq!(
                only_error(&format!("15.01.2024;nákup;AAPL;1;1;{cell}\n"), &c),
                note(2, "validation.currencyInvalid", Some(cell)),
                "{cell}"
            );
        }
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;1;1;\n", &c),
            note(2, "importWizard.row.currencyMissing", None)
        );
        let mut fixed = config();
        fixed.currency_mode = CurrencyMode::Fixed;
        fixed.fixed_currency = Some("dollar".into());
        assert_eq!(
            only_error("15.01.2024;nákup;AAPL;1;1;USD\n", &fixed).key,
            "validation.currencyInvalid"
        );
    }

    #[test]
    fn pence_are_pounds_in_every_source() {
        let c = config();
        for pence in ["GBX", "GBx", "gbx", "GBp"] {
            let t = only_trade(&format!("15.01.2024;nákup;VOD.L;20;4912,35;{pence}\n"), &c);
            assert_eq!(t.currency.as_deref(), Some("GBP"), "{pence}");
            // Exactly 49.1235, not 49.12350000000001.
            assert_eq!(t.price, 49.1235, "{pence}");
            assert_eq!(t.price.to_string(), "49.1235", "{pence}");
        }
        // Pounds stay pounds.
        for pounds in ["GBP", "gbp"] {
            let t = only_trade(&format!("15.01.2024;nákup;VOD.L;20;49,12;{pounds}\n"), &c);
            assert_eq!(
                (t.currency.as_deref(), t.price),
                (Some("GBP"), 49.12),
                "{pounds}"
            );
        }
        // Also with one currency for the whole file.
        let mut fixed = config();
        fixed.currency_mode = CurrencyMode::Fixed;
        fixed.currency_column = None;
        fixed.fixed_currency = Some("GBX".into());
        let t = only_trade("15.01.2024;nákup;VOD.L;20;68,5;\n", &fixed);
        assert_eq!((t.currency.as_deref(), t.price), (Some("GBP"), 0.685));
    }

    // ===================================================================
    // Broker ids and fee columns
    // ===================================================================

    #[test]
    fn the_broker_id_is_the_trimmed_cell() {
        let mut c = config();
        c.external_id_column = Some(6);
        assert_eq!(
            only_trade("15.01.2024;nákup;AAPL;1;1;USD; EOF7504196256 \n", &c)
                .external_id
                .as_deref(),
            Some("EOF7504196256")
        );
        assert_eq!(
            only_trade("15.01.2024;nákup;AAPL;1;1;USD;\n", &c).external_id,
            None
        );
    }

    #[test]
    fn a_fee_column_is_noticed_from_the_headers() {
        let c = config();
        let with = parse_file(
            "Datum;Typ;Symbol;Počet;Cena;Měna;Poplatek\n15.01.2024;nákup;AAPL;1;1;USD;2\n"
                .as_bytes(),
            &c,
        )
        .expect("parse");
        assert!(with.has_fee_column);
        assert!(!parse("15.01.2024;nákup;AAPL;1;1;USD\n").has_fee_column);
        // Degiro's header has its fees in the one the roles know.
        let mut degiro = base_config("degiro");
        degiro.delimiter = ",".into();
        let file = format!("{}\n", DEGIRO_HEADERS.join(","));
        let parsed = parse_file(file.as_bytes(), &degiro).expect("parse");
        assert!(parsed.has_fee_column);
    }

    // ===================================================================
    // Broker shapes
    // ===================================================================

    fn xtb_config() -> StockImportConfig {
        let mut config = xtb::config(&headers(&XTB_HEADERS), &[])
            .expect("config")
            .config;
        config.delimiter = ";".into();
        config.type_values = vec![
            mapping("Stocks/ETF purchase", TypeValueAction::Buy),
            mapping("Stocks/ETF sale", TypeValueAction::Sell),
            mapping("Deposit", TypeValueAction::Skip),
            mapping("Dividend", TypeValueAction::Skip),
        ];
        config
    }

    fn parse_xtb(rows: &str) -> ParsedFile {
        let file = format!("{}\n{rows}", XTB_HEADERS.join(";"));
        parse_file(file.as_bytes(), &xtb_config()).expect("parse")
    }

    #[test]
    fn xtb_trades_come_from_the_comment_with_yahoo_symbols_and_no_currency() {
        let parsed = parse_xtb(
            "610000001;Deposit;03.01.2024 09:15:02;;Bank transfer deposit;1500.00\n\
610000002;Stocks/ETF purchase;04.01.2024 15:31:18;AAPL.US;OPEN BUY 3/3 @ 185.5000;-556.50\n\
610000003;Stocks/ETF purchase;22.03.2024 09:30:00;SPYL.DE;OPEN BUY 34/42.5658 @ 11.7480;-399.43\n\
610000004;Dividend;15.02.2024 06:10:11;AAPL.US;AAPL.US USD 0.2400/ SHR;0.72\n\
610000005;Stocks/ETF sale;21.03.2024 16:45:09;VUSA.UK;CLOSE BUY 1 @ 78.2200;78.22\n",
        );
        assert_eq!(parsed.total_rows, 5);
        assert!(parsed.errors.is_empty());
        assert_eq!(
            parsed.skipped,
            [
                note(2, "importWizard.row.notATrade", Some("Deposit")),
                note(5, "importWizard.row.notATrade", Some("Dividend")),
            ]
        );
        let t = &parsed.trades;
        assert_eq!(
            (t[0].symbol.as_deref(), t[0].quantity, t[0].price),
            (Some("AAPL"), 3.0, 185.5)
        );
        assert_eq!(t[0].instrument_key, "symbol:AAPL");
        assert_eq!(t[0].direction, TradeDirection::Buy);
        assert_eq!(t[0].day, 1704326400, "04.01.2024");
        assert_eq!(t[0].currency, None);
        assert_eq!(t[0].external_id.as_deref(), Some("610000002"));
        assert_eq!(
            (t[1].symbol.as_deref(), t[1].quantity, t[1].price),
            (Some("SPYL.DE"), 34.0, 11.748)
        );
        assert_eq!(
            (t[2].symbol.as_deref(), t[2].direction),
            (Some("VUSA.L"), TradeDirection::Sell)
        );
    }

    #[test]
    fn the_xtb_comment_decides_the_direction() {
        // The type only says "a trade": the comment says which way.
        let parsed =
            parse_xtb("1;Stocks/ETF purchase;04.01.2024 15:31:18;AAPL.US;CLOSE BUY 1 @ 5.0;-5\n");
        assert_eq!(parsed.trades[0].direction, TradeDirection::Sell);
    }

    #[test]
    fn an_xtb_trade_without_a_readable_comment_is_an_error() {
        let parsed = parse_xtb(
            "1;Stocks/ETF purchase;04.01.2024 15:31:18;AAPL.US;something else;-5\n\
2;Stocks/ETF purchase;04.01.2024 15:31:18;AAPL.US;;-5\n",
        );
        assert_eq!(
            parsed.errors,
            [
                note(
                    2,
                    "importWizard.row.commentUnparseable",
                    Some("something else")
                ),
                note(3, "importWizard.row.commentUnparseable", None),
            ]
        );
        assert!(parsed.trades.is_empty());
    }

    fn ibkr_config() -> StockImportConfig {
        let mut config = base_config("ibkr");
        config.date_column = 2;
        config.date_format = "%Y%m%d".into();
        config.symbol_column = Some(0);
        config.isin_column = Some(1);
        config.quantity_column = 4;
        config.price_column = 5;
        config.currency_column = Some(6);
        config.type_column = Some(3);
        config.type_values = vec![
            mapping("BUY", TypeValueAction::Buy),
            mapping("SELL", TypeValueAction::Sell),
        ];
        config.external_id_column = Some(8);
        config.transforms.asset_class_column = Some(7);
        config.transforms.asset_class_allowed = vec!["STK".into()];
        config
    }

    #[test]
    fn ibkr_skips_options_and_currency_pairs_and_the_trailer() {
        let file = "\"Symbol\",\"ISIN\",\"TradeDate\",\"Buy/Sell\",\"Quantity\",\"TradePrice\",\"CurrencyPrimary\",\"AssetClass\",\"TradeID\"\n\
\"AAPL\",\"US0378331005\",\"20230522\",\"BUY\",\"3\",\"172.4\",\"USD\",\"STK\",\"1001\"\n\
\"AAPL\",\"US0378331005\",\"20230630\",\"SELL\",\"-1\",\"189.5\",\"USD\",\"stk\",\"1002\"\n\
\"AAPL  250117C00200000\",\"\",\"20230701\",\"BUY\",\"1\",\"5.1\",\"USD\",\"OPT\",\"1003\"\n\
\"EUR.USD\",\"\",\"20230702\",\"BUY\",\"1000\",\"1.09\",\"USD\",\"CASH\",\"1004\"\n\
\"EOF\",\"U1234567\",\"4\"\n";
        let parsed = parse_file(file.as_bytes(), &ibkr_config()).expect("parse");
        assert_eq!(parsed.total_rows, 5);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.trades.len(), 2);
        let (buy, sell) = (&parsed.trades[0], &parsed.trades[1]);
        assert_eq!(buy.instrument_key, "isin:US0378331005");
        assert_eq!(
            (buy.direction, buy.quantity, buy.price),
            (TradeDirection::Buy, 3.0, 172.4)
        );
        assert_eq!(buy.day, 1684713600);
        assert_eq!(buy.external_id.as_deref(), Some("1001"));
        assert_eq!((sell.direction, sell.quantity), (TradeDirection::Sell, 1.0));
        assert_eq!(
            parsed.skipped,
            [
                note(4, "importWizard.row.assetClass", Some("OPT")),
                note(5, "importWizard.row.assetClass", Some("CASH")),
                note(6, "importWizard.row.notATrade", None),
            ]
        );
    }

    fn degiro_config() -> StockImportConfig {
        let mut config = base_config("degiro");
        config.date_format = "%d-%m-%Y".into();
        config.name_column = Some(2);
        config.isin_column = Some(3);
        config.quantity_column = 6;
        config.price_column = 7;
        config.currency_column = Some(8);
        config.direction_mode = DirectionMode::QuantitySign;
        config.decimal_separator = ",".into();
        config.external_id_column = Some(18);
        config
    }

    #[test]
    fn a_degiro_file_reads_the_sign_the_isin_and_the_currency_after_the_price() {
        let file = format!(
            "{}\n\
15-01-2024,09:31,VANGUARD S&P 500 UCITS ETF,IE00B3XXRP09,EAM,XAMS,5,\"78,2200\",EUR,\"-391,10\",EUR,\"-391,10\",EUR,,\"-1,00\",EUR,\"-392,10\",EUR,6f1c2c9a\n\
20-02-2024,10:15,VANGUARD S&P 500 UCITS ETF,IE00B3XXRP09,EAM,XAMS,-2,\"79,1000\",EUR,\"158,20\",EUR,\"158,20\",EUR,,\"-1,00\",EUR,\"157,20\",EUR,a1b2c3d4\n\
21-02-2024,10:16,VODAFONE GROUP,GB00BH4HKS39,LSE,XLON,100,\"68,52\",GBX,\"6852,00\",GBX,\"79,50\",EUR,\"0,0116\",\"-1,00\",EUR,\"78,50\",EUR,0f0f0f0f\n",
            DEGIRO_HEADERS.join(",")
        );
        let parsed = parse_file(file.as_bytes(), &degiro_config()).expect("parse");
        assert_eq!(
            (parsed.total_rows, parsed.errors.len(), parsed.skipped.len()),
            (3, 0, 0)
        );
        let t = &parsed.trades;
        assert_eq!(t[0].instrument_key, "isin:IE00B3XXRP09");
        assert_eq!(t[0].symbol, None);
        assert_eq!(t[0].name.as_deref(), Some("VANGUARD S&P 500 UCITS ETF"));
        assert_eq!(
            (t[0].direction, t[0].quantity, t[0].price),
            (TradeDirection::Buy, 5.0, 78.22)
        );
        assert_eq!(t[0].currency.as_deref(), Some("EUR"));
        assert_eq!(t[0].external_id.as_deref(), Some("6f1c2c9a"));
        assert_eq!(
            (t[1].direction, t[1].quantity, t[1].price),
            (TradeDirection::Sell, 2.0, 79.1)
        );
        // London prices come in pence.
        assert_eq!(
            (t[2].currency.as_deref(), t[2].quantity, t[2].price),
            (Some("GBP"), 100.0, 0.6852)
        );
    }

    // ===================================================================
    // The file
    // ===================================================================

    #[test]
    fn a_preamble_and_skipped_rows_do_not_shift_the_lines() {
        let mut c = config();
        c.header_row = 2;
        c.skip_rows = 1;
        let file = "Výpis;obchodů\n\nDatum;Typ;Symbol;Počet;Cena;Měna\n01.01.2024;nákup;AAA;1;1;USD\n\n15.01.2024;nákup;AAPL;1;1;USD\n16.01.2024;prodej;AAPL;1;1;USD\n";
        let parsed = parse_file(file.as_bytes(), &c).expect("parse");
        assert_eq!(parsed.total_rows, 2, "the skipped row is not a row");
        assert_eq!(
            parsed.trades.iter().map(|t| t.line).collect::<Vec<_>>(),
            [6, 7]
        );
    }

    #[test]
    fn a_windows_1250_file_is_decoded_with_the_configs_encoding() {
        let text = "Datum;Typ;Symbol;Název;Počet;Cena;Měna\r\n15.1.2024;nákup;CEZ.PR;ČEZ;10;950,00;CZK\r\n";
        let (bytes, _, _) = encoding_rs::WINDOWS_1250.encode(text);
        let mut c = config();
        c.encoding = "windows-1250".into();
        c.name_column = Some(3);
        c.quantity_column = 4;
        c.price_column = 5;
        c.currency_column = Some(6);
        let parsed = parse_file(&bytes, &c).expect("parse");
        assert_eq!(parsed.errors, []);
        assert_eq!(parsed.trades.len(), 1);
        assert_eq!(parsed.trades[0].name.as_deref(), Some("ČEZ"));
        assert_eq!(parsed.trades[0].direction, TradeDirection::Buy);
        assert_eq!(
            parsed.trades[0].day, 1705276800,
            "15.1.2024 without padding"
        );
    }

    #[test]
    fn short_rows_are_missing_cells_not_a_crash() {
        let parsed = parse("15.01.2024;nákup\n16.01.2024;nákup;AAPL;1;1;USD;extra;cells\n");
        assert_eq!(
            parsed.errors,
            [note(2, "importWizard.row.symbolMissing", None)]
        );
        assert_eq!(parsed.trades.len(), 1);
        assert_eq!(parsed.total_rows, 2);
    }

    #[test]
    fn blank_rows_are_not_rows() {
        let parsed = parse("\n ; ; ; ; ;\n15.01.2024;nákup;AAPL;1;1;USD\n\n");
        assert_eq!(parsed.total_rows, 1);
    }

    #[test]
    fn a_file_without_rows_is_empty_not_an_error() {
        let parsed = parse("");
        assert_eq!((parsed.total_rows, parsed.trades.len()), (0, 0));
    }

    #[test]
    fn an_empty_file_or_a_bad_setting_is_a_validation_error() {
        let key = |r: Result<ParsedFile>| match r {
            Err(AppError::Validation(key)) => key,
            other => format!("{other:?}"),
        };
        assert_eq!(key(parse_file(b"", &config())), "validation.csvEmptyFile");
        assert_eq!(
            key(parse_file(b"\n\n", &config())),
            "validation.csvEmptyFile"
        );
        let mut bad = config();
        bad.delimiter = "|".into();
        assert_eq!(
            key(parse_file(HEADER.as_bytes(), &bad)),
            "validation.csvDelimiterInvalid"
        );
        let mut bad = config();
        bad.decimal_separator = "x".into();
        assert_eq!(
            key(parse_file(HEADER.as_bytes(), &bad)),
            "validation.csvDecimalSeparatorInvalid"
        );
    }
}
