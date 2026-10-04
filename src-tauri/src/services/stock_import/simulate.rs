//! Duplicate rules and the holdings/currency simulation behind preview and
//! import.
//!
//! [`simulate`] replays the trades of a parsed file against the database
//! without writing anything and decides what an import would do with each row.
//! The preview shows the result and the import writes exactly the rows it
//! accepts, so what the preview promises is what the import does.
//!
//! The currency of a trade is the one its file states (a currency column, or one
//! currency for the whole file; the parser has already turned a pence cell into
//! pounds): that is a fact no override relabels. Only a trade without one (an
//! `Instrument` mode file such as XTB's) takes its instrument's: the override, which the
//! lookup sets and the user can change, else the guess from the listing's ticker
//! suffix. An instrument currency of `GBX` (or `GBp`) means the file's prices are
//! pence: they are divided by 100 and the trade is in `GBP`, before any rule below
//! sees it, so what is compared, previewed and written is the pound price the
//! position is valued in.
//!
//! Rows are replayed in date order, buys before sells within a day, per
//! ticker, starting from the transactions the ticker already has. A row is
//! decided in this order:
//!
//! 1. its instrument was skipped (`Skipped`);
//! 2. its instrument has no usable ticker, or the row has no currency, or
//!    its currency differs from the position's (the existing position's, else
//!    the first accepted row's of a new one) (`Error`);
//! 3. it is a duplicate (`Duplicate`, written only when its line is in
//!    `importAnywayLines`);
//! 4. it sells more than is held at the end of its day (`Error`);
//! 5. otherwise it is accepted (`New`) and counts for the rows after it.
//!
//! Error rows and unforced duplicates are left out and the replay continues
//! without them.
//!
//! Duplicates, as in the bank import (`csv_import`): a row repeats an
//! existing transaction, or an earlier accepted row of the same file.
//! - Rule 1, the broker's own id (stored as `<source>:<id>`): a stored
//!   transaction has it, under any ticker, or an earlier accepted row of the
//!   file has it. Degiro's order id is shared by the fills of an order, so
//!   there the quantity and price must match too and the rows of a file are
//!   never compared with each other.
//! - Rule 2, the same values: same ticker, UTC day, direction, quantity (within
//!   1e-9 relative) and price (within 1e-6 relative). A row that has a broker
//!   id is distinguished from stored transactions that have another id of the
//!   same source, and from the other rows of its file; it matches stored
//!   transactions without a broker id (an earlier import before ids, a
//!   hand-made trade) and those whose id comes from another source (ids of two
//!   sources cannot be compared, e.g. a preset import before its layout was
//!   saved as a format). A row without one also matches an earlier accepted
//!   row of its file.
//!
//! Stored transactions are not "used up" by a match: re-importing a file is
//! idempotent whatever it repeats.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension};

use super::types::{
    CurrencyMode, ParsedFile, ParsedTrade, StockImportConfig, StockImportInstrument,
    StockInstrumentStatus, StockRowMessage, TradeDirection, SOURCE_DEGIRO,
};
use crate::error::Result;
use crate::services::csv_import::amounts::amount_to_text;
use crate::services::price_api;
use crate::services::quote_unit::{is_pence, pence_to_pounds};

// Row message keys of the simulation (`StockRowMessage.key`, `stocks`
// namespace except the last, which is in `common`); `detail` is what each one
// is about.

/// A transaction with the same values exists (no detail).
pub const KEY_DUPLICATE: &str = "importWizard.row.duplicate";
/// A transaction with the same broker id exists (detail: the broker's id).
pub const KEY_DUPLICATE_BY_ID: &str = "importWizard.row.duplicateById";
/// A sell above the holding at its day (detail: the quantity held).
pub const KEY_SELL_EXCEEDS_HOLDINGS: &str = "importWizard.row.sellExceedsHoldings";
/// The position is in another currency (detail: the position's currency).
pub const KEY_CURRENCY_MISMATCH: &str = "importWizard.row.currencyMismatch";
/// The instrument has no symbol yet (an ISIN only; no detail).
pub const KEY_SYMBOL_MISSING: &str = "importWizard.row.symbolMissing";
/// The trade has no currency and none can be derived (no detail).
pub const KEY_CURRENCY_MISSING: &str = "importWizard.row.currencyMissing";
/// The user excluded the instrument (no detail).
pub const KEY_INSTRUMENT_SKIPPED: &str = "importWizard.row.instrumentSkipped";
/// The symbol cannot be a ticker, e.g. `BRK B` (detail: the symbol).
pub const KEY_TICKER_INVALID: &str = "validation.tickerInvalid";

const SECONDS_PER_DAY: i64 = 86_400;
const QUANTITY_TOLERANCE: f64 = 1e-9;
const PRICE_TOLERANCE: f64 = 1e-6;
/// Broker ids per `IN (...)` query (SQLite caps the number of variables).
const ID_CHUNK: usize = 500;

/// Why a row counts as already imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateKind {
    /// The broker's own transaction id exists already.
    BrokerId,
    /// Same ticker, day, direction, quantity and price.
    Identical,
}

impl DuplicateKind {
    /// The row message key of this kind.
    pub fn key(self) -> &'static str {
        match self {
            DuplicateKind::BrokerId => KEY_DUPLICATE_BY_ID,
            DuplicateKind::Identical => KEY_DUPLICATE,
        }
    }
}

/// What happens to one parsed trade.
#[derive(Debug, Clone, PartialEq)]
pub enum TradeOutcome {
    /// Will be written.
    New,
    /// Matches an earlier transaction; written only when `forced` (its line
    /// is in `importAnywayLines`).
    Duplicate { kind: DuplicateKind, forced: bool },
    /// Left out on purpose.
    Skipped(StockRowMessage),
    /// Cannot be imported.
    Error(StockRowMessage),
}

/// A parsed trade with the instrument overrides applied and its outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatedTrade {
    pub line: usize,
    pub day: i64,
    pub direction: TradeDirection,
    pub instrument_key: String,
    /// Ticker the trade is stored under; `None` while the instrument has none.
    pub ticker: Option<String>,
    /// Name for a position the trade creates; an existing position's own name
    /// when the ticker is one.
    pub company_name: Option<String>,
    pub quantity: f64,
    pub price: f64,
    pub currency: Option<String>,
    /// The broker's id as the file has it.
    pub broker_id: Option<String>,
    /// `<source>:<broker id>`, as stored.
    pub external_id: Option<String>,
    pub outcome: TradeOutcome,
}

impl SimulatedTrade {
    /// The row is written by an import.
    pub fn will_import(&self) -> bool {
        matches!(
            self.outcome,
            TradeOutcome::New | TradeOutcome::Duplicate { forced: true, .. }
        )
    }

    /// The row's own message: why it is a duplicate (also when it is imported
    /// anyway), skipped or an error. `None` for a plain new row.
    pub fn message(&self) -> Option<StockRowMessage> {
        match &self.outcome {
            TradeOutcome::New => None,
            TradeOutcome::Duplicate { kind, .. } => Some(StockRowMessage {
                line: self.line,
                key: kind.key().to_string(),
                detail: match kind {
                    DuplicateKind::BrokerId => self.broker_id.clone(),
                    DuplicateKind::Identical => None,
                },
            }),
            TradeOutcome::Skipped(message) | TradeOutcome::Error(message) => Some(message.clone()),
        }
    }

    /// Why the row is not written; `None` for a row that is.
    pub fn left_out_message(&self) -> Option<StockRowMessage> {
        if self.will_import() {
            None
        } else {
            self.message()
        }
    }

    /// The quantity as it would be stored.
    pub fn quantity_text(&self) -> String {
        amount_to_text(self.quantity)
    }

    /// The price as it would be stored.
    pub fn price_text(&self) -> String {
        amount_to_text(self.price)
    }
}

/// The whole file replayed against the database.
#[derive(Debug, Clone, PartialEq)]
pub struct Simulation {
    /// One entry per `ParsedFile.trades`, in the same order.
    pub trades: Vec<SimulatedTrade>,
    /// In order of first appearance.
    pub instruments: Vec<StockImportInstrument>,
}

fn message(line: usize, key: &str, detail: Option<String>) -> StockRowMessage {
    StockRowMessage {
        line,
        key: key.to_string(),
        detail,
    }
}

/// `a` and `b` agree within a relative `tolerance`.
fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance * a.abs().max(b.abs())
}

/// A currency code as it was typed (trimmed) when it has the shape of one: three letters.
/// The case is kept: `GBp` (pence) is not `GBP` (pounds).
fn currency_code(code: &str) -> Option<&str> {
    let code = code.trim();
    (code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic())).then_some(code)
}

fn normalized_currency(code: &str) -> Option<String> {
    currency_code(code).map(str::to_ascii_uppercase)
}

/// The same shape `StockImportConfig::validate` accepts for an override.
fn is_valid_ticker(ticker: &str) -> bool {
    !ticker.is_empty()
        && ticker.len() <= 20
        && ticker
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '^' | '='))
}

