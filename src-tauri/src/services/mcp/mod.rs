//! Embedded MCP server: tool definitions and configuration.
//!
//! One thin #[tool] method per tool lives here; the SQL bodies live in the
//! domain modules as plain functions taking &Connection (testable, per
//! docs/standards/rust-backend.md rule 1). Tool names and descriptions are
//! ported 1:1 from the retired moony-mcp Node server.

pub mod config;

pub mod analytics;
pub mod bank_accounts;
pub mod bonds;
pub mod crypto;
pub mod exchange_rates;
pub mod insurance;
pub mod investments;
pub mod loans;
pub mod money;
pub mod other_assets;
pub mod portfolio;
pub mod real_estate;
pub mod savings;
pub mod stock_monitor;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use serde_json::Value;
use std::sync::Arc;
use tauri::Emitter;

use crate::commands::categorization::CategorizationState;
use crate::db::Database;
use crate::services::categorization::CategorizationEngine;

/// Payload for the `mcp-data-changed` event emitted to the app UI whenever an
/// MCP tool call writes data (rule-7 refresh happens in the SyncProvider
/// listener, not here).
#[derive(Clone, serde::Serialize)]
struct McpDataChanged {
    domain: String,
    /// (ticker, coingeckoId) pairs — coingeckoId set only for crypto
    tickers: Vec<(String, Option<String>)>,
    #[serde(rename = "earliestDate")]
    earliest_date: Option<i64>,
}

/// Convert a rusqlite Value (supports NULL, INTEGER, REAL, TEXT) to serde_json::Value
pub(crate) fn sql_to_json(v: rusqlite::types::Value) -> Value {
    match v {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(i) => serde_json::json!(i),
        rusqlite::types::Value::Real(f) => serde_json::json!(f),
        rusqlite::types::Value::Text(s) => serde_json::json!(s),
        rusqlite::types::Value::Blob(_) => Value::Null,
    }
}

/// Daily value history of one ticker (`stock_value_history` /
/// `crypto_value_history`), oldest first. The stored `value_czk` is converted
/// into the user's main currency at each row's own day rate (ADR 0001) and
/// returned as `value`; `price` and `currency` stay native. `table` is one of
/// the two fixed table names, never user input.
pub(crate) fn ticker_value_history(
    conn: &rusqlite::Connection,
    table: &'static str,
    ticker: &str,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> crate::error::Result<Value> {
    let mut conditions = vec!["ticker = ?".to_string()];
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(ticker.to_string())];
    if let Some(s) = start_date {
        conditions.push("recorded_at >= ?".into());
        p.push(Box::new(s));
    }
    if let Some(e) = end_date {
        conditions.push("recorded_at <= ?".into());
        p.push(Box::new(e));
    }
    let sql = format!(
        "SELECT id, ticker, recorded_at, value_czk, quantity, price, currency
         FROM {} WHERE {} ORDER BY recorded_at ASC",
        table,
        conditions.join(" AND ")
    );
    let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|x| x.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let cell = |row: &rusqlite::Row, i: usize| {
        row.get::<_, rusqlite::types::Value>(i)
            .unwrap_or(rusqlite::types::Value::Null)
    };
    let stored: Vec<[rusqlite::types::Value; 7]> = stmt
        .query_map(refs.as_slice(), |row| {
            Ok([
                cell(row, 0),
                cell(row, 1),
                cell(row, 2),
                cell(row, 3),
                cell(row, 4),
                cell(row, 5),
                cell(row, 6),
            ])
        })?
        .filter_map(|r| r.ok())
        .collect();

    let ctx = money::MoneyContext::load(conn)?;
    let day_of = |cells: &[rusqlite::types::Value; 7]| match cells[2] {
        rusqlite::types::Value::Integer(day) => day,
        _ => 0,
    };
    let days: Vec<i64> = stored.iter().map(day_of).collect();
    let day_rates = ctx.day_rates_for(conn, &days);

    let rows: Vec<Value> = stored
        .into_iter()
        .map(|cells| {
            let day = day_of(&cells);
            let [id, ticker, recorded_at, value_czk, quantity, price, currency] = cells;
            serde_json::json!({
                "id": sql_to_json(id),
                "ticker": sql_to_json(ticker),
                "recordedAt": sql_to_json(recorded_at),
                "value": ctx.stored_czk_to_main_on(value_czk, day, &day_rates),
                "quantity": sql_to_json(quantity),
                "price": sql_to_json(price),
                "currency": sql_to_json(currency),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "history": rows,
    }))
}

/// Uniform result mapping, matching the Node server's behavior: success is the
/// JSON pretty-printed into one text block; an AppError becomes an isError
/// tool result carrying the error message (never a protocol-level error).
fn to_result(r: crate::error::Result<Value>) -> Result<CallToolResult, McpError> {
    match r {
        Ok(v) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&v)
                .unwrap_or_else(|e| format!("serialization error: {e}")),
        )])),
        Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(
            e.to_string(),
        )])),
    }
}

