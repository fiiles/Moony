//! Server-side ledger filtering and stable pagination.
use crate::error::{AppError, Result};
use crate::models::bank_transactions::{
    BankTransaction, TransactionFilters, TransactionQueryResult,
};
use rusqlite::Connection;

/// The columns of one ledger row, in the order [`transaction_from_row`] reads them. The last one
/// is the file name of the import the row came from, a subquery on `bank_transactions`, so the
/// statement that uses this list must select `FROM bank_transactions` unaliased.
pub(crate) const TRANSACTION_COLUMNS: &str = "id, bank_account_id, transaction_id, tx_type, amount, currency,
             description, counterparty_name, counterparty_iban, booking_date, value_date,
             category_id, merchant_category_code, remittance_info, suggested_category_id,
             status, data_source, created_at, flow_kind, counterparty_bank_code, counterparty_country, import_batch_id, import_line, row_balance,
             (SELECT file_name FROM csv_import_batches WHERE id=bank_transactions.import_batch_id)";

/// A ledger row read with [`TRANSACTION_COLUMNS`].
pub(crate) fn transaction_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BankTransaction> {
    Ok(BankTransaction {
        flow_kind: row.get(18)?,
        id: row.get(0)?,
        bank_account_id: row.get(1)?,
        transaction_id: row.get(2)?,
        tx_type: row.get(3)?,
        amount: row.get(4)?,
        currency: row.get(5)?,
        description: row.get(6)?,
        counterparty_name: row.get(7)?,
        counterparty_iban: row.get(8)?,
        counterparty_bank_code: row.get(19)?,
        counterparty_country: row.get(20)?,
        import_batch_id: row.get(21)?,
        import_line: row.get(22)?,
        row_balance: row.get(23)?,
        import_file_name: row.get(24)?,
        booking_date: row.get(9)?,
        value_date: row.get(10)?,
        category_id: row.get(11)?,
        merchant_category_code: row.get(12)?,
        remittance_info: row.get(13)?,
        suggested_category_id: row.get(14)?,
        status: row.get(15)?,
        data_source: row.get(16)?,
        created_at: row.get(17)?,
    })
}

