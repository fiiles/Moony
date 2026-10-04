//! Insurance commands

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    InsertInsuranceDocument, InsertInsurancePolicy, InsuranceDocument, InsurancePolicy, RealEstate,
};
use tauri::{Manager, State};
use uuid::Uuid;

/// Get all insurance policies
#[tauri::command]
pub async fn get_all_insurance(db: State<'_, Database>) -> Result<Vec<InsurancePolicy>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, type, provider, policy_name, policy_number, start_date, end_date,
                    payment_frequency, one_time_payment, one_time_payment_currency,
                    regular_payment, regular_payment_currency, limits, notes, status,
                    created_at, updated_at
             FROM insurance_policies ORDER BY policy_name",
        )?;

        let policies = stmt
            .query_map([], |row| {
                let limits_json: String = row.get(12)?;

                Ok(InsurancePolicy {
                    id: row.get(0)?,
                    policy_type: row.get(1)?,
                    provider: row.get(2)?,
                    policy_name: row.get(3)?,
                    policy_number: row.get(4)?,
                    start_date: row.get(5)?,
                    end_date: row.get(6)?,
                    payment_frequency: row.get(7)?,
                    one_time_payment: row.get(8)?,
                    one_time_payment_currency: row.get(9)?,
                    regular_payment: row.get(10)?,
                    regular_payment_currency: row.get(11)?,
                    limits: serde_json::from_str(&limits_json).unwrap_or_default(),
                    notes: row.get(13)?,
                    status: row.get(14)?,
                    created_at: row.get(15)?,
                    updated_at: row.get(16)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(policies)
    })
}

/// Create insurance policy
#[tauri::command]
pub async fn create_insurance(
    db: State<'_, Database>,
    data: InsertInsurancePolicy,
) -> Result<InsurancePolicy> {
    db.with_conn(|conn| crate::services::insurance::create_policy(conn, &data))
}

/// Update insurance policy
#[tauri::command]
pub async fn update_insurance(
    db: State<'_, Database>,
    id: String,
    data: InsertInsurancePolicy,
) -> Result<InsurancePolicy> {
    // Validate inputs at the trust boundary
    data.validate()?;

    let now = chrono::Utc::now().timestamp();
    let limits_json = data.limits.and_then(|l| serde_json::to_string(&l).ok());

    db.with_conn(|conn| {
        conn.execute(
            "UPDATE insurance_policies SET
             type = ?1, provider = ?2, policy_name = ?3, policy_number = ?4,
             start_date = ?5, end_date = ?6, payment_frequency = ?7,
             one_time_payment = ?8, one_time_payment_currency = ?9,
             regular_payment = COALESCE(?10, regular_payment),
             regular_payment_currency = COALESCE(?11, regular_payment_currency),
             limits = COALESCE(?12, limits), notes = ?13, status = COALESCE(?14, status),
             updated_at = ?15 WHERE id = ?16",
            rusqlite::params![
                data.policy_type,
                data.provider,
                data.policy_name,
                data.policy_number,
                data.start_date,
                data.end_date,
                data.payment_frequency,
                data.one_time_payment,
                data.one_time_payment_currency,
                data.regular_payment,
                data.regular_payment_currency,
                limits_json,
                data.notes,
                data.status,
                now,
                id
            ],
        )?;

        conn.query_row(
            "SELECT id, type, provider, policy_name, policy_number, start_date, end_date,
                    payment_frequency, one_time_payment, one_time_payment_currency,
                    regular_payment, regular_payment_currency, limits, notes, status,
                    created_at, updated_at
             FROM insurance_policies WHERE id = ?1",
            [&id],
            |row| {
                let limits_json: String = row.get(12)?;
                Ok(InsurancePolicy {
                    id: row.get(0)?,
                    policy_type: row.get(1)?,
                    provider: row.get(2)?,
                    policy_name: row.get(3)?,
                    policy_number: row.get(4)?,
                    start_date: row.get(5)?,
                    end_date: row.get(6)?,
                    payment_frequency: row.get(7)?,
                    one_time_payment: row.get(8)?,
                    one_time_payment_currency: row.get(9)?,
                    regular_payment: row.get(10)?,
                    regular_payment_currency: row.get(11)?,
                    limits: serde_json::from_str(&limits_json).unwrap_or_default(),
                    notes: row.get(13)?,
                    status: row.get(14)?,
                    created_at: row.get(15)?,
                    updated_at: row.get(16)?,
                })
            },
        )
        .map_err(|_| AppError::NotFound("Insurance policy not found".into()))
    })
}

/// Delete insurance policy
#[tauri::command]
pub async fn delete_insurance(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| {
        // Delete documents first
        conn.execute(
            "DELETE FROM insurance_documents WHERE insurance_id = ?1",
            [&id],
        )?;
        conn.execute(
            "DELETE FROM real_estate_insurances WHERE insurance_id = ?1",
            [&id],
        )?;
        let changes = conn.execute("DELETE FROM insurance_policies WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Insurance policy not found".into()));
        }
        Ok(())
    })
}

