//! Export commands for CSV data export
//!
//! Provides CSV export functionality for all asset classes

use crate::db::Database;
use crate::error::Result;
use serde::Serialize;
use tauri::State;

/// Export result containing CSV data
#[derive(Serialize)]
pub struct ExportResult {
    pub csv: String,
    pub filename: String,
    pub count: usize,
}

/// Export stock investment transactions as CSV
/// Format matches the import format: Date, Type, Ticker, Name, Quantity, Price, Currency
#[tauri::command]
pub fn export_stock_transactions(db: State<'_, Database>) -> Result<ExportResult> {
    use chrono::{TimeZone, Utc};

    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT
                t.type,
                i.ticker,
                i.company_name,
                t.quantity,
                t.price_per_unit,
                t.currency,
                t.transaction_date
            FROM investment_transactions t
            JOIN stock_investments i ON t.investment_id = i.id
            ORDER BY t.transaction_date DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?, // type
                row.get::<_, String>(1)?, // ticker
                row.get::<_, String>(2)?, // company_name
                row.get::<_, String>(3)?, // quantity
                row.get::<_, String>(4)?, // price_per_unit
                row.get::<_, String>(5)?, // currency
                row.get::<_, i64>(6)?,    // transaction_date
            ))
        })?;

        let mut csv = String::from("Date,Type,Ticker,Name,Quantity,Price,Currency\n");
        let mut count = 0;

        for row in rows {
            let (tx_type, ticker, company_name, quantity, price, currency, date_ts) = row?;
            // Convert Unix timestamp to YYYY-MM-DD format
            let date_str = Utc
                .timestamp_opt(date_ts, 0)
                .single()
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| date_ts.to_string());
            csv.push_str(&format!(
                "{},{},{},\"{}\",{},{},{}\n",
                date_str,
                tx_type,
                ticker,
                escape_csv(&company_name),
                quantity,
                price,
                currency
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "stock_transactions.csv".to_string(),
            count,
        })
    })
}

/// Export crypto transactions as CSV
/// Format matches the import format: Date, Type, Ticker, Name, Quantity, Price, Currency
#[tauri::command]
pub fn export_crypto_transactions(db: State<'_, Database>) -> Result<ExportResult> {
    use chrono::{TimeZone, Utc};

    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT
                t.type,
                c.ticker,
                c.name,
                t.quantity,
                t.price_per_unit,
                t.currency,
                t.transaction_date
            FROM crypto_transactions t
            JOIN crypto_investments c ON t.investment_id = c.id
            ORDER BY t.transaction_date DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        let mut csv = String::from("Date,Type,Ticker,Name,Quantity,Price,Currency\n");
        let mut count = 0;

        for row in rows {
            let (tx_type, ticker, name, quantity, price, currency, date_ts) = row?;
            // Convert Unix timestamp to YYYY-MM-DD format
            let date_str = Utc
                .timestamp_opt(date_ts, 0)
                .single()
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| date_ts.to_string());
            csv.push_str(&format!(
                "{},{},{},\"{}\",{},{},{}\n",
                date_str,
                tx_type,
                ticker,
                escape_csv(&name),
                quantity,
                price,
                currency
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "crypto_transactions.csv".to_string(),
            count,
        })
    })
}

/// Export bonds as CSV
#[tauri::command]
pub fn export_bonds(db: State<'_, Database>) -> Result<ExportResult> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT name, isin, coupon_value, quantity, currency, interest_rate, maturity_date
            FROM bonds
            ORDER BY name",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })?;

        let mut csv =
            String::from("name,isin,coupon_value,quantity,currency,interest_rate,maturity_date\n");
        let mut count = 0;

        for row in rows {
            let (name, isin, coupon, qty, currency, rate, maturity) = row?;
            csv.push_str(&format!(
                "\"{}\",{},{},{},{},{},{}\n",
                escape_csv(&name),
                isin.unwrap_or_default(),
                coupon,
                qty,
                currency,
                rate,
                maturity.map(|d| d.to_string()).unwrap_or_default()
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "bonds.csv".to_string(),
            count,
        })
    })
}

/// Export real estate as CSV
#[tauri::command]
pub fn export_real_estate(db: State<'_, Database>) -> Result<ExportResult> {
    use chrono::{TimeZone, Utc};

    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT name, address, type, purchase_price, purchase_price_currency,
                    market_price, market_price_currency, monthly_rent, monthly_rent_currency,
                    recurring_costs, notes, purchase_date
            FROM real_estate
            ORDER BY name"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                // recurring_costs JSON; the column is nullable
                row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<i64>>(11)?, // purchase_date, a UTC day
            ))
        })?;

        let mut csv = String::from("name,address,type,purchase_price,purchase_price_currency,purchase_date,market_price,market_price_currency,monthly_rent,monthly_rent_currency,recurring_costs,notes\n");
        let mut count = 0;

        for row in rows {
            let (name, address, prop_type, pp, ppc, mp, mpc, rent, rent_c, costs, notes, bought) =
                row?;
            // YYYY-MM-DD like the transaction exports; empty while unknown
            let bought = bought
                .and_then(|day| Utc.timestamp_opt(day, 0).single())
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            csv.push_str(&format!(
                "\"{}\",\"{}\",{},{},{},{},{},{},{},{},\"{}\",\"{}\"\n",
                escape_csv(&name), escape_csv(&address), prop_type, pp, ppc, bought, mp, mpc,
                rent.unwrap_or_default(), rent_c.unwrap_or_default(),
                escape_csv(&costs), escape_csv(&notes.unwrap_or_default())
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "real_estate.csv".to_string(),
            count,
        })
    })
}

