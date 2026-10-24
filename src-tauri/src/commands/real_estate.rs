//! Real estate commands

use crate::commands::portfolio::schedule_history_rebuild;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    AttachmentKind, InsertPhotoBatch, InsertRealEstate, InsertRealEstateDocument,
    InsertRealEstateOneTimeCost, InsurancePolicy, Loan, RealEstate, RealEstateDocument,
    RealEstateOneTimeCost, RealEstatePhoto, RealEstatePhotoBatch, UpdatePhotoBatch,
};
use crate::services::attachments;
use crate::services::history_plan::{self, PropertyFacts};
use crate::services::history_recalc::HistoryRecalc;
use crate::services::real_estate as real_estate_service;
use crate::services::real_estate_photos as photos_service;
use crate::services::valuations::ValuationKind;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {}", e)))
}

/// Get all real estate
#[tauri::command]
pub async fn get_all_real_estate(db: State<'_, Database>) -> Result<Vec<RealEstate>> {
    db.with_conn(real_estate_service::list_properties)
}

/// Get single real estate
#[tauri::command]
pub async fn get_real_estate(db: State<'_, Database>, id: String) -> Result<Option<RealEstate>> {
    db.with_conn(|conn| real_estate_service::get_property(conn, &id))
}

/// Create real estate
#[tauri::command]
pub async fn create_real_estate(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    data: InsertRealEstate,
) -> Result<RealEstate> {
    let created = db.with_conn(|conn| {
        let id = real_estate_service::create_property(conn, &data)?;
        real_estate_service::get_property(conn, &id)?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created property".into()))
    })?;
    // The value line starts at the purchase date, else at the first estimate, and the days
    // before it change with the remainder of a live row
    schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);
    Ok(created)
}

/// Update real estate (replaces the whole record, see the service)
#[tauri::command]
pub async fn update_real_estate(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    id: String,
    data: InsertRealEstate,
) -> Result<RealEstate> {
    let (before, updated, valuations) = db.with_conn(|conn| {
        let before = real_estate_service::get_property(conn, &id)?;
        let earliest_before =
            history_plan::earliest_valuation_day(conn, ValuationKind::RealEstate, &id)?;
        real_estate_service::update_property(conn, &id, &data, chrono::Utc::now().timestamp())?;
        let updated = real_estate_service::get_property(conn, &id)?
            .ok_or_else(|| AppError::NotFound("Real estate not found".into()))?;
        let earliest_after =
            history_plan::earliest_valuation_day(conn, ValuationKind::RealEstate, &id)?;
        Ok((before, updated, (earliest_before, earliest_after)))
    })?;
    // Every day, when the edit changed a fact the history reads (the purchase date or price, the
    // type, a currency) or wrote the property's first estimate (a price change)
    if let Some(before) = before {
        let job = history_plan::property_updated(
            &PropertyFacts::from(&before),
            &PropertyFacts::from(&updated),
            valuations,
        );
        if let Some(job) = job {
            schedule_history_rebuild(&app, &db, &recalc, vec![job]);
        }
    }
    Ok(updated)
}

/// Delete real estate with everything attached to it
#[tauri::command]
pub async fn delete_real_estate(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    id: String,
) -> Result<()> {
    let dir = data_dir(&app)?;
    db.with_conn(|conn| real_estate_service::delete_property(conn, &dir, &id))?;
    schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);
    Ok(())
}