/// Get single insurance policy by ID
#[tauri::command]
pub async fn get_insurance(db: State<'_, Database>, id: String) -> Result<InsurancePolicy> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, type, provider, policy_name, policy_number, start_date, end_date,
                    payment_frequency, one_time_payment, one_time_payment_currency,
                    regular_payment, regular_payment_currency, limits, notes, status,
                    created_at, updated_at
             FROM insurance_policies WHERE id = ?1",
            [&id],
            |row| {
                let limits_json: String = row.get(12)?;
                Ok(InsurancePolicy {
                    id: row.get(0)?,
                    policy_type: row.get(1)?,
                    provider: row.get(2)?,
                    policy_name: row.get(3)?,
                    policy_number: row.get(4)?,
                    start_date: row.get(5)?,
                    end_date: row.get(6)?,
                    payment_frequency: row.get(7)?,
                    one_time_payment: row.get(8)?,
                    one_time_payment_currency: row.get(9)?,
                    regular_payment: row.get(10)?,
                    regular_payment_currency: row.get(11)?,
                    limits: serde_json::from_str(&limits_json).unwrap_or_default(),
                    notes: row.get(13)?,
                    status: row.get(14)?,
                    created_at: row.get(15)?,
                    updated_at: row.get(16)?,
                })
            },
        )
        .map_err(|_| AppError::NotFound("Insurance policy not found".into()))
    })
}

/// Get all documents for an insurance policy
#[tauri::command]
pub async fn get_insurance_documents(
    db: State<'_, Database>,
    insurance_id: String,
) -> Result<Vec<InsuranceDocument>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, insurance_id, name, description, file_path, file_type, file_size, uploaded_at
             FROM insurance_documents WHERE insurance_id = ?1 ORDER BY uploaded_at DESC"
        )?;

        let documents = stmt.query_map([&insurance_id], |row| {
            Ok(InsuranceDocument {
                id: row.get(0)?,
                insurance_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                file_path: row.get(4)?,
                file_type: row.get(5)?,
                file_size: row.get(6)?,
                uploaded_at: row.get(7)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(documents)
    })
}

/// Add a document to an insurance policy
#[tauri::command]
pub async fn add_insurance_document(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    insurance_id: String,
    file_path: String,
    data: InsertInsuranceDocument,
) -> Result<InsuranceDocument> {
    use std::fs;
    use std::path::Path;

    // Validate inputs at the trust boundary (before any field access)
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Get app data directory for storing documents
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {}", e)))?;
    let documents_dir = app_data_dir.join("insurance_documents").join(&insurance_id);

    // Create documents directory if it doesn't exist
    fs::create_dir_all(&documents_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create documents dir: {}", e)))?;

    // Get file info
    let source_path = Path::new(&file_path);
    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");

    let file_size = fs::metadata(source_path).map(|m| m.len() as i64).ok();

    // Copy file to app data directory
    let dest_path = documents_dir.join(format!("{}_{}", id, file_name));
    fs::copy(source_path, &dest_path)
        .map_err(|e| AppError::Internal(format!("Failed to copy file: {}", e)))?;

    let dest_path_str = dest_path.to_string_lossy().to_string();
    let file_type = data.file_type.unwrap_or_else(|| "other".to_string());

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, description, file_path, file_type, file_size, uploaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![id, insurance_id, data.name, data.description, dest_path_str, file_type, file_size, now],
        )?;

        Ok(InsuranceDocument {
            id,
            insurance_id,
            name: data.name,
            description: data.description,
            file_path: dest_path_str,
            file_type,
            file_size,
            uploaded_at: now,
        })
    })
}

