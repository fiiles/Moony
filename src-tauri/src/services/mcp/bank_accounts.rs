//! MCP tools: bank accounts. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::models::InsertBankTransaction;
use crate::services::categorization::CategorizationEngine;
use crate::services::{bank_accounts as bank_service, dedup};

use super::{sql_to_json, BulkWriteReport, RowError, SkippedRow, MAX_BULK_ROWS};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BankTransactionsArgs {
    #[serde(rename = "accountId")]
    #[schemars(description = "The bank account ID to get transactions for")]
    pub account_id: String,
    #[serde(rename = "dateFrom")]
    #[schemars(description = "Start date as Unix timestamp (seconds)")]
    pub date_from: Option<i64>,
    #[serde(rename = "dateTo")]
    #[schemars(description = "End date as Unix timestamp (seconds)")]
    pub date_to: Option<i64>,
    #[serde(rename = "categoryId")]
    #[schemars(description = "Filter by category ID (ignored when uncategorized is true)")]
    pub category_id: Option<String>,
    #[schemars(
        description = "Return only transactions with no category yet (the ones the app's rules could not place). Takes precedence over categoryId"
    )]
    pub uncategorized: Option<bool>,
    #[serde(rename = "txType")]
    #[schemars(description = "Filter by transaction type: credit or debit")]
    pub tx_type: Option<String>,
    #[schemars(description = "Search text in description or counterparty name")]
    pub search: Option<String>,
    #[schemars(description = "Max transactions to return (default 100, max 1000)")]
    pub limit: Option<i64>,
    #[schemars(description = "Pagination offset (default 0)")]
    pub offset: Option<i64>,
}

pub fn bank_accounts_list(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT ba.id, ba.name, ba.account_type, ba.iban, ba.bban,
                ba.currency, ba.balance, ba.interest_rate,
                ba.has_zone_designation, ba.exclude_from_balance,
                ba.created_at, ba.updated_at,
                i.id, i.name, i.bic, i.country
         FROM bank_accounts ba
         LEFT JOIN institutions i ON ba.institution_id = i.id
         ORDER BY ba.name",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "name": row.get::<_, String>(1)?,
        "accountType": row.get::<_, String>(2)?,
        "iban": sql_to_json(row.get::<_, rusqlite::types::Value>(3).unwrap_or(rusqlite::types::Value::Null)),
        "bban": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
        "currency": row.get::<_, String>(5)?,
        "balance": row.get::<_, String>(6)?,
        "interestRate": sql_to_json(row.get::<_, rusqlite::types::Value>(7).unwrap_or(rusqlite::types::Value::Null)),
        "hasZoneDesignation": row.get::<_, i32>(8)? != 0,
        "excludeFromBalance": row.get::<_, i32>(9)? != 0,
        "createdAt": row.get::<_, i64>(10)?,
        "updatedAt": row.get::<_, i64>(11)?,
        "institutionId": sql_to_json(row.get::<_, rusqlite::types::Value>(12).unwrap_or(rusqlite::types::Value::Null)),
        "institutionName": sql_to_json(row.get::<_, rusqlite::types::Value>(13).unwrap_or(rusqlite::types::Value::Null)),
        "bic": sql_to_json(row.get::<_, rusqlite::types::Value>(14).unwrap_or(rusqlite::types::Value::Null)),
        "country": sql_to_json(row.get::<_, rusqlite::types::Value>(15).unwrap_or(rusqlite::types::Value::Null)),
    })))?.filter_map(|r| r.ok()).collect();
    Ok(Value::Array(rows))
}

// ============================================================================
// Write tools (Task 8: canonical two-phase bulk engine)
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BankTransactionRow {
    #[serde(rename = "transactionId")]
    #[schemars(
        description = "Bank-side transaction id from the export. Pass it whenever the export has one — it gives exact duplicate detection"
    )]
    pub transaction_id: Option<String>,
    #[serde(rename = "type")]
    #[schemars(description = "credit (money in) or debit (money out)")]
    pub tx_type: String,
    #[schemars(description = "Positive amount as a numeric string, e.g. \"250.50\"")]
    pub amount: String,
    #[schemars(description = "3-letter ISO currency code; omit to use the account's currency")]
    pub currency: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "counterpartyName")]
    pub counterparty_name: Option<String>,
    #[serde(rename = "counterpartyIban")]
    pub counterparty_iban: Option<String>,
    #[serde(rename = "bookingDate")]
    #[schemars(description = "Booking date as Unix timestamp (seconds)")]
    pub booking_date: i64,
    #[serde(rename = "valueDate")]
    #[schemars(description = "Value date as Unix timestamp (seconds)")]
    pub value_date: Option<i64>,
    #[serde(rename = "categoryId")]
    #[schemars(
        description = "Existing category id (see bank_categories); omit to leave uncategorized"
    )]
    pub category_id: Option<String>,
    #[serde(rename = "importAnyway", default)]
    #[schemars(
        description = "Set true to import this row although an identical transaction (same date, amount, type, description) already exists — a genuine repeat purchase such as two identical coffees on one day. Send it only for a row that came back in skippedDuplicates with possibleDuplicate: true and that the user confirmed is a separate payment. Has no effect on a bank-side transactionId match, which is always the same transaction."
    )]
    pub import_anyway: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BankTransactionsCreateArgs {
    #[serde(rename = "accountId")]
    #[schemars(description = "The bank account ID to import into (see bank_accounts_list)")]
    pub account_id: String,
    #[schemars(length(max = MAX_BULK_ROWS))]
    pub transactions: Vec<BankTransactionRow>,
}

fn row_to_insert(account_id: &str, row: &BankTransactionRow) -> InsertBankTransaction {
    InsertBankTransaction {
        bank_account_id: account_id.to_string(),
        // An empty/whitespace export id (e.g. an LLM mapping an empty CSV cell
        // to "") is "no id" — store NULL, never '', so the id-dedup rule can't
        // match unrelated id-less rows against each other.
        transaction_id: row
            .transaction_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(String::from),
        tx_type: row.tx_type.clone(),
        amount: row.amount.clone(),
        currency: row.currency.clone(),
        description: row.description.clone(),
        counterparty_name: row.counterparty_name.clone(),
        counterparty_iban: row.counterparty_iban.clone(),
        booking_date: row.booking_date,
        value_date: row.value_date,
        category_id: row.category_id.clone(),
        status: None,
    }
}

