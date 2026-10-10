//! Bank account business logic service
//!
//! All business logic for bank accounts lives here.
//! Commands should only validate input, call these functions, and handle events.
//! This is the SINGLE SOURCE OF TRUTH for bank account operations.

use crate::error::{AppError, Result};
use crate::models::bank_accounts::{BankBalanceRecord, InsertBalanceRecord};
use crate::models::{BankAccount, BankTransaction, InsertBankAccount, InsertBankTransaction};
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

/// Check whether a transaction category exists. Shared by transaction
/// creation, categorization, and the MCP bulk-import validation phase.
pub(crate) fn category_exists(conn: &rusqlite::Connection, id: &str) -> Result<bool> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM transaction_categories WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

/// Get a bank account by ID
/// Helper function to fetch a bank account from the database
pub fn get_account_by_id(conn: &rusqlite::Connection, id: &str) -> Result<BankAccount> {
    let account = conn.query_row(
        "SELECT id, name, account_type, iban, bban, currency, balance, institution_id,
         external_account_id, data_source, last_synced_at, interest_rate, has_zone_designation,
         termination_date, created_at, updated_at, exclude_from_balance, interest_rate_valid_until, balance_as_of
         FROM bank_accounts WHERE id = ?1",
        [id],
        |row| {
            Ok(BankAccount {
                id: row.get(0)?,
                name: row.get(1)?,
                account_type: row.get(2)?,
                iban: row.get(3)?,
                bban: row.get(4)?,
                currency: row.get(5)?,
                balance: row.get(6)?,
                balance_as_of: row.get(18)?,
                institution_id: row.get(7)?,
                external_account_id: row.get(8)?,
                data_source: row.get(9)?,
                last_synced_at: row.get(10)?,
                interest_rate: row.get(11)?,
                has_zone_designation: row.get::<_, i32>(12)? != 0,
                termination_date: row.get(13)?,
                interest_rate_valid_until: row.get(17)?,
                exclude_from_balance: row.get::<_, i32>(16)? != 0,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        },
    )?;
    Ok(account)
}

/// Create a new bank account
/// This is the SINGLE SOURCE OF TRUTH for bank account creation
pub fn create_account(
    conn: &rusqlite::Connection,
    data: &InsertBankAccount,
) -> Result<BankAccount> {
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let account_type = data
        .account_type
        .clone()
        .unwrap_or_else(|| "checking".to_string());
    let currency = data
        .currency
        .clone()
        .unwrap_or_else(|| crate::constants::STORAGE_DEFAULT_CURRENCY.to_string());
    let balance = data.balance.clone().unwrap_or_else(|| "0".to_string());
    let has_zone = data.has_zone_designation.unwrap_or(false);
    let exclude = data.exclude_from_balance.unwrap_or(false);

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO bank_accounts (
            id, name, account_type, iban, bban, currency, balance,
            institution_id, data_source, interest_rate, has_zone_designation,
            termination_date, created_at, updated_at, exclude_from_balance,
            interest_rate_valid_until, balance_as_of
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id,
            data.name,
            account_type,
            data.iban,
            data.bban,
            currency,
            balance,
            data.institution_id,
            "manual",
            data.interest_rate,
            has_zone as i32,
            data.termination_date,
            now,
            now,
            exclude as i32,
            data.interest_rate_valid_until,
            data.balance_as_of.flatten(),
        ],
    )?;
    // A dated opening balance is the first record of the account's history
    if let Some(day) = data.balance_as_of.flatten() {
        tx.execute(
            "INSERT INTO bank_balance_records
                (id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'manual', NULL, ?6)",
            params![Uuid::new_v4().to_string(), id, balance, currency, day, now],
        )?;
    }
    tx.commit()?;

    Ok(BankAccount {
        id,
        name: data.name.clone(),
        account_type,
        iban: data.iban.clone(),
        bban: data.bban.clone(),
        currency,
        balance,
        balance_as_of: data.balance_as_of.flatten(),
        institution_id: data.institution_id.clone(),
        external_account_id: None,
        data_source: "manual".to_string(),
        last_synced_at: None,
        interest_rate: data.interest_rate.clone(),
        has_zone_designation: has_zone,
        termination_date: data.termination_date,
        interest_rate_valid_until: data.interest_rate_valid_until,
        exclude_from_balance: exclude,
        created_at: now,
        updated_at: now,
    })
}

/// Update an existing bank account
/// This is the SINGLE SOURCE OF TRUTH for bank account updates
pub fn update_account(
    conn: &rusqlite::Connection,
    id: &str,
    data: &InsertBankAccount,
) -> Result<BankAccount> {
    let now = chrono::Utc::now().timestamp();

    // Get existing account first
    let existing = get_account_by_id(conn, id)?;

    let account_type = data.account_type.clone().unwrap_or(existing.account_type);
    let currency = data.currency.clone().unwrap_or(existing.currency.clone());
    // The balance is never edited here: it mirrors the newest balance record
    // (record_balance / delete_balance_record).
    let balance = existing.balance.clone();
    let balance_as_of = existing.balance_as_of;
    let has_zone = data
        .has_zone_designation
        .unwrap_or(existing.has_zone_designation);
    let exclude = data
        .exclude_from_balance
        .unwrap_or(existing.exclude_from_balance);

    conn.execute(
        "UPDATE bank_accounts SET 
            name = ?1, account_type = ?2, iban = ?3, bban = ?4, currency = ?5,
            balance = ?6, institution_id = ?7, interest_rate = ?8, has_zone_designation = ?9,
            termination_date = ?10, exclude_from_balance = ?11, updated_at = ?12,
            interest_rate_valid_until = ?13, balance_as_of = ?15
        WHERE id = ?14",
        params![
            data.name,
            account_type,
            data.iban,
            data.bban,
            currency,
            balance,
            data.institution_id,
            data.interest_rate,
            has_zone as i32,
            data.termination_date,
            exclude as i32,
            now,
            data.interest_rate_valid_until,
            id,
            balance_as_of,
        ],
    )?;

    Ok(BankAccount {
        id: id.to_string(),
        name: data.name.clone(),
        account_type,
        iban: data.iban.clone(),
        bban: data.bban.clone(),
        currency,
        balance,
        balance_as_of,
        institution_id: data.institution_id.clone(),
        external_account_id: existing.external_account_id,
        data_source: existing.data_source,
        last_synced_at: existing.last_synced_at,
        interest_rate: data.interest_rate.clone(),
        has_zone_designation: has_zone,
        termination_date: data.termination_date,
        interest_rate_valid_until: data.interest_rate_valid_until,
        exclude_from_balance: exclude,
        created_at: existing.created_at,
        updated_at: now,
    })
}

/// Delete a bank account
/// Note: This does NOT delete related transactions - that's handled by the DB cascade
/// or should be done explicitly before calling this function
pub fn delete_account(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    let changes = conn.execute("DELETE FROM bank_accounts WHERE id = ?1", [id])?;
    if changes == 0 {
        return Err(AppError::NotFound("Bank account not found".into()));
    }
    Ok(())
}

/// Pre-write rules-first enrichment, shared by both import paths (spec D2):
/// a row the caller left uncategorized gets the category the engine's
/// deterministic layers derive, and the `categorization_source` label to store
/// with it. Returns `None` — leaving the row uncategorized — when the caller
/// was explicit, when no engine is available, or when only the ML classifier
/// had an opinion (spec D1).
///
/// The category is dropped if it no longer exists: a rule can in principle
/// outlive its category, and an import that leaves one row uncategorized is
/// far better than one that aborts with `Category not found`.
///
/// This is defense-in-depth more than a commonly-hit path: `categorization_rules.category_id`
/// and `learned_payees.category_id` both carry `FOREIGN KEY ... REFERENCES
/// transaction_categories(id)`, and the app runs with `PRAGMA foreign_keys = ON`,
/// so the DB itself mostly prevents a rule or learned payee from outliving its
/// category. The reachable trigger is mainly the in-memory `CategorizationEngine`
/// going stale relative to the DB (e.g. its rule cache isn't reloaded after a
/// category delete). Without this guard, a stale rule would carry its dead
/// category_id straight into `create_transaction_with_origin`, whose own
/// `category_exists` check would reject the row with a clean `Category not
/// found` for MCP's all-or-nothing bulk create — but for CSV import, whose
/// per-row loop is not wrapped in per-row error recovery, that same rejection
/// would propagate out and abort the entire batch instead of just the one row.
pub fn enrich_with_rules(
    conn: &rusqlite::Connection,
    data: &mut InsertBankTransaction,
    engine: Option<&crate::services::categorization::CategorizationEngine>,
) -> Result<Option<&'static str>> {
    let Some(source) = crate::services::categorization::apply::resolve_and_apply(data, engine)
    else {
        return Ok(None);
    };
    let assigned = data.category_id.clone().unwrap_or_default();
    if !category_exists(conn, &assigned)? {
        log::warn!(
            "Categorization matched missing category {} ({}); importing uncategorized",
            assigned,
            source
        );
        data.category_id = None;
        return Ok(None);
    }
    Ok(Some(source))
}

