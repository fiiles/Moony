//! Wire types of the stock CSV import (TS mirror in `shared/schema.ts`, section
//! "Stock CSV import"). Per-field `#[serde(rename = "camelCase")]`, as
//! everywhere in this codebase; mirror every change in `shared/schema.ts`.
//!
//! Columns are 0-based indexes into the header row, not header names: broker
//! exports have blank and repeated header cells (Degiro's currency columns).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, Result};
use crate::services::csv_import::CsvDateRange;

/// Built-in sources (adapters). A hand mapping is `custom`; a saved custom
/// format is `format:<uuid>`.
pub const SOURCE_XTB: &str = "xtb";
pub const SOURCE_TRADING212: &str = "trading212";
pub const SOURCE_DEGIRO: &str = "degiro";
pub const SOURCE_IBKR: &str = "ibkr";
pub const SOURCE_MOONY: &str = "moony";
pub const SOURCE_CUSTOM: &str = "custom";
pub const SAVED_FORMAT_PREFIX: &str = "format:";

/// Rows shown in a preview; counts always cover the whole file.
pub const PREVIEW_ROWS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum TradeDirection {
    #[serde(rename = "buy")]
    Buy,
    #[serde(rename = "sell")]
    Sell,
}

impl TradeDirection {
    /// The `investment_transactions.type` value.
    pub fn as_str(self) -> &'static str {
        match self {
            TradeDirection::Buy => "buy",
            TradeDirection::Sell => "sell",
        }
    }
}

/// What a value of the type column means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TypeValueAction {
    #[serde(rename = "buy")]
    Buy,
    #[serde(rename = "sell")]
    Sell,
    /// Not a trade (deposit, dividend, fee, …): the row is skipped.
    #[serde(rename = "skip")]
    Skip,
}

/// Where the price currency of a trade comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum CurrencyMode {
    /// A column of the file (`currencyColumn`).
    #[serde(rename = "column")]
    Column,
    /// One currency for the whole file (`fixedCurrency`).
    #[serde(rename = "fixed")]
    Fixed,
    /// The currency of the instrument's listing (XTB exports carry none).
    #[serde(rename = "instrument")]
    Instrument,
}

/// How the direction of a trade is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DirectionMode {
    /// From `typeColumn` through `typeValues`.
    #[serde(rename = "typeColumn")]
    TypeColumn,
    /// From the sign of the quantity (Degiro, IBKR): negative is a sell.
    #[serde(rename = "quantitySign")]
    QuantitySign,
}

/// One value of the type column and what it means (TS `StockTypeValueMapping`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockTypeValueMapping {
    /// The trimmed cell text as it appears in the file.
    pub value: String,
    pub action: TypeValueAction,
}

/// Transforms a plain column mapping cannot express. Adapters set them; saved
/// formats keep them (TS `StockImportTransforms`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockImportTransforms {
    /// XTB: quantity and price are read from the comment held in the quantity
    /// and price columns ("OPEN BUY 34/42.5658 @ 11.7480": 34 at 11.748).
    #[serde(rename = "xtbComment", default)]
    pub xtb_comment: bool,
    /// XTB: symbols carry XTB suffixes (`.US`, `.UK`, `.DE`, …) that are
    /// mapped to Yahoo Finance symbols (`AAPL`, `VUSA.L`, `SPYL.DE`).
    #[serde(rename = "xtbSymbols", default)]
    pub xtb_symbols: bool,
    /// IBKR: a row is skipped unless this column holds one of
    /// `assetClassAllowed` (e.g. `STK`).
    #[serde(rename = "assetClassColumn", default)]
    pub asset_class_column: Option<usize>,
    #[serde(rename = "assetClassAllowed", default)]
    pub asset_class_allowed: Vec<String>,
    /// Degiro: the broker's id is the order's, shared by its fills. A row then
    /// repeats a stored trade by id only with the same quantity and price, and
    /// the rows of one file are never compared by id.
    #[serde(rename = "brokerIdPerOrder", default)]
    pub broker_id_per_order: bool,
}