/// Get one-time costs for real estate
#[tauri::command]
pub async fn get_real_estate_costs(
    db: State<'_, Database>,
    real_estate_id: String,
) -> Result<Vec<RealEstateOneTimeCost>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, real_estate_id, name, description, amount, currency, date, created_at
             FROM real_estate_one_time_costs WHERE real_estate_id = ?1 ORDER BY date DESC",
        )?;

        let costs = stmt
            .query_map([&real_estate_id], |row| {
                Ok(RealEstateOneTimeCost {
                    id: row.get(0)?,
                    real_estate_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    amount: row.get(4)?,
                    currency: row.get(5)?,
                    date: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(costs)
    })
}

/// Create one-time cost
#[tauri::command]
pub async fn create_real_estate_cost(
    db: State<'_, Database>,
    data: InsertRealEstateOneTimeCost,
) -> Result<RealEstateOneTimeCost> {
    db.with_conn(|conn| real_estate_service::create_one_time_cost(conn, data))
}

/// Delete one-time cost
#[tauri::command]
pub async fn delete_real_estate_cost(db: State<'_, Database>, cost_id: String) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM real_estate_one_time_costs WHERE id = ?1",
            [&cost_id],
        )?;
        Ok(())
    })
}

/// Update one-time cost
#[tauri::command]
pub async fn update_real_estate_cost(
    db: State<'_, Database>,
    cost_id: String,
    data: InsertRealEstateOneTimeCost,
) -> Result<RealEstateOneTimeCost> {
    db.with_conn(|conn| real_estate_service::update_one_time_cost(conn, &cost_id, data))
}

/// Link loan to real estate (enforces 1 loan → 1 property)
#[tauri::command]
pub async fn link_loan_to_real_estate(
    db: State<'_, Database>,
    real_estate_id: String,
    loan_id: String,
) -> Result<()> {
    db.with_conn(|conn| {
        // First, remove any existing link for this loan (enforce 1 loan → 1 property)
        conn.execute(
            "DELETE FROM real_estate_loans WHERE loan_id = ?1",
            [&loan_id],
        )?;
        // Then create the new link
        conn.execute(
            "INSERT INTO real_estate_loans (real_estate_id, loan_id) VALUES (?1, ?2)",
            [&real_estate_id, &loan_id],
        )?;
        Ok(())
    })
}

/// Unlink loan from real estate
#[tauri::command]
pub async fn unlink_loan_from_real_estate(
    db: State<'_, Database>,
    real_estate_id: String,
    loan_id: String,
) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM real_estate_loans WHERE real_estate_id = ?1 AND loan_id = ?2",
            [&real_estate_id, &loan_id],
        )?;
        Ok(())
    })
}

/// Get loans linked to real estate
#[tauri::command]
pub async fn get_real_estate_loans(
    db: State<'_, Database>,
    real_estate_id: String,
) -> Result<Vec<Loan>> {
    db.with_conn(|conn| {
        crate::services::loans::list_loans_for_real_estate(
            conn,
            &real_estate_id,
            crate::services::loan_amortization::today_utc_day(),
        )
    })
}

// ============================================================================
// Insurance Linking Commands
// ============================================================================

/// Link insurance to real estate
#[tauri::command]
pub async fn link_insurance_to_real_estate(
    db: State<'_, Database>,
    real_estate_id: String,
    insurance_id: String,
) -> Result<()> {
    db.with_conn(|conn| {
        // First, remove any existing link for this insurance (insurance can only have one real estate)
        conn.execute(
            "DELETE FROM real_estate_insurances WHERE insurance_id = ?1",
            [&insurance_id],
        )?;
        // Then create the new link
        conn.execute(
            "INSERT INTO real_estate_insurances (real_estate_id, insurance_id) VALUES (?1, ?2)",
            [&real_estate_id, &insurance_id],
        )?;
        Ok(())
    })
}

/// Unlink insurance from real estate
#[tauri::command]
pub async fn unlink_insurance_from_real_estate(
    db: State<'_, Database>,
    real_estate_id: String,
    insurance_id: String,
) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM real_estate_insurances WHERE real_estate_id = ?1 AND insurance_id = ?2",
            [&real_estate_id, &insurance_id],
        )?;
        Ok(())
    })
}