/// Create a bank transaction. SINGLE SOURCE OF TRUTH — used by the Tauri
/// command and the MCP write tool.
pub fn create_transaction(
    conn: &rusqlite::Connection,
    data: &InsertBankTransaction,
) -> Result<BankTransaction> {
    create_transaction_with_origin(conn, data, "manual", None, None)
}

/// The same write path with an explicit origin: the CSV import stamps rows
/// with data_source 'csv_import' and links them to their csv_import_batches
/// row so a batch can be deleted as a unit.
///
/// `categorization_source` records how `data.category_id` was decided (spec
/// D3): the import paths pass `rule` / `exact_match` / `own_account` for a
/// category the engine's deterministic layers supplied. `None` — the only
/// value the manual UI path passes — stores NULL, which continues to mean
/// "nobody recorded a provenance for this".
pub fn create_transaction_with_origin(
    conn: &rusqlite::Connection,
    data: &InsertBankTransaction,
    data_source: &str,
    import_batch_id: Option<&str>,
    categorization_source: Option<&str>,
) -> Result<BankTransaction> {
    data.validate()?;

    if let Some(ref cat) = data.category_id {
        if !category_exists(conn, cat)? {
            return Err(AppError::NotFound(format!("Category not found: {}", cat)));
        }
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Normalize enum-like fields to lowercase so stored values match both
    // validate()'s lowercase comparisons and the dedup queries, which compare
    // a lowercased tx_type against the stored value.
    let transaction_id = data
        .transaction_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let tx_type = data.tx_type.to_lowercase();
    let status = data
        .status
        .as_deref()
        .map(str::to_lowercase)
        .unwrap_or_else(|| "booked".to_string());

    // Get account currency if not provided
    let currency = if let Some(ref c) = data.currency {
        c.clone()
    } else {
        conn.query_row(
            "SELECT currency FROM bank_accounts WHERE id = ?1",
            [&data.bank_account_id],
            |row| row.get::<_, String>(0),
        )?
    };

    conn.execute(
        "INSERT INTO bank_transactions (
            id, bank_account_id, transaction_id, tx_type, amount, currency,
            description, counterparty_name, counterparty_iban, booking_date, value_date,
            category_id, status, data_source, created_at, import_batch_id,
            categorization_source, flow_kind, counterparty_bank_code, counterparty_country
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            id,
            data.bank_account_id,
            transaction_id,
            tx_type,
            data.amount,
            currency,
            data.description,
            data.counterparty_name,
            data.counterparty_iban,
            data.booking_date,
            data.value_date,
            data.category_id,
            status,
            data_source,
            now,
            import_batch_id,
            categorization_source,
            data.flow_kind.map(|kind| kind.to_string()),
            data.counterparty_bank_code,
            data.counterparty_country,
        ],
    )?;

    Ok(BankTransaction {
        flow_kind: data.flow_kind,
        id,
        bank_account_id: data.bank_account_id.clone(),
        transaction_id,
        tx_type,
        amount: data.amount.clone(),
        currency,
        description: data.description.clone(),
        counterparty_name: data.counterparty_name.clone(),
        counterparty_iban: data.counterparty_iban.clone(),
        counterparty_bank_code: data.counterparty_bank_code.clone(),
        counterparty_country: data.counterparty_country.clone(),
        booking_date: data.booking_date,
        value_date: data.value_date,
        category_id: data.category_id.clone(),
        merchant_category_code: None,
        remittance_info: None,
        suggested_category_id: None,
        status,
        data_source: data_source.to_string(),
        created_at: now,
        import_batch_id: import_batch_id.map(str::to_string),
        import_file_name: None,
        import_line: None,
        row_balance: None,
    })
}

