//! Inspection of a file: shape, source, suggested configuration.
//!
//! The file is decoded, its delimiter and header row found (by the stock
//! column roles, so a preamble is skipped) and its rows walked once: they are
//! counted, the first are kept for detection and display, and every column's
//! distinct values are tallied (the type column's become the type-value table).
//! The source is a saved format whose header signature matches, else the first
//! built-in adapter that claims the file, else none; the configuration is that
//! source's, or a complete suggestion from the headers when every required
//! column is there.

use std::collections::{HashMap, HashSet};

use crate::error::{AppError, Result};
use crate::services::csv_import::decode::{decode_csv_content, detect_csv_delimiter, read_headers};

use super::adapters::{
    self, base_config, decimal_of, detect_column_date, AdapterConfig, StockAdapter,
};
use super::columns::{
    column_for, fold_value, suggest_stock_columns, suggest_type_action, StockRole,
};
use super::detect::{data_records, delimiter_char, detect_stock_header_row};
use super::types::{
    header_signature, CurrencyMode, DirectionMode, SavedStockImportFormat, StockColumnSuggestion,
    StockColumnValues, StockCsvInspectOptions, StockCsvInspection, StockImportConfig,
    StockTypeValueMapping, StockTypeValueStat, SAVED_FORMAT_PREFIX, SOURCE_CUSTOM,
};

/// Rows kept for detection: the source, the date format, the decimal separator.
const SAMPLE_ROWS: usize = 2000;
/// Rows shown in the inspection.
const DISPLAY_ROWS: usize = 8;
/// A column with more distinct values is not listed in `columnValues`: a
/// column of types has a handful.
const COLUMN_VALUES_LIMIT: usize = 30;
/// Distinct values tallied per column. More means free text, ids or numbers;
/// the configuration of a source still lists every value of its type column up
/// to this many.
const TRACKED_VALUES: usize = 200;

/// How the file is laid out: what an adapter cannot know and the config needs.
struct Layout {
    delimiter: char,
    encoding: &'static str,
    header_row: usize,
    skip_rows: usize,
}

fn apply_layout(config: &mut StockImportConfig, layout: &Layout) {
    config.delimiter = layout.delimiter.to_string();
    config.encoding = layout.encoding.to_string();
    config.header_row = layout.header_row;
    config.skip_rows = layout.skip_rows;
}

/// The distinct non-empty values of one column in order of first appearance:
/// `(value, count, first line)`.
#[derive(Default)]
struct Tally {
    values: Vec<(String, usize, usize)>,
    index: HashMap<String, usize>,
    /// More than `TRACKED_VALUES` distinct values: not tracked any further.
    overflowed: bool,
}

impl Tally {
    fn add(&mut self, cell: &str, line: usize) {
        if self.overflowed {
            return;
        }
        let value = cell.trim();
        if value.is_empty() {
            return;
        }
        if let Some(&position) = self.index.get(value) {
            self.values[position].1 += 1;
        } else if self.values.len() >= TRACKED_VALUES {
            self.overflowed = true;
        } else {
            self.index.insert(value.to_string(), self.values.len());
            self.values.push((value.to_string(), 1, line));
        }
    }
}

/// What one pass over the rows yields.
struct Walk {
    /// Non-blank data rows.
    row_count: usize,
    /// The first `SAMPLE_ROWS` rows, each as wide as the headers.
    rows: Vec<Vec<String>>,
    /// Per column.
    tallies: Vec<Tally>,
}

fn walk_rows(
    content: &str,
    delimiter: char,
    header_row: usize,
    skip_rows: usize,
    width: usize,
) -> Walk {
    let mut walk = Walk {
        row_count: 0,
        rows: Vec::new(),
        tallies: (0..width).map(|_| Tally::default()).collect(),
    };
    for (line, result) in data_records(content, delimiter, header_row, skip_rows) {
        let Ok(record) = result else { continue };
        if record.iter().all(|cell| cell.trim().is_empty()) {
            continue;
        }
        walk.row_count += 1;
        for (column, tally) in walk.tallies.iter_mut().enumerate() {
            if let Some(cell) = record.get(column) {
                tally.add(cell, line);
            }
        }
        if walk.rows.len() < SAMPLE_ROWS {
            let mut row: Vec<String> = record.iter().take(width).map(str::to_string).collect();
            row.resize(width, String::new());
            walk.rows.push(row);
        }
    }
    walk
}

/// A saved format whose headers are the file's.
struct SavedMatch<'a> {
    format: &'a SavedStockImportFormat,
    /// The header row the signature matched at.
    header_row: usize,
}

/// The saved formats whose `header_signature` is the file's headers', newest
/// (first in the list) first; `wanted` (an id the caller asked for) goes
/// before the others. A format saved with another header row than the one
/// detected now is looked for there, unless the caller fixed the row.
fn find_saved<'a>(
    saved: &'a [SavedStockImportFormat],
    content: &str,
    delimiter: char,
    headers: &[String],
    header_row: usize,
    header_row_forced: bool,
    wanted: Option<&str>,
) -> Option<SavedMatch<'a>> {
    let signature = header_signature(headers);
    let mut matches: Vec<SavedMatch<'a>> = saved
        .iter()
        .filter_map(|format| {
            if format.header_signature == signature {
                return Some(SavedMatch { format, header_row });
            }
            let saved_row = format.config.header_row;
            if header_row_forced || saved_row == header_row {
                return None;
            }
            let at_saved_row = read_headers(content, delimiter, saved_row);
            (header_signature(&at_saved_row) == format.header_signature).then_some(SavedMatch {
                format,
                header_row: saved_row,
            })
        })
        .collect();
    if let Some(id) = wanted {
        if let Some(position) = matches.iter().position(|m| m.format.id == id) {
            return Some(matches.swap_remove(position));
        }
    }
    matches.into_iter().next()
}

/// Where the configuration comes from.
enum Basis<'a> {
    Saved(&'a SavedStockImportFormat),
    Adapter(Box<AdapterConfig>),
    Generic,
}

/// The source the caller asked for, if the file fits it; else the saved
/// format, else the adapter the file was detected as; else the generic
/// suggestion. A forced source that does not fit is ignored, not an error:
/// `detectedSource` tells the user what the file is.
fn choose_basis<'a>(
    forced: Option<&str>,
    saved: Option<&SavedMatch<'a>>,
    detected: Option<&'static StockAdapter>,
    headers: &[String],
    rows: &[Vec<String>],
) -> Basis<'a> {
    match forced {
        Some(SOURCE_CUSTOM) => return Basis::Generic,
        Some(id) if id.starts_with(SAVED_FORMAT_PREFIX) => {
            if let Some(m) = saved.filter(|m| m.format.id == id) {
                return Basis::Saved(m.format);
            }
        }
        Some(id) => {
            if let Some(config) = adapters::by_source(id).and_then(|a| (a.config)(headers, rows)) {
                return Basis::Adapter(Box::new(config));
            }
        }
        None => {}
    }
    if let Some(m) = saved {
        return Basis::Saved(m.format);
    }
    if let Some(config) = detected.and_then(|a| (a.config)(headers, rows)) {
        return Basis::Adapter(Box::new(config));
    }
    Basis::Generic
}