/// Get insurances linked to real estate
#[tauri::command]
pub async fn get_real_estate_insurances(
    db: State<'_, Database>,
    real_estate_id: String,
) -> Result<Vec<InsurancePolicy>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT i.id, i.type, i.provider, i.policy_name, i.policy_number, i.start_date, i.end_date,
                    i.payment_frequency, i.one_time_payment, i.one_time_payment_currency,
                    i.regular_payment, i.regular_payment_currency, i.limits, i.notes, i.status,
                    i.created_at, i.updated_at
             FROM insurance_policies i
             INNER JOIN real_estate_insurances rei ON i.id = rei.insurance_id
             WHERE rei.real_estate_id = ?1"
        )?;

        let policies = stmt.query_map([&real_estate_id], |row| {
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
        })?.filter_map(|r| r.ok()).collect();

        Ok(policies)
    })
}

// ============================================================================
// Photo Batch Commands
// ============================================================================

/// Get all photo batches for a real estate property
#[tauri::command]
pub async fn get_real_estate_photo_batches(
    db: State<'_, Database>,
    real_estate_id: String,
) -> Result<Vec<RealEstatePhotoBatch>> {
    db.with_conn(|conn| {
        // Get batches
        let mut batch_stmt = conn.prepare(
            "SELECT id, real_estate_id, photo_date, description, created_at
             FROM real_estate_photo_batches WHERE real_estate_id = ?1 ORDER BY photo_date DESC",
        )?;

        let batches: Vec<(String, String, i64, Option<String>, i64)> = batch_stmt
            .query_map([&real_estate_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Get photos for each batch
        let mut photo_stmt = conn.prepare(
            "SELECT id, batch_id, created_at
             FROM real_estate_photos WHERE batch_id = ?1 ORDER BY created_at",
        )?;

        let result: Vec<RealEstatePhotoBatch> = batches
            .into_iter()
            .map(
                |(id, real_estate_id, photo_date, description, created_at)| {
                    let photos: Vec<RealEstatePhoto> = photo_stmt
                        .query_map([&id], |row| {
                            Ok(RealEstatePhoto {
                                id: row.get(0)?,
                                batch_id: row.get(1)?,
                                created_at: row.get(2)?,
                            })
                        })
                        .ok()
                        .map(|iter| iter.filter_map(|r| r.ok()).collect())
                        .unwrap_or_default();

                    RealEstatePhotoBatch {
                        id,
                        real_estate_id,
                        photo_date,
                        description,
                        photos,
                        created_at,
                    }
                },
            )
            .collect();

        Ok(result)
    })
}

/// Create a new photo batch (without photos initially)
#[tauri::command]
pub async fn create_photo_batch(
    db: State<'_, Database>,
    real_estate_id: String,
    data: InsertPhotoBatch,
) -> Result<RealEstatePhotoBatch> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date, description, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, real_estate_id, data.photo_date, data.description, now],
        )?;

        Ok(RealEstatePhotoBatch {
            id,
            real_estate_id,
            photo_date: data.photo_date,
            description: data.description,
            photos: vec![],
            created_at: now,
        })
    })
}