/// Delete an insurance document
#[tauri::command]
pub async fn delete_insurance_document(db: State<'_, Database>, document_id: String) -> Result<()> {
    use std::fs;

    // Get file path before deleting
    let file_path: String = db.with_conn(|conn| {
        conn.query_row(
            "SELECT file_path FROM insurance_documents WHERE id = ?1",
            [&document_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::NotFound("Document not found".into()))
    })?;

    // Delete from database
    db.with_conn(|conn| {
        let changes = conn.execute(
            "DELETE FROM insurance_documents WHERE id = ?1",
            [&document_id],
        )?;
        if changes == 0 {
            return Err(AppError::NotFound("Document not found".into()));
        }
        Ok(())
    })?;

    // Delete file (ignore errors if file doesn't exist)
    let _ = fs::remove_file(&file_path);

    Ok(())
}

/// Open an insurance document with the system default application
#[tauri::command]
pub async fn open_insurance_document(db: State<'_, Database>, document_id: String) -> Result<()> {
    let file_path: String = db.with_conn(|conn| {
        conn.query_row(
            "SELECT file_path FROM insurance_documents WHERE id = ?1",
            [&document_id],
            |row| row.get(0),
        )
        .map_err(|_| AppError::NotFound("Document not found".into()))
    })?;

    // Open with system default application
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&file_path)
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to open file: {}", e)))?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&file_path)
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to open file: {}", e)))?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&file_path)
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to open file: {}", e)))?;

    Ok(())
}

// ============================================================================
// Real Estate Linking Commands
// ============================================================================

/// Get the real estate linked to an insurance policy (if any)
#[tauri::command]
pub async fn get_insurance_real_estate(
    db: State<'_, Database>,
    insurance_id: String,
) -> Result<Option<RealEstate>> {
    db.with_conn(|conn| {
        let result = conn.query_row(
            "SELECT r.id, r.name, r.address, r.type, r.purchase_price, r.purchase_price_currency,
                    r.market_price, r.market_price_currency, r.monthly_rent, r.monthly_rent_currency,
                    r.recurring_costs, r.photos, r.notes, r.created_at, r.updated_at
             FROM real_estate r
             INNER JOIN real_estate_insurances rei ON r.id = rei.real_estate_id
             WHERE rei.insurance_id = ?1",
            [&insurance_id],
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
            Err(e) => Err(e.into()),
        }
    })
}

/// Get all insurances that are not linked to any real estate
#[tauri::command]
pub async fn get_available_insurances(db: State<'_, Database>) -> Result<Vec<InsurancePolicy>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, type, provider, policy_name, policy_number, start_date, end_date,
                    payment_frequency, one_time_payment, one_time_payment_currency,
                    regular_payment, regular_payment_currency, limits, notes, status,
                    created_at, updated_at
             FROM insurance_policies
             WHERE id NOT IN (SELECT insurance_id FROM real_estate_insurances)
             ORDER BY policy_name",
        )?;

        let policies = stmt
            .query_map([], |row| {
                let limits_json: String = row.get(12)?;

                Ok(InsurancePolicy {
                    id: row.get(0)?,
                    policy_type: row.get(1)?,
                    provider: row.get(2)?,
                    policy_name: row.get(3)?,
                    policy_number: row.get(4)?,
                    start_date: row.get(5)?,
                    end_date: row.get(6)?,
                    payment_frequency: row.get(7)?,
                    one_time_payment: row.get(8)?,
                    one_time_payment_currency: row.get(9)?,
                    regular_payment: row.get(10)?,
                    regular_payment_currency: row.get(11)?,
                    limits: serde_json::from_str(&limits_json).unwrap_or_default(),
                    notes: row.get(13)?,
                    status: row.get(14)?,
                    created_at: row.get(15)?,
                    updated_at: row.get(16)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(policies)
    })
}