/// Every value of the type column in the file with a meaning: the config's own
/// choices first (matched by folded text), the keyword suggestion for the rest.
/// A value a config does not list is skipped when the file is read, so a
/// saved format must not lose the values its file did not have.
fn complete_type_values(config: &StockImportConfig, walk: &Walk) -> Vec<StockTypeValueMapping> {
    let mut values = config.type_values.clone();
    let (DirectionMode::TypeColumn, Some(column)) = (config.direction_mode, config.type_column)
    else {
        return values;
    };
    let Some(tally) = walk.tallies.get(column) else {
        return values;
    };
    let mut listed: HashSet<String> = values.iter().map(|v| fold_value(&v.value)).collect();
    for (value, _, _) in &tally.values {
        if listed.insert(fold_value(value)) {
            values.push(StockTypeValueMapping {
                value: value.clone(),
                action: suggest_type_action(value),
            });
        }
    }
    values
}

fn from_saved(format: &SavedStockImportFormat, layout: &Layout, walk: &Walk) -> StockImportConfig {
    let mut config = format.config.clone();
    apply_layout(&mut config, layout);
    // What the user decided about another file does not travel with the mapping.
    config.instrument_overrides.clear();
    config.import_anyway_lines.clear();
    config.type_values = complete_type_values(&config, walk);
    config
}

fn from_adapter(mut config: StockImportConfig, layout: &Layout, walk: &Walk) -> StockImportConfig {
    apply_layout(&mut config, layout);
    config.type_values = complete_type_values(&config, walk);
    config
}

/// The date format of the file's date column (as the generic suggestion
/// found it) and whether day and month could be told apart.
struct DateGuess {
    format: Option<String>,
    ambiguous: bool,
}

fn guess_date(suggestions: &[StockColumnSuggestion], rows: &[Vec<String>]) -> DateGuess {
    match column_for(suggestions, StockRole::Date).and_then(|c| detect_column_date(rows, c)) {
        Some((format, ambiguous)) => DateGuess {
            format: Some(format),
            ambiguous,
        },
        None => DateGuess {
            format: None,
            ambiguous: false,
        },
    }
}

/// 1, 2, 3, …: a row number, not an id of the broker.
fn is_running_number(values: &[&str]) -> bool {
    values.len() >= 3
        && values.windows(2).all(
            |pair| match (pair[0].parse::<u64>(), pair[1].parse::<u64>()) {
                (Ok(a), Ok(b)) => a.checked_add(1) == Some(b),
                _ => false,
            },
        )
}

/// A guessed id column is trusted when it has values, none repeats and it is
/// not a row counter: a counter would make every second file look like a
/// duplicate of the first, and a repeated id is no key.
fn is_broker_id(rows: &[Vec<String>], column: usize) -> bool {
    let values: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get(column))
        .map(|cell| cell.trim())
        .filter(|cell| !cell.is_empty())
        .collect();
    let mut seen = HashSet::new();
    !values.is_empty() && values.iter().all(|v| seen.insert(*v)) && !is_running_number(&values)
}

/// A complete configuration from the generic suggestions, or `None` while a
/// required piece is unknown: the date (and its format), a symbol or an ISIN,
/// the quantity, the price and a currency column. Without a type column the
/// sign of the quantity decides the direction.
fn generic_config(
    suggestions: &[StockColumnSuggestion],
    walk: &Walk,
    layout: &Layout,
    date: &DateGuess,
) -> Option<StockImportConfig> {
    let column = |role| column_for(suggestions, role);
    let date_column = column(StockRole::Date)?;
    let quantity_column = column(StockRole::Quantity)?;
    let price_column = column(StockRole::Price)?;
    let currency_column = column(StockRole::Currency)?;
    let (symbol_column, isin_column) = (column(StockRole::Symbol), column(StockRole::Isin));
    if symbol_column.is_none() && isin_column.is_none() {
        return None;
    }
    let date_format = date.format.clone()?;

    let mut config = base_config(SOURCE_CUSTOM);
    apply_layout(&mut config, layout);
    config.date_column = date_column;
    config.date_format = date_format;
    config.symbol_column = symbol_column;
    config.isin_column = isin_column;
    config.name_column = column(StockRole::Name);
    config.quantity_column = quantity_column;
    config.price_column = price_column;
    config.currency_mode = CurrencyMode::Column;
    config.currency_column = Some(currency_column);
    match column(StockRole::Type) {
        Some(type_column) => {
            config.direction_mode = DirectionMode::TypeColumn;
            config.type_column = Some(type_column);
        }
        None => config.direction_mode = DirectionMode::QuantitySign,
    }
    config.external_id_column =
        column(StockRole::ExternalId).filter(|&c| is_broker_id(&walk.rows, c));
    config.decimal_separator = decimal_of(&walk.rows, &[quantity_column, price_column]);
    config.type_values = complete_type_values(&config, walk);
    Some(config)
}

/// The roles a configuration maps, with their columns.
fn config_roles(config: &StockImportConfig) -> Vec<(StockRole, usize)> {
    let mut roles = vec![(StockRole::Date, config.date_column)];
    if config.direction_mode == DirectionMode::TypeColumn {
        roles.extend(config.type_column.map(|c| (StockRole::Type, c)));
    }
    roles.extend(config.symbol_column.map(|c| (StockRole::Symbol, c)));
    roles.extend(config.isin_column.map(|c| (StockRole::Isin, c)));
    roles.extend(config.name_column.map(|c| (StockRole::Name, c)));
    roles.push((StockRole::Quantity, config.quantity_column));
    roles.push((StockRole::Price, config.price_column));
    if config.currency_mode == CurrencyMode::Column {
        roles.extend(config.currency_column.map(|c| (StockRole::Currency, c)));
    }
    roles.extend(
        config
            .external_id_column
            .map(|c| (StockRole::ExternalId, c)),
    );
    roles
}

/// The suggestions of a source: its own columns at confidence 1.0, the
/// generic guess for the roles it does not define (the fee) unless the column
/// is one it claimed. In role order.
fn source_suggestions(
    config: &StockImportConfig,
    generic: &[StockColumnSuggestion],
) -> Vec<StockColumnSuggestion> {
    let own = config_roles(config);
    let claimed: Vec<usize> = own.iter().map(|(_, column)| *column).collect();
    StockRole::ALL
        .iter()
        .filter_map(|role| {
            if let Some((_, column)) = own.iter().find(|(r, _)| r == role) {
                return Some(StockColumnSuggestion {
                    role: role.key().to_string(),
                    column: *column,
                    confidence: 1.0,
                });
            }
            generic
                .iter()
                .find(|s| s.role == role.key() && !claimed.contains(&s.column))
                .cloned()
        })
        .collect()
}