/// One skipped row in a bulk write (duplicate detected in phase 2).
#[derive(Debug, serde::Serialize)]
pub struct SkippedRow {
    pub index: usize,
    pub reason: String,
    /// `true` when the row was left out only because it looks identical to an
    /// existing one and may be a genuine repeat (two identical purchases) — the
    /// client can resend it with `importAnyway: true` once the user confirms.
    /// Absent for certain duplicates (same bank-side id) and for tools that
    /// have no such escape hatch.
    #[serde(rename = "possibleDuplicate", skip_serializing_if = "is_false")]
    pub possible_duplicate: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl SkippedRow {
    /// A row that is certainly already there / not to be written.
    pub fn new(index: usize, reason: String) -> Self {
        Self {
            index,
            reason,
            possible_duplicate: false,
        }
    }

    /// A row that only looks like a duplicate (see `possible_duplicate`).
    pub fn possible_duplicate(index: usize, reason: String) -> Self {
        Self {
            index,
            reason,
            possible_duplicate: true,
        }
    }
}

/// One rejected row in a bulk write (validation failure in phase 1).
#[derive(Debug, serde::Serialize)]
pub struct RowError {
    pub index: usize,
    pub message: String,
}

/// Uniform result of every bulk write tool. Invariant: `errors` non-empty
/// implies `created == 0` and nothing was written — whether the rejection
/// came from phase-1 field validation (bank, stock, crypto) or a phase-2
/// business-rule failure that rolled back the whole batch (stock/crypto only:
/// sell without position, currency mismatch, missing coingeckoId).
#[derive(Debug, serde::Serialize)]
pub struct BulkWriteReport {
    pub created: usize,
    #[serde(rename = "skippedDuplicates")]
    pub skipped_duplicates: Vec<SkippedRow>,
    pub errors: Vec<RowError>,
}

fn report_result(report: BulkWriteReport) -> Result<CallToolResult, McpError> {
    to_result(serde_json::to_value(&report).map_err(Into::into))
}

/// Cap on the size of any bulk-write tool's array argument (`transactions`,
/// `assignments`). It exists to keep a single tool call's JSON arguments
/// small enough for a client — in practice an LLM emitting the call — to
/// produce reliably; it is not a server-side cost control (the server
/// itself categorizes and writes hundreds of rows in well under a second).
/// Advertised to callers via the tool's JSON schema as `maxItems` (see the
/// `#[schemars(length(max = ...))]` attribute on each Vec field below) and
/// enforced as the very first check in every bulk write, before any other
/// validation or DB work.
pub(crate) const MAX_BULK_ROWS: usize = 200;

/// Phase-0 check shared by every bulk write tool: reject an oversized batch
/// before touching the DB. Must run before any other validation so an
/// over-cap call always fails fast with this message rather than some other,
/// more confusing error surfacing first.
pub(crate) fn check_bulk_size(len: usize) -> crate::error::Result<()> {
    if len > MAX_BULK_ROWS {
        return Err(crate::error::AppError::Validation(format!(
            "batch too large: {len} rows, at most {MAX_BULK_ROWS} per call — split the export \
             across several calls (duplicates are skipped, so overlapping batches are safe)"
        )));
    }
    Ok(())
}

#[derive(Clone)]
pub struct MoonyMcp {
    db: Database,
    app: tauri::AppHandle,
    tool_router: ToolRouter<Self>,
}

impl MoonyMcp {
    pub fn new(db: Database, app: tauri::AppHandle) -> Self {
        Self {
            db,
            app,
            tool_router: Self::tool_router(),
        }
    }

    /// The app's categorization engine, reached through the AppHandle the
    /// event bridge already holds (spec D6). `categorize()` is sync and
    /// network-free, so tool methods can use it inline and stay sync
    /// (ADR 0006).
    ///
    /// `None` only if the state were not registered; the import then simply
    /// skips rules-first categorization instead of failing.
    fn categorization_engine(&self) -> Option<Arc<CategorizationEngine>> {
        use tauri::Manager;
        let engine = self
            .app
            .try_state::<CategorizationState>()
            .map(|state| state.0.clone());
        if engine.is_none() {
            // Silent None here would present as "the rules quietly stopped
            // working" — every row would import uncategorized with no error.
            log::warn!(
                "CategorizationState not registered; MCP import will skip rules-first categorization"
            );
        }
        engine
    }

