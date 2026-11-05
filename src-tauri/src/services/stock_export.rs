//! The stock transactions CSV export. Its header is the one the Moony import adapter reads
//! (`stock_import::adapters::moony`), so an export can be imported again, fees included.

use chrono::{TimeZone, Utc};
use rusqlite::Connection;

use crate::error::Result;

/// The export header. `Fee` is the fee in the trade's own currency, empty when none was
/// recorded; the import reads it as a fee in the `Currency` column's currency.
const HEADER: &str = "Date,Type,Ticker,Name,Quantity,Price,Currency,Fee\n";

fn escape_csv(s: &str) -> String {
    s.replace('"', "\"\"")
}

/// Every stock transaction, newest first, as CSV text, and how many rows it has.
pub fn stock_transactions_csv(conn: &Connection) -> Result<(String, usize)> {
    let mut stmt = conn.prepare(
        "SELECT
            t.type,
            i.ticker,
            i.company_name,
            t.quantity,
            t.price_per_unit,
            t.currency,
            t.transaction_date,
            t.fee
        FROM investment_transactions t
        JOIN stock_investments i ON t.investment_id = i.id
        ORDER BY t.transaction_date DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,         // type
            row.get::<_, String>(1)?,         // ticker
            row.get::<_, String>(2)?,         // company_name
            row.get::<_, String>(3)?,         // quantity
            row.get::<_, String>(4)?,         // price_per_unit
            row.get::<_, String>(5)?,         // currency
            row.get::<_, i64>(6)?,            // transaction_date
            row.get::<_, Option<String>>(7)?, // fee
        ))
    })?;

    let mut csv = String::from(HEADER);
    let mut count = 0;
    for row in rows {
        let (tx_type, ticker, company_name, quantity, price, currency, date_ts, fee) = row?;
        let date_str = Utc
            .timestamp_opt(date_ts, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| date_ts.to_string());
        csv.push_str(&format!(
            "{},{},{},\"{}\",{},{},{},{}\n",
            date_str,
            tx_type,
            ticker,
            escape_csv(&company_name),
            quantity,
            price,
            currency,
            fee.unwrap_or_default()
        ));
        count += 1;
    }
    Ok((csv, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::moony;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_investments (id TEXT PRIMARY KEY, ticker TEXT, company_name TEXT);
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY, investment_id TEXT, type TEXT, quantity TEXT,
                price_per_unit TEXT, currency TEXT, transaction_date INTEGER, fee TEXT
            );
            INSERT INTO stock_investments VALUES ('i1', 'AAPL', 'Apple "Inc"');
            INSERT INTO investment_transactions VALUES
                ('t1', 'i1', 'buy', '10', '150.5', 'USD', 1700000000, '1.25'),
                ('t2', 'i1', 'sell', '4', '170', 'USD', 1710000000, NULL);
            "#,
        )
        .expect("schema");
        conn
    }

    #[test]
    fn the_export_carries_the_fee_under_the_header_the_moony_adapter_reads() {
        let conn = setup_test_db();
        let (csv, count) = stock_transactions_csv(&conn).expect("export");
        assert_eq!(count, 2);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "Date,Type,Ticker,Name,Quantity,Price,Currency,Fee"
        );
        // newest first; a missing fee stays empty
        assert_eq!(
            lines[1],
            "2024-03-09,sell,AAPL,\"Apple \"\"Inc\"\"\",4,170,USD,"
        );
        assert_eq!(
            lines[2],
            "2023-11-14,buy,AAPL,\"Apple \"\"Inc\"\"\",10,150.5,USD,1.25"
        );

        let headers: Vec<String> = lines[0].split(',').map(str::to_string).collect();
        let config = moony::config(&headers, &[])
            .expect("the adapter reads the export")
            .config;
        assert_eq!(config.fee_column, Some(7));
        assert_eq!(config.fee_currency_column, None);
    }
}