/// The user's choice for one instrument in the preview (TS `StockInstrumentOverride`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockInstrumentOverride {
    /// `StockImportInstrument.key`.
    pub key: String,
    /// Yahoo Finance symbol the trades are stored under.
    #[serde(default)]
    pub ticker: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// ISO 4217 code, or `GBX` when the file's prices are in pence (they are stored as pounds).
    /// It is the currency of the trades that name none (`CurrencyMode::Instrument`; needed when
    /// the instrument could not be resolved); a currency the file states, in a column or for the
    /// whole file, is never overridden.
    #[serde(default)]
    pub currency: Option<String>,
    /// Exclude every trade of this instrument.
    #[serde(default)]
    pub skip: bool,
}

/// Configuration of one preview or import (TS `StockImportConfig`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockImportConfig {
    /// `xtb`, `trading212`, `degiro`, `ibkr`, `moony`, `custom` or `format:<id>`.
    pub source: String,
    /// One character: `,`, `;` or a tab.
    pub delimiter: String,
    /// Text encoding name as `inspect` reports it (`UTF-8`, `windows-1250`, …).
    pub encoding: String,
    /// 0-based line of the header row.
    #[serde(rename = "headerRow")]
    pub header_row: usize,
    /// Data rows to skip after the header row.
    #[serde(rename = "skipRows", default)]
    pub skip_rows: usize,
    #[serde(rename = "dateColumn")]
    pub date_column: usize,
    /// chrono format of the date part (`%d.%m.%Y`, `%Y%m%d`); text after the
    /// date (a time) is ignored. Parsed strictly: a row that does not match is
    /// an error, never re-read in another order.
    #[serde(rename = "dateFormat")]
    pub date_format: String,
    #[serde(rename = "symbolColumn", default)]
    pub symbol_column: Option<usize>,
    #[serde(rename = "isinColumn", default)]
    pub isin_column: Option<usize>,
    #[serde(rename = "nameColumn", default)]
    pub name_column: Option<usize>,
    #[serde(rename = "quantityColumn")]
    pub quantity_column: usize,
    #[serde(rename = "priceColumn")]
    pub price_column: usize,
    #[serde(rename = "currencyMode")]
    pub currency_mode: CurrencyMode,
    #[serde(rename = "currencyColumn", default)]
    pub currency_column: Option<usize>,
    #[serde(rename = "fixedCurrency", default)]
    pub fixed_currency: Option<String>,
    #[serde(rename = "directionMode")]
    pub direction_mode: DirectionMode,
    #[serde(rename = "typeColumn", default)]
    pub type_column: Option<usize>,
    /// Meaning of each type value; a value not listed is skipped.
    #[serde(rename = "typeValues", default)]
    pub type_values: Vec<StockTypeValueMapping>,
    /// `","` or `"."`.
    #[serde(rename = "decimalSeparator")]
    pub decimal_separator: String,
    /// The broker's own transaction id (duplicate rule 1).
    #[serde(rename = "externalIdColumn", default)]
    pub external_id_column: Option<usize>,
    #[serde(default)]
    pub transforms: StockImportTransforms,
    #[serde(rename = "instrumentOverrides", default)]
    pub instrument_overrides: Vec<StockInstrumentOverride>,
    /// 1-based file lines of duplicate rows to import anyway.
    #[serde(rename = "importAnywayLines", default)]
    pub import_anyway_lines: Vec<usize>,
}

fn is_currency_code(code: &str) -> bool {
    code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic())
}