    /// Notify the app UI that MCP changed data (rule-7 refresh happens in
    /// the SyncProvider listener). Best-effort — a missing/closed frontend
    /// window is fine, so emit errors are swallowed.
    fn emit_data_changed(
        &self,
        domain: &str,
        tickers: &[(String, Option<String>)],
        earliest_date: Option<i64>,
    ) {
        let _ = self.app.emit(
            "mcp-data-changed",
            McpDataChanged {
                domain: domain.to_string(),
                tickers: tickers.to_vec(),
                earliest_date,
            },
        );
    }

    /// Shared body for the six single-create #[tool] methods: emit the
    /// data-changed event only when the row was actually created, then
    /// serialize the result (or the error) into the uniform tool result shape.
    fn create_tool_result(
        &self,
        domain: &str,
        result: crate::error::Result<Value>,
    ) -> Result<CallToolResult, McpError> {
        if result.is_ok() {
            self.emit_data_changed(domain, &[], None);
        }
        to_result(result)
    }

    /// Shared body for every bulk-write #[tool] method: emit the data-changed
    /// event only when rows were actually created, then serialize the report
    /// (or the error) into the uniform tool result shape.
    fn bulk_tool_result(
        &self,
        domain: &str,
        tickers: &[(String, Option<String>)],
        earliest_date: Option<i64>,
        report: crate::error::Result<BulkWriteReport>,
    ) -> Result<CallToolResult, McpError> {
        match report {
            Ok(r) => {
                if r.created > 0 {
                    self.emit_data_changed(domain, tickers, earliest_date);
                }
                report_result(r)
            }
            Err(e) => to_result(Err(e)),
        }
    }
}

// Tool methods are sync and run SQLite queries inline on the tokio worker
// (same behavior as the REST handlers they replaced). Accepted for this
// local single-user app: the DB connection mutex serializes queries anyway,
// and MCP traffic is a single client. Revisit only if tools ever do network
// or multi-second work (see ADR 0006).
#[tool_router]
impl MoonyMcp {
    #[tool(
        description = "List all bond holdings with name, ISIN, coupon value, quantity, currency, interest rate, and maturity date."
    )]
    fn bonds_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(bonds::bonds_list))
    }

    #[tool(
        description = "List all loans (mortgages, personal loans, etc.) with original principal, outstandingBalance (amortized from the annuity as of today, in the loan's currency), currency, interest rate, monthly payment, start date, end date and the optional manual balance (balanceAnchorAmount/balanceAnchorDate)."
    )]
    fn loans_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(loans::loans_list))
    }

    #[tool(
        description = "List all bank and savings accounts with balances, interest rates, and tiered interest rate zones if applicable."
    )]
    fn savings_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(savings::savings_list))
    }

    #[tool(
        description = "Get stored currency exchange rates. Rates are relative to Moony's internal base currency CZK (rate = 1.0): multiply an amount by its currency's rate to get CZK. The user's main currency is given as `mainCurrency`; the other tools already report amounts in it. Rates are fetched from the European Central Bank and cached locally."
    )]
    fn exchange_rates_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(exchange_rates::exchange_rates_list))
    }

    #[tool(
        description = "Get current portfolio metrics: net worth breakdown by asset class (savings, investments, crypto, bonds, real estate, other assets, liabilities). All monetary values are in the user's main currency, given as `mainCurrency`."
    )]
    fn portfolio_get_metrics(
        &self,
        Parameters(args): Parameters<portfolio::PortfolioMetricsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let exclude = args.exclude_personal_real_estate.unwrap_or(false);
        to_result(
            self.db
                .with_conn(|conn| portfolio::portfolio_metrics(conn, exclude)),
        )
    }

    #[tool(
        description = "Get historical portfolio snapshots showing net worth over time. Each record contains per-asset-class totals in the user's main currency (the `mainCurrency` field), each snapshot converted at its own day's exchange rate. Useful for trend analysis."
    )]
    fn portfolio_get_history(
        &self,
        Parameters(args): Parameters<portfolio::PortfolioHistoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.unwrap_or(365).clamp(1, 3650);
        to_result(self.db.with_conn(|conn| {
            portfolio::portfolio_history(conn, args.start_date, args.end_date, limit)
        }))
    }

    #[tool(
        description = "List all stock investments with ticker, company name, quantity, average purchase price, currency, and current market price."
    )]
    fn investments_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(investments::investments_list))
    }

    #[tool(
        description = "Get full details for a specific stock investment including all its transactions."
    )]
    fn investment_detail(
        &self,
        Parameters(args): Parameters<investments::InvestmentDetailArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(
            self.db
                .with_conn(|conn| investments::investment_detail(conn, &args.id)),
        )
    }

    #[tool(
        description = "Get all stock buy/sell transactions, optionally filtered by ticker symbol."
    )]
    fn stock_transactions(
        &self,
        Parameters(args): Parameters<investments::StockTransactionsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.unwrap_or(200).clamp(1, 1000);
        to_result(
            self.db.with_conn(|conn| {
                investments::stock_transactions(conn, args.ticker.clone(), limit)
            }),
        )
    }

    #[tool(
        description = "Get historical value snapshots for a specific stock ticker. Each snapshot's `value` is in the user's main currency (the `mainCurrency` field) at that day's exchange rate; `price` and `currency` are the native quote."
    )]
    fn stock_value_history(
        &self,
        Parameters(args): Parameters<investments::StockValueHistoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(|conn| {
            investments::stock_value_history(conn, &args.ticker, args.start_date, args.end_date)
        }))
    }

    #[tool(
        description = "List all cryptocurrency holdings with ticker, name, CoinGecko ID, quantity, average purchase price, and current market price."
    )]
    fn crypto_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(crypto::crypto_list))
    }

    #[tool(
        description = "Get all cryptocurrency buy/sell transactions, optionally filtered by ticker symbol."
    )]
    fn crypto_transactions(
        &self,
        Parameters(args): Parameters<crypto::CryptoTransactionsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.unwrap_or(200).clamp(1, 1000);
        to_result(
            self.db
                .with_conn(|conn| crypto::crypto_transactions(conn, args.ticker.clone(), limit)),
        )
    }

    #[tool(
        description = "Get historical value snapshots for a specific cryptocurrency ticker. Each snapshot's `value` is in the user's main currency (the `mainCurrency` field) at that day's exchange rate; `price` and `currency` are the native quote."
    )]
    fn crypto_value_history(
        &self,
        Parameters(args): Parameters<crypto::CryptoValueHistoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(|conn| {
            crypto::crypto_value_history(conn, &args.ticker, args.start_date, args.end_date)
        }))
    }

    #[tool(
        description = "List all bank accounts with institution info, account type, IBAN, balance, currency, and interest rate."
    )]
    fn bank_accounts_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(bank_accounts::bank_accounts_list))
    }

    #[tool(
        description = "Get bank transactions for a specific account with optional filtering by date range, category, transaction type, and text search. Pass uncategorized: true to get only the rows with no category yet — category_id IS NULL, whether that's because import rules didn't match, the row predates this app's categorization engine, or a category was cleared (takes precedence over categoryId)."
    )]
    fn bank_transactions(
        &self,
        Parameters(args): Parameters<bank_accounts::BankTransactionsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.unwrap_or(100).clamp(1, 1000);
        let offset = args.offset.unwrap_or(0).max(0);
        let account_id = args.account_id.clone();
        to_result(self.db.with_conn(|conn| {
            bank_accounts::bank_transactions(conn, &account_id, &args, limit, offset)
        }))
    }

    #[tool(
        description = "List all transaction categories (system-defined and user-created) with their icons, colors, and hierarchy."
    )]
    fn bank_categories(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(bank_accounts::bank_categories))
    }

    #[tool(
        description = "List all real estate properties with name, address, type, purchase price, market price, and monthly rent."
    )]
    fn real_estate_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(real_estate::real_estate_list))
    }

    #[tool(description = "Get full details for a specific property including one-time costs.")]
    fn real_estate_detail(
        &self,
        Parameters(args): Parameters<real_estate::RealEstateDetailArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(
            self.db
                .with_conn(|conn| real_estate::real_estate_detail(conn, &args.id)),
        )
    }

    #[tool(
        description = "List all other assets such as precious metals, art, collectibles, etc. with quantity, market price, average purchase price, and yield info."
    )]
    fn other_assets_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(other_assets::other_assets_list))
    }

    #[tool(description = "Get buy/sell transactions for a specific other asset.")]
    fn other_assets_transactions(
        &self,
        Parameters(args): Parameters<other_assets::OtherAssetsTransactionsArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(
            self.db
                .with_conn(|conn| other_assets::other_assets_transactions(conn, &args.asset_id)),
        )
    }

    #[tool(
        description = "List all insurance policies (life, health, property, etc.) with type, provider, policy name, payment info, coverage limits, and status."
    )]
    fn insurance_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(insurance::insurance_list))
    }

    #[tool(
        description = "Get full details for a specific insurance policy including any linked documents."
    )]
    fn insurance_detail(
        &self,
        Parameters(args): Parameters<insurance::InsuranceDetailArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(
            self.db
                .with_conn(|conn| insurance::insurance_detail(conn, &args.id)),
        )
    }

    #[tool(
        description = "Create a new insurance policy in Moony.\n\nCall this only after presenting the extracted data to the user and receiving their confirmation.\n\nValid values:\n- type: life | health | property | vehicle | travel | liability | accident | other\n- paymentFrequency: monthly | quarterly | semi_annually | annually | one_time\n- status: active | inactive (defaults to active)\n- Dates: Unix timestamps in seconds\n- Payments: numeric strings e.g. \"1200\"\n- Currencies: 3-letter ISO codes e.g. \"CZK\", \"EUR\", \"USD\"; a missing currency defaults to the user's main currency"
    )]
    fn insurance_create(
        &self,
        Parameters(body): Parameters<crate::models::InsertInsurancePolicy>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .db
            .with_conn(|conn| insurance::insurance_create(conn, body));
        self.create_tool_result("insurance", result)
    }

    #[tool(
        description = "Get an income vs expenses cashflow report based on user-defined cashflow items (recurring income/expenses), loan payments, insurance premiums, and savings account interest. View as monthly or yearly totals. Amounts are in the user's main currency, given as `mainCurrency`."
    )]
    fn cashflow_report(
        &self,
        Parameters(args): Parameters<analytics::CashflowArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(
            self.db
                .with_conn(|conn| analytics::cashflow_report(conn, args.view_type.clone())),
        )
    }

    #[tool(
        description = "Get a budget vs actual spending report by category for a given time period. Shows how much was spent vs budget goals. Amounts are in the user's main currency, given as `mainCurrency`."
    )]
    fn budgeting_report(
        &self,
        Parameters(args): Parameters<analytics::BudgetingArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(|conn| {
            analytics::budgeting_report(
                conn,
                Some(args.start_date),
                Some(args.end_date),
                args.timeframe.clone(),
            )
        }))
    }

    #[tool(
        description = "Get all stock investments with their current value, gain/loss, dividend yield, and tag groupings. Amounts are in the user's main currency, given as `mainCurrency`."
    )]
    fn stocks_analysis(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(analytics::stocks_analysis))
    }

    #[tool(
        description = "Get aggregated portfolio metrics grouped by stock tag. Shows total value, cost basis, gain/loss, and dividend yield per tag. Amounts are in the user's main currency, given as `mainCurrency`."
    )]
    fn tag_metrics(
        &self,
        Parameters(args): Parameters<analytics::TagMetricsArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(
            self.db
                .with_conn(|conn| analytics::tag_metrics(conn, args.tag_ids.clone())),
        )
    }

    #[tool(
        description = "Create a bank account in Moony.\n\nCall this only after presenting the parsed data to the user and receiving their confirmation.\nValid accountType values: checking | savings | credit_card | investment. Amounts are numeric strings; currencies are 3-letter ISO codes (currency defaults to the user's main currency)."
    )]
    fn bank_account_create(
        &self,
        Parameters(args): Parameters<bank_accounts::BankAccountCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .db
            .with_conn(|conn| bank_accounts::bank_account_create(conn, &args));
        self.create_tool_result("bank", result)
    }

    #[tool(
        description = "Bulk-import bank transactions into one account (designed for parsed CSV/statement exports).\n\nCall this only after presenting the parsed data to the user and receiving their confirmation. Import in batches of about 100 rows at a time (max 200 per call); split larger exports across multiple calls — safe to do because duplicate rows are skipped and reported.\nRows without a categoryId are auto-categorized by the app's own rules (custom rules, learned payees, own-account transfers, default system rules) wherever one matches; anything the rules can't place is left uncategorized rather than guessed — call bank_transactions with uncategorized: true afterward to find those and assign the rest yourself. Rows that arrive with an explicit categoryId are always left as given.\nAll-or-nothing: if any row is invalid or references a missing category, the whole batch is rejected with per-row errors and nothing is written. Duplicates: pass the bank-side transactionId whenever the export has one — it is then the only duplicate key (a row whose id already exists is skipped and reported; two rows with different ids are two payments, however alike). Rows without a transactionId that exactly match an existing transaction (account+date+amount+type+description) are skipped and reported in skippedDuplicates with possibleDuplicate: true, because they may be a genuine repeat purchase (two identical coffees on one day) rather than a re-import. Tell the user about every such row; only if they confirm it is a separate payment, resend just that row with importAnyway: true. Re-importing an overlapping export is safe.\ntype: credit | debit. Dates: Unix timestamps (seconds). Amounts: positive numeric strings."
    )]
    fn bank_transactions_create(
        &self,
        Parameters(args): Parameters<bank_accounts::BankTransactionsCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let engine = self.categorization_engine();
        let report = self.db.with_conn_mut(|conn| {
            bank_accounts::bank_transactions_create(conn, &args, engine.as_deref())
        });
        self.bulk_tool_result("bank", &[], None, report)
    }

    #[tool(
        description = "Suggest categories for existing bank transactions (bulk). Suggestions are NOT final: each one lands as a pending suggestion the user reviews and confirms inside the Moony app. When the user confirms a suggestion, the app learns a payee rule and will categorize that merchant automatically from then on — tell the user to review your suggestions in the app after you are done. Categories must already exist (see bank_categories).\n\nSubmit about 100 assignments at a time (max 200 per call); split larger batches across multiple calls. A transaction that already has a final category — manual, rule-derived, or a previously confirmed suggestion — is skipped and reported in skippedDuplicates; the user can recategorize it in the app. A pending unconfirmed suggestion is replaced by a new one. Rows with a pending suggestion show it as suggestedCategoryId in the bank_transactions read tool — skip those unless you mean to revise them.\nAll-or-nothing on genuinely invalid input: an unknown transaction or category id, or the same transaction assigned twice in one call, rejects the whole batch with per-row errors and nothing is written."
    )]
    fn bank_transactions_categorize(
        &self,
        Parameters(args): Parameters<bank_accounts::BankTransactionsCategorizeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let report = self
            .db
            .with_conn_mut(|conn| bank_accounts::bank_transactions_categorize(conn, &args));
        self.bulk_tool_result("bank", &[], None, report)
    }

    #[tool(
        description = "Bulk-import stock buy/sell transactions (designed for parsed broker exports). Holdings are created automatically on the first buy of a new ticker — no separate create call needed. A sell requires an existing position; every transaction of one ticker must use one currency.\n\nCall this only after presenting the parsed data to the user and receiving their confirmation. Import in batches of about 100 rows at a time (max 200 per call); split larger exports across multiple calls — safe to do because duplicate rows are skipped and reported.\nAll-or-nothing: invalid rows or business-rule failures (sell without position, currency mismatch) reject the whole batch with per-row errors. Duplicates (same ticker+date+type+quantity+price) are skipped and reported.\ntype: buy | sell. Dates: Unix timestamps (seconds). Quantities/prices: positive numeric strings."
    )]
    fn stock_transactions_create(
        &self,
        Parameters(args): Parameters<investments::StockTransactionsCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let report = self
            .db
            .with_conn_mut(|conn| investments::stock_transactions_create(conn, &args));
        let tickers = investments::distinct_tickers(&args);
        let earliest = investments::earliest_date(&args);
        self.bulk_tool_result("stocks", &tickers, earliest, report)
    }

    #[tool(
        description = "Bulk-import cryptocurrency buy/sell transactions (designed for parsed exchange exports). Holdings are created automatically on the first buy of a new coin — supply coingeckoId (e.g. \"bitcoin\") for coins not yet in Moony so prices can be fetched. A sell requires an existing position; every transaction of one coin must use one currency.\n\nCall this only after presenting the parsed data to the user and receiving their confirmation. Import in batches of about 100 rows at a time (max 200 per call); split larger exports across multiple calls — safe to do because duplicate rows are skipped and reported.\nAll-or-nothing with per-row errors; duplicates (same ticker+date+type+quantity+price) are skipped and reported.\ntype: buy | sell. Dates: Unix timestamps (seconds). Quantities/prices: positive numeric strings."
    )]
    fn crypto_transactions_create(
        &self,
        Parameters(args): Parameters<crypto::CryptoTransactionsCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let report = self
            .db
            .with_conn_mut(|conn| crypto::crypto_transactions_create(conn, &args));
        let tickers = crypto::distinct_tickers(&args);
        let earliest = crypto::earliest_date(&args);
        self.bulk_tool_result("crypto", &tickers, earliest, report)
    }

    #[tool(
        description = "Create a bond holding in Moony.\n\nCall this only after presenting the parsed data to the user and receiving their confirmation.\nAmounts are numeric strings; currency is a 3-letter ISO code (defaults to the user's main currency); maturityDate is a Unix timestamp (seconds); ISIN, when provided, must be 12 characters."
    )]
    fn bond_create(
        &self,
        Parameters(args): Parameters<crate::models::InsertBond>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.db.with_conn(|conn| bonds::bond_create(conn, &args));
        self.create_tool_result("bonds", result)
    }

    #[tool(
        description = "Create a loan (mortgage, personal loan, …) in Moony.\n\nCall this only after presenting the parsed data to the user and receiving their confirmation.\nprincipal is the original amount and must be positive; currency is a 3-letter ISO code (defaults to the user's main currency); interestRate is a percent 0–100; startDate/endDate are Unix timestamps (seconds); endDate must be after startDate. The balance is amortized from principal at startDate; optionally pass balanceAnchorAmount together with balanceAnchorDate (Unix seconds, not before startDate) to start from a real balance instead."
    )]
    fn loan_create(
        &self,
        Parameters(args): Parameters<crate::models::InsertLoan>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.db.with_conn(|conn| loans::loan_create(conn, &args));
        self.create_tool_result("loans", result)
    }

    #[tool(
        description = "Create a real estate property in Moony (photos can only be added in the app UI).\n\nCall this only after presenting the parsed data to the user and receiving their confirmation.\nPrices/rent are numeric strings with 3-letter ISO currency codes (a missing currency defaults to the user's main currency); recurringCosts entries are {name, amount, frequency, currency?}."
    )]
    fn real_estate_create(
        &self,
        Parameters(args): Parameters<real_estate::RealEstateCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .db
            .with_conn(|conn| real_estate::real_estate_create(conn, &args));
        self.create_tool_result("real-estate", result)
    }

    #[tool(
        description = "Create an other-asset holding (precious metals, art, collectibles, …) in Moony, optionally with an initial buy transaction.\n\nCall this only after presenting the parsed data to the user and receiving their confirmation.\nyieldType: none | percentage | fixed. Amounts are numeric strings; currency is a 3-letter ISO code (defaults to the user's main currency); dates are Unix timestamps (seconds). quantity and averagePurchasePrice are derived from transactions (via this call's initialTransaction or a later other_asset_transactions_create import) — any value passed for them is overwritten by the first import, so leave them unset."
    )]
    fn other_asset_create(
        &self,
        Parameters(args): Parameters<other_assets::OtherAssetCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .db
            .with_conn(|conn| other_assets::other_asset_create(conn, &args));
        self.create_tool_result("other-assets", result)
    }

    #[tool(
        description = "Bulk-import buy/sell transactions for one existing other asset.\n\nCall this only after presenting the parsed data to the user and receiving their confirmation. Import in batches of about 100 rows at a time (max 200 per call); split larger exports across multiple calls — safe to do because duplicate rows are skipped and reported.\nAll-or-nothing with per-row errors; duplicates (same asset+date+type+quantity+price) are skipped and reported. type: buy | sell."
    )]
    fn other_asset_transactions_create(
        &self,
        Parameters(args): Parameters<other_assets::OtherAssetTransactionsCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let report = self
            .db
            .with_conn_mut(|conn| other_assets::other_asset_transactions_create(conn, &args));
        // No tickers (other assets aren't priced per symbol), but the earliest
        // date still matters: a backdated row needs the portfolio history recalculated,
        // exactly as the UI path does (commands/other_assets.rs).
        let earliest = other_assets::earliest_date(&args);
        self.bulk_tool_result("other-assets", &[], earliest, report)
    }

    #[tool(
        description = "List the stocks the user follows in Stock Monitor (their watchlist), with the cached price, previous close, 52-week range, their own target price, the size of their notes in bytes (notesBytes), and whether the stock is also held in the portfolio. Note bodies are deliberately omitted so this stays cheap however long they grow — read one with watchlist_notes_get. Following is independent of ownership: a followed stock is not part of net worth."
    )]
    fn watchlist_list(&self) -> Result<CallToolResult, McpError> {
        to_result(self.db.with_conn(stock_monitor::watchlist_list))
    }

    #[tool(
        description = "Follow a stock in Stock Monitor so the user tracks its price. Idempotent: following an already-followed ticker keeps its existing target price and notes. Does not buy anything and does not change net worth. Use the Yahoo-style symbol, including the exchange suffix for non-US listings (AAPL, BMW.DE, EWG.L)."
    )]
    fn watchlist_follow(
        &self,
        Parameters(args): Parameters<stock_monitor::WatchlistFollowArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .db
            .with_conn(|conn| stock_monitor::watchlist_follow(conn, &args.ticker));
        self.create_tool_result("stock-monitor", result)
    }

    #[tool(
        description = "Overwrite the user's research notes for a stock in Stock Monitor. Markdown; the app renders it. This REPLACES the whole document, so prefer watchlist_append_notes when you are adding to existing notes — it costs a fraction of the tokens. Use this one only to rewrite from scratch, and read watchlist_notes_get first if the current text must be preserved. Follows the ticker automatically when it is not followed yet. Call this only after presenting the notes to the user and receiving their confirmation."
    )]
    fn watchlist_update_notes(
        &self,
        Parameters(args): Parameters<stock_monitor::WatchlistNotesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.db.with_conn(|conn| {
            stock_monitor::watchlist_update_notes(conn, &args.ticker, args.notes.clone())
        });
        self.create_tool_result("stock-monitor", result)
    }

    #[tool(
        description = "Read the user's full research notes for one stock in Stock Monitor. Use this instead of watchlist_list whenever you need the note text itself — watchlist_list reports only their size. An un-followed ticker answers with followed: false and empty notes rather than an error."
    )]
    fn watchlist_notes_get(
        &self,
        Parameters(args): Parameters<stock_monitor::WatchlistTickerArgs>,
    ) -> Result<CallToolResult, McpError> {
        to_result(
            self.db
                .with_conn(|conn| stock_monitor::watchlist_notes_get(conn, &args.ticker)),
        )
    }

    #[tool(
        description = "Add a section to the user's research notes for a stock in Stock Monitor, keeping what is already there. Send ONLY the new Markdown — it is appended after a blank line, so never resend the existing text. This is the cheap way to record new research: prefer it over watchlist_update_notes, which makes you reproduce the whole document. Follows the ticker automatically when it is not followed yet. Call this only after presenting the addition to the user and receiving their confirmation."
    )]
    fn watchlist_append_notes(
        &self,
        Parameters(args): Parameters<stock_monitor::WatchlistAppendNotesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.db.with_conn(|conn| {
            stock_monitor::watchlist_append_notes(conn, &args.ticker, args.notes.clone())
        });
        self.create_tool_result("stock-monitor", result)
    }

    #[tool(
        description = "Set the user's own target price for a stock in Stock Monitor, or clear it by passing targetPrice: null. The target is an informative reference point shown next to the price — it triggers no order and no alert. Given in the stock's own currency as a decimal string. Follows the ticker automatically when it is not followed yet. Call this only after presenting the target to the user and receiving their confirmation."
    )]
    fn watchlist_set_target_price(
        &self,
        Parameters(args): Parameters<stock_monitor::WatchlistTargetPriceArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.db.with_conn(|conn| {
            stock_monitor::watchlist_set_target_price(conn, &args.ticker, args.target_price.clone())
        });
        self.create_tool_result("stock-monitor", result)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MoonyMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Query Moony personal finance data (read tools), import parsed exports (bulk \
                 *_create write tools — always show the user the parsed data and get their \
                 confirmation first), and categorize bank payments. Monetary values are in the \
                 user's main currency (the `mainCurrency` field) unless a currency field says \
                 otherwise; money amounts are decimal strings; dates are Unix timestamps in \
                 seconds."
                    .to_string(),
            )
    }
}