/// The columns with at most `COLUMN_VALUES_LIMIT` distinct values.
fn column_values(tallies: &[Tally]) -> Vec<StockColumnValues> {
    tallies
        .iter()
        .enumerate()
        .filter(|(_, tally)| {
            !tally.overflowed && (1..=COLUMN_VALUES_LIMIT).contains(&tally.values.len())
        })
        .map(|(column, tally)| {
            let mut values: Vec<StockTypeValueStat> = tally
                .values
                .iter()
                .map(|(value, count, first_line)| StockTypeValueStat {
                    value: value.clone(),
                    count: *count,
                    first_line: *first_line,
                    suggested: suggest_type_action(value),
                })
                .collect();
            // The most frequent first; ties by where they first appear.
            values.sort_by(|a, b| b.count.cmp(&a.count).then(a.first_line.cmp(&b.first_line)));
            StockColumnValues { column, values }
        })
        .collect()
}

/// Inspect the bytes of a file. `saved` are the user's saved formats, matched
/// by `header_signature`; a forced `options.source` wins over detection when
/// the file fits it (`custom` always fits).
///
/// The only errors are an empty file (`validation.csvEmptyFile`) and a
/// delimiter that is not `,`, `;` or a tab; anything else yields a best-effort
/// result the user can correct. `detected_source` is what the file is,
/// whatever was asked for.
pub fn inspect(
    bytes: &[u8],
    file_name: &str,
    options: &StockCsvInspectOptions,
    saved: &[SavedStockImportFormat],
) -> Result<StockCsvInspection> {
    let decoded = decode_csv_content(bytes, options.encoding.as_deref());
    let content = decoded.text.as_str();
    let delimiter = match options.delimiter.as_deref() {
        Some(delimiter) => delimiter_char(delimiter)?,
        None => detect_csv_delimiter(content),
    };
    let mut header_row = options
        .header_row
        .unwrap_or_else(|| detect_stock_header_row(content, delimiter));
    let mut headers = read_headers(content, delimiter, header_row);
    if headers.iter().all(|h| h.is_empty()) {
        return Err(AppError::Validation("validation.csvEmptyFile".into()));
    }

    let wanted = options
        .source
        .as_deref()
        .filter(|source| source.starts_with(SAVED_FORMAT_PREFIX));
    let saved_match = find_saved(
        saved,
        content,
        delimiter,
        &headers,
        header_row,
        options.header_row.is_some(),
        wanted,
    );
    if let Some(m) = &saved_match {
        if m.header_row != header_row {
            header_row = m.header_row;
            headers = read_headers(content, delimiter, header_row);
        }
    }
    let skip_rows = options
        .skip_rows
        .or_else(|| saved_match.as_ref().map(|m| m.format.config.skip_rows))
        .unwrap_or(0);

    let walk = walk_rows(content, delimiter, header_row, skip_rows, headers.len());
    let layout = Layout {
        delimiter,
        encoding: decoded.encoding,
        header_row,
        skip_rows,
    };

    let detected_adapter = adapters::detect(&headers, &walk.rows);
    let detected_source = saved_match
        .as_ref()
        .map(|m| m.format.id.clone())
        .or_else(|| detected_adapter.map(|a| a.source.to_string()));

    let generic = suggest_stock_columns(&headers);
    let date_guess = guess_date(&generic, &walk.rows);
    let basis = choose_basis(
        options.source.as_deref(),
        saved_match.as_ref(),
        detected_adapter,
        &headers,
        &walk.rows,
    );

    // The configuration, whether the date format is a guess, and the
    // suggestions that go with it.
    let (config, date_ambiguous, suggestions) = match basis {
        Basis::Saved(format) => {
            let config = from_saved(format, &layout, &walk);
            let suggestions = source_suggestions(&config, &generic);
            (Some(config), false, suggestions)
        }
        Basis::Adapter(adapter) => {
            let config = from_adapter(adapter.config, &layout, &walk);
            let suggestions = source_suggestions(&config, &generic);
            (Some(config), adapter.date_ambiguous, suggestions)
        }
        Basis::Generic => {
            let config = generic_config(&generic, &walk, &layout, &date_guess);
            (config, date_guess.ambiguous, generic.clone())
        }
    };

    let date_format = match &config {
        Some(config) => Some(config.date_format.clone()),
        None => date_guess.format,
    };
    let date_known = date_format.is_some();
    let decimal_separator = match &config {
        Some(config) => config.decimal_separator.clone(),
        None => {
            let columns: Vec<usize> = [StockRole::Quantity, StockRole::Price]
                .iter()
                .filter_map(|role| column_for(&generic, *role))
                .collect();
            decimal_of(&walk.rows, &columns)
        }
    };

    Ok(StockCsvInspection {
        file_name: file_name.to_string(),
        encoding: decoded.encoding.to_string(),
        delimiter: delimiter.to_string(),
        header_row,
        sample_rows: walk.rows.iter().take(DISPLAY_ROWS).cloned().collect(),
        row_count: walk.row_count,
        detected_source,
        config,
        suggestions,
        column_values: column_values(&walk.tallies),
        date_format,
        date_format_ambiguous: date_ambiguous && date_known,
        decimal_separator,
        has_fee_column: column_for(&generic, StockRole::Fee).is_some(),
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::test_support::*;
    use crate::services::stock_import::adapters::{degiro, trading212};
    use crate::services::stock_import::types::{
        header_signature, CurrencyMode, DirectionMode, StockImportConfig, StockImportTransforms,
        StockInstrumentOverride, TypeValueAction,
    };

    const TRADING212_FILE: &str = "Action,Time,ISIN,Ticker,Name,No. of shares,Price / share,Currency (Price / share),Exchange rate,Result,Currency (Result),Total,Currency (Total),Withholding tax,Currency (Withholding tax),Notes,ID,Currency conversion fee,Currency (Currency conversion fee)\n\
Deposit,2023-12-18 11:45:06.326,,,,,,,,,,31.00,\"EUR\",,,\"Transaction ID: 1\",8d3b8c00-0000-4000-8000-000000000001,,\n\
Market buy,2023-12-18 14:30:03.613,US17275R1023,CSCO,\"Cisco Systems\",0.0290530000,49.96,USD,1.09303,,\"EUR\",1.33,\"EUR\",,,,EOF7504196256,,\n\
Market sell,2023-12-26 14:30:05.104,US04634X2027,ASTR,\"Astra Space\",0.6125400000,1.26,USD,1.10231,-3.08,\"EUR\",0.70,\"EUR\",,,,EOF7802023054,,\n\
Dividend (Ordinary),2023-12-27 12:05:25,US56035L1044,MAIN,\"Main Street Capital\",0.1543340000,0.23,USD,Not available,,,0.03,\"EUR\",0.01,USD,,,,\n";

    const XTB_FILE: &str = "ID;Type;Time;Symbol;Comment;Amount\n\
530691593;Deposit;12.04.2024 13:00:22;;Bank transfer deposit;500\n\
530692719;Stocks/ETF purchase;12.04.2024 13:01:45;SPYL.DE;OPEN BUY 34/42.5658 @ 11.7480;-399.43\n\
530690001;Stocks/ETF sale;15.04.2024 09:30:10;AAPL.US;CLOSE BUY 1 @ 26.280;26.28\n\
530690002;Dividend;16.04.2024 06:10:11;AAPL.US;AAPL.US USD 0.2400/ SHR;0.72\n";

    const DEGIRO_FILE: &str = "Date,Time,Product,ISIN,Reference exchange,Venue,Quantity,Price,,Local value,,Value,,Exchange rate,Transaction and/or third party fees,,Total,,Order ID\n\
15-01-2024,09:31,VANGUARD S&P 500 UCITS ETF,IE00B3XXRP09,EAM,XAMS,5,\"78,2200\",EUR,\"-391,10\",EUR,\"-391,10\",EUR,,\"-1,00\",EUR,\"-392,10\",EUR,6f1c2c9a-4a8e-4d5b-9c1e-2b7a1d9e0f11\n\
20-02-2024,10:15,VANGUARD S&P 500 UCITS ETF,IE00B3XXRP09,EAM,XAMS,-2,\"79,1000\",EUR,\"158,20\",EUR,\"158,20\",EUR,,\"-1,00\",EUR,\"157,20\",EUR,a1b2c3d4-0000-4000-8000-000000000002\n";

    const IBKR_FILE: &str = "\"BOF\",\"U1234567\",\"Activity Flex Query\",\"20230101\",\"20231231\"\n\
\"Symbol\",\"ISIN\",\"TradeDate\",\"Buy/Sell\",\"Quantity\",\"TradePrice\",\"CurrencyPrimary\",\"AssetClass\",\"TradeID\"\n\
\"AAPL\",\"US0378331005\",\"20230522\",\"BUY\",\"3\",\"172.4\",\"USD\",\"STK\",\"1001\"\n\
\"AAPL\",\"US0378331005\",\"20230630\",\"SELL\",\"-1\",\"189.5\",\"USD\",\"STK\",\"1002\"\n\
\"AAPL  250117C00200000\",\"\",\"20230701\",\"BUY\",\"1\",\"5.1\",\"USD\",\"OPT\",\"1003\"\n\
\"EOF\",\"U1234567\",\"3\"\n";

    const CZECH_TABLE: &str = "Datum;Typ;Symbol;Název;Počet;Cena;Měna\r\n\
15.1.2024;nákup;AAPL;Apple Inc.;10;185,5;USD\r\n\
05.02.2024;prodej;CEZ.PR;ČEZ;20;950,00;CZK\r\n";

    const OTHER_BROKER: &str = "Trade Date;Action;Symbol;Shares;Unit Price;CCY\n\
15.01.2025;BUY;aapl;10;180,50;USD\n\
20.02.2025;Sell;AAPL;15;185,25;USD\n\
05.04.2025;Dividend;MSFT;0;1,20;USD\n";

    fn options() -> StockCsvInspectOptions {
        StockCsvInspectOptions::default()
    }

    fn run(content: &str) -> StockCsvInspection {
        inspect(content.as_bytes(), "file.csv", &options(), &[]).expect("inspect")
    }

    fn config_of(i: &StockCsvInspection) -> &StockImportConfig {
        i.config.as_ref().expect("a ready config")
    }

    fn action(config: &StockImportConfig, value: &str) -> Option<TypeValueAction> {
        config
            .type_values
            .iter()
            .find(|t| t.value == value)
            .map(|t| t.action)
    }

    fn suggestion(i: &StockCsvInspection, role: &str) -> Option<(usize, f64)> {
        i.suggestions
            .iter()
            .find(|s| s.role == role)
            .map(|s| (s.column, s.confidence))
    }

    fn saved_format(
        id: &str,
        signature_of: &[&str],
        config: StockImportConfig,
    ) -> SavedStockImportFormat {
        SavedStockImportFormat {
            id: id.to_string(),
            name: "My broker".to_string(),
            header_signature: header_signature(&headers(signature_of)),
            config,
            created_at: 1,
        }
    }

    // ===================================================================
    // Sources
    // ===================================================================

    #[test]
    fn a_trading212_file_is_recognised_and_configured() {
        let i = run(TRADING212_FILE);
        assert_eq!(i.file_name, "file.csv");
        assert_eq!(
            (i.encoding.as_str(), i.delimiter.as_str(), i.header_row),
            ("UTF-8", ",", 0)
        );
        assert_eq!(i.headers.len(), 19);
        assert_eq!(i.headers[7], "Currency (Price / share)");
        assert_eq!(i.row_count, 4);
        assert!(i.sample_rows.iter().all(|r| r.len() == 19));
        assert_eq!(i.detected_source.as_deref(), Some("trading212"));

        let c = config_of(&i);
        assert_eq!(c.source, "trading212");
        assert_eq!(
            (
                c.delimiter.as_str(),
                c.encoding.as_str(),
                c.header_row,
                c.skip_rows
            ),
            (",", "UTF-8", 0, 0)
        );
        assert_eq!(c.currency_column, Some(7));
        assert_eq!(action(c, "Market buy"), Some(TypeValueAction::Buy));
        assert_eq!(action(c, "Market sell"), Some(TypeValueAction::Sell));
        assert_eq!(action(c, "Deposit"), Some(TypeValueAction::Skip));
        assert_eq!(
            action(c, "Dividend (Ordinary)"),
            Some(TypeValueAction::Skip)
        );
        assert_eq!(c.type_values.len(), 4);
        assert!(c.validate().is_ok());

        assert_eq!(i.date_format.as_deref(), Some("%Y-%m-%d"));
        assert!(!i.date_format_ambiguous);
        assert_eq!(i.decimal_separator, ".");
        assert!(i.has_fee_column, "the currency conversion fee column");
    }

    #[test]
    fn the_suggestions_of_a_source_are_certain_and_the_rest_is_generic() {
        let i = run(TRADING212_FILE);
        for (role, column) in [
            ("type", 0),
            ("date", 1),
            ("isin", 2),
            ("symbol", 3),
            ("name", 4),
            ("quantity", 5),
            ("price", 6),
            ("currency", 7),
            ("externalId", 16),
        ] {
            assert_eq!(suggestion(&i, role), Some((column, 1.0)), "{role}");
        }
        // The fee is no part of the preset: the generic guess fills in.
        assert_eq!(suggestion(&i, "fee"), Some((17, 0.85)));
    }

    #[test]
    fn an_xtb_file_reads_trades_from_comments_and_skips_the_rest() {
        let i = run(XTB_FILE);
        assert_eq!((i.delimiter.as_str(), i.row_count), (";", 4));
        assert_eq!(i.detected_source.as_deref(), Some("xtb"));
        let c = config_of(&i);
        assert_eq!(c.source, "xtb");
        assert!(c.transforms.xtb_comment && c.transforms.xtb_symbols);
        assert_eq!(c.currency_mode, CurrencyMode::Instrument);
        assert_eq!(action(c, "Stocks/ETF purchase"), Some(TypeValueAction::Buy));
        assert_eq!(action(c, "Stocks/ETF sale"), Some(TypeValueAction::Sell));
        assert_eq!(action(c, "Deposit"), Some(TypeValueAction::Skip));
        assert_eq!(action(c, "Dividend"), Some(TypeValueAction::Skip));
        assert_eq!(i.date_format.as_deref(), Some("%d.%m.%Y"));
        assert!(!i.date_format_ambiguous, "XTB's format is known");
        assert_eq!(i.decimal_separator, ".");
        assert!(!i.has_fee_column);
        assert!(c.validate().is_ok());
        // The comment is both the quantity and the price.
        assert_eq!(suggestion(&i, "quantity"), Some((4, 1.0)));
        assert_eq!(suggestion(&i, "price"), Some((4, 1.0)));
    }

    #[test]
    fn a_degiro_file_with_decimal_commas() {
        let i = run(DEGIRO_FILE);
        assert_eq!(i.delimiter, ",", "the quoted commas do not count");
        assert_eq!(i.detected_source.as_deref(), Some("degiro"));
        let c = config_of(&i);
        assert_eq!(c.decimal_separator, ",");
        assert_eq!(i.decimal_separator, ",");
        assert_eq!(c.direction_mode, DirectionMode::QuantitySign);
        assert_eq!((c.type_column, c.type_values.len()), (None, 0));
        assert_eq!(c.currency_column, Some(8));
        assert_eq!(c.date_format, "%d-%m-%Y");
        assert!(i.has_fee_column);
        // Blank headers stay blank.
        assert_eq!(i.headers[8], "");
        assert!(c.validate().is_ok());
    }

    #[test]
    fn an_ibkr_flex_query_with_a_header_record() {
        let i = run(IBKR_FILE);
        assert_eq!(i.header_row, 1, "the BOF line is no header");
        assert_eq!(i.detected_source.as_deref(), Some("ibkr"));
        assert_eq!(i.row_count, 4, "the EOF record is a row of the file");
        let c = config_of(&i);
        assert_eq!((c.header_row, c.date_format.as_str()), (1, "%Y%m%d"));
        assert_eq!(c.transforms.asset_class_column, Some(7));
        assert_eq!(action(c, "BUY"), Some(TypeValueAction::Buy));
        assert_eq!(action(c, "SELL"), Some(TypeValueAction::Sell));
        assert!(c.validate().is_ok());
        assert_eq!(i.date_format.as_deref(), Some("%Y%m%d"));
    }

    #[test]
    fn the_czech_table_saved_by_excel_in_windows_1250() {
        let (bytes, _, _) = encoding_rs::WINDOWS_1250.encode(CZECH_TABLE);
        let i = inspect(&bytes, "tabulka.csv", &options(), &[]).expect("inspect");
        assert_eq!(i.encoding, "windows-1250");
        assert_eq!(i.delimiter, ";");
        assert_eq!(
            i.headers,
            ["Datum", "Typ", "Symbol", "Název", "Počet", "Cena", "Měna"]
        );
        assert_eq!(i.detected_source.as_deref(), Some("moony"));
        let c = config_of(&i);
        assert_eq!(c.encoding, "windows-1250");
        assert_eq!(c.decimal_separator, ",");
        assert_eq!(c.date_format, "%d.%m.%Y");
        assert!(!i.date_format_ambiguous, "15.1. cannot be month-first");
        assert_eq!(action(c, "nákup"), Some(TypeValueAction::Buy));
        assert_eq!(action(c, "prodej"), Some(TypeValueAction::Sell));
        assert!(c.validate().is_ok());
    }

    #[test]
    fn moonys_own_export() {
        let i = run("Date,Type,Ticker,Name,Quantity,Price,Currency\n2024-01-15,buy,AAPL,Apple Inc.,10,185.5,USD\n2024-02-20,sell,AAPL,Apple Inc.,5,190.25,USD\n");
        assert_eq!(i.detected_source.as_deref(), Some("moony"));
        let c = config_of(&i);
        assert_eq!(
            (c.date_format.as_str(), c.decimal_separator.as_str()),
            ("%Y-%m-%d", ".")
        );
        assert_eq!(action(c, "buy"), Some(TypeValueAction::Buy));
        assert_eq!(action(c, "sell"), Some(TypeValueAction::Sell));
    }

    // ===================================================================
    // Files without a source
    // ===================================================================

    #[test]
    fn another_broker_gets_a_complete_suggestion() {
        let i = run(OTHER_BROKER);
        assert_eq!(i.detected_source, None);
        let c = config_of(&i);
        assert_eq!(c.source, "custom");
        assert_eq!(c.date_column, 0);
        assert_eq!(c.date_format, "%d.%m.%Y");
        assert_eq!(
            (c.symbol_column, c.isin_column, c.name_column),
            (Some(2), None, None)
        );
        assert_eq!((c.quantity_column, c.price_column), (3, 4));
        assert_eq!(
            (c.currency_mode, c.currency_column),
            (CurrencyMode::Column, Some(5))
        );
        assert_eq!(
            (c.direction_mode, c.type_column),
            (DirectionMode::TypeColumn, Some(1))
        );
        assert_eq!(c.decimal_separator, ",");
        assert_eq!(action(c, "BUY"), Some(TypeValueAction::Buy));
        assert_eq!(action(c, "Sell"), Some(TypeValueAction::Sell));
        assert_eq!(action(c, "Dividend"), Some(TypeValueAction::Skip));
        assert!(c.validate().is_ok());
        // Generic suggestions carry their confidence.
        assert_eq!(suggestion(&i, "date"), Some((0, 0.95)));
        assert_eq!(suggestion(&i, "quantity"), Some((3, 0.95)));
    }

    #[test]
    fn a_missing_currency_column_leaves_no_ready_config() {
        let i = run("Date;Type;Symbol;Quantity;Price\n15.01.2025;buy;AAPL;10;180,50\n");
        assert_eq!(i.detected_source, None);
        assert!(i.config.is_none());
        // The columns that were found are still suggested.
        assert_eq!(suggestion(&i, "date"), Some((0, 0.95)));
        assert_eq!(suggestion(&i, "price"), Some((4, 0.95)));
        assert_eq!(suggestion(&i, "currency"), None);
        assert_eq!(i.date_format.as_deref(), Some("%d.%m.%Y"));
        assert_eq!(i.decimal_separator, ",");
    }

    #[test]
    fn a_file_with_neither_symbol_nor_isin_has_no_ready_config() {
        let i = run("Date;Type;Quantity;Price;Currency\n15.01.2025;buy;10;180,50;USD\n");
        assert!(i.config.is_none());
    }

    #[test]
    fn without_a_type_column_the_sign_of_the_quantity_decides() {
        let i = run("Date,Ticker,Quantity,Price,Currency\n2025-01-15,AAPL,10,180.5,USD\n2025-02-20,AAPL,-4,185.25,USD\n");
        let c = config_of(&i);
        assert_eq!(
            (c.direction_mode, c.type_column),
            (DirectionMode::QuantitySign, None)
        );
        assert!(c.type_values.is_empty());
        assert!(c.validate().is_ok());
    }

    #[test]
    fn an_isin_alone_is_enough_to_identify_the_instrument() {
        let i = run("Date,ISIN,Quantity,Price,Currency\n2025-01-15,IE00B3XXRP09,10,78.2,EUR\n");
        let c = config_of(&i);
        assert_eq!((c.symbol_column, c.isin_column), (None, Some(1)));
    }

    #[test]
    fn day_and_month_order_is_flagged_when_the_file_cannot_tell() {
        let i = run("Date;Action;Symbol;Shares;Price;Currency\n01.02.2025;buy;AAPL;1;10,5;USD\n03.04.2025;buy;AAPL;1;11,5;USD\n");
        assert_eq!(i.date_format.as_deref(), Some("%d.%m.%Y"));
        assert!(i.date_format_ambiguous);
        assert_eq!(config_of(&i).date_format, "%d.%m.%Y");
    }

    #[test]
    fn dates_that_do_not_read_leave_the_format_unknown() {
        let i = run("Date;Action;Symbol;Shares;Price;Currency\nJan 5th;buy;AAPL;1;10,5;USD\n");
        assert_eq!(i.date_format, None);
        assert!(i.config.is_none(), "the user has to give the date format");
        assert_eq!(suggestion(&i, "date"), Some((0, 0.95)));
    }

    // ===================================================================
    // The broker's id column is only trusted when it can be
    // ===================================================================

    #[test]
    fn a_running_number_is_not_a_broker_id() {
        let i = run("ID;Trade Date;Action;Symbol;Shares;Price;CCY\n1;2025-01-15;buy;AAPL;1;10;USD\n2;2025-01-16;buy;AAPL;1;11;USD\n3;2025-01-17;buy;AAPL;1;12;USD\n");
        assert_eq!(i.detected_source, None);
        assert_eq!(suggestion(&i, "externalId"), Some((0, 0.95)));
        assert_eq!(config_of(&i).external_id_column, None);
    }

    #[test]
    fn repeated_ids_are_no_ids_either() {
        let i = run("Order ID;Trade Date;Action;Symbol;Shares;Price;CCY\nX1;2025-01-15;buy;AAPL;1;10;USD\nX1;2025-01-16;buy;AAPL;1;11;USD\nX2;2025-01-17;buy;AAPL;1;12;USD\n");
        assert_eq!(suggestion(&i, "externalId"), Some((0, 0.95)));
        assert_eq!(config_of(&i).external_id_column, None);
    }

    #[test]
    fn real_looking_ids_are_kept() {
        let i = run("Order ID;Trade Date;Action;Symbol;Shares;Price;CCY\nA1001;2025-01-15;buy;AAPL;1;10;USD\nA1007;2025-01-16;buy;AAPL;1;11;USD\nA1042;2025-01-17;buy;AAPL;1;12;USD\n");
        assert_eq!(config_of(&i).external_id_column, Some(0));
    }

    // ===================================================================
    // Saved formats and forced sources
    // ===================================================================

    fn saved_trading212(id: &str) -> SavedStockImportFormat {
        let rows = rows(&[]);
        let mut config = trading212::config(&headers(&TRADING212_HEADERS), &rows)
            .expect("config")
            .config;
        config.source = id.to_string();
        config.delimiter = ";".to_string();
        config.type_values = vec![
            crate::services::stock_import::types::StockTypeValueMapping {
                value: "Market buy".into(),
                action: TypeValueAction::Buy,
            },
        ];
        config.instrument_overrides = vec![StockInstrumentOverride {
            key: "isin:US17275R1023".into(),
            ticker: Some("CSCO".into()),
            name: None,
            currency: None,
            skip: true,
        }];
        config.import_anyway_lines = vec![3];
        saved_format(id, &TRADING212_HEADERS, config)
    }

    #[test]
    fn a_saved_format_wins_over_the_adapter() {
        let saved = [saved_trading212("format:abc")];
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &options(), &saved).expect("inspect");
        assert_eq!(i.detected_source.as_deref(), Some("format:abc"));
        let c = config_of(&i);
        assert_eq!(c.source, "format:abc");
        // The layout is the file's; the mapping is the saved one's.
        assert_eq!((c.delimiter.as_str(), c.header_row), (",", 0));
        assert_eq!(c.currency_column, Some(7));
        // Decisions about another file do not travel with the mapping.
        assert!(c.instrument_overrides.is_empty() && c.import_anyway_lines.is_empty());
        assert!(!i.date_format_ambiguous);
        assert_eq!(i.date_format.as_deref(), Some("%Y-%m-%d"));
    }

    #[test]
    fn a_saved_format_keeps_its_type_values_and_learns_the_new_ones() {
        let saved = [saved_trading212("format:abc")];
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &options(), &saved).expect("inspect");
        let c = config_of(&i);
        // Listed values keep their meaning, whatever the keywords say.
        assert_eq!(c.type_values[0].value, "Market buy");
        assert_eq!(action(c, "Market buy"), Some(TypeValueAction::Buy));
        // Values of this file that were not listed get the keyword suggestion
        // (not silently skipped).
        assert_eq!(action(c, "Market sell"), Some(TypeValueAction::Sell));
        assert_eq!(action(c, "Deposit"), Some(TypeValueAction::Skip));
        assert_eq!(c.type_values.len(), 4);
    }

    #[test]
    fn a_saved_format_with_a_value_set_to_skip_keeps_it() {
        let mut saved = saved_trading212("format:abc");
        saved.config.type_values = vec![
            crate::services::stock_import::types::StockTypeValueMapping {
                value: "market buy".into(),
                action: TypeValueAction::Skip,
            },
        ];
        let i =
            inspect(TRADING212_FILE.as_bytes(), "f.csv", &options(), &[saved]).expect("inspect");
        // Matched by folded text: "Market buy" is the listed "market buy".
        assert_eq!(
            action(config_of(&i), "market buy"),
            Some(TypeValueAction::Skip)
        );
        assert_eq!(config_of(&i).type_values.len(), 4, "not listed twice");
        assert_eq!(action(config_of(&i), "Market buy"), None);
    }

    #[test]
    fn a_format_that_does_not_match_is_ignored() {
        let other = saved_format(
            "format:x",
            &["Foo", "Bar"],
            saved_trading212("format:x").config,
        );
        let i =
            inspect(TRADING212_FILE.as_bytes(), "f.csv", &options(), &[other]).expect("inspect");
        assert_eq!(i.detected_source.as_deref(), Some("trading212"));
    }

    #[test]
    fn a_format_is_found_at_the_header_row_it_was_saved_with() {
        // Nothing in this file looks like a header to the detector, so it would
        // read line 0; the saved format knows better.
        let content = "Title\nMore\nTrade;Instr;Qty;Px;Cur\n2025-01-15;AAPL;5;10;USD\n";
        let mut config = degiro::config(&headers(&DEGIRO_HEADERS), &[])
            .expect("config")
            .config;
        config.source = "format:late".into();
        config.header_row = 2;
        config.delimiter = ";".into();
        let saved = saved_format(
            "format:late",
            &["Trade", "Instr", "Qty", "Px", "Cur"],
            config,
        );
        let i = inspect(content.as_bytes(), "f.csv", &options(), &[saved]).expect("inspect");
        assert_eq!(i.header_row, 2);
        assert_eq!(i.headers, ["Trade", "Instr", "Qty", "Px", "Cur"]);
        assert_eq!(i.detected_source.as_deref(), Some("format:late"));
        assert_eq!(config_of(&i).header_row, 2);
        assert_eq!(i.row_count, 1);

        // A header row the caller forced is not second-guessed.
        let forced = StockCsvInspectOptions {
            header_row: Some(0),
            ..options()
        };
        let saved = saved_format(
            "format:late",
            &["Trade", "Instr", "Qty", "Px", "Cur"],
            config_of(&i).clone(),
        );
        let i = inspect(content.as_bytes(), "f.csv", &forced, &[saved]).expect("inspect");
        assert_eq!(i.header_row, 0);
        assert_eq!(i.detected_source, None);
    }

    #[test]
    fn forcing_custom_gives_the_generic_mapping_but_still_reports_the_source() {
        let forced = StockCsvInspectOptions {
            source: Some("custom".into()),
            ..options()
        };
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &forced, &[]).expect("inspect");
        assert_eq!(
            i.detected_source.as_deref(),
            Some("trading212"),
            "what the file is"
        );
        let c = config_of(&i);
        assert_eq!(c.source, "custom");
        assert!(c.transforms == StockImportTransforms::default());
        // Generic columns: the Time column is the date, the first currency
        // column the price's.
        assert_eq!((c.date_column, c.currency_column), (1, Some(7)));
        assert_eq!(suggestion(&i, "date"), Some((1, 0.85)));
    }

    #[test]
    fn a_forced_source_that_does_not_fit_the_file_is_ignored() {
        let forced = StockCsvInspectOptions {
            source: Some("xtb".into()),
            ..options()
        };
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &forced, &[]).expect("inspect");
        assert_eq!(i.detected_source.as_deref(), Some("trading212"));
        assert_eq!(config_of(&i).source, "trading212");
        // A source that does not exist is ignored too.
        let forced = StockCsvInspectOptions {
            source: Some("nobody".into()),
            ..options()
        };
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &forced, &[]).expect("inspect");
        assert_eq!(config_of(&i).source, "trading212");
    }

    #[test]
    fn a_forced_source_that_fits_is_used() {
        let forced = StockCsvInspectOptions {
            source: Some("trading212".into()),
            ..options()
        };
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &forced, &[]).expect("inspect");
        assert_eq!(config_of(&i).source, "trading212");
    }

    #[test]
    fn a_forced_saved_format_picks_among_formats_with_the_same_headers() {
        let saved = [
            saved_trading212("format:new"),
            saved_trading212("format:old"),
        ];
        // The first (newest) wins by default ...
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &options(), &saved).expect("inspect");
        assert_eq!(config_of(&i).source, "format:new");
        // ... the caller can ask for the other.
        let forced = StockCsvInspectOptions {
            source: Some("format:old".into()),
            ..options()
        };
        let i = inspect(TRADING212_FILE.as_bytes(), "f.csv", &forced, &saved).expect("inspect");
        assert_eq!(config_of(&i).source, "format:old");
        assert_eq!(i.detected_source.as_deref(), Some("format:old"));
    }

    // ===================================================================
    // Shape of the file
    // ===================================================================

    #[test]
    fn column_values_list_low_cardinality_columns() {
        let i = run("Date;Action;Symbol;Shares;Price;Currency\n\
15.01.2025;buy;AAPL;1;10,5;USD\n\
16.01.2025;dividend;AAPL;1;0,2;USD\n\
17.01.2025;buy;MSFT;1;11,5;USD\n\
18.01.2025;sell;AAPL;1;12,5;USD\n\
19.01.2025;dividend;MSFT;1;0,3;USD\n\
20.01.2025;buy;AAPL;1;13,5;USD\n");
        let action_column = i
            .column_values
            .iter()
            .find(|c| c.column == 1)
            .expect("action column");
        let values: Vec<(&str, usize, usize, TypeValueAction)> = action_column
            .values
            .iter()
            .map(|v| (v.value.as_str(), v.count, v.first_line, v.suggested))
            .collect();
        // Most frequent first; the line is the 1-based line of the first row.
        assert_eq!(
            values,
            [
                ("buy", 3, 2, TypeValueAction::Buy),
                ("dividend", 2, 3, TypeValueAction::Skip),
                ("sell", 1, 5, TypeValueAction::Sell),
            ]
        );
        // Columns are listed in file order.
        let columns: Vec<usize> = i.column_values.iter().map(|c| c.column).collect();
        let mut sorted = columns.clone();
        sorted.sort_unstable();
        assert_eq!(columns, sorted);
    }

    #[test]
    fn a_column_with_more_than_thirty_values_is_not_listed() {
        let mut thirty = String::from("Date;Action;Symbol;Shares;Price;Currency\n");
        let mut thirty_one = thirty.clone();
        for n in 1..=31 {
            let row = format!("{n:02}.01.2025;buy;S{n};1;10,5;USD\n");
            if n <= 30 {
                thirty.push_str(&row);
            }
            thirty_one.push_str(&row);
        }
        let symbol_listed = |i: &StockCsvInspection| i.column_values.iter().any(|c| c.column == 2);
        assert!(symbol_listed(&run(&thirty)));
        assert!(!symbol_listed(&run(&thirty_one)));
        // The type column still has one value.
        assert!(run(&thirty_one).column_values.iter().any(|c| c.column == 1));
    }

    #[test]
    fn an_adapters_type_values_cover_every_value_even_beyond_thirty() {
        let mut content = String::from("Date;Action;Symbol;Shares;Price;Currency\n");
        for n in 1..=40 {
            content.push_str(&format!("15.01.2025;Kind {n};AAPL;1;10,5;USD\n"));
        }
        content.push_str("16.01.2025;buy;AAPL;1;10,5;USD\n");
        let i = run(&content);
        // 41 distinct values: not in columnValues, but the config knows them all.
        assert!(!i.column_values.iter().any(|c| c.column == 1));
        assert_eq!(config_of(&i).type_values.len(), 41);
        assert_eq!(action(config_of(&i), "buy"), Some(TypeValueAction::Buy));
    }

    #[test]
    fn sample_rows_are_the_first_eight_padded_to_the_header_width() {
        let mut content = String::from("Date;Type;Symbol;Quantity;Price;Currency\n");
        for n in 1..=20 {
            content.push_str(&format!("{n:02}.01.2025;buy;AAPL;1;10,5;USD\n"));
        }
        let i = run(&content);
        assert_eq!(i.row_count, 20);
        assert_eq!(i.sample_rows.len(), 8);
        assert_eq!(i.sample_rows[0][0], "01.01.2025");
        assert_eq!(i.sample_rows[7][0], "08.01.2025");

        // Short rows are padded, long rows cut.
        let i = run("Date;Type;Symbol;Quantity;Price;Currency\n15.01.2025;buy\n16.01.2025;buy;AAPL;1;10,5;USD;extra;more\n");
        assert!(i.sample_rows.iter().all(|r| r.len() == 6));
        assert_eq!(i.sample_rows[0], ["15.01.2025", "buy", "", "", "", ""]);
        assert_eq!(i.sample_rows[1][5], "USD");
    }

    #[test]
    fn blank_lines_and_blank_rows_are_not_data() {
        let i = run("Date;Type;Symbol;Quantity;Price;Currency\n\n15.01.2025;buy;AAPL;1;10,5;USD\n ; ; ; ; ; \n\n16.01.2025;buy;AAPL;1;10,5;USD\n");
        assert_eq!(i.row_count, 2);
        assert_eq!(i.sample_rows.len(), 2);
    }

    #[test]
    fn a_bom_and_windows_line_ends_are_no_matter() {
        let content = "\u{feff}Date;Type;Symbol;Quantity;Price;Currency\r\n15.01.2025;buy;AAPL;1;10,5;USD\r\n";
        let i = run(content);
        assert_eq!(i.headers[0], "Date");
        assert_eq!(i.row_count, 1);
        assert_eq!(i.sample_rows[0][5], "USD");
    }

    #[test]
    fn the_callers_options_are_honoured() {
        let content = "Report;generated;by broker\nTrade Date;Action;Symbol;Shares;Price;CCY\n15.01.2025;buy;AAPL;1;10,5;USD\n16.01.2025;sell;AAPL;1;11,5;USD\n";
        let i = run(content);
        assert_eq!((i.header_row, i.row_count), (1, 2));
        // A different header row, rows to skip, a delimiter.
        let forced = StockCsvInspectOptions {
            header_row: Some(0),
            ..options()
        };
        let i = inspect(content.as_bytes(), "f.csv", &forced, &[]).expect("inspect");
        assert_eq!((i.header_row, i.headers[0].as_str()), (0, "Report"));
        let skip = StockCsvInspectOptions {
            skip_rows: Some(1),
            ..options()
        };
        let i = inspect(content.as_bytes(), "f.csv", &skip, &[]).expect("inspect");
        assert_eq!(i.row_count, 1);
        assert_eq!(i.sample_rows[0][0], "16.01.2025");
        assert_eq!(config_of(&i).skip_rows, 1);
        let comma = StockCsvInspectOptions {
            delimiter: Some(",".into()),
            ..options()
        };
        let i = inspect(content.as_bytes(), "f.csv", &comma, &[]).expect("inspect");
        assert_eq!(i.delimiter, ",");
        assert_eq!(i.headers.len(), 1);
    }

    #[test]
    fn an_empty_file_or_a_bad_delimiter_is_a_validation_error() {
        for content in ["", "\n\n", "   \n"] {
            let err = inspect(content.as_bytes(), "f.csv", &options(), &[]).expect_err("empty");
            assert!(
                matches!(&err, AppError::Validation(key) if key == "validation.csvEmptyFile"),
                "{content:?}: {err:?}"
            );
        }
        let bad = StockCsvInspectOptions {
            delimiter: Some("|".into()),
            ..options()
        };
        let err = inspect(XTB_FILE.as_bytes(), "f.csv", &bad, &[]).expect_err("delimiter");
        assert!(
            matches!(&err, AppError::Validation(key) if key == "validation.csvDelimiterInvalid")
        );
        // A tab is fine.
        let tab = StockCsvInspectOptions {
            delimiter: Some("\t".into()),
            ..options()
        };
        assert!(inspect(XTB_FILE.as_bytes(), "f.csv", &tab, &[]).is_ok());
    }

    #[test]
    fn a_header_only_file_is_inspected_without_rows() {
        let i = run("Trade Date;Action;Symbol;Shares;Price;CCY\n");
        assert_eq!((i.row_count, i.sample_rows.len()), (0, 0));
        assert_eq!(i.date_format, None, "no dates to read the format from");
        assert!(i.config.is_none());
        // The Moony table is known without a row: the format of its export.
        let moony = run("Date;Type;Symbol;Quantity;Price;Currency\n");
        assert_eq!(moony.detected_source.as_deref(), Some("moony"));
        assert_eq!(moony.date_format.as_deref(), Some("%Y-%m-%d"));
        let t212 = run(TRADING212_FILE.lines().next().unwrap_or(""));
        assert_eq!(t212.detected_source.as_deref(), Some("trading212"));
        assert_eq!(config_of(&t212).date_format, "%Y-%m-%d");
    }

    #[test]
    fn the_fee_column_is_reported() {
        assert!(run("Date;Type;Symbol;Quantity;Price;Currency;Poplatek\n15.01.2025;buy;AAPL;1;10,5;USD;1\n").has_fee_column);
        assert!(!run(OTHER_BROKER).has_fee_column);
    }
}