impl StockImportConfig {
    /// Structural checks at the trust boundary; every error is a
    /// `validation.*` key (common namespace).
    pub fn validate(&self) -> Result<()> {
        let invalid = |key: &str| Err(AppError::Validation(format!("validation.{key}")));
        if !matches!(self.delimiter.as_str(), "," | ";" | "\t") {
            return invalid("csvDelimiterInvalid");
        }
        if !matches!(self.decimal_separator.as_str(), "," | ".") {
            return invalid("csvDecimalSeparatorInvalid");
        }
        if self.date_format.trim().is_empty() {
            return invalid("dateFormatRequired");
        }
        if self.symbol_column.is_none() && self.isin_column.is_none() {
            return invalid("stockImportSymbolRequired");
        }
        match self.currency_mode {
            CurrencyMode::Column if self.currency_column.is_none() => {
                return invalid("stockImportCurrencyRequired")
            }
            CurrencyMode::Fixed
                if !self
                    .fixed_currency
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(is_currency_code) =>
            {
                return invalid("currencyInvalid")
            }
            _ => {}
        }
        if self.direction_mode == DirectionMode::TypeColumn && self.type_column.is_none() {
            return invalid("stockImportTypeRequired");
        }
        for o in &self.instrument_overrides {
            if let Some(currency) = o.currency.as_deref() {
                if !is_currency_code(currency.trim()) {
                    return invalid("currencyInvalid");
                }
            }
            if let Some(ticker) = o.ticker.as_deref() {
                let t = ticker.trim();
                if t.is_empty()
                    || t.len() > 20
                    || !t
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '^' | '='))
                {
                    return invalid("tickerInvalid");
                }
            }
        }
        Ok(())
    }

    /// The override for an instrument key, if any.
    pub fn override_for(&self, key: &str) -> Option<&StockInstrumentOverride> {
        self.instrument_overrides.iter().find(|o| o.key == key)
    }
}

/// Options of an inspection (TS `StockCsvInspectOptions`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockCsvInspectOptions {
    /// Use this source instead of detecting one.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(rename = "headerRow", default)]
    pub header_row: Option<usize>,
    #[serde(rename = "skipRows", default)]
    pub skip_rows: Option<usize>,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub delimiter: Option<String>,
}

/// A column suggested for a role (TS `StockColumnSuggestion`). Roles: `date`,
/// `type`, `symbol`, `isin`, `name`, `quantity`, `price`, `currency`,
/// `externalId`, `fee`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockColumnSuggestion {
    pub role: String,
    pub column: usize,
    /// 0.0–1.0 (1.0 when a source's adapter set it).
    pub confidence: f64,
}

/// A distinct value of a low-cardinality column (TS `StockTypeValueStat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockTypeValueStat {
    pub value: String,
    pub count: usize,
    /// 1-based file line of the first row with this value.
    #[serde(rename = "firstLine")]
    pub first_line: usize,
    /// Meaning suggested from keywords in several languages.
    pub suggested: TypeValueAction,
}

/// Distinct values of one column, for the type-value table (TS `StockColumnValues`).
/// Only columns with at most 30 distinct non-empty values are listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockColumnValues {
    pub column: usize,
    pub values: Vec<StockTypeValueStat>,
}

/// What a file looks like and how it would be read (TS `StockCsvInspection`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockCsvInspection {
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub encoding: String,
    pub delimiter: String,
    #[serde(rename = "headerRow")]
    pub header_row: usize,
    /// Header cells as in the file (a blank cell stays empty; the UI labels it
    /// by position).
    pub headers: Vec<String>,
    /// The first data rows (at most 8), each padded to `headers.len()`.
    #[serde(rename = "sampleRows")]
    pub sample_rows: Vec<Vec<String>>,
    /// Data rows after the header (and skipped rows).
    #[serde(rename = "rowCount")]
    pub row_count: usize,
    /// Built-in source or saved format the headers belong to.
    #[serde(rename = "detectedSource")]
    pub detected_source: Option<String>,
    /// Ready configuration: the detected source's, or a complete suggestion
    /// from the headers; `None` when a required column is still unknown.
    pub config: Option<StockImportConfig>,
    pub suggestions: Vec<StockColumnSuggestion>,
    #[serde(rename = "columnValues")]
    pub column_values: Vec<StockColumnValues>,
    /// Detected format of the date column, when one is known.
    #[serde(rename = "dateFormat")]
    pub date_format: Option<String>,
    /// Every sampled date reads both day-first and month-first.
    #[serde(rename = "dateFormatAmbiguous")]
    pub date_format_ambiguous: bool,
    #[serde(rename = "decimalSeparator")]
    pub decimal_separator: String,
    /// The file has a fee/commission column (fees are not imported).
    #[serde(rename = "hasFeeColumn")]
    pub has_fee_column: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum StockRowStatus {
    /// Will be imported.
    #[serde(rename = "new")]
    New,
    /// Matches an existing transaction; skipped unless imported anyway.
    #[serde(rename = "duplicate")]
    Duplicate,
    /// Not a trade, or excluded on purpose (see `message`).
    #[serde(rename = "skipped")]
    Skipped,
    /// Cannot be imported (see `message`).
    #[serde(rename = "error")]
    Error,
}

