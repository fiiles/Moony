//! Insurance commands

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    InsertInsuranceDocument, InsertInsurancePolicy, InsuranceDocument, InsurancePolicy, RealEstate,
};
use crate::services::{attachments, real_estate_photos};
use tauri::{Manager, State};

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
    db.with_conn(|conn| crate::services::insurance::update_policy(conn, &id, &data))
}

/// Delete insurance policy with its documents
#[tauri::command]
pub async fn delete_insurance(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    id: String,
) -> Result<()> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {}", e)))?;
    db.with_conn(|conn| crate::services::insurance::delete_policy(conn, &dir, &id))
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
            "SELECT id, insurance_id, name, description, file_path, file_type, file_size, uploaded_at,
                    mime_type, original_name
             FROM insurance_documents WHERE insurance_id = ?1 ORDER BY uploaded_at DESC"
        )?;

        let documents = stmt.query_map([&insurance_id], |row| {
            let file_path: Option<String> = row.get(4)?;
            let mime_type: Option<String> = row.get(8)?;
            let original_name: Option<String> = row.get(9)?;
            let (mime_type, extension) = crate::services::attachments::document_meta(
                file_path.as_deref(),
                mime_type.as_deref(),
                original_name.as_deref(),
            );
            Ok(InsuranceDocument {
                id: row.get(0)?,
                insurance_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                mime_type,
                extension,
                file_type: row.get(5)?,
                file_size: row.get(6)?,
                uploaded_at: row.get(7)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(documents)
    })
}

/// Add a document to an insurance policy. The file the user picked is stored in the encrypted
/// attachments store.
#[tauri::command]
pub async fn add_insurance_document(
    db: State<'_, Database>,
    insurance_id: String,
    file_path: String,
    data: InsertInsuranceDocument,
) -> Result<InsuranceDocument> {
    // Validate inputs at the trust boundary (before any field access)
    data.validate()?;
    let file_type = data.file_type.as_deref().unwrap_or("other");
    let doc = crate::services::attachments::NewDocument {
        name: &data.name,
        description: data.description.as_deref(),
        file_type,
    };
    let now = chrono::Utc::now().timestamp();
    db.with_conn(|conn| {
        crate::services::attachments::add_insurance_document(
            conn,
            &insurance_id,
            std::path::Path::new(&file_path),
            &doc,
            now,
        )
    })
}

/// Add a document to an insurance policy from its bytes (the phone: a photographed contract or a
/// file from the Files app, which the webview holds only as bytes and may not write to disk). The
/// bytes are the IPC body; the `owner-id`, `file-name` and `document` headers carry the policy's
/// id, the file's name and the entries as `InsertInsuranceDocument` JSON, the last two
/// percent-encoded. The bytes go straight into the encrypted attachments store.
#[tauri::command]
pub async fn add_insurance_document_bytes(
    db: State<'_, Database>,
    request: tauri::ipc::Request<'_>,
) -> Result<InsuranceDocument> {
    let upload = attachments::document_upload::<InsertInsuranceDocument>(request.headers())?;
    // Validate inputs at the trust boundary (before any field access)
    upload.data.validate()?;
    let bytes = real_estate_photos::photo_bytes(request.body())?;
    let doc = attachments::NewDocument {
        name: &upload.data.name,
        description: upload.data.description.as_deref(),
        file_type: upload.data.file_type.as_deref().unwrap_or("other"),
    };
    let now = chrono::Utc::now().timestamp();
    db.with_conn(|conn| {
        attachments::add_insurance_document_bytes(
            conn,
            &upload.owner_id,
            &upload.file_name,
            &bytes,
            &doc,
            now,
        )
    })
}

/// Delete an insurance document
#[tauri::command]
pub async fn delete_insurance_document(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    document_id: String,
) -> Result<()> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {}", e)))?;
    db.with_conn(|conn| {
        crate::services::attachments::delete_document(
            conn,
            &dir,
            crate::models::AttachmentKind::InsuranceDocument,
            &document_id,
        )
    })
}

/// Open an insurance document in the system default application, through a temporary decrypted
/// copy (D3)
#[tauri::command]
pub async fn open_insurance_document(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    document_id: String,
) -> Result<()> {
    crate::commands::attachments::open_elsewhere(
        &app,
        &db,
        crate::models::AttachmentKind::InsuranceDocument,
        &document_id,
    )
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
                    r.recurring_costs, r.photos, r.notes, r.created_at, r.updated_at,
                    r.purchase_date
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
                    purchase_date: row.get(15)?,
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