/// Two-phase bulk create. Phase 1 (no writes): account exists, every row
/// validates, every referenced category exists — any failure returns the full
/// error list with created == 0. Phase 2: one SQL transaction; duplicate rows
/// are skipped and reported; everything else is inserted; commit.
///
/// Rows that arrive without a `categoryId` are categorized by the app's own
/// deterministic layers just before the INSERT (spec D1/D2), stamped with
/// their provenance. `engine == None` disables that enrichment (unit tests),
/// leaving the pre-feature behavior: every row uncategorized.
pub fn bank_transactions_create(
    conn: &mut rusqlite::Connection,
    args: &BankTransactionsCreateArgs,
    engine: Option<&CategorizationEngine>,
) -> Result<BulkWriteReport> {
    super::check_bulk_size(args.transactions.len())?;

    let mut errors: Vec<RowError> = Vec::new();

    let account_exists: Option<String> = conn
        .query_row(
            "SELECT id FROM bank_accounts WHERE id = ?1",
            [&args.account_id],
            |row| row.get(0),
        )
        .optional()?;
    if account_exists.is_none() {
        return Err(AppError::NotFound(format!(
            "Bank account not found: {}",
            args.account_id
        )));
    }

    for (i, row) in args.transactions.iter().enumerate() {
        if let Err(e) = row_to_insert(&args.account_id, row).validate() {
            errors.push(RowError {
                index: i,
                message: e.to_string(),
            });
        }
        if let Some(ref cat) = row.category_id {
            if !bank_service::category_exists(conn, cat)? {
                errors.push(RowError {
                    index: i,
                    message: format!("Category not found: {}", cat),
                });
            }
        }
    }
    if !errors.is_empty() {
        return Ok(BulkWriteReport {
            created: 0,
            skipped_duplicates: vec![],
            errors,
        });
    }

    // Dedup runs INSIDE the transaction, so it also catches duplicates within
    // the batch itself: after row 3 inserts, an identical row 7 matches it.
    let tx = conn.transaction()?;
    let mut created = 0usize;
    let mut skipped: Vec<SkippedRow> = Vec::new();
    for (i, row) in args.transactions.iter().enumerate() {
        match dedup::check_bank_transaction(
            &tx,
            &args.account_id,
            row.transaction_id.as_deref(),
            row.booking_date,
            row.value_date,
            &row.amount,
            &row.tx_type.to_lowercase(),
            row.description.as_deref(),
        )? {
            // The bank says it is the same transaction: never imported again.
            Some(dup @ dedup::BankDuplicate::BankId(_)) => {
                skipped.push(SkippedRow::new(i, dup.reason()));
                continue;
            }
            // Looks identical, but may be a genuine repeat purchase:
            // left out with an explicit way for the client to override it.
            Some(dup @ dedup::BankDuplicate::Identical) if !row.import_anyway.unwrap_or(false) => {
                skipped.push(SkippedRow::possible_duplicate(
                    i,
                    format!(
                        "{} — if this is a genuine repeat purchase, resend this row with importAnyway: true",
                        dup.reason()
                    ),
                ));
                continue;
            }
            Some(dedup::BankDuplicate::Identical) | None => {}
        }
        let mut insert = row_to_insert(&args.account_id, row);
        let categorization_source = bank_service::enrich_with_rules(&tx, &mut insert, engine)?;
        // data_source stays "manual" (what create_transaction has always
        // written for this tool) — only the categorization provenance is new.
        bank_service::create_transaction_with_origin(
            &tx,
            &insert,
            "manual",
            None,
            categorization_source,
        )?;
        created += 1;
    }
    tx.commit()?;

    Ok(BulkWriteReport {
        created,
        skipped_duplicates: skipped,
        errors: vec![],
    })
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BankAccountCreateArgs {
    #[schemars(description = "Account display name")]
    pub name: String,
    #[serde(rename = "accountType")]
    #[schemars(description = "checking (default), savings, credit_card, or investment")]
    pub account_type: Option<String>,
    pub iban: Option<String>,
    pub bban: Option<String>,
    #[schemars(description = "3-letter ISO currency code, defaults to CZK")]
    pub currency: Option<String>,
    #[schemars(description = "Opening balance as a numeric string, defaults to \"0\"")]
    pub balance: Option<String>,
    #[serde(rename = "interestRate")]
    #[schemars(description = "Annual interest rate percent as a numeric string")]
    pub interest_rate: Option<String>,
}

pub fn bank_account_create(conn: &Connection, args: &BankAccountCreateArgs) -> Result<Value> {
    // Entity duplicates are errors (spec D4): same name (case-insensitive) or same non-empty IBAN.
    // Note: SQLite LOWER() folds ASCII only — it does NOT fold diacritics
    // ("Účet" vs "účet" are still distinct), so this is not full Unicode
    // case-insensitivity.
    let dup: Option<String> = conn
        .query_row(
            "SELECT id FROM bank_accounts
             WHERE LOWER(name) = LOWER(?1)
                OR (?2 IS NOT NULL AND ?2 != '' AND iban = ?2)",
            rusqlite::params![args.name, args.iban],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = dup {
        return Err(AppError::Validation(format!(
            "A bank account with this name or IBAN already exists (id: {})",
            id
        )));
    }
    let data = crate::models::InsertBankAccount {
        name: args.name.clone(),
        account_type: args.account_type.clone(),
        iban: args.iban.clone(),
        bban: args.bban.clone(),
        currency: args.currency.clone(),
        balance: args.balance.clone(),
        institution_id: None,
        interest_rate: args.interest_rate.clone(),
        has_zone_designation: None,
        termination_date: None,
        exclude_from_balance: None,
    };
    let account = bank_service::create_account(conn, &data)?;
    Ok(serde_json::to_value(account)?)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CategoryAssignment {
    #[serde(rename = "transactionId")]
    pub transaction_id: String,
    #[serde(rename = "categoryId")]
    #[schemars(description = "Existing category id (see bank_categories)")]
    pub category_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BankTransactionsCategorizeArgs {
    #[schemars(length(max = MAX_BULK_ROWS))]
    pub assignments: Vec<CategoryAssignment>,
}

/// The transaction's current category id and that category's display name,
/// or `None` when the transaction does not exist.
fn current_category(
    conn: &Connection,
    transaction_id: &str,
) -> Result<Option<(Option<String>, Option<String>)>> {
    let found = conn
        .query_row(
            "SELECT bt.category_id, tc.name FROM bank_transactions bt
             LEFT JOIN transaction_categories tc ON bt.category_id = tc.id
             WHERE bt.id = ?1",
            [transaction_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(found)
}

/// Phase 1: every transaction id and category id must exist, and no
/// transaction may be assigned twice in one batch. Phase 2: all updates in
/// one SQL transaction. `created` in the report = rows updated.
///
/// Categories assigned here are SUGGESTIONS (spec 2026-08-27 D5): they land
/// in `suggested_category_id`, never in `category_id`. The user confirms or
/// declines each one in the app; confirmation stamps `mcp` provenance and
/// teaches the learned-payee layer. Rows that already carry a final category
/// are skipped and reported — nothing an MCP client does replaces manual
/// work or rule output. A prior pending suggestion is replaced.
pub fn bank_transactions_categorize(
    conn: &mut rusqlite::Connection,
    args: &BankTransactionsCategorizeArgs,
) -> Result<BulkWriteReport> {
    super::check_bulk_size(args.assignments.len())?;

    let mut errors: Vec<RowError> = Vec::new();
    let mut skipped: Vec<SkippedRow> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (i, a) in args.assignments.iter().enumerate() {
        // A repeated transaction id would run both UPDATEs (last wins) and
        // overcount `created` — all-or-nothing, so reject it up front.
        if !seen.insert(a.transaction_id.as_str()) {
            errors.push(RowError {
                index: i,
                message: format!("duplicate assignment for transaction {}", a.transaction_id),
            });
        }
        match current_category(conn, &a.transaction_id)? {
            None => errors.push(RowError {
                index: i,
                message: format!("Transaction not found: {}", a.transaction_id),
            }),
            Some((Some(existing_id), existing_name)) => skipped.push(SkippedRow::new(
                i,
                format!(
                    "already categorized as {} — the user can recategorize in the app",
                    existing_name.unwrap_or(existing_id)
                ),
            )),
            Some((None, _)) => {}
        }
        if !bank_service::category_exists(conn, &a.category_id)? {
            errors.push(RowError {
                index: i,
                message: format!("Category not found: {}", a.category_id),
            });
        }
    }
    if !errors.is_empty() {
        return Ok(BulkWriteReport {
            created: 0,
            skipped_duplicates: vec![],
            errors,
        });
    }

    let skipped_indices: std::collections::HashSet<usize> =
        skipped.iter().map(|s| s.index).collect();
    let tx = conn.transaction()?;
    let mut created = 0usize;
    for (i, a) in args.assignments.iter().enumerate() {
        if skipped_indices.contains(&i) {
            continue;
        }
        tx.execute(
            "UPDATE bank_transactions SET suggested_category_id = ?1 WHERE id = ?2",
            rusqlite::params![a.category_id, a.transaction_id],
        )?;
        created += 1;
    }
    tx.commit()?;
    Ok(BulkWriteReport {
        created,
        skipped_duplicates: skipped,
        errors: vec![],
    })
}

pub fn bank_transactions(
    conn: &Connection,
    account_id: &str,
    args: &BankTransactionsArgs,
    limit: i64,
    offset: i64,
) -> Result<Value> {
    let mut conditions = vec!["bt.bank_account_id = ?".to_string()];
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(account_id.to_string())];
    if let Some(v) = args.date_from {
        conditions.push("bt.booking_date >= ?".into());
        p.push(Box::new(v));
    }
    if let Some(v) = args.date_to {
        conditions.push("bt.booking_date <= ?".into());
        p.push(Box::new(v));
    }
    // "Only what the rules could not place" (spec D5). Deliberately wins over
    // categoryId rather than erroring: the two together would always return an
    // empty page, and the arg description says which one applies.
    if args.uncategorized == Some(true) {
        conditions.push("bt.category_id IS NULL".into());
    } else if let Some(ref v) = args.category_id {
        conditions.push("bt.category_id = ?".into());
        p.push(Box::new(v.clone()));
    }
    if let Some(ref v) = args.tx_type {
        conditions.push("bt.tx_type = ?".into());
        p.push(Box::new(v.clone()));
    }
    if let Some(ref v) = args.search {
        conditions.push("(bt.description LIKE ? OR bt.counterparty_name LIKE ?)".into());
        let pat = format!("%{}%", v);
        p.push(Box::new(pat.clone()));
        p.push(Box::new(pat));
    }
    let where_clause = conditions.join(" AND ");
    let count_sql = format!(
        "SELECT count(*) FROM bank_transactions bt WHERE {}",
        where_clause
    );
    let count_refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|x| x.as_ref()).collect();
    let total: i64 = conn.query_row(&count_sql, count_refs.as_slice(), |r| r.get(0))?;

    let sql = format!(
        "SELECT bt.id, bt.tx_type, bt.amount, bt.currency, bt.description,
                bt.counterparty_name, bt.counterparty_iban, bt.booking_date,
                bt.value_date, bt.status, bt.remittance_info, bt.suggested_category_id,
                bt.categorization_source, bt.created_at,
                tc.name, tc.icon, tc.color
         FROM bank_transactions bt
         LEFT JOIN transaction_categories tc ON bt.category_id = tc.id
         WHERE {} ORDER BY bt.booking_date DESC LIMIT ? OFFSET ?",
        where_clause
    );
    p.push(Box::new(limit));
    p.push(Box::new(offset));
    let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|x| x.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let txs: Vec<Value> = stmt.query_map(refs.as_slice(), |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "txType": row.get::<_, String>(1)?,
        "amount": row.get::<_, String>(2)?,
        "currency": row.get::<_, String>(3)?,
        "description": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
        "counterpartyName": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
        "counterpartyIban": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
        "bookingDate": row.get::<_, i64>(7)?,
        "valueDate": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
        "status": sql_to_json(row.get::<_, rusqlite::types::Value>(9).unwrap_or(rusqlite::types::Value::Null)),
        "remittanceInfo": sql_to_json(row.get::<_, rusqlite::types::Value>(10).unwrap_or(rusqlite::types::Value::Null)),
        "suggestedCategoryId": sql_to_json(row.get::<_, rusqlite::types::Value>(11).unwrap_or(rusqlite::types::Value::Null)),
        "categorizationSource": sql_to_json(row.get::<_, rusqlite::types::Value>(12).unwrap_or(rusqlite::types::Value::Null)),
        "createdAt": row.get::<_, i64>(13)?,
        "categoryName": sql_to_json(row.get::<_, rusqlite::types::Value>(14).unwrap_or(rusqlite::types::Value::Null)),
        "categoryIcon": sql_to_json(row.get::<_, rusqlite::types::Value>(15).unwrap_or(rusqlite::types::Value::Null)),
        "categoryColor": sql_to_json(row.get::<_, rusqlite::types::Value>(16).unwrap_or(rusqlite::types::Value::Null)),
    })))?.filter_map(|r| r.ok()).collect();
    Ok(serde_json::json!({ "total": total, "limit": limit, "offset": offset, "transactions": txs }))
}

pub fn bank_categories(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT id, name, icon, color, parent_id, sort_order, is_system, created_at
         FROM transaction_categories ORDER BY sort_order ASC",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "name": row.get::<_, String>(1)?,
        "icon": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
        "color": sql_to_json(row.get::<_, rusqlite::types::Value>(3).unwrap_or(rusqlite::types::Value::Null)),
        "parentId": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
        "sortOrder": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
        "isSystem": row.get::<_, i32>(6)? != 0,
        "createdAt": row.get::<_, i64>(7)?,
    })))?.filter_map(|r| r.ok()).collect();
    Ok(Value::Array(rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE bank_accounts (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                account_type TEXT NOT NULL DEFAULT 'checking',
                iban TEXT,
                bban TEXT,
                currency TEXT NOT NULL DEFAULT 'CZK',
                balance TEXT NOT NULL DEFAULT '0',
                institution_id TEXT,
                external_account_id TEXT,
                data_source TEXT NOT NULL DEFAULT 'manual',
                last_synced_at INTEGER,
                interest_rate TEXT,
                has_zone_designation INTEGER NOT NULL DEFAULT 0,
                termination_date INTEGER,
                exclude_from_balance INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE transaction_categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                icon TEXT,
                color TEXT,
                parent_id TEXT REFERENCES transaction_categories(id),
                sort_order INTEGER NOT NULL DEFAULT 0,
                is_system INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
                transaction_id TEXT,
                tx_type TEXT NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                description TEXT,
                counterparty_name TEXT,
                counterparty_iban TEXT,
                booking_date INTEGER NOT NULL,
                value_date INTEGER,
                category_id TEXT REFERENCES transaction_categories(id),
                merchant_category_code TEXT,
                remittance_info TEXT,
                suggested_category_id TEXT REFERENCES transaction_categories(id),
                status TEXT NOT NULL DEFAULT 'booked',
                data_source TEXT NOT NULL DEFAULT 'manual',
                created_at INTEGER NOT NULL DEFAULT 0,
                import_batch_id TEXT,
                categorization_source TEXT,
                UNIQUE(bank_account_id, transaction_id)
            );

            INSERT INTO bank_accounts (id, name, iban, currency, created_at, updated_at)
                VALUES ('acc-1', 'Main Account', 'CZ6508000000192000145399', 'CZK', 0, 0);

            INSERT INTO transaction_categories (id, name, created_at)
                VALUES ('cat-1', 'Groceries', 0);
            "#,
        )
        .expect("schema");
        conn
    }

    fn row(
        tx_type: &str,
        amount: &str,
        booking_date: i64,
        desc: Option<&str>,
    ) -> BankTransactionRow {
        BankTransactionRow {
            transaction_id: None,
            tx_type: tx_type.to_string(),
            amount: amount.to_string(),
            currency: None,
            description: desc.map(String::from),
            counterparty_name: None,
            counterparty_iban: None,
            booking_date,
            value_date: None,
            category_id: None,
            import_anyway: None,
        }
    }

    fn tx_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM bank_transactions", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn create_rejects_unknown_account() {
        let mut conn = setup_test_db();
        let args = BankTransactionsCreateArgs {
            account_id: "no-such-account".into(),
            transactions: vec![row("debit", "10", 1700000000, None)],
        };
        assert!(matches!(
            bank_transactions_create(&mut conn, &args, None).unwrap_err(),
            AppError::NotFound(_)
        ));
        assert_eq!(tx_count(&conn), 0);
    }

    #[test]
    fn create_rejects_oversized_batch() {
        let mut conn = setup_test_db();
        let oversized_count = MAX_BULK_ROWS + 1;
        let mut transactions = Vec::with_capacity(oversized_count);
        for i in 0..oversized_count {
            transactions.push(row("debit", "10", 1_700_000_000 + i as i64, None));
        }
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions,
        };
        let err = bank_transactions_create(&mut conn, &args, None).unwrap_err();
        match err {
            AppError::Validation(msg) => {
                assert!(
                    msg.contains(&oversized_count.to_string()),
                    "message must state the actual count: {msg}"
                );
                assert!(
                    msg.contains(&MAX_BULK_ROWS.to_string()),
                    "message must state the cap: {msg}"
                );
            }
            other => panic!("expected Validation error, got {other:?}"),
        }
        assert_eq!(tx_count(&conn), 0, "an oversized batch must write nothing");
    }

    #[test]
    fn create_phase1_rejection_writes_nothing() {
        let mut conn = setup_test_db();
        let mut bad_category = row("credit", "50", 1700000300, Some("salary"));
        bad_category.category_id = Some("ghost-cat".into());
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![
                row("banana", "10", 1700000000, None), // invalid type
                row("debit", "20", 1700000100, Some("valid row")),
                bad_category, // unknown category
            ],
        };
        let report = bank_transactions_create(&mut conn, &args, None).unwrap();
        assert_eq!(report.created, 0);
        assert!(report.skipped_duplicates.is_empty());
        assert_eq!(report.errors.len(), 2);
        assert_eq!(report.errors[0].index, 0);
        assert_eq!(report.errors[1].index, 2);
        assert!(report.errors[1].message.contains("ghost-cat"));
        assert_eq!(tx_count(&conn), 0, "good row must not be inserted either");
    }

    #[test]
    fn create_happy_path_creates_all() {
        let mut conn = setup_test_db();
        let mut with_category = row("credit", "999.99", 1700000200, Some("salary"));
        with_category.category_id = Some("cat-1".into());
        let mut with_currency = row("debit", "30", 1700000100, Some("lunch"));
        with_currency.currency = Some("EUR".into());
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![
                row("debit", "10", 1700000000, Some("groceries")),
                with_currency,
                with_category,
            ],
        };
        let report = bank_transactions_create(&mut conn, &args, None).unwrap();
        assert_eq!(report.created, 3);
        assert!(report.skipped_duplicates.is_empty());
        assert!(report.errors.is_empty());
        assert_eq!(tx_count(&conn), 3);
        // Currency fallback to the account's currency
        let czk: i64 = conn
            .query_row(
                "SELECT count(*) FROM bank_transactions WHERE currency = 'CZK'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(czk, 2);
    }

    #[test]
    fn create_skips_duplicates_including_intra_batch() {
        let mut conn = setup_test_db();
        let first = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![row("debit", "10", 1700000000, Some("groceries"))],
        };
        assert_eq!(
            bank_transactions_create(&mut conn, &first, None)
                .unwrap()
                .created,
            1
        );

        let second = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![
                row("debit", "10", 1700000000, Some("groceries")), // dup of existing
                row("credit", "77", 1700000500, Some("fresh")),
                row("credit", "77", 1700000500, Some("fresh")), // intra-batch dup
            ],
        };
        let report = bank_transactions_create(&mut conn, &second, None).unwrap();
        assert_eq!(report.created, 1);
        assert!(report.errors.is_empty());
        assert_eq!(report.skipped_duplicates.len(), 2);
        assert_eq!(report.skipped_duplicates[0].index, 0);
        assert_eq!(report.skipped_duplicates[1].index, 2);
        assert_eq!(tx_count(&conn), 2);
    }

    #[test]
    fn create_dedups_on_bank_side_transaction_id() {
        let mut conn = setup_test_db();
        let mut with_id = row("debit", "10", 1700000000, Some("groceries"));
        with_id.transaction_id = Some("FIO-123".into());
        let first = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![with_id],
        };
        assert_eq!(
            bank_transactions_create(&mut conn, &first, None)
                .unwrap()
                .created,
            1
        );

        // Same bank-side id, different other fields → still a duplicate.
        let mut same_id = row("credit", "9999", 1800000000, Some("looks different"));
        same_id.transaction_id = Some("FIO-123".into());
        let second = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![same_id],
        };
        let report = bank_transactions_create(&mut conn, &second, None).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert!(report.skipped_duplicates[0].reason.contains("FIO-123"));
        assert_eq!(tx_count(&conn), 1);
    }

    #[test]
    fn create_stores_lowercase_so_reimport_matches() {
        let mut conn = setup_test_db();
        let first = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![row("DEBIT", "10", 1700000000, Some("groceries"))],
        };
        assert_eq!(
            bank_transactions_create(&mut conn, &first, None)
                .unwrap()
                .created,
            1
        );
        let stored: String = conn
            .query_row("SELECT tx_type FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, "debit", "uppercase input is normalized on insert");

        // Re-import of the same row in canonical lowercase must be caught.
        let second = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![row("debit", "10", 1700000000, Some("groceries"))],
        };
        let report = bank_transactions_create(&mut conn, &second, None).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(tx_count(&conn), 1);
    }

    #[test]
    fn create_treats_empty_transaction_id_as_no_id() {
        let mut conn = setup_test_db();
        let mut r1 = row("debit", "10", 1700000000, Some("coffee"));
        r1.transaction_id = Some("".into());
        let mut r2 = row("debit", "20", 1700000100, Some("lunch"));
        r2.transaction_id = Some("".into());
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![r1, r2],
        };
        let report = bank_transactions_create(&mut conn, &args, None).unwrap();
        assert_eq!(
            report.created, 2,
            "empty ids must not dedup distinct transactions"
        );
        assert!(report.skipped_duplicates.is_empty());
        assert!(report.errors.is_empty());
        // Stored as NULL, never ''
        let non_null_ids: i64 = conn
            .query_row(
                "SELECT count(*) FROM bank_transactions WHERE transaction_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(non_null_ids, 0);
    }

    // ---- identical purchases ----

    #[test]
    fn create_imports_identical_purchases_that_carry_distinct_bank_ids() {
        let mut conn = setup_test_db();
        let mut first = row("debit", "99", 1700000000, Some("coffee"));
        first.transaction_id = Some("FIO-1".into());
        let mut second = row("debit", "99", 1700000000, Some("coffee"));
        second.transaction_id = Some("FIO-2".into());
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![first, second],
        };
        let report = bank_transactions_create(&mut conn, &args, None).unwrap();
        assert_eq!(report.created, 2, "the bank id is the only key");
        assert!(report.skipped_duplicates.is_empty());
        assert_eq!(tx_count(&conn), 2);
    }

    #[test]
    fn create_flags_identical_rows_without_ids_as_possible_duplicates() {
        let mut conn = setup_test_db();
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![
                row("debit", "99", 1700000000, Some("coffee")),
                row("debit", "99", 1700000000, Some("coffee")),
            ],
        };
        let report = bank_transactions_create(&mut conn, &args, None).unwrap();
        assert_eq!(report.created, 1);
        assert_eq!(report.skipped_duplicates.len(), 1);
        let skipped = &report.skipped_duplicates[0];
        assert_eq!(skipped.index, 1);
        assert!(
            skipped.possible_duplicate,
            "an id-less repeat is only a suspect"
        );
        assert!(
            skipped.reason.contains("importAnyway"),
            "the reason tells the client how to act: {}",
            skipped.reason
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["skippedDuplicates"][0]["possibleDuplicate"], true);
    }

    #[test]
    fn create_import_anyway_imports_a_genuine_repeat_purchase() {
        let mut conn = setup_test_db();
        let mut repeat = row("debit", "99", 1700000000, Some("coffee"));
        repeat.import_anyway = Some(true);
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![row("debit", "99", 1700000000, Some("coffee")), repeat],
        };
        let report = bank_transactions_create(&mut conn, &args, None).unwrap();
        assert_eq!(report.created, 2);
        assert!(report.skipped_duplicates.is_empty());

        // The same flagged row sent again later is imported again — the flag
        // is the client's explicit decision, so only send it for rows that
        // came back as possible duplicates.
        let mut again = row("debit", "99", 1700000000, Some("coffee"));
        again.import_anyway = Some(true);
        let report = bank_transactions_create(
            &mut conn,
            &BankTransactionsCreateArgs {
                account_id: "acc-1".into(),
                transactions: vec![again],
            },
            None,
        )
        .unwrap();
        assert_eq!(report.created, 1);
        assert_eq!(tx_count(&conn), 3);
    }

    #[test]
    fn create_import_anyway_does_not_override_a_bank_id_hit() {
        let mut conn = setup_test_db();
        let mut stored = row("debit", "99", 1700000000, Some("coffee"));
        stored.transaction_id = Some("FIO-1".into());
        bank_transactions_create(
            &mut conn,
            &BankTransactionsCreateArgs {
                account_id: "acc-1".into(),
                transactions: vec![stored],
            },
            None,
        )
        .unwrap();

        let mut resent = row("debit", "99", 1700000000, Some("coffee"));
        resent.transaction_id = Some("FIO-1".into());
        resent.import_anyway = Some(true);
        let report = bank_transactions_create(
            &mut conn,
            &BankTransactionsCreateArgs {
                account_id: "acc-1".into(),
                transactions: vec![resent],
            },
            None,
        )
        .unwrap();
        assert_eq!(report.created, 0, "the bank says it is the same payment");
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert!(!report.skipped_duplicates[0].possible_duplicate);
        assert!(report.skipped_duplicates[0].reason.contains("FIO-1"));
        assert_eq!(tx_count(&conn), 1);
    }

    // ---- rules-first import (spec D1/D2) ----

    /// Engine whose single custom rule sends anything mentioning "albert" to
    /// the seeded 'cat-1'.
    fn engine_with_albert_rule(category_id: &str) -> CategorizationEngine {
        use crate::services::categorization::{CategorizationRule, RuleType};
        CategorizationEngine::new(vec![CategorizationRule::new(
            "r1".into(),
            "Albert".into(),
            RuleType::Contains,
            "albert".into(),
            category_id.into(),
        )])
    }

    fn category_and_source(
        conn: &Connection,
        description: &str,
    ) -> (Option<String>, Option<String>) {
        conn.query_row(
            "SELECT category_id, categorization_source FROM bank_transactions WHERE description = ?1",
            [description],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    #[test]
    fn create_auto_applies_rule_matches_with_provenance() {
        let mut conn = setup_test_db();
        let engine = engine_with_albert_rule("cat-1");
        let mut explicit = row("debit", "30", 1700000200, Some("Albert explicit"));
        explicit.category_id = Some("cat-1".into());
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![
                row("debit", "10", 1700000000, Some("Platba kartou Albert")),
                row("debit", "20", 1700000100, Some("Unknown merchant XYZ")),
                explicit,
            ],
        };
        let report = bank_transactions_create(&mut conn, &args, Some(&engine)).unwrap();
        assert_eq!(report.created, 3);

        assert_eq!(
            category_and_source(&conn, "Platba kartou Albert"),
            (Some("cat-1".into()), Some("rule".into())),
            "a rule match is applied and stamped"
        );
        assert_eq!(
            category_and_source(&conn, "Unknown merchant XYZ"),
            (None, None),
            "nothing matched: the row stays uncategorized for the caller to decide"
        );
        assert_eq!(
            category_and_source(&conn, "Albert explicit"),
            (Some("cat-1".into()), None),
            "an explicit categoryId is kept and gets no auto-applied provenance"
        );
    }

    #[test]
    fn create_without_an_engine_leaves_everything_uncategorized() {
        let mut conn = setup_test_db();
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![row("debit", "10", 1700000000, Some("Platba kartou Albert"))],
        };
        assert_eq!(
            bank_transactions_create(&mut conn, &args, None)
                .unwrap()
                .created,
            1
        );
        assert_eq!(
            category_and_source(&conn, "Platba kartou Albert"),
            (None, None)
        );
    }

    #[test]
    fn create_survives_a_rule_pointing_at_a_missing_category() {
        let mut conn = setup_test_db();
        let engine = engine_with_albert_rule("cat-deleted");
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![row("debit", "10", 1700000000, Some("Platba kartou Albert"))],
        };
        let report = bank_transactions_create(&mut conn, &args, Some(&engine)).unwrap();
        assert_eq!(
            report.created, 1,
            "a stale rule must not abort the whole import"
        );
        assert_eq!(
            category_and_source(&conn, "Platba kartou Albert"),
            (None, None)
        );
    }

    // ---- read filters ----

    fn read_args(account_id: &str) -> BankTransactionsArgs {
        BankTransactionsArgs {
            account_id: account_id.to_string(),
            date_from: None,
            date_to: None,
            category_id: None,
            uncategorized: None,
            tx_type: None,
            search: None,
            limit: None,
            offset: None,
        }
    }

    /// Descriptions of the transactions the read tool returned, in order.
    fn read_descriptions(conn: &Connection, args: &BankTransactionsArgs) -> Vec<String> {
        let value = bank_transactions(conn, &args.account_id, args, 100, 0).unwrap();
        value["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["description"].as_str().unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn uncategorized_filter_returns_only_rows_without_a_category() {
        let mut conn = setup_test_db();
        let engine = engine_with_albert_rule("cat-1");
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![
                row("debit", "10", 1700000000, Some("Platba kartou Albert")),
                row("debit", "20", 1700000100, Some("Unknown merchant XYZ")),
            ],
        };
        assert_eq!(
            bank_transactions_create(&mut conn, &args, Some(&engine))
                .unwrap()
                .created,
            2
        );

        let all = read_args("acc-1");
        assert_eq!(read_descriptions(&conn, &all).len(), 2);

        let mut only_uncategorized = read_args("acc-1");
        only_uncategorized.uncategorized = Some(true);
        assert_eq!(
            read_descriptions(&conn, &only_uncategorized),
            vec!["Unknown merchant XYZ".to_string()],
            "the rule-matched row is not what the LLM still has to place"
        );
        let value = bank_transactions(&conn, "acc-1", &only_uncategorized, 100, 0).unwrap();
        assert_eq!(value["total"], 1, "the count honors the filter too");

        // false behaves exactly like omitting it.
        let mut explicit_false = read_args("acc-1");
        explicit_false.uncategorized = Some(false);
        assert_eq!(read_descriptions(&conn, &explicit_false).len(), 2);
    }

    #[test]
    fn uncategorized_filter_wins_over_category_id() {
        let mut conn = setup_test_db();
        let engine = engine_with_albert_rule("cat-1");
        let args = BankTransactionsCreateArgs {
            account_id: "acc-1".into(),
            transactions: vec![
                row("debit", "10", 1700000000, Some("Platba kartou Albert")),
                row("debit", "20", 1700000100, Some("Unknown merchant XYZ")),
            ],
        };
        bank_transactions_create(&mut conn, &args, Some(&engine)).unwrap();

        let mut both = read_args("acc-1");
        both.uncategorized = Some(true);
        both.category_id = Some("cat-1".into());
        assert_eq!(
            read_descriptions(&conn, &both),
            vec!["Unknown merchant XYZ".to_string()],
            "documented precedence: uncategorized wins, instead of an empty page"
        );
    }

    fn account_args(name: &str, iban: Option<&str>) -> BankAccountCreateArgs {
        BankAccountCreateArgs {
            name: name.to_string(),
            account_type: None,
            iban: iban.map(String::from),
            bban: None,
            currency: None,
            balance: None,
            interest_rate: None,
        }
    }

    #[test]
    fn account_create_happy_path() {
        let conn = setup_test_db();
        let value = bank_account_create(&conn, &account_args("Fresh Account", None)).unwrap();
        assert_eq!(value["name"], "Fresh Account");
        assert_eq!(value["accountType"], "checking");
        assert_eq!(value["currency"], "CZK");
        assert_eq!(value["balance"], "0");
    }

    #[test]
    fn account_create_rejects_duplicate_name_case_insensitive() {
        let conn = setup_test_db();
        // Seeded account is 'Main Account'
        assert!(matches!(
            bank_account_create(&conn, &account_args("main account", None)).unwrap_err(),
            AppError::Validation(_)
        ));
    }

    #[test]
    fn account_create_rejects_duplicate_iban() {
        let conn = setup_test_db();
        assert!(matches!(
            bank_account_create(
                &conn,
                &account_args("Different Name", Some("CZ6508000000192000145399"))
            )
            .unwrap_err(),
            AppError::Validation(_)
        ));
    }

    #[test]
    fn account_create_validates_input() {
        let conn = setup_test_db();
        let mut args = account_args("Broken", None);
        args.account_type = Some("hedge_fund".into());
        assert!(matches!(
            bank_account_create(&conn, &args).unwrap_err(),
            AppError::Validation(_)
        ));
    }

    fn seed_tx(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date)
             VALUES (?1, 'acc-1', 'debit', '10', 'CZK', 1700000000)",
            [id],
        )
        .unwrap();
    }

    #[test]
    fn categorize_rejects_oversized_batch() {
        let mut conn = setup_test_db();
        let oversized_count = MAX_BULK_ROWS + 1;
        let assignments: Vec<CategoryAssignment> = (0..oversized_count)
            .map(|i| CategoryAssignment {
                transaction_id: format!("t{i}"),
                category_id: "cat-1".into(),
            })
            .collect();
        let args = BankTransactionsCategorizeArgs { assignments };
        let err = bank_transactions_categorize(&mut conn, &args).unwrap_err();
        match err {
            AppError::Validation(msg) => {
                assert!(
                    msg.contains(&oversized_count.to_string()),
                    "message must state the actual count: {msg}"
                );
                assert!(
                    msg.contains(&MAX_BULK_ROWS.to_string()),
                    "message must state the cap: {msg}"
                );
            }
            other => panic!("expected Validation error, got {other:?}"),
        }
        let categorized: i64 = conn
            .query_row(
                "SELECT count(*) FROM bank_transactions WHERE category_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(categorized, 0, "an oversized batch must write nothing");
    }

    #[test]
    fn categorize_all_or_nothing() {
        let mut conn = setup_test_db();
        seed_tx(&conn, "t1");
        seed_tx(&conn, "t2");
        let args = BankTransactionsCategorizeArgs {
            assignments: vec![
                CategoryAssignment {
                    transaction_id: "t1".into(),
                    category_id: "cat-1".into(),
                },
                CategoryAssignment {
                    transaction_id: "ghost-tx".into(),
                    category_id: "cat-1".into(),
                },
                CategoryAssignment {
                    transaction_id: "t2".into(),
                    category_id: "ghost-cat".into(),
                },
            ],
        };
        let report = bank_transactions_categorize(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 2);
        assert_eq!(report.errors[0].index, 1);
        assert!(report.errors[0].message.contains("ghost-tx"));
        assert_eq!(report.errors[1].index, 2);
        assert!(report.errors[1].message.contains("ghost-cat"));
        // The valid assignment in the same batch must NOT have been applied.
        let categorized: i64 = conn
            .query_row(
                "SELECT count(*) FROM bank_transactions WHERE category_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(categorized, 0);
    }

    #[test]
    fn categorize_rejects_duplicate_assignments() {
        let mut conn = setup_test_db();
        seed_tx(&conn, "t1");
        let args = BankTransactionsCategorizeArgs {
            assignments: vec![
                CategoryAssignment {
                    transaction_id: "t1".into(),
                    category_id: "cat-1".into(),
                },
                CategoryAssignment {
                    transaction_id: "t1".into(),
                    category_id: "cat-1".into(),
                },
            ],
        };
        let report = bank_transactions_categorize(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(
            report.errors[0].index, 1,
            "the repeat is the error, not the first"
        );
        assert!(report.errors[0].message.contains("duplicate assignment"));
        let categorized: i64 = conn
            .query_row(
                "SELECT count(*) FROM bank_transactions WHERE category_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(categorized, 0, "all-or-nothing: nothing applied");
    }

    #[test]
    fn categorize_happy_path_updates_all() {
        let mut conn = setup_test_db();
        seed_tx(&conn, "t1");
        seed_tx(&conn, "t2");
        let args = BankTransactionsCategorizeArgs {
            assignments: vec![
                CategoryAssignment {
                    transaction_id: "t1".into(),
                    category_id: "cat-1".into(),
                },
                CategoryAssignment {
                    transaction_id: "t2".into(),
                    category_id: "cat-1".into(),
                },
            ],
        };
        let report = bank_transactions_categorize(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2);
        assert!(report.errors.is_empty());
        assert!(report.skipped_duplicates.is_empty());
        // Assignments land as SUGGESTIONS: category_id stays NULL until the
        // user confirms in the app (spec 2026-08-27 D5).
        let suggested: i64 = conn
            .query_row(
                "SELECT count(*) FROM bank_transactions WHERE suggested_category_id = 'cat-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(suggested, 2);
        let categorized: i64 = conn
            .query_row(
                "SELECT count(*) FROM bank_transactions WHERE category_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(categorized, 0, "a suggestion never sets the final category");
        assert_eq!(categorization_source_of(&conn, "t1"), None);
    }

    // ---- suggestions never touch final categories (spec 2026-08-27 D5) ----

    fn categorize_args(assignments: Vec<(&str, &str)>) -> BankTransactionsCategorizeArgs {
        BankTransactionsCategorizeArgs {
            assignments: assignments
                .into_iter()
                .map(|(transaction_id, category_id)| CategoryAssignment {
                    transaction_id: transaction_id.into(),
                    category_id: category_id.into(),
                })
                .collect(),
        }
    }

    fn categorization_source_of(conn: &Connection, id: &str) -> Option<String> {
        conn.query_row(
            "SELECT categorization_source FROM bank_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn category_of(conn: &Connection, id: &str) -> Option<String> {
        conn.query_row(
            "SELECT category_id FROM bank_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// Give a seeded transaction a category the way a rule or the user would:
    /// directly, with whatever provenance.
    fn preset_category(conn: &Connection, id: &str, category_id: &str, source: Option<&str>) {
        conn.execute(
            "UPDATE bank_transactions SET category_id = ?1, categorization_source = ?2 WHERE id = ?3",
            rusqlite::params![category_id, source, id],
        )
        .unwrap();
    }

    fn suggested_of(conn: &Connection, id: &str) -> Option<String> {
        conn.query_row(
            "SELECT suggested_category_id FROM bank_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn categorize_skips_rows_with_a_final_category() {
        let mut conn = setup_test_db();
        seed_tx(&conn, "t1");
        seed_tx(&conn, "t2");
        conn.execute(
            "INSERT INTO transaction_categories (id, name, created_at) VALUES ('cat-2', 'Dining', 0)",
            [],
        )
        .unwrap();
        // t1 was categorized by a rule at import time; t2 is still open.
        preset_category(&conn, "t1", "cat-1", Some("rule"));

        let args = categorize_args(vec![("t1", "cat-2"), ("t2", "cat-2")]);
        let report = bank_transactions_categorize(&mut conn, &args).unwrap();

        assert_eq!(report.created, 1, "only the uncategorized row is written");
        assert!(report.errors.is_empty());
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(report.skipped_duplicates[0].index, 0);
        assert!(
            report.skipped_duplicates[0].reason.contains("Groceries"),
            "the skip names the existing category: {}",
            report.skipped_duplicates[0].reason
        );

        assert_eq!(category_of(&conn, "t1").as_deref(), Some("cat-1"));
        assert_eq!(
            categorization_source_of(&conn, "t1").as_deref(),
            Some("rule"),
            "the protected row is untouched, provenance included"
        );
        assert_eq!(
            category_of(&conn, "t2"),
            None,
            "a suggestion never sets the final category"
        );
        assert_eq!(suggested_of(&conn, "t2").as_deref(), Some("cat-2"));
    }

    #[test]
    fn categorize_replaces_a_pending_suggestion() {
        let mut conn = setup_test_db();
        seed_tx(&conn, "t1");
        conn.execute(
            "INSERT INTO transaction_categories (id, name, created_at) VALUES ('cat-2', 'Dining', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE bank_transactions SET suggested_category_id = 'cat-1' WHERE id = 't1'",
            [],
        )
        .unwrap();

        let args = categorize_args(vec![("t1", "cat-2")]);
        let report = bank_transactions_categorize(&mut conn, &args).unwrap();

        assert_eq!(report.created, 1);
        assert!(report.skipped_duplicates.is_empty());
        assert_eq!(suggested_of(&conn, "t1").as_deref(), Some("cat-2"));
        assert_eq!(category_of(&conn, "t1"), None);
    }

    #[test]
    fn categorize_still_rejects_invalid_ids_even_when_rows_would_be_skipped() {
        let mut conn = setup_test_db();
        seed_tx(&conn, "t1");
        preset_category(&conn, "t1", "cat-1", Some("rule"));

        let args = categorize_args(vec![("t1", "cat-1"), ("ghost-tx", "cat-1")]);
        let report = bank_transactions_categorize(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert!(
            report.skipped_duplicates.is_empty(),
            "an error batch reports errors only, and writes nothing"
        );
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("ghost-tx"));
    }
}