/// A per-row note or problem. Never English prose: `key` is an i18n key
/// (`importWizard.row.*` in the `stocks` namespace, `validation.*` in
/// `common`) and `detail` the raw value it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockRowMessage {
    /// 1-based file line of the row.
    pub line: usize,
    pub key: String,
    pub detail: Option<String>,
}

/// One row of the preview table (TS `StockPreviewRow`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockPreviewRow {
    pub line: usize,
    /// UTC day (unix seconds), when the date parsed.
    pub day: Option<i64>,
    pub direction: Option<TradeDirection>,
    #[serde(rename = "instrumentKey")]
    pub instrument_key: Option<String>,
    /// Ticker the trade would be stored under.
    pub ticker: Option<String>,
    /// Decimal text, as it would be stored.
    pub quantity: Option<String>,
    pub price: Option<String>,
    pub currency: Option<String>,
    pub status: StockRowStatus,
    pub message: Option<StockRowMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum StockInstrumentStatus {
    /// The ticker is an existing Moony position.
    #[serde(rename = "existing")]
    Existing,
    /// A new position will be created under `ticker`.
    #[serde(rename = "new")]
    New,
    /// Only an ISIN is known and no symbol was chosen yet.
    #[serde(rename = "missingSymbol")]
    MissingSymbol,
    /// Excluded by the user.
    #[serde(rename = "skipped")]
    Skipped,
}

/// One security of the file (TS `StockImportInstrument`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockImportInstrument {
    /// `isin:<ISIN>` when the file has an ISIN, otherwise `symbol:<SYMBOL>`.
    pub key: String,
    /// Symbol as read from the file (after the source's transforms).
    pub symbol: Option<String>,
    pub isin: Option<String>,
    pub name: Option<String>,
    /// Currency of its trades (file or override); `None` while unknown.
    pub currency: Option<String>,
    #[serde(rename = "tradeCount")]
    pub trade_count: usize,
    /// Ticker the trades will be stored under (override, else file symbol).
    pub ticker: Option<String>,
    pub status: StockInstrumentStatus,
    /// Currency of the existing position when it differs (its trades are errors).
    #[serde(rename = "positionCurrency")]
    pub position_currency: Option<String>,
}

/// Row counts of a preview, over the whole file (TS `StockImportCounts`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockImportCounts {
    pub total: usize,
    #[serde(rename = "willImport")]
    pub will_import: usize,
    pub duplicates: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Dry run of an import (TS `StockImportPreview`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockImportPreview {
    pub instruments: Vec<StockImportInstrument>,
    /// The first `PREVIEW_ROWS` rows in file order.
    pub rows: Vec<StockPreviewRow>,
    pub counts: StockImportCounts,
    /// Trade days of the rows that will be imported.
    #[serde(rename = "dateRange")]
    pub date_range: Option<CsvDateRange>,
    #[serde(rename = "hasFeeColumn")]
    pub has_fee_column: bool,
}

/// What to look up on Yahoo Finance for one instrument (TS `StockInstrumentQuery`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockInstrumentQuery {
    pub key: String,
    pub symbol: Option<String>,
    pub isin: Option<String>,
    pub name: Option<String>,
    /// Currency of the trades; prefers a listing in it.
    pub currency: Option<String>,
}