fn non_empty(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// A stored decimal as `recalculate_investment_metrics` reads it: the first
/// word (legacy rows may carry a unit text). Unreadable values are `None`:
/// they never match a row and count as nothing held.
fn parse_stored_decimal(text: &str) -> Option<f64> {
    text.split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// A quantity for a message: eight decimals at most, no trailing zeros, so a
/// sum of fractional trades does not show float noise (`0.3`, not
/// `0.30000000000000004`).
fn rounded_quantity(quantity: f64) -> String {
    let text = format!("{quantity:.8}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The UTC midnight of the day `timestamp` falls on. Stored dates are day
/// stamps, but a transaction created without a date carries the time it was
/// made; it belongs to that day all the same.
fn utc_day(timestamp: i64) -> i64 {
    timestamp.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY
}

fn direction_rank(direction: TradeDirection) -> u8 {
    match direction {
        TradeDirection::Buy => 0,
        TradeDirection::Sell => 1,
    }
}

// ---------------------------------------------------------------------------
// What the database already holds
// ---------------------------------------------------------------------------

const STORED_COLUMNS: &str = "type, quantity, price_per_unit, transaction_date, external_id";

#[derive(Debug)]
struct Stored {
    direction: Option<TradeDirection>,
    quantity: Option<f64>,
    price: Option<f64>,
    day: i64,
    external_id: Option<String>,
}

impl Stored {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let kind: String = row.get(0)?;
        let quantity: String = row.get(1)?;
        let price: String = row.get(2)?;
        Ok(Stored {
            direction: match kind.to_lowercase().as_str() {
                "buy" => Some(TradeDirection::Buy),
                "sell" => Some(TradeDirection::Sell),
                _ => None,
            },
            quantity: parse_stored_decimal(&quantity),
            price: parse_stored_decimal(&price),
            day: utc_day(row.get(3)?),
            external_id: row.get(4)?,
        })
    }

    fn signed_quantity(&self) -> f64 {
        let quantity = self.quantity.unwrap_or(0.0);
        match self.direction {
            Some(TradeDirection::Buy) => quantity,
            Some(TradeDirection::Sell) => -quantity,
            None => 0.0,
        }
    }

    fn same_quantity_and_price(&self, trade: &ParsedTrade) -> bool {
        self.quantity
            .is_some_and(|q| close(q, trade.quantity, QUANTITY_TOLERANCE))
            && self
                .price
                .is_some_and(|p| close(p, trade.price, PRICE_TOLERANCE))
    }

    fn same_trade(&self, trade: &ParsedTrade) -> bool {
        self.direction == Some(trade.direction)
            && self.day == trade.day
            && self.same_quantity_and_price(trade)
    }
}

#[derive(Debug, Clone)]
struct Position {
    id: String,
    company_name: String,
    currency: String,
}

fn find_position(conn: &Connection, ticker: &str) -> Result<Option<Position>> {
    Ok(conn
        .query_row(
            "SELECT id, company_name, currency FROM stock_investments WHERE ticker = ?1",
            [ticker],
            |row| {
                Ok(Position {
                    id: row.get(0)?,
                    company_name: row.get(1)?,
                    currency: row.get::<_, String>(2)?.to_uppercase(),
                })
            },
        )
        .optional()?)
}

fn load_position_rows(conn: &Connection, investment_id: &str) -> Result<Vec<Stored>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {STORED_COLUMNS} FROM investment_transactions WHERE investment_id = ?1"
    ))?;
    let rows = stmt.query_map([investment_id], Stored::from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Stored transactions that carry one of `ids`, grouped by id.
fn load_rows_by_external_id(
    conn: &Connection,
    ids: &[String],
) -> Result<HashMap<String, Vec<Stored>>> {
    let mut by_id: HashMap<String, Vec<Stored>> = HashMap::new();
    for chunk in ids.chunks(ID_CHUNK) {
        let marks = vec!["?"; chunk.len()].join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT {STORED_COLUMNS} FROM investment_transactions WHERE external_id IN ({marks})"
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), Stored::from_row)?;
        for row in rows {
            let stored = row?;
            if let Some(id) = stored.external_id.clone() {
                by_id.entry(id).or_default().push(stored);
            }
        }
    }
    Ok(by_id)
}

// ---------------------------------------------------------------------------
// The replay
// ---------------------------------------------------------------------------

/// One row accepted from the file.
struct Accepted {
    day: i64,
    direction: TradeDirection,
    quantity: f64,
    price: f64,
}

/// Everything known about one ticker while the file is replayed.
struct TickerState {
    position: Option<Position>,
    /// The transactions the ticker has in the database.
    stored: Vec<Stored>,
    /// Currency of the position: the existing one, else the first accepted
    /// row's.
    currency: Option<String>,
    /// Net quantity of the rows accepted from the file so far.
    accepted_net: f64,
    accepted: Vec<Accepted>,
}

impl TickerState {
    fn load(conn: &Connection, ticker: &str) -> Result<Self> {
        let position = find_position(conn, ticker)?;
        let stored = match &position {
            Some(position) => load_position_rows(conn, &position.id)?,
            None => Vec::new(),
        };
        Ok(TickerState {
            currency: position.as_ref().map(|p| p.currency.clone()),
            position,
            stored,
            accepted_net: 0.0,
            accepted: Vec::new(),
        })
    }

    /// Quantity held at the end of `day`: the stored transactions up to it
    /// plus the rows accepted so far (the replay is in date order, so they
    /// are all on or before `day`).
    fn held_at(&self, day: i64) -> f64 {
        let stored: f64 = self
            .stored
            .iter()
            .filter(|s| s.day <= day)
            .map(Stored::signed_quantity)
            .sum();
        (stored + self.accepted_net).max(0.0)
    }

    fn accept(&mut self, trade: &ParsedTrade, currency: &str) {
        self.accepted_net += match trade.direction {
            TradeDirection::Buy => trade.quantity,
            TradeDirection::Sell => -trade.quantity,
        };
        self.accepted.push(Accepted {
            day: trade.day,
            direction: trade.direction,
            quantity: trade.quantity,
            price: trade.price,
        });
        if self.currency.is_none() {
            self.currency = Some(currency.to_string());
        }
    }
}

/// What the instrument-level overrides and the file say about one instrument.
struct InstrumentPlan {
    key: String,
    symbol: Option<String>,
    isin: Option<String>,
    file_name: Option<String>,
    trade_count: usize,
    ticker: Option<String>,
    /// A symbol that cannot be a ticker (the instrument then has none).
    invalid_symbol: Option<String>,
    skipped: bool,
    override_name: Option<String>,
    /// As typed (trimmed): `GBp` and `GBP` differ only by case.
    override_currency: Option<String>,
    position: Option<Position>,
    /// Currency of the first trade that has one.
    currency: Option<String>,
}

/// Per-trade facts that do not depend on the replay.
struct TradePlan {
    instrument: usize,
    ticker: Option<String>,
    currency: Option<String>,
    /// The price as it is stored, in `currency` (pence converted).
    price: f64,
    broker_id: Option<String>,
    external_id: Option<String>,
    company_name: Option<String>,
}

struct Run<'a> {
    config: &'a StockImportConfig,
    /// Stored transactions by broker id (`<source>:<id>`).
    by_external: HashMap<String, Vec<Stored>>,
    states: HashMap<String, TickerState>,
    /// Broker ids of the rows accepted from the file.
    seen_ids: HashSet<String>,
    /// A broker id identifies one trade. Degiro's order id is shared by the
    /// fills of an order.
    ids_are_unique: bool,
}