/// Same bank ID turns pending into booked, retaining the original row ownership,
/// explicit user flow and categorization. A later batch cannot delete this row.
pub fn promote_pending_transaction(
    conn: &rusqlite::Connection,
    id: &str,
    data: &InsertBankTransaction,
) -> Result<()> {
    data.validate()?;
    // Compare legacy padded IDs using the shared normalization, but retain the
    // original stored value for the guarded update and for source provenance.
    let stored_id: Option<String> = conn
        .query_row(
            "SELECT transaction_id FROM bank_transactions WHERE id=?1 AND bank_account_id=?2 AND status='pending'",
            params![id, data.bank_account_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let incoming_id = super::dedup::normalized_bank_id(data.transaction_id.as_deref());
    if incoming_id.is_none()
        || incoming_id != super::dedup::normalized_bank_id(stored_id.as_deref())
    {
        return Err(AppError::Validation(
            "validation.pendingPromotionInvalid".into(),
        ));
    }
    let count=conn.execute("UPDATE bank_transactions SET status='booked',booking_date=?1,value_date=?2,amount=?3,currency=COALESCE(?4,currency),tx_type=?5,description=COALESCE(?6,description),counterparty_name=COALESCE(?7,counterparty_name),counterparty_iban=COALESCE(?8,counterparty_iban),counterparty_bank_code=COALESCE(?9,counterparty_bank_code),counterparty_country=COALESCE(?10,counterparty_country) WHERE id=?11 AND bank_account_id=?12 AND transaction_id=?13 AND status='pending'",params![data.booking_date,data.value_date,data.amount,data.currency,data.tx_type.to_lowercase(),data.description,data.counterparty_name,data.counterparty_iban,data.counterparty_bank_code,data.counterparty_country,id,data.bank_account_id,stored_id])?;
    if count != 1 {
        return Err(AppError::Validation(
            "validation.pendingPromotionInvalid".into(),
        ));
    }
    Ok(())
}

/// Set (or clear) a transaction's category. Checks both sides exist.
///
/// `categorization_source` is always written alongside the category, so the
/// provenance can never outlive the category it described: accepting an MCP
/// suggestion passes `Some("mcp")`, and the manual UI path passes `None`,
/// which stores NULL — "no recorded provenance" (spec D3). Any category write
/// also clears a pending `suggested_category_id`: once a category is decided,
/// a stale suggestion must not linger. This function is deliberately
/// unguarded; the no-overwrite rule lives in the MCP layer, not here, so the
/// UI keeps re-assigning categories freely.
pub fn set_transaction_category(
    conn: &rusqlite::Connection,
    transaction_id: &str,
    category_id: Option<&str>,
    categorization_source: Option<&str>,
) -> Result<()> {
    if let Some(cat) = category_id {
        if !category_exists(conn, cat)? {
            return Err(AppError::NotFound(format!("Category not found: {}", cat)));
        }
    }
    let updated = conn.execute(
        "UPDATE bank_transactions
         SET category_id = ?1, categorization_source = ?2, suggested_category_id = NULL
         WHERE id = ?3",
        rusqlite::params![category_id, categorization_source, transaction_id],
    )?;
    if updated == 0 {
        return Err(AppError::NotFound(format!(
            "Transaction not found: {}",
            transaction_id
        )));
    }
    Ok(())
}

/// Set or clear explicit financial meaning independently of category.
pub fn set_transaction_flow(
    conn: &rusqlite::Connection,
    transaction_id: &str,
    flow_kind: Option<crate::models::FlowKind>,
) -> Result<()> {
    let updated = conn.execute(
        "UPDATE bank_transactions SET flow_kind=?1 WHERE id=?2",
        params![flow_kind.map(|kind| kind.to_string()), transaction_id],
    )?;
    if updated == 0 {
        return Err(AppError::NotFound(format!(
            "Transaction not found: {transaction_id}"
        )));
    }
    Ok(())
}

/// Mirror the newest dated record into `bank_accounts.balance` / `balance_as_of`;
/// without any record the account goes back to `'0'` / unknown date.
fn refresh_balance_columns(conn: &rusqlite::Connection, account_id: &str) -> Result<()> {
    let newest: Option<(String, i64)> = conn
        .query_row(
            "SELECT amount, as_of FROM bank_balance_records
             WHERE bank_account_id = ?1 ORDER BY as_of DESC LIMIT 1",
            [account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match newest {
        Some((amount, day)) => conn.execute(
            "UPDATE bank_accounts SET balance = ?1, balance_as_of = ?2, updated_at = unixepoch() WHERE id = ?3",
            params![amount, day, account_id],
        )?,
        None => conn.execute(
            "UPDATE bank_accounts SET balance = '0', balance_as_of = NULL, updated_at = unixepoch() WHERE id = ?1",
            [account_id],
        )?,
    };
    Ok(())
}

/// Record a dated balance. The same day is replaced; an older day is history and
/// leaves the current balance alone; the newest day becomes the current balance.
pub fn record_balance(
    conn: &rusqlite::Connection,
    account_id: &str,
    data: &InsertBalanceRecord,
) -> Result<BankAccount> {
    data.validate()?;
    let tx = conn.unchecked_transaction()?;
    let existing = get_account_by_id(&tx, account_id)?;
    if !existing.currency.eq_ignore_ascii_case(&data.currency) {
        return Err(AppError::Validation(
            "validation.balanceCurrencyMismatch".into(),
        ));
    }
    let has_dated: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM bank_balance_records WHERE bank_account_id = ?1)",
        [account_id],
        |r| r.get(0),
    )?;
    let undated_nonzero = existing.balance_as_of.is_none()
        && super::dedup::canonical_amount(&existing.balance) != super::dedup::canonical_amount("0");
    if !has_dated && undated_nonzero && !data.confirm_unknown_as_of {
        return Err(AppError::Validation(
            "validation.balanceAnchorUnknown".into(),
        ));
    }
    if let Some(batch) = &data.source_batch_id {
        let belongs: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM csv_import_batches WHERE id = ?1 AND bank_account_id = ?2)",
            params![batch, account_id],
            |r| r.get(0),
        )?;
        if !belongs {
            return Err(AppError::Validation(
                "validation.balanceBatchMismatch".into(),
            ));
        }
    }
    let source = if data.source_batch_id.is_some() {
        "statement"
    } else {
        "manual"
    };
    tx.execute(
        "INSERT INTO bank_balance_records
            (id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch())
         ON CONFLICT(bank_account_id, as_of) DO UPDATE SET
            amount = excluded.amount, currency = excluded.currency, source = excluded.source,
            source_batch_id = excluded.source_batch_id, recorded_at = unixepoch()",
        params![
            Uuid::new_v4().to_string(),
            account_id,
            data.amount,
            existing.currency,
            data.as_of,
            source,
            data.source_batch_id,
        ],
    )?;
    refresh_balance_columns(&tx, account_id)?;
    let account = get_account_by_id(&tx, account_id)?;
    tx.commit()?;
    Ok(account)
}