/// A Yahoo Finance listing (TS `StockInstrumentCandidate`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockInstrumentCandidate {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    /// Currency of the listing. For the listing the lookup chose (`best`) it is what Yahoo
    /// reports for its quote, so a London ETF in dollars reads `USD`; every other candidate, and
    /// a `best` whose quote could not be looked up, carries the guess from the symbol's exchange
    /// suffix. `GBX` stands for pence: the quote, and so the prices of a file that follows it,
    /// are a hundredth of `GBP`.
    pub currency: String,
}

/// Result of looking up one instrument (TS `StockInstrumentResolution`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockInstrumentResolution {
    pub key: String,
    pub candidates: Vec<StockInstrumentCandidate>,
    /// The exact symbol, else the listing in the trade currency, else the first.
    pub best: Option<StockInstrumentCandidate>,
    /// The lookup failed (offline, rate limit): unverified, not unknown.
    #[serde(rename = "lookupFailed")]
    pub lookup_failed: bool,
}

/// Outcome of an import (TS `StockImportResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockImportResult {
    /// `None` when nothing was imported (no batch is recorded).
    #[serde(rename = "batchId")]
    pub batch_id: Option<String>,
    pub imported: usize,
    pub duplicates: usize,
    pub skipped: usize,
    pub errors: usize,
    /// Tickers of positions the import created.
    #[serde(rename = "newPositions")]
    pub new_positions: Vec<String>,
    /// Tickers of existing positions that received trades.
    #[serde(rename = "updatedPositions")]
    pub updated_positions: Vec<String>,
    /// Duplicate, skip and error messages in file order.
    pub messages: Vec<StockRowMessage>,
    /// Earliest imported trade day (history rebuild start).
    #[serde(rename = "earliestDay")]
    pub earliest_day: Option<i64>,
}