impl Run<'_> {
    fn duplicate_of(
        &self,
        state: &TickerState,
        trade: &ParsedTrade,
        external_id: Option<&str>,
    ) -> Option<DuplicateKind> {
        if let Some(id) = external_id {
            let stored_by_id = self.by_external.get(id).is_some_and(|rows| {
                rows.iter()
                    .any(|s| self.ids_are_unique || s.same_quantity_and_price(trade))
            });
            if stored_by_id || (self.ids_are_unique && self.seen_ids.contains(id)) {
                return Some(DuplicateKind::BrokerId);
            }
        }
        // Rows with a broker id are told apart by it: a stored transaction with
        // an id of the same source is another trade. One without an id, or with
        // an id of another source (an import before the layout was saved as a
        // format, a custom mapping), can only be compared by its values.
        let source_prefix = format!("{}:", self.config.source);
        let stored_alike = state.stored.iter().any(|s| {
            s.same_trade(trade)
                && (external_id.is_none()
                    || !s
                        .external_id
                        .as_deref()
                        .is_some_and(|id| id.starts_with(&source_prefix)))
        });
        let repeated_in_file = external_id.is_none()
            && state.accepted.iter().any(|a| {
                a.day == trade.day
                    && a.direction == trade.direction
                    && close(a.quantity, trade.quantity, QUANTITY_TOLERANCE)
                    && close(a.price, trade.price, PRICE_TOLERANCE)
            });
        (stored_alike || repeated_in_file).then_some(DuplicateKind::Identical)
    }

    fn evaluate(
        &mut self,
        trade: &ParsedTrade,
        plan: &TradePlan,
        instrument: &InstrumentPlan,
    ) -> TradeOutcome {
        let line = trade.line;
        if instrument.skipped {
            return TradeOutcome::Skipped(message(line, KEY_INSTRUMENT_SKIPPED, None));
        }
        let Some(ticker) = plan.ticker.as_deref() else {
            return TradeOutcome::Error(match &instrument.invalid_symbol {
                Some(symbol) => message(line, KEY_TICKER_INVALID, Some(symbol.clone())),
                None => message(line, KEY_SYMBOL_MISSING, None),
            });
        };
        let Some(currency) = plan.currency.as_deref() else {
            return TradeOutcome::Error(message(line, KEY_CURRENCY_MISSING, None));
        };
        let Some(state) = self.states.get(ticker) else {
            // Every ticker is loaded when the instruments are planned.
            return TradeOutcome::Error(message(line, KEY_SYMBOL_MISSING, None));
        };

        if let Some(position_currency) = &state.currency {
            if position_currency != currency {
                return TradeOutcome::Error(message(
                    line,
                    KEY_CURRENCY_MISMATCH,
                    Some(position_currency.clone()),
                ));
            }
        }

        let duplicate = self.duplicate_of(state, trade, plan.external_id.as_deref());
        if let Some(kind) = duplicate {
            if !self.config.import_anyway_lines.contains(&line) {
                return TradeOutcome::Duplicate {
                    kind,
                    forced: false,
                };
            }
        }

        if trade.direction == TradeDirection::Sell {
            let held = state.held_at(trade.day);
            if trade.quantity > held + 1e-9 * held.max(trade.quantity).max(1.0) {
                return TradeOutcome::Error(message(
                    line,
                    KEY_SELL_EXCEEDS_HOLDINGS,
                    Some(rounded_quantity(held)),
                ));
            }
        }

        if let Some(state) = self.states.get_mut(ticker) {
            state.accept(trade, currency);
        }
        if let Some(id) = &plan.external_id {
            self.seen_ids.insert(id.clone());
        }
        match duplicate {
            Some(kind) => TradeOutcome::Duplicate { kind, forced: true },
            None => TradeOutcome::New,
        }
    }
}

fn plan_instruments(
    trades: &[ParsedTrade],
    config: &StockImportConfig,
) -> (Vec<InstrumentPlan>, Vec<usize>) {
    let mut plans: Vec<InstrumentPlan> = Vec::new();
    let mut index_of: HashMap<&str, usize> = HashMap::new();
    let mut instrument_of_trade = Vec::with_capacity(trades.len());
    for trade in trades {
        let index = *index_of
            .entry(trade.instrument_key.as_str())
            .or_insert_with(|| {
                plans.push(InstrumentPlan {
                    key: trade.instrument_key.clone(),
                    symbol: None,
                    isin: None,
                    file_name: None,
                    trade_count: 0,
                    ticker: None,
                    invalid_symbol: None,
                    skipped: false,
                    override_name: None,
                    override_currency: None,
                    position: None,
                    currency: None,
                });
                plans.len() - 1
            });
        let plan = &mut plans[index];
        plan.trade_count += 1;
        if plan.symbol.is_none() {
            plan.symbol = non_empty(trade.symbol.as_deref()).map(|s| s.to_uppercase());
        }
        if plan.isin.is_none() {
            plan.isin = non_empty(trade.isin.as_deref()).map(|s| s.to_uppercase());
        }
        if plan.file_name.is_none() {
            plan.file_name = non_empty(trade.name.as_deref());
        }
        instrument_of_trade.push(index);
    }

    for plan in &mut plans {
        let over = config.override_for(&plan.key);
        plan.skipped = over.is_some_and(|o| o.skip);
        plan.override_name = over.and_then(|o| non_empty(o.name.as_deref()));
        plan.override_currency = over
            .and_then(|o| o.currency.as_deref())
            .and_then(currency_code)
            .map(str::to_string);
        let wanted = over
            .and_then(|o| non_empty(o.ticker.as_deref()))
            .map(|t| t.to_uppercase())
            .or_else(|| plan.symbol.clone());
        match wanted {
            Some(ticker) if is_valid_ticker(&ticker) => plan.ticker = Some(ticker),
            Some(invalid) => plan.invalid_symbol = Some(invalid),
            None => {}
        }
    }
    (plans, instrument_of_trade)
}

/// A trade's currency and its price in that currency, as they are stored.
fn priced(
    trade: &ParsedTrade,
    plan: &InstrumentPlan,
    config: &StockImportConfig,
) -> (Option<String>, f64) {
    // What the file states is a fact: an override must not relabel prices it does not convert.
    if let Some(own) = trade.currency.as_deref().and_then(normalized_currency) {
        return (Some(own), trade.price);
    }
    let listing = plan.override_currency.clone().or_else(|| {
        (config.currency_mode == CurrencyMode::Instrument)
            .then(|| {
                plan.ticker
                    .as_deref()
                    .map(|t| price_api::get_currency_from_ticker(t).to_string())
            })
            .flatten()
    });
    match listing {
        // The prices follow the exchange's quote unit: pence, stored as pounds.
        Some(code) if is_pence(&code) => (Some("GBP".to_string()), pence_to_pounds(trade.price)),
        Some(code) => (normalized_currency(&code), trade.price),
        None => (None, trade.price),
    }
}