/// Export real estate one-time costs as CSV
#[tauri::command]
pub fn export_real_estate_costs(db: State<'_, Database>) -> Result<ExportResult> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT r.name, c.name, c.description, c.amount, c.currency, c.date
            FROM real_estate_costs c
            JOIN real_estate r ON c.real_estate_id = r.id
            ORDER BY r.name, c.date",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut csv = String::from("real_estate_name,name,description,amount,currency,date\n");
        let mut count = 0;

        for row in rows {
            let (re_name, name, desc, amount, currency, date) = row?;
            csv.push_str(&format!(
                "\"{}\",\"{}\",\"{}\",{},{},{}\n",
                escape_csv(&re_name),
                escape_csv(&name),
                escape_csv(&desc.unwrap_or_default()),
                amount,
                currency,
                date
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "real_estate_costs.csv".to_string(),
            count,
        })
    })
}

/// Export insurance policies as CSV
#[tauri::command]
pub fn export_insurance_policies(db: State<'_, Database>) -> Result<ExportResult> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT type, provider, policy_name, policy_number, start_date, end_date,
                    payment_frequency, one_time_payment, one_time_payment_currency,
                    regular_payment, regular_payment_currency, limits, notes, status
            FROM insurance_policies
            ORDER BY policy_name"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,  // limits JSON
                row.get::<_, Option<String>>(12)?,
                row.get::<_, String>(13)?,
            ))
        })?;

        let mut csv = String::from("type,provider,policy_name,policy_number,start_date,end_date,payment_frequency,one_time_payment,one_time_payment_currency,regular_payment,regular_payment_currency,limits,notes,status\n");
        let mut count = 0;

        for row in rows {
            let (ptype, provider, name, number, start, end, freq, otp, otp_c, rp, rp_c, limits, notes, status) = row?;
            csv.push_str(&format!(
                "{},\"{}\",\"{}\",{},{},{},{},{},{},{},{},\"{}\",\"{}\",{}\n",
                ptype, escape_csv(&provider), escape_csv(&name), number.unwrap_or_default(),
                start, end.map(|d| d.to_string()).unwrap_or_default(), freq,
                otp.unwrap_or_default(), otp_c.unwrap_or_default(), rp, rp_c,
                escape_csv(&limits), escape_csv(&notes.unwrap_or_default()), status
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "insurance_policies.csv".to_string(),
            count,
        })
    })
}

/// Export loans as CSV
#[tauri::command]
pub fn export_loans(db: State<'_, Database>) -> Result<ExportResult> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT name, principal, currency, interest_rate, interest_rate_validity_date,
                    monthly_payment, start_date, end_date
            FROM loans
            ORDER BY name"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<i64>>(7)?,
            ))
        })?;

        let mut csv = String::from("name,principal,currency,interest_rate,interest_rate_validity_date,monthly_payment,start_date,end_date\n");
        let mut count = 0;

        for row in rows {
            let (name, principal, currency, rate, rate_valid, payment, start, end) = row?;
            csv.push_str(&format!(
                "\"{}\",{},{},{},{},{},{},{}\n",
                escape_csv(&name), principal, currency, rate,
                rate_valid.map(|d| d.to_string()).unwrap_or_default(),
                payment, start, end.map(|d| d.to_string()).unwrap_or_default()
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "loans.csv".to_string(),
            count,
        })
    })
}

/// Export other assets as CSV
#[tauri::command]
pub fn export_other_assets(db: State<'_, Database>) -> Result<ExportResult> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT name, quantity, market_price, currency, average_purchase_price, yield_type, yield_value
            FROM other_assets
            ORDER BY name"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;

        let mut csv = String::from("name,quantity,market_price,currency,average_purchase_price,yield_type,yield_value\n");
        let mut count = 0;

        for row in rows {
            let (name, qty, price, currency, avg_price, ytype, yvalue) = row?;
            csv.push_str(&format!(
                "\"{}\",{},{},{},{},{},{}\n",
                escape_csv(&name), qty, price, currency, avg_price, ytype,
                yvalue.unwrap_or_default()
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "other_assets.csv".to_string(),
            count,
        })
    })
}

/// Export other asset transactions as CSV
#[tauri::command]
pub fn export_other_asset_transactions(db: State<'_, Database>) -> Result<ExportResult> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.name, t.type, t.quantity, t.price_per_unit, t.currency, t.transaction_date
            FROM other_asset_transactions t
            JOIN other_assets a ON t.asset_id = a.id
            ORDER BY a.name, t.transaction_date",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut csv =
            String::from("asset_name,type,quantity,price_per_unit,currency,transaction_date\n");
        let mut count = 0;

        for row in rows {
            let (name, tx_type, qty, price, currency, date) = row?;
            csv.push_str(&format!(
                "\"{}\",{},{},{},{},{}\n",
                escape_csv(&name),
                tx_type,
                qty,
                price,
                currency,
                date
            ));
            count += 1;
        }

        Ok(ExportResult {
            csv,
            filename: "other_asset_transactions.csv".to_string(),
            count,
        })
    })
}

/// Helper to escape CSV values (double quotes)
fn escape_csv(s: &str) -> String {
    s.replace('"', "\"\"")
}