/// One recorded import (TS `StockImportBatch`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockImportBatch {
    pub id: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub source: String,
    #[serde(rename = "tradeCount")]
    pub trade_count: usize,
    /// Transactions of the batch still present (edits keep them, deletes do not).
    #[serde(rename = "remainingCount")]
    pub remaining_count: usize,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Outcome of undoing a batch (TS `StockImportUndoResult`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StockImportUndoResult {
    pub removed: usize,
    /// Positions deleted because no transaction was left.
    #[serde(rename = "removedPositions")]
    pub removed_positions: Vec<String>,
    /// Every ticker the batch touched (history rebuild).
    pub tickers: Vec<String>,
    #[serde(rename = "earliestDay")]
    pub earliest_day: Option<i64>,
}

/// A custom mapping remembered for files with the same headers (TS `SavedStockImportFormat`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SavedStockImportFormat {
    pub id: String,
    pub name: String,
    /// `header_signature` of the file it was saved from.
    #[serde(rename = "headerSignature")]
    pub header_signature: String,
    pub config: StockImportConfig,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Identity of a header row for saved formats: trimmed, lowercased cells
/// joined by `|`, so case, surrounding spaces and a BOM do not matter.
pub fn header_signature(headers: &[String]) -> String {
    headers
        .iter()
        .map(|h| {
            h.trim()
                .trim_start_matches('\u{feff}')
                .trim()
                .to_lowercase()
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// One trade read from a file, before instrument overrides and simulation.
/// Internal (not a wire type): produced by `parse::parse_file`, consumed by
/// `preview` and `import`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTrade {
    /// 1-based file line.
    pub line: usize,
    /// UTC day, unix seconds.
    pub day: i64,
    pub direction: TradeDirection,
    /// `isin:<ISIN>` when an ISIN is known, otherwise `symbol:<SYMBOL>`.
    pub instrument_key: String,
    /// After the source's transforms (XTB suffix → Yahoo symbol), uppercase.
    pub symbol: Option<String>,
    /// Uppercase.
    pub isin: Option<String>,
    pub name: Option<String>,
    /// Always > 0 (the sign is in `direction`).
    pub quantity: f64,
    /// Always > 0, in `currency`; GBX/GBp prices are already in GBP.
    pub price: f64,
    /// Uppercase ISO code; `None` with `CurrencyMode::Instrument`.
    pub currency: Option<String>,
    pub external_id: Option<String>,
}

/// A whole file read with one config (internal).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedFile {
    pub trades: Vec<ParsedTrade>,
    /// Rows that are not trades (skipped) or could not be read (errors),
    /// keyed by line: `importWizard.row.*` / `validation.*`.
    pub skipped: Vec<StockRowMessage>,
    pub errors: Vec<StockRowMessage>,
    /// Data rows considered (every row ends in exactly one of trades,
    /// skipped or errors).
    pub total_rows: usize,
    pub has_fee_column: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> StockImportConfig {
        StockImportConfig {
            source: SOURCE_CUSTOM.into(),
            delimiter: ";".into(),
            encoding: "UTF-8".into(),
            header_row: 0,
            skip_rows: 0,
            date_column: 0,
            date_format: "%d.%m.%Y".into(),
            symbol_column: Some(2),
            isin_column: None,
            name_column: None,
            quantity_column: 3,
            price_column: 4,
            currency_mode: CurrencyMode::Column,
            currency_column: Some(5),
            fixed_currency: None,
            direction_mode: DirectionMode::TypeColumn,
            type_column: Some(1),
            type_values: vec![StockTypeValueMapping {
                value: "Nákup".into(),
                action: TypeValueAction::Buy,
            }],
            decimal_separator: ",".into(),
            external_id_column: None,
            transforms: StockImportTransforms::default(),
            instrument_overrides: vec![],
            import_anyway_lines: vec![],
        }
    }

    fn key(r: Result<()>) -> String {
        match r {
            Err(AppError::Validation(k)) => k,
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn a_complete_config_is_valid() {
        assert!(config().validate().is_ok());
    }

    #[test]
    fn structural_problems_are_validation_keys() {
        let mut c = config();
        c.delimiter = "|".into();
        assert_eq!(key(c.validate()), "validation.csvDelimiterInvalid");

        let mut c = config();
        c.symbol_column = None;
        assert_eq!(key(c.validate()), "validation.stockImportSymbolRequired");

        let mut c = config();
        c.currency_column = None;
        assert_eq!(key(c.validate()), "validation.stockImportCurrencyRequired");

        let mut c = config();
        c.currency_mode = CurrencyMode::Fixed;
        c.fixed_currency = Some("dollar".into());
        assert_eq!(key(c.validate()), "validation.currencyInvalid");

        let mut c = config();
        c.type_column = None;
        assert_eq!(key(c.validate()), "validation.stockImportTypeRequired");

        let mut c = config();
        c.direction_mode = DirectionMode::QuantitySign;
        c.type_column = None;
        assert!(c.validate().is_ok());
    }

    #[test]
    fn overrides_are_checked() {
        let mut c = config();
        c.instrument_overrides = vec![StockInstrumentOverride {
            key: "symbol:AAPL".into(),
            ticker: Some("VWCE.DE".into()),
            name: None,
            currency: Some("EUR".into()),
            skip: false,
        }];
        assert!(c.validate().is_ok());
        assert_eq!(c.override_for("symbol:AAPL").map(|o| o.skip), Some(false));

        c.instrument_overrides[0].ticker = Some("BAD TICKER".into());
        assert_eq!(key(c.validate()), "validation.tickerInvalid");
    }

    #[test]
    fn header_signature_ignores_case_spaces_and_bom() {
        let a = header_signature(&["\u{feff}Date ".into(), "Ticker".into(), "".into()]);
        let b = header_signature(&["date".into(), " TICKER".into(), "".into()]);
        assert_eq!(a, b);
        assert_eq!(a, "date|ticker|");
    }
}