/// Replay `parsed` against the database: apply the instrument overrides, then
/// decide every trade (see the module docs). Reads only.
pub fn simulate(
    conn: &Connection,
    parsed: &ParsedFile,
    config: &StockImportConfig,
) -> Result<Simulation> {
    let trades = &parsed.trades;
    let (mut plans, instrument_of_trade) = plan_instruments(trades, config);

    let mut states: HashMap<String, TickerState> = HashMap::new();
    for plan in &plans {
        if let Some(ticker) = &plan.ticker {
            if !states.contains_key(ticker) {
                states.insert(ticker.clone(), TickerState::load(conn, ticker)?);
            }
        }
    }
    for plan in &mut plans {
        plan.position = plan
            .ticker
            .as_ref()
            .and_then(|t| states.get(t))
            .and_then(|state| state.position.clone());
    }

    // Per-trade facts: ticker, currency, stored broker id, name.
    let trade_plans: Vec<TradePlan> = trades
        .iter()
        .zip(&instrument_of_trade)
        .map(|(trade, &instrument)| {
            let plan = &plans[instrument];
            let (currency, price) = priced(trade, plan, config);
            let broker_id = non_empty(trade.external_id.as_deref());
            TradePlan {
                instrument,
                ticker: plan.ticker.clone(),
                currency,
                price,
                external_id: broker_id
                    .as_ref()
                    .map(|id| format!("{}:{}", config.source, id)),
                broker_id,
                company_name: match &plan.position {
                    Some(position) => Some(position.company_name.clone()),
                    None => plan
                        .override_name
                        .clone()
                        .or_else(|| non_empty(trade.name.as_deref())),
                },
            }
        })
        .collect();
    for (trade_plan, plan_index) in trade_plans.iter().zip(&instrument_of_trade) {
        let plan = &mut plans[*plan_index];
        if plan.currency.is_none() {
            plan.currency = trade_plan.currency.clone();
        }
    }
    // From here on every rule sees the price as it is stored: pence converted to pounds, so
    // duplicates are compared against the stored pound rows.
    let priced_trades: Vec<ParsedTrade> = trades
        .iter()
        .zip(&trade_plans)
        .map(|(trade, plan)| ParsedTrade {
            price: plan.price,
            ..trade.clone()
        })
        .collect();
    let trades = &priced_trades;

    let mut ids: Vec<String> = trade_plans
        .iter()
        .filter_map(|p| p.external_id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    let mut run = Run {
        config,
        by_external: load_rows_by_external_id(conn, &ids)?,
        states,
        seen_ids: HashSet::new(),
        // A remembered Degiro mapping has a source of its own: the flag travels with it.
        ids_are_unique: !config.transforms.broker_id_per_order && config.source != SOURCE_DEGIRO,
    };

    let mut order: Vec<usize> = (0..trades.len()).collect();
    order.sort_by_key(|&i| (trades[i].day, direction_rank(trades[i].direction), i));
    let mut decided: Vec<(usize, TradeOutcome)> = order
        .into_iter()
        .map(|i| {
            let plan = &trade_plans[i];
            (i, run.evaluate(&trades[i], plan, &plans[plan.instrument]))
        })
        .collect();
    decided.sort_by_key(|(i, _)| *i);

    let simulated = trades
        .iter()
        .zip(trade_plans)
        .zip(decided)
        .map(|((trade, plan), (_, outcome))| SimulatedTrade {
            line: trade.line,
            day: trade.day,
            direction: trade.direction,
            instrument_key: trade.instrument_key.clone(),
            ticker: plan.ticker,
            company_name: plan.company_name,
            quantity: trade.quantity,
            price: trade.price,
            currency: plan.currency,
            broker_id: plan.broker_id,
            external_id: plan.external_id,
            outcome,
        })
        .collect();

    let instruments = plans.into_iter().map(into_instrument).collect();
    Ok(Simulation {
        trades: simulated,
        instruments,
    })
}

fn into_instrument(plan: InstrumentPlan) -> StockImportInstrument {
    let status = if plan.skipped {
        StockInstrumentStatus::Skipped
    } else if plan.ticker.is_none() {
        StockInstrumentStatus::MissingSymbol
    } else if plan.position.is_some() {
        StockInstrumentStatus::Existing
    } else {
        StockInstrumentStatus::New
    };
    let position_currency = match (&plan.position, &plan.currency) {
        (Some(position), Some(currency)) if &position.currency != currency => {
            Some(position.currency.clone())
        }
        _ => None,
    };
    StockImportInstrument {
        name: plan
            .override_name
            .or(plan.file_name)
            .or_else(|| plan.position.as_ref().map(|p| p.company_name.clone())),
        key: plan.key,
        symbol: plan.symbol,
        isin: plan.isin,
        currency: plan.currency,
        trade_count: plan.trade_count,
        ticker: plan.ticker,
        status,
        position_currency,
    }
}

/// Shared fixtures of the stock import tests: a minimal schema and builders
/// for parsed trades.
#[cfg(test)]
pub(crate) mod test_db {
    use rusqlite::Connection;

    use super::super::types::{
        CurrencyMode, DirectionMode, ParsedFile, ParsedTrade, StockImportConfig,
        StockImportTransforms, StockInstrumentOverride, TradeDirection,
    };

    /// 2024-01-01, UTC midnight.
    pub const DAY0: i64 = 1_704_067_200;

    /// UTC day number `n` after [`DAY0`] (negative: before).
    pub fn day(n: i64) -> i64 {
        DAY0 + n * 86_400
    }

    pub fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE stock_import_batches (
                id TEXT PRIMARY KEY,
                file_name TEXT NOT NULL,
                source TEXT NOT NULL,
                trade_count INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                import_batch_id TEXT REFERENCES stock_import_batches(id) ON DELETE SET NULL,
                external_id TEXT
            );
            CREATE TABLE stock_investment_tags (
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                tag_id TEXT NOT NULL,
                PRIMARY KEY (investment_id, tag_id)
            );
            CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            "#,
        )
        .expect("schema");
        conn
    }

    /// An existing position; returns its id.
    pub fn add_position(conn: &Connection, ticker: &str, name: &str, currency: &str) -> String {
        let id = format!("pos-{ticker}");
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
             VALUES (?1, ?2, ?3, '0', ?4)",
            rusqlite::params![id, ticker, name, currency],
        )
        .expect("position");
        id
    }

    /// A stored transaction of `ticker`'s position (created when missing,
    /// USD); returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn add_stored(
        conn: &Connection,
        ticker: &str,
        kind: &str,
        quantity: &str,
        price: &str,
        day: i64,
        external_id: Option<&str>,
    ) -> String {
        let position: String = conn
            .query_row(
                "SELECT id FROM stock_investments WHERE ticker = ?1",
                [ticker],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| add_position(conn, ticker, ticker, "USD"));
        let id = format!("tx-{}", next_number(conn));
        conn.execute(
            "INSERT INTO investment_transactions
                 (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, external_id)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, 'USD', ?7, ?8)",
            rusqlite::params![id, position, kind, ticker, quantity, price, day, external_id],
        )
        .expect("stored transaction");
        id
    }

    fn next_number(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) + 1 FROM investment_transactions",
            [],
            |r| r.get(0),
        )
        .expect("count")
    }

    /// A parsed trade of `symbol` (key `symbol:<SYMBOL>`), priced in `currency`.
    pub fn trade(
        line: usize,
        day: i64,
        direction: TradeDirection,
        symbol: &str,
        quantity: f64,
        price: f64,
        currency: &str,
    ) -> ParsedTrade {
        ParsedTrade {
            line,
            day,
            direction,
            instrument_key: format!("symbol:{symbol}"),
            symbol: Some(symbol.to_string()),
            isin: None,
            name: None,
            quantity,
            price,
            currency: Some(currency.to_string()),
            external_id: None,
        }
    }

    pub fn buy(line: usize, day: i64, symbol: &str, quantity: f64, price: f64) -> ParsedTrade {
        trade(
            line,
            day,
            TradeDirection::Buy,
            symbol,
            quantity,
            price,
            "USD",
        )
    }

    pub fn sell(line: usize, day: i64, symbol: &str, quantity: f64, price: f64) -> ParsedTrade {
        trade(
            line,
            day,
            TradeDirection::Sell,
            symbol,
            quantity,
            price,
            "USD",
        )
    }

    pub fn with_id(mut trade: ParsedTrade, id: &str) -> ParsedTrade {
        trade.external_id = Some(id.to_string());
        trade
    }

    pub fn parsed(trades: Vec<ParsedTrade>) -> ParsedFile {
        ParsedFile {
            total_rows: trades.len(),
            trades,
            ..ParsedFile::default()
        }
    }

    /// A complete, valid configuration for `source` (a currency column, type
    /// column, no overrides).
    pub fn config(source: &str) -> StockImportConfig {
        StockImportConfig {
            source: source.to_string(),
            delimiter: ",".into(),
            encoding: "UTF-8".into(),
            header_row: 0,
            skip_rows: 0,
            date_column: 0,
            date_format: "%Y-%m-%d".into(),
            symbol_column: Some(1),
            isin_column: None,
            name_column: None,
            quantity_column: 2,
            price_column: 3,
            currency_mode: CurrencyMode::Column,
            currency_column: Some(4),
            fixed_currency: None,
            direction_mode: DirectionMode::QuantitySign,
            type_column: None,
            type_values: vec![],
            decimal_separator: ".".into(),
            external_id_column: None,
            transforms: StockImportTransforms::default(),
            instrument_overrides: vec![],
            import_anyway_lines: vec![],
        }
    }

    pub fn over(key: &str) -> StockInstrumentOverride {
        StockInstrumentOverride {
            key: key.to_string(),
            ticker: None,
            name: None,
            currency: None,
            skip: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        CurrencyMode, ParsedTrade, StockInstrumentOverride, StockInstrumentStatus, SOURCE_DEGIRO,
    };
    use super::test_db::*;
    use super::*;

    fn run(
        conn: &rusqlite::Connection,
        trades: Vec<ParsedTrade>,
        config: &StockImportConfig,
    ) -> Simulation {
        simulate(conn, &parsed(trades), config).expect("simulation")
    }

    fn outcomes(sim: &Simulation) -> Vec<TradeOutcome> {
        sim.trades.iter().map(|t| t.outcome.clone()).collect()
    }

    fn error_key(outcome: &TradeOutcome) -> (&str, Option<&str>) {
        match outcome {
            TradeOutcome::Error(m) => (m.key.as_str(), m.detail.as_deref()),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn duplicate(kind: DuplicateKind) -> TradeOutcome {
        TradeOutcome::Duplicate {
            kind,
            forced: false,
        }
    }

    // ---- instruments, tickers, names, currencies ---------------------------

    #[test]
    fn a_new_instrument_is_stored_under_its_file_symbol() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                buy(3, day(1), "AAPL", 5.0, 110.0),
            ],
            &config("custom"),
        );

        assert_eq!(outcomes(&sim), vec![TradeOutcome::New, TradeOutcome::New]);
        assert_eq!(sim.trades[0].ticker.as_deref(), Some("AAPL"));
        assert_eq!(sim.trades[0].currency.as_deref(), Some("USD"));
        assert_eq!(sim.instruments.len(), 1);
        let aapl = &sim.instruments[0];
        assert_eq!(aapl.key, "symbol:AAPL");
        assert_eq!(aapl.status, StockInstrumentStatus::New);
        assert_eq!(aapl.trade_count, 2);
        assert_eq!(aapl.ticker.as_deref(), Some("AAPL"));
        assert_eq!(aapl.currency.as_deref(), Some("USD"));
        assert_eq!(aapl.position_currency, None);
    }

    #[test]
    fn instruments_are_listed_in_order_of_first_appearance() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "MSFT", 1.0, 1.0),
                buy(3, day(1), "AAPL", 1.0, 1.0),
                buy(4, day(2), "MSFT", 1.0, 1.0),
            ],
            &config("custom"),
        );
        let keys: Vec<&str> = sim.instruments.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, vec!["symbol:MSFT", "symbol:AAPL"]);
        assert_eq!(sim.instruments[0].trade_count, 2);
    }

    #[test]
    fn overrides_replace_ticker_and_name_but_not_the_currency_a_file_states() {
        let conn = db();
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            key: "symbol:VUSA".into(),
            ticker: Some(" vusa.l ".into()),
            name: Some("Vanguard S&P 500".into()),
            currency: Some("gbp".into()),
            skip: false,
        }];
        let sim = run(
            &conn,
            vec![trade(
                2,
                day(0),
                TradeDirection::Buy,
                "VUSA",
                3.0,
                70.0,
                "EUR",
            )],
            &cfg,
        );

        let t = &sim.trades[0];
        assert_eq!(t.outcome, TradeOutcome::New);
        assert_eq!(t.ticker.as_deref(), Some("VUSA.L"));
        assert_eq!(t.company_name.as_deref(), Some("Vanguard S&P 500"));
        assert_eq!(
            t.currency.as_deref(),
            Some("EUR"),
            "the file's own currency is a fact: relabelling its prices would convert nothing"
        );
        let i = &sim.instruments[0];
        assert_eq!(i.ticker.as_deref(), Some("VUSA.L"));
        assert_eq!(i.name.as_deref(), Some("Vanguard S&P 500"));
        assert_eq!(i.currency.as_deref(), Some("EUR"));
        assert_eq!(
            i.symbol.as_deref(),
            Some("VUSA"),
            "the symbol stays as the file has it"
        );
    }

    #[test]
    fn a_skipped_instrument_excludes_all_its_trades_only() {
        let conn = db();
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            skip: true,
            ..over("symbol:OPT")
        }];
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "OPT", 1.0, 1.0),
                buy(3, day(0), "AAPL", 1.0, 1.0),
                sell(4, day(1), "OPT", 1.0, 1.0),
            ],
            &cfg,
        );

        assert!(
            matches!(&sim.trades[0].outcome, TradeOutcome::Skipped(m) if m.key == KEY_INSTRUMENT_SKIPPED && m.line == 2)
        );
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
        assert!(matches!(sim.trades[2].outcome, TradeOutcome::Skipped(_)));
        assert_eq!(sim.instruments[0].status, StockInstrumentStatus::Skipped);
        assert_eq!(sim.instruments[1].status, StockInstrumentStatus::New);
        assert!(!sim.trades[0].will_import() && sim.trades[1].will_import());
    }

    #[test]
    fn an_existing_position_is_matched_by_symbol_and_keeps_its_name() {
        let conn = db();
        add_position(&conn, "AAPL", "Apple Inc.", "USD");
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            name: Some("Ignored".into()),
            ..over("symbol:AAPL")
        }];
        let sim = run(&conn, vec![buy(2, day(0), "AAPL", 1.0, 1.0)], &cfg);

        assert_eq!(sim.instruments[0].status, StockInstrumentStatus::Existing);
        assert_eq!(sim.instruments[0].ticker.as_deref(), Some("AAPL"));
        assert_eq!(
            sim.trades[0].company_name.as_deref(),
            Some("Apple Inc."),
            "rows added to a position carry its name"
        );
    }

    #[test]
    fn the_file_name_names_a_new_position_when_nothing_overrides_it() {
        let conn = db();
        let mut t = buy(2, day(0), "AAPL", 1.0, 1.0);
        t.name = Some("  Apple Inc  ".into());
        let sim = run(&conn, vec![t], &config("custom"));
        assert_eq!(sim.trades[0].company_name.as_deref(), Some("Apple Inc"));
        assert_eq!(sim.instruments[0].name.as_deref(), Some("Apple Inc"));
    }

    #[test]
    fn instrument_currency_mode_uses_the_override_else_the_listing_currency() {
        let conn = db();
        let mut cfg = config("xtb");
        cfg.currency_mode = CurrencyMode::Instrument;
        cfg.currency_column = None;
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            currency: Some("CHF".into()),
            ..over("symbol:NOVN")
        }];
        let mut trades = vec![
            buy(2, day(0), "VUSA.L", 1.0, 1.0),
            buy(3, day(0), "AAPL", 1.0, 1.0),
            buy(4, day(0), "SPYL.DE", 1.0, 1.0),
            buy(5, day(0), "NOVN", 1.0, 1.0),
        ];
        for t in &mut trades {
            t.currency = None;
        }
        let sim = run(&conn, trades, &cfg);

        let currencies: Vec<&str> = sim
            .trades
            .iter()
            .map(|t| t.currency.as_deref().unwrap_or("-"))
            .collect();
        assert_eq!(currencies, vec!["GBP", "USD", "EUR", "CHF"]);
        assert!(sim.trades.iter().all(|t| t.outcome == TradeOutcome::New));
    }

    // ---- the file's currency, the listing's currency and pence ------------------

    fn instrument_mode() -> StockImportConfig {
        let mut cfg = config("xtb");
        cfg.currency_mode = CurrencyMode::Instrument;
        cfg.currency_column = None;
        cfg
    }

    /// A trade as an Instrument-mode parse returns it: the file names no currency.
    fn unpriced(mut trade: ParsedTrade) -> ParsedTrade {
        trade.currency = None;
        trade
    }

    fn with_currency_override(key: &str, currency: &str) -> Vec<StockInstrumentOverride> {
        vec![StockInstrumentOverride {
            currency: Some(currency.into()),
            ..over(key)
        }]
    }

    #[test]
    fn the_currency_a_file_states_wins_in_every_mode_that_has_one() {
        // Column and Fixed: the cell, or the one currency of the file, is a fact.
        for mode in [CurrencyMode::Column, CurrencyMode::Fixed] {
            let conn = db();
            let mut cfg = config("custom");
            cfg.currency_mode = mode;
            cfg.instrument_overrides = with_currency_override("symbol:VUSA", "gbp");
            let sim = run(
                &conn,
                vec![trade(
                    2,
                    day(0),
                    TradeDirection::Buy,
                    "VUSA",
                    3.0,
                    70.0,
                    "EUR",
                )],
                &cfg,
            );
            assert_eq!(sim.trades[0].currency.as_deref(), Some("EUR"), "{mode:?}");
            assert_eq!(sim.trades[0].price, 70.0, "{mode:?}");
            assert_eq!(sim.instruments[0].currency.as_deref(), Some("EUR"));
            assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        }
    }

    #[test]
    fn an_override_cannot_turn_a_price_the_file_states_into_pence() {
        // A Trading 212 pence cell reaches the simulation as GBP already divided by 100: an
        // override of GBX on top of it converts nothing a second time.
        let conn = db();
        let mut cfg = config("trading212");
        cfg.instrument_overrides = with_currency_override("symbol:VOD.L", "GBX");
        let sim = run(
            &conn,
            vec![trade(
                2,
                day(0),
                TradeDirection::Buy,
                "VOD.L",
                100.0,
                0.6852,
                "GBP",
            )],
            &cfg,
        );
        let t = &sim.trades[0];
        assert_eq!((t.price, t.currency.as_deref()), (0.6852, Some("GBP")));
        assert_eq!(t.price_text(), "0.6852");
    }

    #[test]
    fn in_instrument_mode_the_override_applies_before_the_listing_guess() {
        let conn = db();
        let mut cfg = instrument_mode();
        cfg.instrument_overrides = with_currency_override("symbol:VUSD.L", "usd");
        let sim = run(
            &conn,
            vec![
                unpriced(buy(2, day(0), "VUSD.L", 1.0, 100.0)),
                unpriced(buy(3, day(0), "VUSA.L", 1.0, 100.0)),
            ],
            &cfg,
        );
        let currencies: Vec<&str> = sim
            .trades
            .iter()
            .map(|t| t.currency.as_deref().unwrap_or("-"))
            .collect();
        assert_eq!(
            currencies,
            vec!["USD", "GBP"],
            "the .L guess only where nothing says better"
        );
    }

    #[test]
    fn a_gbx_override_makes_the_prices_pence_and_stores_them_as_pounds() {
        for code in ["GBX", "gbx", "GBx", "GBp", " GBX "] {
            let conn = db();
            let mut cfg = instrument_mode();
            cfg.instrument_overrides = with_currency_override("symbol:BARC.L", code);
            let sim = run(
                &conn,
                vec![
                    unpriced(buy(2, day(0), "BARC.L", 10.0, 443.65)),
                    unpriced(sell(3, day(1), "BARC.L", 4.0, 4912.35)),
                ],
                &cfg,
            );

            assert_eq!(
                outcomes(&sim),
                vec![TradeOutcome::New, TradeOutcome::New],
                "{code}"
            );
            for t in &sim.trades {
                assert_eq!(t.currency.as_deref(), Some("GBP"), "{code}");
            }
            assert_eq!(sim.trades[0].price, 4.4365, "{code}");
            assert_eq!(sim.trades[0].price_text(), "4.4365", "{code}");
            assert_eq!(sim.trades[1].price_text(), "49.1235", "{code}");
            assert_eq!(
                sim.instruments[0].currency.as_deref(),
                Some("GBP"),
                "the instrument is held in pounds"
            );
        }
    }

    #[test]
    fn a_pound_override_leaves_the_prices_alone() {
        for code in ["GBP", "gbp"] {
            let conn = db();
            let mut cfg = instrument_mode();
            cfg.instrument_overrides = with_currency_override("symbol:VUSA.L", code);
            let sim = run(
                &conn,
                vec![unpriced(buy(2, day(0), "VUSA.L", 1.0, 74.12))],
                &cfg,
            );
            assert_eq!(sim.trades[0].price, 74.12, "{code}");
            assert_eq!(sim.trades[0].currency.as_deref(), Some("GBP"));
        }
    }

    #[test]
    fn pence_are_converted_before_the_currency_of_the_position_is_compared() {
        let conn = db();
        add_position(&conn, "BARC.L", "Barclays", "GBP");
        let mut cfg = instrument_mode();
        cfg.instrument_overrides = with_currency_override("symbol:BARC.L", "GBX");
        let sim = run(
            &conn,
            vec![unpriced(buy(2, day(0), "BARC.L", 10.0, 443.65))],
            &cfg,
        );
        assert_eq!(
            sim.trades[0].outcome,
            TradeOutcome::New,
            "GBX is not GBP's mismatch"
        );
        assert_eq!(sim.instruments[0].position_currency, None);

        // A position in another currency still mismatches, against the stored currency GBP.
        add_position(&conn, "LLOY.L", "Lloyds", "EUR");
        cfg.instrument_overrides = with_currency_override("symbol:LLOY.L", "GBX");
        let sim = run(
            &conn,
            vec![unpriced(buy(2, day(0), "LLOY.L", 1.0, 104.0))],
            &cfg,
        );
        assert_eq!(
            error_key(&sim.trades[0].outcome),
            (KEY_CURRENCY_MISMATCH, Some("EUR"))
        );
        assert_eq!(sim.instruments[0].currency.as_deref(), Some("GBP"));
    }

    #[test]
    fn a_pence_file_imported_again_finds_its_duplicates_against_the_stored_pound_rows() {
        let conn = db();
        // What the first import wrote: the prices in pounds.
        add_position(&conn, "BARC.L", "Barclays", "GBP");
        add_stored(&conn, "BARC.L", "buy", "10", "4.4365", day(0), None);
        add_stored(&conn, "BARC.L", "sell", "4", "49.1235", day(1), None);
        let mut cfg = instrument_mode();
        cfg.instrument_overrides = with_currency_override("symbol:BARC.L", "GBX");

        let sim = run(
            &conn,
            vec![
                unpriced(buy(2, day(0), "BARC.L", 10.0, 443.65)), // 4.4365 pounds, stored
                unpriced(sell(3, day(1), "BARC.L", 4.0, 4912.35)), // 49.1235 pounds, stored
                unpriced(buy(4, day(2), "BARC.L", 1.0, 450.0)),   // new
            ],
            &cfg,
        );

        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[1].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[2].outcome, TradeOutcome::New);
        assert!(!sim.trades[0].will_import() && sim.trades[2].will_import());
    }

    #[test]
    fn the_same_file_in_pence_and_without_the_override_is_not_a_duplicate() {
        // Without the override the 443.65 stays pounds: another price, another trade.
        let conn = db();
        add_position(&conn, "BARC.L", "Barclays", "GBP");
        add_stored(&conn, "BARC.L", "buy", "10", "4.4365", day(0), None);
        let sim = run(
            &conn,
            vec![unpriced(buy(2, day(0), "BARC.L", 10.0, 443.65))],
            &instrument_mode(),
        );
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(sim.trades[0].price, 443.65);
    }

    // ---- duplicates --------------------------------------------------------

    #[test]
    fn an_identical_stored_trade_is_a_duplicate_compared_numerically() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10.0", "100.00", day(0), None);
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                buy(3, day(1), "AAPL", 10.0, 100.0), // another day
                sell(4, day(0), "AAPL", 10.0, 100.0), // another direction
                buy(5, day(0), "AAPL", 10.0, 100.5), // another price
            ],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
        assert_eq!(
            sim.trades[2].outcome,
            TradeOutcome::New,
            "a sell is no duplicate of a buy"
        );
        assert_eq!(sim.trades[3].outcome, TradeOutcome::New);
        assert!(!sim.trades[0].will_import());
    }

    #[test]
    fn quantity_and_price_tolerances_are_relative() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "1000000", "100", day(0), None);
        let sim = run(
            &conn,
            vec![
                buy(
                    2,
                    day(0),
                    "AAPL",
                    1_000_000.0 * (1.0 + 1e-10),
                    100.0 * (1.0 + 1e-7),
                ),
                buy(3, day(0), "AAPL", 1_000_000.0 * (1.0 + 1e-6), 100.0),
                buy(4, day(0), "AAPL", 1_000_000.0, 100.0 * (1.0 + 1e-4)),
            ],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(
            sim.trades[1].outcome,
            TradeOutcome::New,
            "quantity off by 1e-6"
        );
        assert_eq!(
            sim.trades[2].outcome,
            TradeOutcome::New,
            "price off by 1e-4"
        );
    }

    #[test]
    fn an_import_anyway_line_forces_a_duplicate_in() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let mut cfg = config("custom");
        cfg.import_anyway_lines = vec![2];
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                buy(3, day(0), "AAPL", 10.0, 100.0),
            ],
            &cfg,
        );
        assert_eq!(
            sim.trades[0].outcome,
            TradeOutcome::Duplicate {
                kind: DuplicateKind::Identical,
                forced: true
            }
        );
        assert!(sim.trades[0].will_import());
        assert_eq!(sim.trades[1].outcome, duplicate(DuplicateKind::Identical));
    }

    #[test]
    fn the_same_broker_id_is_a_duplicate_even_when_the_values_were_edited() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "99", day(0), Some("xtb:111"));
        let sim = run(
            &conn,
            vec![
                with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "111"),
                with_id(buy(3, day(0), "AAPL", 10.0, 100.0), "112"),
            ],
            &config("xtb"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::BrokerId));
        assert_eq!(sim.trades[0].external_id.as_deref(), Some("xtb:111"));
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
    }

    #[test]
    fn a_broker_id_of_another_source_is_not_the_same_id() {
        let conn = db();
        add_stored(
            &conn,
            "AAPL",
            "buy",
            "10",
            "99",
            day(0),
            Some("trading212:111"),
        );
        let sim = run(
            &conn,
            vec![with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "111")],
            &config("xtb"),
        );
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
    }

    #[test]
    fn broker_ids_are_looked_up_across_tickers() {
        let conn = db();
        add_stored(&conn, "OLD", "buy", "10", "100", day(0), Some("xtb:5"));
        let sim = run(
            &conn,
            vec![with_id(buy(2, day(0), "NEW", 10.0, 100.0), "5")],
            &config("xtb"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::BrokerId));
    }

    #[test]
    fn broker_ids_are_looked_up_in_chunks_without_missing_any() {
        let conn = db();
        // More ids than one query carries: the first 700 exist already.
        let stored = 700;
        let file_rows = 1_300;
        for i in 0..stored {
            add_stored(
                &conn,
                "AAPL",
                "buy",
                "1",
                "1",
                day(i),
                Some(&format!("xtb:{i}")),
            );
        }
        let trades: Vec<ParsedTrade> = (0..file_rows)
            .map(|i| {
                with_id(
                    buy(i as usize + 2, day(i), "AAPL", 1.0, 1.0),
                    &i.to_string(),
                )
            })
            .collect();

        let sim = run(&conn, trades, &config("xtb"));

        let duplicates = sim
            .trades
            .iter()
            .filter(|t| t.outcome == duplicate(DuplicateKind::BrokerId))
            .count();
        let new = sim
            .trades
            .iter()
            .filter(|t| t.outcome == TradeOutcome::New)
            .count();
        assert_eq!(
            (duplicates, new),
            (stored as usize, (file_rows - stored) as usize)
        );
        assert!(sim.trades[..stored as usize]
            .iter()
            .all(|t| t.outcome == duplicate(DuplicateKind::BrokerId)));
    }

    #[test]
    fn degiro_ids_also_need_the_same_quantity_and_price() {
        let conn = db();
        // One order, two fills: the order id is on both.
        add_stored(
            &conn,
            "AAPL",
            "buy",
            "40",
            "25.5",
            day(0),
            Some("degiro:O1"),
        );
        add_stored(
            &conn,
            "AAPL",
            "buy",
            "60",
            "25.5",
            day(0),
            Some("degiro:O1"),
        );
        let sim = run(
            &conn,
            vec![
                with_id(buy(2, day(0), "AAPL", 60.0, 25.5), "O1"),
                with_id(buy(3, day(0), "AAPL", 30.0, 25.5), "O1"),
                with_id(buy(4, day(0), "AAPL", 40.0, 26.0), "O1"),
            ],
            &config(SOURCE_DEGIRO),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::BrokerId));
        assert_eq!(
            sim.trades[1].outcome,
            TradeOutcome::New,
            "another fill of the order"
        );
        assert_eq!(
            sim.trades[2].outcome,
            TradeOutcome::New,
            "same id, another price"
        );
    }

    #[test]
    fn a_row_with_a_broker_id_only_matches_stored_rows_that_have_none() {
        let conn = db();
        // Same values as the file row, but the broker told us it is another trade.
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), Some("xtb:1"));
        let sim = run(
            &conn,
            vec![with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "2")],
            &config("xtb"),
        );
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);

        // An earlier import without ids is still caught.
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let sim = run(
            &conn,
            vec![with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "2")],
            &config("xtb"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
    }

    /// Ids of another source (a preset before the user saved its layout as a
    /// format, or a custom mapping) cannot be compared with the file's, so the
    /// values decide: re-importing the same statement under a saved format
    /// must not write every trade twice.
    #[test]
    fn a_stored_row_with_an_id_of_another_source_is_compared_by_its_values() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), Some("xtb:111"));
        add_stored(&conn, "AAPL", "buy", "5", "120", day(1), Some("xtb:112"));
        let sim = run(
            &conn,
            vec![
                with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "111"),
                with_id(buy(3, day(1), "AAPL", 5.0, 120.0), "112"),
                with_id(buy(4, day(2), "AAPL", 1.0, 130.0), "113"),
            ],
            &config("format:2b9c"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[1].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[2].outcome, TradeOutcome::New);
    }

    #[test]
    fn a_row_without_a_broker_id_matches_any_stored_row_with_the_same_values() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), Some("xtb:1"));
        let sim = run(
            &conn,
            vec![buy(2, day(0), "AAPL", 10.0, 100.0)],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
    }

    #[test]
    fn a_repeated_row_of_the_file_is_a_duplicate_when_it_has_no_broker_id() {
        let conn = db();
        let mut cfg = config("custom");
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                buy(3, day(0), "AAPL", 10.0, 100.0),
            ],
            &cfg,
        );
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(sim.trades[1].outcome, duplicate(DuplicateKind::Identical));

        // Forced, it is accepted and counts for what follows.
        cfg.import_anyway_lines = vec![3];
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                buy(3, day(0), "AAPL", 10.0, 100.0),
                sell(4, day(1), "AAPL", 20.0, 100.0),
            ],
            &cfg,
        );
        assert!(sim.trades[1].will_import());
        assert_eq!(
            sim.trades[2].outcome,
            TradeOutcome::New,
            "20 are held after both buys"
        );
    }

    #[test]
    fn rows_with_different_broker_ids_are_different_trades_even_when_alike() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                with_id(buy(2, day(0), "AAPL", 5.0, 100.0), "1"),
                with_id(buy(3, day(0), "AAPL", 5.0, 100.0), "2"),
            ],
            &config("xtb"),
        );
        assert_eq!(outcomes(&sim), vec![TradeOutcome::New, TradeOutcome::New]);
    }

    #[test]
    fn the_same_broker_id_twice_in_one_file_is_one_trade_but_not_for_degiro() {
        let conn = db();
        let rows = || {
            vec![
                with_id(buy(2, day(0), "AAPL", 5.0, 100.0), "7"),
                with_id(buy(3, day(0), "AAPL", 5.0, 100.0), "7"),
            ]
        };
        let sim = run(&conn, rows(), &config("trading212"));
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(sim.trades[1].outcome, duplicate(DuplicateKind::BrokerId));

        // Degiro's order id is shared by the fills of an order: equal fills are two trades.
        let sim = run(&conn, rows(), &config(SOURCE_DEGIRO));
        assert_eq!(outcomes(&sim), vec![TradeOutcome::New, TradeOutcome::New]);
    }

    /// A Degiro mapping the user edited and remembered imports under its own
    /// source (`format:<id>`); the rule that its ids are order ids travels with
    /// the mapping, so the second fill of an order is still a trade.
    #[test]
    fn a_remembered_degiro_mapping_keeps_order_ids_per_fill() {
        let conn = db();
        let mut cfg = config("format:3f2a");
        cfg.transforms.broker_id_per_order = true;
        let sim = run(
            &conn,
            vec![
                with_id(buy(2, day(0), "AAPL", 3.0, 81.20), "ord-1"),
                with_id(buy(3, day(0), "AAPL", 2.0, 81.25), "ord-1"),
            ],
            &cfg,
        );
        assert_eq!(outcomes(&sim), vec![TradeOutcome::New, TradeOutcome::New]);

        // Once imported, each fill is found again by its id, quantity and price.
        add_stored(
            &conn,
            "AAPL",
            "buy",
            "3",
            "81.20",
            day(0),
            Some("format:3f2a:ord-1"),
        );
        add_stored(
            &conn,
            "AAPL",
            "buy",
            "2",
            "81.25",
            day(0),
            Some("format:3f2a:ord-1"),
        );
        let sim = run(
            &conn,
            vec![
                with_id(buy(2, day(0), "AAPL", 3.0, 81.20), "ord-1"),
                with_id(buy(3, day(0), "AAPL", 2.0, 81.25), "ord-1"),
            ],
            &cfg,
        );
        assert_eq!(
            outcomes(&sim),
            vec![
                duplicate(DuplicateKind::BrokerId),
                duplicate(DuplicateKind::BrokerId)
            ]
        );
    }

    // ---- holdings ----------------------------------------------------------

    #[test]
    fn a_sell_above_the_holding_is_an_error_and_the_rest_continues_without_it() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                sell(3, day(1), "AAPL", 15.0, 110.0), // too much
                sell(4, day(2), "AAPL", 8.0, 110.0),  // fine: the bad sell took nothing
                sell(5, day(3), "AAPL", 3.0, 110.0),  // 2 left
            ],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(
            error_key(&sim.trades[1].outcome),
            (KEY_SELL_EXCEEDS_HOLDINGS, Some("10"))
        );
        assert_eq!(sim.trades[2].outcome, TradeOutcome::New);
        assert_eq!(
            error_key(&sim.trades[3].outcome),
            (KEY_SELL_EXCEEDS_HOLDINGS, Some("2"))
        );
        assert_eq!(sim.trades[1].line, 3, "the message names the file line");
        match &sim.trades[1].outcome {
            TradeOutcome::Error(m) => assert_eq!(m.line, 3),
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_held_quantity_in_a_message_has_no_float_noise() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 0.1, 1.0),
                buy(3, day(0), "AAPL", 0.2, 1.0),
                sell(4, day(1), "AAPL", 1.0, 1.0),
            ],
            &config("custom"),
        );
        assert_eq!(
            error_key(&sim.trades[2].outcome),
            (KEY_SELL_EXCEEDS_HOLDINGS, Some("0.3"))
        );
        assert_eq!(rounded_quantity(0.0), "0");
        assert_eq!(rounded_quantity(1000.0), "1000");
        assert_eq!(rounded_quantity(12.345_678_9), "12.3456789");
        assert_eq!(rounded_quantity(0.000_000_001), "0");
    }

    #[test]
    fn sells_are_checked_in_date_order_not_file_order() {
        let conn = db();
        // The sell comes first in the file but a day after the buy.
        let sim = run(
            &conn,
            vec![
                sell(2, day(1), "AAPL", 5.0, 1.0),
                buy(3, day(0), "AAPL", 10.0, 1.0),
            ],
            &config("custom"),
        );
        assert_eq!(outcomes(&sim), vec![TradeOutcome::New, TradeOutcome::New]);

        // A sell dated before the buy holds nothing yet.
        let sim = run(
            &conn,
            vec![
                buy(2, day(1), "AAPL", 10.0, 1.0),
                sell(3, day(0), "AAPL", 5.0, 1.0),
            ],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(
            error_key(&sim.trades[1].outcome),
            (KEY_SELL_EXCEEDS_HOLDINGS, Some("0"))
        );
    }

    #[test]
    fn buys_come_before_sells_within_a_day() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                sell(2, day(0), "AAPL", 10.0, 1.0),
                buy(3, day(0), "AAPL", 10.0, 1.0),
            ],
            &config("custom"),
        );
        assert_eq!(outcomes(&sim), vec![TradeOutcome::New, TradeOutcome::New]);
    }

    #[test]
    fn the_existing_holding_counts() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(-5), None);
        add_stored(&conn, "AAPL", "sell", "4", "100", day(-3), None);
        // 6 held. A stored buy dated after the sell day does not count yet.
        add_stored(&conn, "AAPL", "buy", "100", "100", day(5), None);
        let sim = run(
            &conn,
            vec![
                sell(2, day(0), "AAPL", 6.0, 1.0),
                sell(3, day(1), "AAPL", 0.5, 1.0),
            ],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(
            error_key(&sim.trades[1].outcome),
            (KEY_SELL_EXCEEDS_HOLDINGS, Some("0"))
        );
    }

    #[test]
    fn duplicates_and_errors_add_nothing_to_the_holding() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0), // duplicate of the stored buy
                buy(3, day(0), "AAPL", 5.0, 100.0),  // new: 15 held
                sell(4, day(1), "AAPL", 16.0, 100.0), // above 15
            ],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
        assert_eq!(
            error_key(&sim.trades[2].outcome),
            (KEY_SELL_EXCEEDS_HOLDINGS, Some("15"))
        );
    }

    #[test]
    fn instruments_that_share_a_ticker_share_its_holding() {
        let conn = db();
        let mut by_isin = buy(2, day(0), "AAPL", 10.0, 100.0);
        by_isin.instrument_key = "isin:US0378331005".into();
        by_isin.isin = Some("US0378331005".into());
        let sim = run(
            &conn,
            vec![by_isin, sell(3, day(1), "AAPL", 10.0, 100.0)],
            &config("custom"),
        );
        assert_eq!(outcomes(&sim), vec![TradeOutcome::New, TradeOutcome::New]);
        assert_eq!(sim.instruments.len(), 2);
    }

    // ---- currency ----------------------------------------------------------

    #[test]
    fn a_currency_that_differs_from_the_existing_position_is_an_error() {
        let conn = db();
        add_position(&conn, "AAPL", "Apple", "USD");
        let sim = run(
            &conn,
            vec![
                trade(2, day(0), TradeDirection::Buy, "AAPL", 1.0, 1.0, "EUR"),
                buy(3, day(0), "AAPL", 1.0, 1.0),
            ],
            &config("custom"),
        );
        assert_eq!(
            error_key(&sim.trades[0].outcome),
            (KEY_CURRENCY_MISMATCH, Some("USD"))
        );
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
        assert_eq!(
            sim.instruments[0].currency.as_deref(),
            Some("EUR"),
            "the first trade's"
        );
        assert_eq!(sim.instruments[0].position_currency.as_deref(), Some("USD"));
    }

    #[test]
    fn the_first_accepted_row_sets_the_currency_of_a_new_position() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                // an error row (sells nothing) must not decide the currency
                trade(2, day(0), TradeDirection::Sell, "AAPL", 1.0, 1.0, "CHF"),
                buy(3, day(1), "AAPL", 1.0, 1.0),
                trade(4, day(2), TradeDirection::Buy, "AAPL", 1.0, 1.0, "EUR"),
            ],
            &config("custom"),
        );
        assert_eq!(
            error_key(&sim.trades[0].outcome).0,
            KEY_SELL_EXCEEDS_HOLDINGS
        );
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
        assert_eq!(
            error_key(&sim.trades[2].outcome),
            (KEY_CURRENCY_MISMATCH, Some("USD"))
        );
        assert_eq!(
            sim.instruments[0].position_currency, None,
            "no position exists yet"
        );
    }

    #[test]
    fn a_trade_without_any_currency_is_an_error() {
        let conn = db();
        let mut t = buy(2, day(0), "AAPL", 1.0, 1.0);
        t.currency = None;
        let sim = run(&conn, vec![t], &config("custom"));
        assert_eq!(error_key(&sim.trades[0].outcome).0, KEY_CURRENCY_MISSING);
        assert_eq!(sim.instruments[0].currency, None);
    }

    // ---- symbols -----------------------------------------------------------

    #[test]
    fn an_instrument_without_a_symbol_has_no_ticker_until_one_is_chosen() {
        let conn = db();
        let mut t = buy(2, day(0), "X", 1.0, 1.0);
        t.instrument_key = "isin:US0378331005".into();
        t.symbol = None;
        t.isin = Some("US0378331005".into());

        let sim = run(&conn, vec![t.clone()], &config("degiro"));
        assert_eq!(
            error_key(&sim.trades[0].outcome),
            (KEY_SYMBOL_MISSING, None)
        );
        assert_eq!(sim.trades[0].ticker, None);
        assert_eq!(
            sim.instruments[0].status,
            StockInstrumentStatus::MissingSymbol
        );
        assert_eq!(sim.instruments[0].ticker, None);
        assert_eq!(sim.instruments[0].isin.as_deref(), Some("US0378331005"));

        let mut cfg = config("degiro");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            ticker: Some("AAPL".into()),
            ..over("isin:US0378331005")
        }];
        let sim = run(&conn, vec![t], &cfg);
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(sim.instruments[0].status, StockInstrumentStatus::New);
        assert_eq!(sim.instruments[0].ticker.as_deref(), Some("AAPL"));
    }

    #[test]
    fn a_skipped_instrument_without_a_symbol_is_skipped_not_missing() {
        let conn = db();
        let mut t = buy(2, day(0), "X", 1.0, 1.0);
        t.instrument_key = "isin:US0378331005".into();
        t.symbol = None;
        let mut cfg = config("degiro");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            skip: true,
            ..over("isin:US0378331005")
        }];
        let sim = run(&conn, vec![t], &cfg);
        assert_eq!(sim.instruments[0].status, StockInstrumentStatus::Skipped);
    }

    #[test]
    fn a_symbol_that_cannot_be_a_ticker_is_reported_as_such() {
        let conn = db();
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "BRK B", 1.0, 1.0),
                buy(3, day(0), "A".repeat(21).as_str(), 1.0, 1.0),
            ],
            &config("ibkr"),
        );
        assert_eq!(
            error_key(&sim.trades[0].outcome),
            (KEY_TICKER_INVALID, Some("BRK B"))
        );
        assert_eq!(sim.trades[0].ticker, None);
        assert_eq!(
            sim.instruments[0].status,
            StockInstrumentStatus::MissingSymbol
        );
        assert_eq!(error_key(&sim.trades[1].outcome).0, KEY_TICKER_INVALID);

        // Choosing a valid symbol fixes it.
        let mut cfg = config("ibkr");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            ticker: Some("BRK-B".into()),
            ..over("symbol:BRK B")
        }];
        let sim = run(&conn, vec![buy(2, day(0), "BRK B", 1.0, 1.0)], &cfg);
        assert_eq!(sim.trades[0].outcome, TradeOutcome::New);
        assert_eq!(sim.trades[0].ticker.as_deref(), Some("BRK-B"));
    }

    #[test]
    fn one_stored_trade_matches_every_identical_row_so_a_re_import_stays_idempotent() {
        let conn = db();
        // The first import wrote the first of two identical hand-made rows; the
        // second one was a duplicate (not forced) and stayed out.
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                buy(3, day(0), "AAPL", 10.0, 100.0),
            ],
            &config("custom"),
        );
        assert_eq!(
            outcomes(&sim),
            vec![
                duplicate(DuplicateKind::Identical),
                duplicate(DuplicateKind::Identical)
            ]
        );
    }

    #[test]
    fn left_out_rows_carry_their_message() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), Some("xtb:9"));
        let mut cfg = config("xtb");
        cfg.instrument_overrides = vec![StockInstrumentOverride {
            skip: true,
            ..over("symbol:OPT")
        }];
        let sim = run(
            &conn,
            vec![
                with_id(buy(2, day(0), "AAPL", 10.0, 100.0), "9"), // broker id
                buy(3, day(0), "AAPL", 10.0, 100.0),               // values
                buy(4, day(0), "OPT", 1.0, 1.0),                   // skipped instrument
                sell(5, day(0), "MSFT", 1.0, 1.0),                 // nothing held
                buy(6, day(1), "AAPL", 1.0, 1.0),                  // new
            ],
            &cfg,
        );
        let keys: Vec<Option<(usize, String, Option<String>)>> = sim
            .trades
            .iter()
            .map(|t| t.left_out_message().map(|m| (m.line, m.key, m.detail)))
            .collect();
        assert_eq!(
            keys,
            vec![
                Some((2, KEY_DUPLICATE_BY_ID.to_string(), Some("9".to_string()))),
                Some((3, KEY_DUPLICATE.to_string(), None)),
                Some((4, KEY_INSTRUMENT_SKIPPED.to_string(), None)),
                Some((
                    5,
                    KEY_SELL_EXCEEDS_HOLDINGS.to_string(),
                    Some("0".to_string())
                )),
                None,
            ]
        );
    }

    #[test]
    fn a_forced_duplicate_is_imported_and_has_no_left_out_message() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0), None);
        let mut cfg = config("custom");
        cfg.import_anyway_lines = vec![2];
        let sim = run(&conn, vec![buy(2, day(0), "AAPL", 10.0, 100.0)], &cfg);
        assert!(sim.trades[0].will_import());
        assert_eq!(sim.trades[0].left_out_message(), None);
    }

    #[test]
    fn a_stored_date_with_a_time_of_day_still_belongs_to_its_day() {
        let conn = db();
        // Made without a date: stamped with the time of creation, 14:00 UTC.
        add_stored(&conn, "AAPL", "buy", "10", "100", day(0) + 14 * 3_600, None);
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),  // the same trade
                sell(3, day(0), "AAPL", 10.0, 100.0), // held at the end of that day
            ],
            &config("custom"),
        );
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
    }

    #[test]
    fn stored_values_that_do_not_parse_never_match_and_count_as_nothing() {
        let conn = db();
        add_stored(&conn, "AAPL", "buy", "ten", "100", day(0), None);
        add_stored(&conn, "AAPL", "buy", "10 shares", "100", day(0), None); // legacy unit suffix
        let sim = run(
            &conn,
            vec![
                buy(2, day(0), "AAPL", 10.0, 100.0),
                sell(3, day(1), "AAPL", 10.0, 100.0),
            ],
            &config("custom"),
        );
        // the "10 shares" row is read as 10, like the position recalculation does
        assert_eq!(sim.trades[0].outcome, duplicate(DuplicateKind::Identical));
        assert_eq!(sim.trades[1].outcome, TradeOutcome::New);
    }
}
