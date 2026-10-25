//! Storing uploaded real estate photos in the encrypted attachments store (ADR 0017).
//!
//! The webview sends each photo's bytes over IPC. A photo is kept as two JPEG blobs: the photo
//! (longest edge at most 2560 px) and a 400 px thumbnail; the row references both. Nothing is
//! written to disk outside `attachments.db`.
//!
//! Decoding and resizing take a while for a camera photo, so they run in [`prepare_photo`] without
//! the connection; [`record_photo`] then stores both blobs and the row in one transaction.
//!
//! Photos from earlier versions are plain files under [`PHOTOS_DIR`] until the conversion reaches
//! them; reading and deleting handle both (`services::attachments`).

use crate::error::{AppError, Result};
use crate::models::RealEstatePhoto;
use crate::services::attachment_store;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use rusqlite::{Connection, OptionalExtension};
use std::borrow::Cow;
use std::io::Cursor;
use tauri::ipc::InvokeBody;
use uuid::Uuid;

/// Folder under the app data dir that holds the photos of earlier versions.
pub const PHOTOS_DIR: &str = "real_estate_photos";
/// A larger photo is scaled down to this longest edge.
const MAX_EDGE: u32 = 2560;
/// Longest edge of a thumbnail.
const THUMB_EDGE: u32 = 400;

/// A decoded upload, encoded as the photo's and the thumbnail's JPEG bytes.
pub struct PreparedPhoto {
    full: Vec<u8>,
    thumb: Vec<u8>,
}

/// The image bytes of an upload request. The webview sends them as a raw body; when the custom
/// protocol IPC is unavailable (blocked by the CSP, or a platform without it) Tauri falls back to
/// postMessage and the same bytes arrive as a JSON array of numbers.
pub fn photo_bytes(body: &InvokeBody) -> Result<Cow<'_, [u8]>> {
    let not_bytes =
        || AppError::Internal("the upload expects the file bytes as the request body".into());
    match body {
        InvokeBody::Raw(bytes) => Ok(Cow::Borrowed(bytes)),
        InvokeBody::Json(serde_json::Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(not_bytes)
            })
            .collect::<Result<Vec<u8>>>()
            .map(Cow::Owned),
        InvokeBody::Json(_) => Err(not_bytes()),
    }
}

/// The property a photo batch belongs to.
pub fn batch_property(conn: &Connection, batch_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT real_estate_id FROM real_estate_photo_batches WHERE id = ?1",
        [batch_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Batch not found".into()))
}

/// Decode an uploaded image and encode the photo and its thumbnail as JPEG, in memory.
pub fn prepare_photo(bytes: &[u8]) -> Result<PreparedPhoto> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::Internal(format!("Failed to read the uploaded image: {}", e)))?;
    let full = if img.width() > MAX_EDGE || img.height() > MAX_EDGE {
        img.resize(MAX_EDGE, MAX_EDGE, FilterType::Lanczos3)
    } else {
        img.clone()
    };
    Ok(PreparedPhoto {
        full: encode_jpeg(&full)?,
        thumb: encode_jpeg(&img.thumbnail(THUMB_EDGE, THUMB_EDGE))?,
    })
}

/// JPEG has no alpha channel, so the image is flattened to RGB first.
fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img.to_rgb8())
        .write_to(&mut out, ImageFormat::Jpeg)
        .map_err(|e| AppError::Internal(format!("Failed to encode image: {}", e)))?;
    Ok(out.into_inner())
}