/// Every dated balance of the account, newest first.
pub fn list_balance_records(
    conn: &rusqlite::Connection,
    account_id: &str,
) -> Result<Vec<BankBalanceRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at
         FROM bank_balance_records WHERE bank_account_id = ?1 ORDER BY as_of DESC",
    )?;
    let rows = stmt
        .query_map([account_id], |r| {
            Ok(BankBalanceRecord {
                id: r.get(0)?,
                bank_account_id: r.get(1)?,
                amount: r.get(2)?,
                currency: r.get(3)?,
                as_of: r.get(4)?,
                source: r.get(5)?,
                source_batch_id: r.get(6)?,
                recorded_at: r.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Delete one record and re-mirror the newest remaining one into the columns.
pub fn delete_balance_record(conn: &rusqlite::Connection, record_id: &str) -> Result<BankAccount> {
    let tx = conn.unchecked_transaction()?;
    let account_id: String = tx
        .query_row(
            "SELECT bank_account_id FROM bank_balance_records WHERE id = ?1",
            [record_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("Balance record not found: {record_id}")))?;
    tx.execute(
        "DELETE FROM bank_balance_records WHERE id = ?1",
        [record_id],
    )?;
    refresh_balance_columns(&tx, &account_id)?;
    let account = get_account_by_id(&tx, &account_id)?;
    tx.commit()?;
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InsertBankAccount;
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
                balance TEXT NOT NULL DEFAULT '0', balance_as_of INTEGER,
                institution_id TEXT,
                external_account_id TEXT,
                data_source TEXT NOT NULL DEFAULT 'manual',
                last_synced_at INTEGER,
                interest_rate TEXT,
                has_zone_designation INTEGER NOT NULL DEFAULT 0,
                termination_date INTEGER,
                interest_rate_valid_until INTEGER,
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
                counterparty_iban TEXT, counterparty_bank_code TEXT, counterparty_country TEXT, import_line INTEGER, row_balance TEXT,
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
                flow_kind TEXT,
                UNIQUE(bank_account_id, transaction_id)
            );

            CREATE TABLE csv_import_batches (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL
            );

            CREATE TABLE bank_balance_records (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                as_of INTEGER NOT NULL,
                source TEXT NOT NULL DEFAULT 'manual',
                source_batch_id TEXT REFERENCES csv_import_batches(id) ON DELETE SET NULL,
                recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE (bank_account_id, as_of)
            );

            INSERT INTO bank_accounts (id, name, currency, created_at, updated_at)
                VALUES ('acc-1', 'EUR Account', 'EUR', 0, 0);

            INSERT INTO transaction_categories (id, name, created_at)
                VALUES ('cat-1', 'Groceries', 0);

            INSERT INTO bank_transactions (
                id, bank_account_id, tx_type, amount, currency, booking_date, status, data_source, created_at
            ) VALUES ('tx-1', 'acc-1', 'debit', '100', 'EUR', 1700000000, 'booked', 'manual', 0);
            "#,
        )
        .expect("schema");
        conn
    }

    fn minimal_insert(name: &str) -> InsertBankAccount {
        InsertBankAccount {
            name: name.to_string(),
            account_type: None,
            iban: None,
            bban: None,
            currency: None,
            balance: None,
            balance_as_of: None,
            institution_id: None,
            interest_rate: None,
            has_zone_designation: None,
            termination_date: None,
            interest_rate_valid_until: None,
            exclude_from_balance: None,
        }
    }

    #[test]
    fn validate_rejects_nonfinite_account_zone_amounts() {
        for amount in ["NaN", "inf", "-inf", "1e999"] {
            for field in ["fromAmount", "toAmount"] {
                let mut json = serde_json::json!({"savingsAccountId":"acc-1","fromAmount":"0","toAmount":"100","interestRate":"3"});
                json[field] = serde_json::json!(amount);
                let data: crate::models::InsertSavingsAccountZone =
                    serde_json::from_value(json).unwrap();
                assert!(data.validate().is_err(), "accepted {field}={amount}");
            }
        }
    }

    #[test]
    fn validate_rejects_nonfinite_account_balances() {
        for balance in ["NaN", "inf", "-inf", "1e999"] {
            let mut data = minimal_insert("Main");
            data.balance = Some(balance.into());
            assert!(data.validate().is_err(), "accepted {balance}");
        }
    }

    #[test]
    fn test_create_account_sets_defaults() {
        let conn = setup_test_db();
        let account = create_account(&conn, &minimal_insert("Main Account")).expect("create");
        assert_eq!(account.name, "Main Account");
        assert_eq!(account.account_type, "checking");
        assert_eq!(account.currency, "CZK");
        assert_eq!(account.balance, "0");
        assert_eq!(account.data_source, "manual");
        assert!(!account.id.is_empty());
    }

    #[test]
    fn test_create_account_with_iban() {
        let conn = setup_test_db();
        let mut data = minimal_insert("Savings");
        data.iban = Some("CZ6508000000192000145399".to_string());
        data.currency = Some("CZK".to_string());
        let account = create_account(&conn, &data).expect("create");
        assert_eq!(account.iban.as_deref(), Some("CZ6508000000192000145399"));
    }

    #[test]
    fn test_create_account_validates_first() {
        let conn = setup_test_db();
        // Empty name fails validation inside the service itself (spec D2:
        // every creation fn validates first — MCP calls it directly).
        assert!(matches!(
            create_account(&conn, &minimal_insert("")).unwrap_err(),
            AppError::Validation(_)
        ));
        let mut data = minimal_insert("Ok Name");
        data.account_type = Some("hedge_fund".to_string());
        assert!(matches!(
            create_account(&conn, &data).unwrap_err(),
            AppError::Validation(_)
        ));
    }

    #[test]
    fn test_exclude_from_balance_round_trips_and_defaults_to_included() {
        let conn = setup_test_db();
        let created = create_account(&conn, &minimal_insert("Joint account")).expect("create");
        assert!(
            !created.exclude_from_balance,
            "new accounts count toward net worth"
        );

        let mut data = minimal_insert("Joint account");
        data.exclude_from_balance = Some(true);
        let updated = update_account(&conn, &created.id, &data).expect("update");
        assert!(updated.exclude_from_balance);
        assert!(
            get_account_by_id(&conn, &created.id)
                .expect("get")
                .exclude_from_balance
        );

        // `None` keeps the stored value instead of resetting it.
        let keep = minimal_insert("Joint account");
        let kept = update_account(&conn, &created.id, &keep).expect("update");
        assert!(kept.exclude_from_balance);
    }

    #[test]
    fn the_promo_rate_end_is_stored_and_updated() {
        let conn = setup_test_db();
        let mut data = minimal_insert("Spořák");
        data.interest_rate_valid_until = Some(1_790_000_000);
        let created = create_account(&conn, &data).expect("create");
        assert_eq!(created.interest_rate_valid_until, Some(1_790_000_000));
        assert_eq!(
            get_account_by_id(&conn, &created.id)
                .expect("read")
                .interest_rate_valid_until,
            Some(1_790_000_000)
        );
        data.interest_rate_valid_until = None;
        let updated = update_account(&conn, &created.id, &data).expect("update");
        assert_eq!(updated.interest_rate_valid_until, None);
    }

    #[test]
    fn test_get_account_by_id_not_found() {
        let conn = setup_test_db();
        let result = get_account_by_id(&conn, "nonexistent-id");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_then_get_account() {
        let conn = setup_test_db();
        let created = create_account(&conn, &minimal_insert("Test Account")).expect("create");
        let fetched = get_account_by_id(&conn, &created.id).expect("get");
        assert_eq!(created.id, fetched.id);
        assert_eq!(fetched.name, "Test Account");
    }

    #[test]
    fn test_delete_account() {
        let conn = setup_test_db();
        let account = create_account(&conn, &minimal_insert("Delete Me")).expect("create");
        delete_account(&conn, &account.id).expect("delete");
        let result = get_account_by_id(&conn, &account.id);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_nonexistent_account_returns_error() {
        let conn = setup_test_db();
        let result = delete_account(&conn, "does-not-exist");
        assert!(result.is_err());
    }

    #[test]
    fn transaction_flow_edit_and_clear_are_persisted_and_missing_is_an_error() {
        let conn = setup_test_db();
        set_transaction_flow(&conn, "tx-1", Some(crate::models::FlowKind::Transfer)).unwrap();
        let flow: Option<String> = conn
            .query_row(
                "SELECT flow_kind FROM bank_transactions WHERE id='tx-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(flow.as_deref(), Some("transfer"));
        set_transaction_flow(&conn, "tx-1", None).unwrap();
        let flow: Option<String> = conn
            .query_row(
                "SELECT flow_kind FROM bank_transactions WHERE id='tx-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(flow, None);
        assert!(matches!(
            set_transaction_flow(&conn, "missing", None),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn transaction_flow_persists_and_category_only_edits_preserve_it() {
        let conn = setup_test_db();
        let data: InsertBankTransaction=serde_json::from_value(serde_json::json!({"bankAccountId":"acc-1", "type":"credit", "amount":"50", "bookingDate":1700000000,"flowKind":"transfer"})).unwrap();
        let transaction = create_transaction(&conn, &data).unwrap();
        set_transaction_category(&conn, &transaction.id, Some("cat-1"), None).unwrap();
        let kind: String = conn
            .query_row(
                "SELECT flow_kind FROM bank_transactions WHERE id=?1",
                [&transaction.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "transfer");
    }

    #[test]
    fn create_transaction_falls_back_to_account_currency() {
        let conn = setup_test_db(); // must seed one bank_accounts row id='acc-1', currency='EUR'
        let mut data = crate::models::InsertBankTransaction {
            flow_kind: None,
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bank_code: None,
            counterparty_country: None,
            booking_date: 1700000000,
            value_date: None,
            category_id: None,

            status: None,
        };
        let tx = create_transaction(&conn, &data).unwrap();
        assert_eq!(tx.currency, "EUR");
        assert_eq!(tx.status, "booked");
        data.currency = Some("USD".into());
        assert_eq!(create_transaction(&conn, &data).unwrap().currency, "USD");
    }

    #[test]
    fn create_transaction_normalizes_case() {
        let conn = setup_test_db();
        let data = crate::models::InsertBankTransaction {
            flow_kind: None,
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "DEBIT".into(),
            amount: "10".into(),
            currency: None,
            description: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bank_code: None,
            counterparty_country: None,
            booking_date: 1700000000,
            value_date: None,
            category_id: None,

            status: Some("Pending".into()),
        };
        let tx = create_transaction(&conn, &data).unwrap();
        assert_eq!(tx.tx_type, "debit");
        assert_eq!(tx.status, "pending");
        let (stored_type, stored_status): (String, String) = conn
            .query_row(
                "SELECT tx_type, status FROM bank_transactions WHERE id = ?1",
                [&tx.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored_type, "debit", "stored lowercased so dedup matches");
        assert_eq!(stored_status, "pending");
    }

    #[test]
    fn create_transaction_validates_and_checks_category() {
        let conn = setup_test_db();
        let mut data = crate::models::InsertBankTransaction {
            flow_kind: None,
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: None,
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bank_code: None,
            counterparty_country: None,
            booking_date: 1700000000,
            value_date: None,
            category_id: None,

            status: None,
        };
        data.tx_type = "banana".into();
        assert!(create_transaction(&conn, &data).is_err());
        data.tx_type = "debit".into();
        data.category_id = Some("no-such-category".into());
        assert!(matches!(
            create_transaction(&conn, &data).unwrap_err(),
            crate::error::AppError::NotFound(_)
        ));
    }

    #[test]
    fn create_transaction_with_origin_records_source_and_batch() {
        let conn = setup_test_db();
        let data = crate::models::InsertBankTransaction {
            flow_kind: None,
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: Some("from csv".into()),
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bank_code: None,
            counterparty_country: None,
            booking_date: 1700000000,
            value_date: None,
            category_id: None,

            status: None,
        };
        let tx = create_transaction_with_origin(&conn, &data, "csv_import", Some("batch-1"), None)
            .unwrap();
        assert_eq!(tx.data_source, "csv_import");
        let (source, batch): (String, Option<String>) = conn
            .query_row(
                "SELECT data_source, import_batch_id FROM bank_transactions WHERE id = ?1",
                [&tx.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "csv_import");
        assert_eq!(batch.as_deref(), Some("batch-1"));

        // The plain create keeps its manual origin and no batch link.
        let manual = create_transaction(&conn, &data).unwrap();
        let (source, batch): (String, Option<String>) = conn
            .query_row(
                "SELECT data_source, import_batch_id FROM bank_transactions WHERE id = ?1",
                [&manual.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "manual");
        assert!(batch.is_none());
    }

    fn stored_categorization_source(conn: &Connection, id: &str) -> Option<String> {
        conn.query_row(
            "SELECT categorization_source FROM bank_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn create_transaction_records_categorization_provenance() {
        let conn = setup_test_db();
        let mut data = crate::models::InsertBankTransaction {
            flow_kind: None,
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: Some("rule-matched".into()),
            counterparty_name: None,
            counterparty_iban: None,
            counterparty_bank_code: None,
            counterparty_country: None,
            booking_date: 1700000000,
            value_date: None,
            category_id: Some("cat-1".into()),

            status: None,
        };
        let stamped =
            create_transaction_with_origin(&conn, &data, "csv_import", None, Some("rule")).unwrap();
        assert_eq!(
            stored_categorization_source(&conn, &stamped.id).as_deref(),
            Some("rule")
        );

        // No label → NULL, which keeps meaning "no recorded provenance"
        // (the manual UI path).
        data.description = Some("manually categorized".into());
        let unstamped = create_transaction(&conn, &data).unwrap();
        assert!(stored_categorization_source(&conn, &unstamped.id).is_none());
    }

    #[test]
    fn set_transaction_category_checks_both_sides() {
        let conn = setup_test_db(); // seed one transaction 'tx-1' and one category 'cat-1'
        set_transaction_category(&conn, "tx-1", Some("cat-1"), None).unwrap();
        assert!(matches!(
            set_transaction_category(&conn, "tx-1", Some("ghost"), None).unwrap_err(),
            crate::error::AppError::NotFound(_)
        ));
        assert!(matches!(
            set_transaction_category(&conn, "ghost", Some("cat-1"), None).unwrap_err(),
            crate::error::AppError::NotFound(_)
        ));
        set_transaction_category(&conn, "tx-1", None, None).unwrap(); // clearing is allowed
    }

    #[test]
    fn set_transaction_category_rewrites_provenance_every_time() {
        let conn = setup_test_db();
        set_transaction_category(&conn, "tx-1", Some("cat-1"), Some("mcp")).unwrap();
        assert_eq!(
            stored_categorization_source(&conn, "tx-1").as_deref(),
            Some("mcp")
        );
        // The manual UI path passes None, which clears the stale label rather
        // than leaving 'mcp' on a category the user just chose themselves.
        set_transaction_category(&conn, "tx-1", Some("cat-1"), None).unwrap();
        assert!(stored_categorization_source(&conn, "tx-1").is_none());
    }

    fn record(amount: &str, as_of: i64) -> crate::models::bank_accounts::InsertBalanceRecord {
        crate::models::bank_accounts::InsertBalanceRecord {
            amount: amount.into(),
            currency: "EUR".into(),
            as_of,
            confirm_unknown_as_of: false,
            source_batch_id: None,
        }
    }

    const D1: i64 = 1767225600; // 2026-01-01
    const D2: i64 = 1767312000; // 2026-01-02
    const D3: i64 = 1767398400; // 2026-01-03

    #[test]
    fn record_balance_mirrors_the_newest_record_into_the_columns() {
        let conn = setup_test_db();
        let a = record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("100", Some(D2)));
        // An older day is history and leaves the columns alone
        let a = record_balance(&conn, "acc-1", &record("50", D1)).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("100", Some(D2)));
        // A newer day becomes the current balance
        let a = record_balance(&conn, "acc-1", &record("120", D3)).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("120", Some(D3)));
        let list = list_balance_records(&conn, "acc-1").unwrap();
        assert_eq!(
            list.iter()
                .map(|r| (r.as_of, r.amount.as_str(), r.source.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (D3, "120", "manual"),
                (D2, "100", "manual"),
                (D1, "50", "manual")
            ]
        );
    }

    #[test]
    fn record_balance_replaces_the_same_day() {
        let conn = setup_test_db();
        record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        let a = record_balance(&conn, "acc-1", &record("101", D2)).unwrap();
        assert_eq!(a.balance, "101");
        let list = list_balance_records(&conn, "acc-1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].amount, "101");
    }

    #[test]
    fn record_balance_rejects_wrong_currency_future_day_and_foreign_batch() {
        let conn = setup_test_db();
        let mut data = record("100", D2);
        data.currency = "CZK".into();
        assert!(record_balance(&conn, "acc-1", &data).is_err());
        let future = chrono::Utc::now().timestamp().div_euclid(86400) * 86400 + 86400;
        assert!(record_balance(&conn, "acc-1", &record("100", future)).is_err());
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, created_at, updated_at) VALUES ('acc-2', 'Other', 'EUR', 0, 0);
             INSERT INTO csv_import_batches (id, bank_account_id) VALUES ('batch-2', 'acc-2');
             INSERT INTO csv_import_batches (id, bank_account_id) VALUES ('batch-1', 'acc-1');",
        )
        .unwrap();
        let mut foreign = record("100", D2);
        foreign.source_batch_id = Some("batch-2".into());
        assert!(record_balance(&conn, "acc-1", &foreign).is_err());
        let mut own = record("100", D2);
        own.source_batch_id = Some("batch-1".into());
        record_balance(&conn, "acc-1", &own).unwrap();
        let list = list_balance_records(&conn, "acc-1").unwrap();
        assert_eq!(
            (list[0].source.as_str(), list[0].source_batch_id.as_deref()),
            ("statement", Some("batch-1"))
        );
    }

    #[test]
    fn undated_nonzero_balance_needs_confirmation_only_before_the_first_record() {
        let conn = setup_test_db();
        conn.execute(
            "UPDATE bank_accounts SET balance='999' WHERE id='acc-1'",
            [],
        )
        .unwrap();
        assert!(record_balance(&conn, "acc-1", &record("100", D2)).is_err());
        let mut confirmed = record("100", D2);
        confirmed.confirm_unknown_as_of = true;
        assert_eq!(
            record_balance(&conn, "acc-1", &confirmed).unwrap().balance,
            "100"
        );
        // From now on the flag is not needed
        assert!(record_balance(&conn, "acc-1", &record("110", D3)).is_ok());
        // An undated balance of exactly 0 never needs confirmation
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, balance, created_at, updated_at) VALUES ('acc-3', 'Zero', 'EUR', '0', 0, 0);",
        )
        .unwrap();
        assert!(record_balance(&conn, "acc-3", &record("5", D2)).is_ok());
    }

    #[test]
    fn delete_balance_record_refreshes_and_clears_the_columns() {
        let conn = setup_test_db();
        record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        record_balance(&conn, "acc-1", &record("120", D3)).unwrap();
        let newest = list_balance_records(&conn, "acc-1").unwrap()[0].id.clone();
        let a = delete_balance_record(&conn, &newest).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("100", Some(D2)));
        let last = list_balance_records(&conn, "acc-1").unwrap()[0].id.clone();
        let a = delete_balance_record(&conn, &last).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("0", None));
        assert!(delete_balance_record(&conn, "missing").is_err());
    }

    #[test]
    fn create_account_with_a_dated_balance_writes_the_first_record() {
        let conn = setup_test_db();
        let mut data = minimal_insert("Opening");
        data.currency = Some("EUR".into());
        data.balance = Some("250".into());
        data.balance_as_of = Some(Some(D2));
        let account = create_account(&conn, &data).unwrap();
        let list = list_balance_records(&conn, &account.id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (
                list[0].amount.as_str(),
                list[0].as_of,
                list[0].currency.as_str()
            ),
            ("250", D2, "EUR")
        );
        // Without a date: undated balance, no record (unchanged behaviour)
        let mut undated = minimal_insert("Undated");
        undated.balance = Some("10".into());
        let account = create_account(&conn, &undated).unwrap();
        assert!(list_balance_records(&conn, &account.id).unwrap().is_empty());
        assert_eq!(account.balance_as_of, None);
    }

    #[test]
    fn update_account_ignores_balance_fields() {
        let conn = setup_test_db();
        record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        let mut edit = minimal_insert("Renamed");
        edit.currency = Some("EUR".into());
        edit.balance = Some("5".into());
        edit.balance_as_of = Some(None);
        let a = update_account(&conn, "acc-1", &edit).unwrap();
        assert_eq!(
            (a.name.as_str(), a.balance.as_str(), a.balance_as_of),
            ("Renamed", "100", Some(D2))
        );
    }
}