/// Self-verifying: prove that `#[schemars(length(max = ...))]` on each bulk
/// write's Vec field actually produces `maxItems` in the JSON schema rmcp
/// advertises for the tool's arguments — not just that the attribute
/// compiles. `schema_for_input` is the exact function the `#[tool]` macro
/// uses to build the advertised `inputSchema`, so this checks what a client
/// actually sees.
#[cfg(test)]
mod bulk_schema_tests {
    use super::MAX_BULK_ROWS;
    use rmcp::handler::server::tool::schema_for_input;

    fn max_items_of(schema: &serde_json::Map<String, serde_json::Value>, property: &str) -> u64 {
        schema["properties"][property]["maxItems"]
            .as_u64()
            .unwrap_or_else(|| panic!("maxItems must be present on the {property} array"))
    }

    #[test]
    fn bank_transactions_create_advertises_max_items() {
        let schema =
            schema_for_input::<super::bank_accounts::BankTransactionsCreateArgs>().unwrap();
        assert_eq!(max_items_of(&schema, "transactions"), MAX_BULK_ROWS as u64);
    }

    #[test]
    fn bank_transactions_categorize_advertises_max_items() {
        let schema =
            schema_for_input::<super::bank_accounts::BankTransactionsCategorizeArgs>().unwrap();
        assert_eq!(max_items_of(&schema, "assignments"), MAX_BULK_ROWS as u64);
    }

    #[test]
    fn stock_transactions_create_advertises_max_items() {
        let schema = schema_for_input::<super::investments::StockTransactionsCreateArgs>().unwrap();
        assert_eq!(max_items_of(&schema, "transactions"), MAX_BULK_ROWS as u64);
    }

    #[test]
    fn crypto_transactions_create_advertises_max_items() {
        let schema = schema_for_input::<super::crypto::CryptoTransactionsCreateArgs>().unwrap();
        assert_eq!(max_items_of(&schema, "transactions"), MAX_BULK_ROWS as u64);
    }

    #[test]
    fn other_asset_transactions_create_advertises_max_items() {
        let schema =
            schema_for_input::<super::other_assets::OtherAssetTransactionsCreateArgs>().unwrap();
        assert_eq!(max_items_of(&schema, "transactions"), MAX_BULK_ROWS as u64);
    }
}