/// Add one photo to a batch. The webview sends the image bytes as the IPC body, so no copy of the
/// photo is written to the temp dir, and the batch id in the `batch-id` header. The photo goes into
/// the encrypted attachments store.
#[tauri::command]
pub async fn add_photo_to_batch(
    db: State<'_, Database>,
    request: tauri::ipc::Request<'_>,
) -> Result<RealEstatePhoto> {
    let bytes = photos_service::photo_bytes(request.body())?;
    let batch_id = request
        .headers()
        .get("batch-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Internal("add_photo_to_batch expects a batch-id header".into()))?;

    let prepared = photos_service::prepare_photo(&bytes)?;
    let now = chrono::Utc::now().timestamp();
    db.with_conn(|conn| photos_service::record_photo(conn, batch_id, &prepared, now))
}

/// Update photo batch metadata
#[tauri::command]
pub async fn update_photo_batch(
    db: State<'_, Database>,
    batch_id: String,
    data: UpdatePhotoBatch,
) -> Result<RealEstatePhotoBatch> {
    db.with_conn(|conn| {
        // Build update query dynamically
        let mut updates = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(date) = data.photo_date {
            updates.push("photo_date = ?");
            params.push(Box::new(date));
        }
        if let Some(ref desc) = data.description {
            updates.push("description = ?");
            params.push(Box::new(desc.clone()));
        }

        if updates.is_empty() {
            return Err(AppError::Validation("validation.noFieldsToUpdate".into()));
        }

        let sql = format!(
            "UPDATE real_estate_photo_batches SET {} WHERE id = ?",
            updates.join(", ")
        );
        params.push(Box::new(batch_id.clone()));

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, params_ref.as_slice())?;

        // Fetch updated batch with photos
        let batch = conn.query_row(
            "SELECT id, real_estate_id, photo_date, description, created_at
             FROM real_estate_photo_batches WHERE id = ?1",
            [&batch_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;

        let mut photo_stmt = conn.prepare(
            "SELECT id, batch_id, created_at
             FROM real_estate_photos WHERE batch_id = ?1",
        )?;
        let photos: Vec<RealEstatePhoto> = photo_stmt
            .query_map([&batch_id], |row| {
                Ok(RealEstatePhoto {
                    id: row.get(0)?,
                    batch_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(RealEstatePhotoBatch {
            id: batch.0,
            real_estate_id: batch.1,
            photo_date: batch.2,
            description: batch.3,
            photos,
            created_at: batch.4,
        })
    })
}

/// Delete a photo batch and all its photos
#[tauri::command]
pub async fn delete_photo_batch(
    app_handle: tauri::AppHandle,
    db: State<'_, Database>,
    batch_id: String,
) -> Result<()> {
    let dir = data_dir(&app_handle)?;
    db.with_conn(|conn| attachments::delete_photo_batch(conn, &dir, &batch_id))
}

/// Delete a single photo from a batch
#[tauri::command]
pub async fn delete_real_estate_photo(
    app_handle: tauri::AppHandle,
    db: State<'_, Database>,
    photo_id: String,
) -> Result<()> {
    let dir = data_dir(&app_handle)?;
    db.with_conn(|conn| attachments::delete_photo(conn, &dir, &photo_id))
}

// ============================================================================
// Document Commands
// ============================================================================

/// Get all documents for a real estate property
#[tauri::command]
pub async fn get_real_estate_documents(
    db: State<'_, Database>,
    real_estate_id: String,
) -> Result<Vec<RealEstateDocument>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, real_estate_id, name, description, file_path, file_type, file_size, uploaded_at,
                    mime_type, original_name
             FROM real_estate_documents WHERE real_estate_id = ?1 ORDER BY uploaded_at DESC"
        )?;

        let documents = stmt.query_map([&real_estate_id], |row| {
            let file_path: Option<String> = row.get(4)?;
            let mime_type: Option<String> = row.get(8)?;
            let original_name: Option<String> = row.get(9)?;
            let (mime_type, extension) = crate::services::attachments::document_meta(
                file_path.as_deref(),
                mime_type.as_deref(),
                original_name.as_deref(),
            );
            Ok(RealEstateDocument {
                id: row.get(0)?,
                real_estate_id: row.get(1)?,
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

/// Add a document to a real estate property. The file the user picked is stored in the encrypted
/// attachments store.
#[tauri::command]
pub async fn add_real_estate_document(
    db: State<'_, Database>,
    real_estate_id: String,
    file_path: String,
    data: InsertRealEstateDocument,
) -> Result<RealEstateDocument> {
    data.validate()?;
    let file_type = data.file_type.as_deref().unwrap_or("other");
    let doc = attachments::NewDocument {
        name: &data.name,
        description: data.description.as_deref(),
        file_type,
    };
    let now = chrono::Utc::now().timestamp();
    db.with_conn(|conn| {
        attachments::add_real_estate_document(
            conn,
            &real_estate_id,
            std::path::Path::new(&file_path),
            &doc,
            now,
        )
    })
}

/// Add a document to a real estate property from its bytes (the phone: a photographed contract or
/// a file from the Files app, which the webview holds only as bytes and may not write to disk).
/// The bytes are the IPC body; the `owner-id`, `file-name` and `document` headers carry the
/// property's id, the file's name and the entries as `InsertRealEstateDocument` JSON, the last two
/// percent-encoded. The bytes go straight into the encrypted attachments store.
#[tauri::command]
pub async fn add_real_estate_document_bytes(
    db: State<'_, Database>,
    request: tauri::ipc::Request<'_>,
) -> Result<RealEstateDocument> {
    let upload = attachments::document_upload::<InsertRealEstateDocument>(request.headers())?;
    upload.data.validate()?;
    let bytes = photos_service::photo_bytes(request.body())?;
    let doc = attachments::NewDocument {
        name: &upload.data.name,
        description: upload.data.description.as_deref(),
        file_type: upload.data.file_type.as_deref().unwrap_or("other"),
    };
    let now = chrono::Utc::now().timestamp();
    db.with_conn(|conn| {
        attachments::add_real_estate_document_bytes(
            conn,
            &upload.owner_id,
            &upload.file_name,
            &bytes,
            &doc,
            now,
        )
    })
}

/// Delete a real estate document
#[tauri::command]
pub async fn delete_real_estate_document(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    document_id: String,
) -> Result<()> {
    let dir = data_dir(&app)?;
    db.with_conn(|conn| {
        attachments::delete_document(conn, &dir, AttachmentKind::RealEstateDocument, &document_id)
    })
}

/// Open a real estate document in the system default application, through a temporary decrypted
/// copy (D3)
#[tauri::command]
pub async fn open_real_estate_document(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    document_id: String,
) -> Result<()> {
    crate::commands::attachments::open_elsewhere(
        &app,
        &db,
        AttachmentKind::RealEstateDocument,
        &document_id,
    )
}

// ---------------------------------------------------------------------------
// Valuation log (redesign phase 2)
// ---------------------------------------------------------------------------

/// Dated valuations of a property, oldest first.
#[tauri::command]
pub async fn get_real_estate_valuations(
    db: State<'_, Database>,
    real_estate_id: String,
) -> Result<Vec<crate::models::AssetValuation>> {
    db.with_conn(|conn| {
        crate::services::valuations::list_valuations(
            conn,
            crate::services::valuations::ValuationKind::RealEstate,
            &real_estate_id,
        )
    })
}

/// Dated valuations of every property, oldest first (the list page marks them on its chart).
#[tauri::command]
pub async fn get_all_real_estate_valuations(
    db: State<'_, Database>,
) -> Result<Vec<crate::models::AssetValuation>> {
    db.with_conn(|conn| {
        crate::services::valuations::list_all_valuations(
            conn,
            crate::services::valuations::ValuationKind::RealEstate,
        )
    })
}

/// Record an estimate ("Přecenit"); the newest one becomes the market price.
#[tauri::command]
pub async fn add_real_estate_valuation(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    data: crate::models::InsertAssetValuation,
) -> Result<crate::models::AssetValuation> {
    let saved = db.with_conn(|conn| {
        crate::services::valuations::add_valuation(conn, ValuationKind::RealEstate, &data)
    })?;
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();
    // An estimate changes the days it applies to; the first one also opens the days back to
    // the purchase date
    schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);
    Ok(saved)
}

/// Remove an estimate; the market price follows the newest remaining one.
#[tauri::command]
pub async fn delete_real_estate_valuation(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    valuation_id: String,
) -> Result<()> {
    db.with_conn(|conn| {
        crate::services::valuations::delete_valuation(
            conn,
            ValuationKind::RealEstate,
            &valuation_id,
        )
    })?;
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();
    schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::all_days()]);
    Ok(())
}