/// Store a prepared photo in its batch: both blobs and the row, in one transaction.
pub fn record_photo(
    conn: &Connection,
    batch_id: &str,
    prepared: &PreparedPhoto,
    now: i64,
) -> Result<RealEstatePhoto> {
    batch_property(conn, batch_id)?;
    let photo = RealEstatePhoto {
        id: Uuid::new_v4().to_string(),
        batch_id: batch_id.to_string(),
        created_at: now,
    };
    let tx = conn.unchecked_transaction()?;
    let full = attachment_store::put_bytes(&tx, &prepared.full)?;
    let thumb = attachment_store::put_bytes(&tx, &prepared.thumb)?;
    tx.execute(
        "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            photo.id,
            photo.batch_id,
            full.id,
            thumb.id,
            photo.created_at
        ],
    )?;
    tx.commit()?;
    Ok(photo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::files_schema;
    use crate::models::AttachmentKind;
    use crate::services::attachments::{self, Variant};

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("foreign keys");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        files_schema::migrate(&conn).expect("files schema");
        conn.execute_batch(
            r#"
            CREATE TABLE real_estate_photo_batches (
                id TEXT PRIMARY KEY,
                real_estate_id TEXT NOT NULL,
                photo_date INTEGER NOT NULL,
                description TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE real_estate_photos (
                id TEXT PRIMARY KEY,
                batch_id TEXT NOT NULL REFERENCES real_estate_photo_batches(id) ON DELETE CASCADE,
                file_path TEXT,
                thumbnail_path TEXT,
                blob_id TEXT,
                thumb_blob_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                CHECK ((file_path IS NOT NULL AND thumbnail_path IS NOT NULL)
                    OR (blob_id IS NOT NULL AND thumb_blob_id IS NOT NULL))
            );
            INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date)
                VALUES ('batch-1', 'prop-1', 0);
            "#,
        )
        .expect("schema");
        conn
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 128]));
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .expect("encode png");
        out.into_inner()
    }

    fn blob_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM files.blobs", [], |r| r.get(0))
            .expect("count")
    }

    #[test]
    fn photo_bytes_takes_a_raw_body_as_is() {
        let body = InvokeBody::Raw(vec![1, 2, 255]);
        assert_eq!(photo_bytes(&body).expect("bytes").as_ref(), &[1, 2, 255]);
    }

    #[test]
    fn photo_bytes_reads_the_json_array_of_the_postmessage_fallback() {
        let body = InvokeBody::Json(serde_json::json!([1, 2, 255]));
        assert_eq!(photo_bytes(&body).expect("bytes").as_ref(), &[1, 2, 255]);
    }

    #[test]
    fn photo_bytes_rejects_anything_but_bytes() {
        for value in [
            serde_json::json!({ "batchId": "b" }),
            serde_json::json!([1, 256]),
            serde_json::json!([1, -1]),
            serde_json::json!([1, "2"]),
        ] {
            let err = photo_bytes(&InvokeBody::Json(value)).expect_err("not bytes");
            assert!(matches!(err, AppError::Internal(_)));
        }
    }

    #[test]
    fn batch_property_names_the_owning_property() {
        let conn = setup_test_db();
        assert_eq!(batch_property(&conn, "batch-1").expect("found"), "prop-1");
    }

    #[test]
    fn an_unknown_batch_is_not_found_and_stores_nothing() {
        let conn = setup_test_db();
        let err = batch_property(&conn, "nope").expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
        let prepared = prepare_photo(&png(10, 10)).expect("prepared");
        assert!(matches!(
            record_photo(&conn, "nope", &prepared, 0),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(blob_count(&conn), 0);
    }

    #[test]
    fn a_large_photo_is_scaled_down_and_stored_as_two_jpeg_blobs() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");

        let prepared = prepare_photo(&png(3000, 1500)).expect("prepared");
        let photo = record_photo(&conn, "batch-1", &prepared, 1_700_000_000).expect("recorded");
        assert_eq!(photo.batch_id, "batch-1");
        assert_eq!(photo.created_at, 1_700_000_000);
        assert_eq!(blob_count(&conn), 2);

        let read = |variant| {
            let resolved =
                attachments::resolve(&conn, dir.path(), AttachmentKind::Photo, &photo.id, variant)
                    .expect("resolve")
                    .expect("row");
            attachments::read_bytes(&conn, &resolved).expect("bytes")
        };
        let full_bytes = read(Variant::Full);
        let full = image::load_from_memory(&full_bytes).expect("full");
        assert_eq!((full.width(), full.height()), (2560, 1280));
        assert_eq!(
            image::guess_format(&full_bytes).expect("format"),
            ImageFormat::Jpeg
        );
        let thumb = image::load_from_memory(&read(Variant::Thumbnail)).expect("thumb");
        assert_eq!((thumb.width(), thumb.height()), (400, 200));

        let paths: (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT file_path, thumbnail_path FROM real_estate_photos WHERE id = ?1",
                [&photo.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row");
        assert_eq!(paths, (None, None), "a new photo has no file on disk");
        assert!(std::fs::read_dir(dir.path()).expect("dir").next().is_none());
    }

    #[test]
    fn a_small_photo_keeps_its_size() {
        let prepared = prepare_photo(&png(800, 600)).expect("prepared");
        let full = image::load_from_memory(&prepared.full).expect("full");
        assert_eq!((full.width(), full.height()), (800, 600));
    }

    #[test]
    fn unreadable_bytes_are_refused() {
        assert!(matches!(
            prepare_photo(b"not an image"),
            Err(AppError::Internal(_))
        ));
    }

    #[test]
    fn nothing_is_stored_when_the_row_cannot_be_written() {
        let conn = setup_test_db();
        conn.execute_batch("DROP TABLE real_estate_photos")
            .expect("drop");
        let prepared = prepare_photo(&png(10, 10)).expect("prepared");
        assert!(record_photo(&conn, "batch-1", &prepared, 0).is_err());
        assert_eq!(blob_count(&conn), 0);
    }
}