pub fn get_transactions(
    conn: &Connection,
    account_id: &str,
    filters: &TransactionFilters,
) -> Result<TransactionQueryResult> {
    let limit = filters.limit.unwrap_or(50);
    let offset = filters.offset.unwrap_or(0);
    if !(1..=5000).contains(&limit) || offset < 0 {
        return Err(AppError::Validation("validation.paginationInvalid".into()));
    }
    // Build base query
    let mut sql =
        format!("SELECT {TRANSACTION_COLUMNS} FROM bank_transactions WHERE bank_account_id = ?");

    let mut count_sql =
        String::from("SELECT COUNT(*) FROM bank_transactions WHERE bank_account_id = ?");

    // Build dynamic filters
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(account_id.to_string())];

    if let Some(date_from) = filters.date_from {
        sql.push_str(" AND booking_date >= ?");
        count_sql.push_str(" AND booking_date >= ?");
        params_vec.push(Box::new(date_from));
    }
    if let Some(date_to) = filters.date_to {
        sql.push_str(" AND booking_date <= ?");
        count_sql.push_str(" AND booking_date <= ?");
        params_vec.push(Box::new(date_to));
    }
    if let Some(ref category_id) = filters.category_id {
        if category_id == "uncategorized" {
            sql.push_str(" AND category_id IS NULL");
            count_sql.push_str(" AND category_id IS NULL");
        } else {
            sql.push_str(" AND category_id = ?");
            count_sql.push_str(" AND category_id = ?");
            params_vec.push(Box::new(category_id.clone()));
        }
    }
    if let Some(ref tx_type) = filters.tx_type {
        sql.push_str(" AND tx_type = ?");
        count_sql.push_str(" AND tx_type = ?");
        params_vec.push(Box::new(tx_type.clone()));
    }
    if let Some(ref search) = filters.search {
        sql.push_str(" AND (description LIKE ? OR counterparty_name LIKE ?)");
        count_sql.push_str(" AND (description LIKE ? OR counterparty_name LIKE ?)");
        let pattern = format!("%{}%", search);
        params_vec.push(Box::new(pattern.clone()));
        params_vec.push(Box::new(pattern));
    }

    let column = match filters.sort_by.as_deref().unwrap_or("date") {
        "date" => "booking_date",
        "description" => "COALESCE(counterparty_name,description,'')",
        "category" => "COALESCE(category_id,'')",
        "amount" => "CAST(amount AS REAL) * CASE WHEN tx_type='credit' THEN 1 ELSE -1 END",
        _ => return Err(AppError::Validation("validation.paginationInvalid".into())),
    };
    let direction = if filters.ascending.unwrap_or(false) {
        "ASC"
    } else {
        "DESC"
    };
    sql.push_str(&format!(
        " ORDER BY {column} {direction}, created_at {direction}, id {direction}"
    ));
    sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

    // Execute query
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let transactions: Vec<BankTransaction> = stmt
        .query_map(params_refs.as_slice(), transaction_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Get total count (reuse params without search duplicates)
    let count_params: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let total: i64 = conn.query_row(&count_sql, count_params.as_slice(), |row| row.get(0))?;

    Ok(TransactionQueryResult {
        transactions,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE csv_import_batches(id TEXT PRIMARY KEY,file_name TEXT);
          CREATE TABLE bank_transactions(id TEXT PRIMARY KEY,bank_account_id TEXT,transaction_id TEXT,tx_type TEXT,amount TEXT,currency TEXT,description TEXT,counterparty_name TEXT,counterparty_iban TEXT,booking_date INTEGER,value_date INTEGER,category_id TEXT,merchant_category_code TEXT,remittance_info TEXT,suggested_category_id TEXT,status TEXT,data_source TEXT,created_at INTEGER,flow_kind TEXT,counterparty_bank_code TEXT,counterparty_country TEXT,import_batch_id TEXT,import_line INTEGER,row_balance TEXT);").unwrap();
        for n in 0..1213 {
            conn.execute("INSERT INTO bank_transactions(id,bank_account_id,tx_type,amount,currency,description,booking_date,status,data_source,created_at,category_id) VALUES (?1,'a','debit',?2,'CZK','Merchant',10,'booked','manual',10,?3)",rusqlite::params![format!("t{n:04}"),n.to_string(),if n%2==0{Some("food")}else{None}]).unwrap();
        }
        conn
    }
    #[test]
    fn history_beyond_thousand_is_stable_filtered_and_complete() {
        let conn = db();
        let mut filters = TransactionFilters {
            limit: Some(100),
            ..Default::default()
        };
        let mut ids = std::collections::HashSet::new();
        for offset in (0..1213).step_by(100) {
            filters.offset = Some(offset);
            let page = get_transactions(&conn, "a", &filters).unwrap();
            assert_eq!(page.total, 1213);
            for tx in page.transactions {
                assert!(ids.insert(tx.id));
            }
        }
        assert_eq!(ids.len(), 1213);
        filters.offset = Some(0);
        filters.category_id = Some("uncategorized".into());
        filters.search = Some("Merchant".into());
        let result = get_transactions(&conn, "a", &filters).unwrap();
        assert_eq!(result.total, 606);
        assert!(result
            .transactions
            .iter()
            .all(|tx| tx.category_id.is_none()));
        filters.category_id = None;
        filters.sort_by = Some("amount".into());
        filters.ascending = Some(true);
        assert_eq!(
            get_transactions(&conn, "a", &filters).unwrap().transactions[0].amount,
            "1212"
        );
    }
    #[test]
    fn invalid_history_pagination_is_rejected() {
        let conn = db();
        let filters = TransactionFilters {
            offset: Some(-1),
            ..Default::default()
        };
        assert!(get_transactions(&conn, "a", &filters).is_err());
    }
}
