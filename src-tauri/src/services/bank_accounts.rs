//! Bank account business logic service
//!
//! All business logic for bank accounts lives here.
//! Commands should only validate input, call these functions, and handle events.
//! This is the SINGLE SOURCE OF TRUTH for bank account operations.

use crate::error::{AppError, Result};
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
         termination_date, created_at, updated_at, exclude_from_balance
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
                institution_id: row.get(7)?,
                external_account_id: row.get(8)?,
                data_source: row.get(9)?,
                last_synced_at: row.get(10)?,
                interest_rate: row.get(11)?,
                has_zone_designation: row.get::<_, i32>(12)? != 0,
                termination_date: row.get(13)?,
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
    let currency = data.currency.clone().unwrap_or_else(|| "CZK".to_string());
    let balance = data.balance.clone().unwrap_or_else(|| "0".to_string());
    let has_zone = data.has_zone_designation.unwrap_or(false);
    let exclude = data.exclude_from_balance.unwrap_or(false);

    conn.execute(
        "INSERT INTO bank_accounts (
            id, name, account_type, iban, bban, currency, balance,
            institution_id, data_source, interest_rate, has_zone_designation,
            termination_date, created_at, updated_at, exclude_from_balance
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
        ],
    )?;

    Ok(BankAccount {
        id,
        name: data.name.clone(),
        account_type,
        iban: data.iban.clone(),
        bban: data.bban.clone(),
        currency,
        balance,
        institution_id: data.institution_id.clone(),
        external_account_id: None,
        data_source: "manual".to_string(),
        last_synced_at: None,
        interest_rate: data.interest_rate.clone(),
        has_zone_designation: has_zone,
        termination_date: data.termination_date,
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
    let currency = data.currency.clone().unwrap_or(existing.currency);
    let balance = data.balance.clone().unwrap_or(existing.balance);
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
            termination_date = ?10, exclude_from_balance = ?11, updated_at = ?12
        WHERE id = ?13",
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
            id,
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
        institution_id: data.institution_id.clone(),
        external_account_id: existing.external_account_id,
        data_source: existing.data_source,
        last_synced_at: existing.last_synced_at,
        interest_rate: data.interest_rate.clone(),
        has_zone_designation: has_zone,
        termination_date: data.termination_date,
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
            categorization_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id,
            data.bank_account_id,
            data.transaction_id,
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
        ],
    )?;

    Ok(BankTransaction {
        id,
        bank_account_id: data.bank_account_id.clone(),
        transaction_id: data.transaction_id.clone(),
        tx_type,
        amount: data.amount.clone(),
        currency,
        description: data.description.clone(),
        counterparty_name: data.counterparty_name.clone(),
        counterparty_iban: data.counterparty_iban.clone(),
        booking_date: data.booking_date,
        value_date: data.value_date,
        category_id: data.category_id.clone(),
        merchant_category_code: None,
        remittance_info: None,
        suggested_category_id: None,
        status,
        data_source: data_source.to_string(),
        created_at: now,
    })
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
            institution_id: None,
            interest_rate: None,
            has_zone_designation: None,
            termination_date: None,
            exclude_from_balance: None,
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
    fn create_transaction_falls_back_to_account_currency() {
        let conn = setup_test_db(); // must seed one bank_accounts row id='acc-1', currency='EUR'
        let mut data = crate::models::InsertBankTransaction {
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: None,
            counterparty_name: None,
            counterparty_iban: None,
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
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "DEBIT".into(),
            amount: "10".into(),
            currency: None,
            description: None,
            counterparty_name: None,
            counterparty_iban: None,
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
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: None,
            counterparty_name: None,
            counterparty_iban: None,
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
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: Some("from csv".into()),
            counterparty_name: None,
            counterparty_iban: None,
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
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "10".into(),
            currency: None,
            description: Some("rule-matched".into()),
            counterparty_name: None,
            counterparty_iban: None,
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
}
