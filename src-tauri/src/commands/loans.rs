//! Loan commands. Thin wrappers: the SQL, validation and amortization live in
//! `services/loans.rs`.

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{InsertLoan, Loan, LoanSchedule, RealEstate};
use crate::services::loan_amortization::today_utc_day;
use crate::services::loans as loans_service;
use tauri::State;

/// Get all loans, each with its amortized outstanding balance as of today
#[tauri::command]
pub async fn get_all_loans(db: State<'_, Database>) -> Result<Vec<Loan>> {
    db.with_conn(|conn| loans_service::list_loans(conn, today_utc_day()))
}

/// Repayment schedule and totals of one loan as of today
#[tauri::command]
pub async fn get_loan_schedule(db: State<'_, Database>, id: String) -> Result<LoanSchedule> {
    db.with_conn(|conn| loans_service::loan_schedule(conn, &id, today_utc_day()))
}

/// Create loan
#[tauri::command]
pub async fn create_loan(db: State<'_, Database>, data: InsertLoan) -> Result<Loan> {
    db.with_conn(|conn| loans_service::create_loan(conn, &data))
}

/// Update loan
#[tauri::command]
pub async fn update_loan(db: State<'_, Database>, id: String, data: InsertLoan) -> Result<Loan> {
    db.with_conn(|conn| loans_service::update_loan(conn, &id, &data))
}

/// Delete loan
#[tauri::command]
pub async fn delete_loan(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| {
        // Remove links first
        conn.execute("DELETE FROM real_estate_loans WHERE loan_id = ?1", [&id])?;
        let changes = conn.execute("DELETE FROM loans WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Loan not found".into()));
        }
        Ok(())
    })
}

// ============================================================================
// Real Estate Linking Commands
// ============================================================================

/// Get the real estate linked to a loan (if any)
#[tauri::command]
pub async fn get_loan_real_estate(
    db: State<'_, Database>,
    loan_id: String,
) -> Result<Option<RealEstate>> {
    db.with_conn(|conn| {
        let result = conn.query_row(
            "SELECT r.id, r.name, r.address, r.type, r.purchase_price, r.purchase_price_currency,
                    r.market_price, r.market_price_currency, r.monthly_rent, r.monthly_rent_currency,
                    r.recurring_costs, r.photos, r.notes, r.created_at, r.updated_at
             FROM real_estate r
             INNER JOIN real_estate_loans rel ON r.id = rel.real_estate_id
             WHERE rel.loan_id = ?1",
            [&loan_id],
            |row| {
                let rc_json: String = row.get(10)?;
                let photos_json: String = row.get(11)?;

                Ok(RealEstate {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    address: row.get(2)?,
                    property_type: row.get(3)?,
                    purchase_price: row.get(4)?,
                    purchase_price_currency: row.get(5)?,
                    market_price: row.get(6)?,
                    market_price_currency: row.get(7)?,
                    monthly_rent: row.get(8)?,
                    monthly_rent_currency: row.get(9)?,
                    recurring_costs: serde_json::from_str(&rc_json).unwrap_or_default(),
                    photos: serde_json::from_str(&photos_json).unwrap_or_default(),
                    notes: row.get(12)?,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            },
        );

        match result {
            Ok(re) => Ok(Some(re)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    })
}

/// Get all loans not linked to any real estate
#[tauri::command]
pub async fn get_available_loans(db: State<'_, Database>) -> Result<Vec<Loan>> {
    db.with_conn(|conn| loans_service::list_available_loans(conn, today_utc_day()))
}

// ---------------------------------------------------------------------------
// Loan events (redesign phase 2)
// ---------------------------------------------------------------------------

/// Extra payments, rate changes and balance checks of a loan, oldest first.
#[tauri::command]
pub async fn get_loan_events(
    db: State<'_, Database>,
    loan_id: String,
) -> Result<Vec<crate::models::LoanEvent>> {
    db.with_conn(|conn| crate::services::loan_events::list_events(conn, &loan_id))
}

/// Record an event; it re-anchors the amortization on its day (see the service).
#[tauri::command]
pub async fn add_loan_event(
    db: State<'_, Database>,
    data: crate::models::InsertLoanEvent,
) -> Result<crate::models::LoanEvent> {
    let saved = db.with_conn(|conn| {
        crate::services::loan_events::add_event(
            conn,
            &data,
            crate::services::loan_amortization::today_utc_day(),
        )
    })?;
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();
    Ok(saved)
}

/// Remove an event from the trace (the anchor it set stays).
#[tauri::command]
pub async fn delete_loan_event(db: State<'_, Database>, event_id: String) -> Result<()> {
    db.with_conn(|conn| crate::services::loan_events::delete_event(conn, &event_id))
}
