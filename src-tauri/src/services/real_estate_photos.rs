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
use crate::models::{AttachmentKind, RealEstatePhoto, RealEstatePhotoBatch};
use crate::services::attachment_store;
use crate::services::attachments::{ContentLookup, Variant};
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

/// The bytes of an upload request (a photo or a document). The webview sends them as a raw body;
/// when the custom protocol IPC is unavailable (blocked by the CSP, or a platform without it)
/// Tauri falls back to postMessage and the same bytes arrive as a JSON array of numbers.
///
/// A body longer than `max_len` bytes (a JSON array with more than `max_len` items) is refused
/// with `validation.attachmentTooLarge` before anything is converted, so a huge array never
/// becomes a byte vector. The count is checked before any item is read.
pub fn upload_bytes(body: &InvokeBody, max_len: u64) -> Result<Cow<'_, [u8]>> {
    let not_bytes =
        || AppError::Internal("the upload expects the file bytes as the request body".into());
    let too_large = || AppError::Validation("validation.attachmentTooLarge".into());
    match body {
        InvokeBody::Raw(bytes) => {
            if bytes.len() as u64 > max_len {
                return Err(too_large());
            }
            Ok(Cow::Borrowed(bytes))
        }
        InvokeBody::Json(serde_json::Value::Array(items)) => {
            if items.len() as u64 > max_len {
                return Err(too_large());
            }
            items
                .iter()
                .map(|v| {
                    v.as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or_else(not_bytes)
                })
                .collect::<Result<Vec<u8>>>()
                .map(Cow::Owned)
        }
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

/// A prepared photo of two given byte strings, for tests that need rows with content without
/// decoding an image.
#[cfg(test)]
pub(crate) fn prepared_for_test(full: Vec<u8>, thumb: Vec<u8>) -> PreparedPhoto {
    PreparedPhoto { full, thumb }
}

/// Store a prepared photo in its batch: both blobs and the row, in one transaction.
pub fn record_photo(
    conn: &Connection,
    batch_id: &str,
    prepared: &PreparedPhoto,
    now: i64,
) -> Result<RealEstatePhoto> {
    batch_property(conn, batch_id)?;
    let id = Uuid::new_v4().to_string();
    let tx = conn.unchecked_transaction()?;
    let full = attachment_store::put_bytes(&tx, &prepared.full)?;
    let thumb = attachment_store::put_bytes(&tx, &prepared.thumb)?;
    tx.execute(
        "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, batch_id, full.id, thumb.id, now],
    )?;
    tx.commit()?;
    let content = ContentLookup::new(conn)?;
    let (blob, thumb_blob) = (Some(full.id.as_str()), Some(thumb.id.as_str()));
    Ok(RealEstatePhoto {
        id,
        batch_id: batch_id.to_string(),
        created_at: now,
        content: content.of(AttachmentKind::Photo, blob, thumb_blob, Variant::Full)?,
        thumb_content: content.of(AttachmentKind::Photo, blob, thumb_blob, Variant::Thumbnail)?,
    })
}

/// The photos of a batch, oldest first, each with where its content and thumbnail are.
pub fn list_photos(conn: &Connection, batch_id: &str) -> Result<Vec<RealEstatePhoto>> {
    photos_of(conn, &ContentLookup::new(conn)?, batch_id)
}

fn photos_of(
    conn: &Connection,
    content: &ContentLookup,
    batch_id: &str,
) -> Result<Vec<RealEstatePhoto>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, batch_id, created_at, blob_id, thumb_blob_id
         FROM real_estate_photos WHERE batch_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt.query_map([batch_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut photos = Vec::new();
    for row in rows {
        let (id, batch_id, created_at, blob, thumb) = row?;
        let (blob, thumb) = (blob.as_deref(), thumb.as_deref());
        photos.push(RealEstatePhoto {
            id,
            batch_id,
            created_at,
            content: content.of(AttachmentKind::Photo, blob, thumb, Variant::Full)?,
            thumb_content: content.of(AttachmentKind::Photo, blob, thumb, Variant::Thumbnail)?,
        });
    }
    Ok(photos)
}

/// A batch with its photos.
pub fn batch(conn: &Connection, batch_id: &str) -> Result<RealEstatePhotoBatch> {
    let content = ContentLookup::new(conn)?;
    let (id, real_estate_id, photo_date, description, created_at) = conn
        .query_row(
            "SELECT id, real_estate_id, photo_date, description, created_at
             FROM real_estate_photo_batches WHERE id = ?1",
            [batch_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("Batch not found".into()))?;
    Ok(RealEstatePhotoBatch {
        photos: photos_of(conn, &content, &id)?,
        id,
        real_estate_id,
        photo_date,
        description,
        created_at,
    })
}

/// The photo batches of a property, newest first, with their photos (oldest first).
pub fn list_batches(conn: &Connection, real_estate_id: &str) -> Result<Vec<RealEstatePhotoBatch>> {
    let content = ContentLookup::new(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, real_estate_id, photo_date, description, created_at
         FROM real_estate_photo_batches WHERE real_estate_id = ?1 ORDER BY photo_date DESC",
    )?;
    let rows = stmt.query_map([real_estate_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut batches = Vec::new();
    for row in rows {
        let (id, real_estate_id, photo_date, description, created_at) = row?;
        batches.push(RealEstatePhotoBatch {
            photos: photos_of(conn, &content, &id)?,
            id,
            real_estate_id,
            photo_date,
            description,
            created_at,
        });
    }
    Ok(batches)
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
            CREATE TABLE sync_state (id INTEGER PRIMARY KEY, enabled INTEGER NOT NULL DEFAULT 0);
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
    fn upload_bytes_takes_a_raw_body_as_is() {
        let body = InvokeBody::Raw(vec![1, 2, 255]);
        assert_eq!(
            upload_bytes(&body, 16).expect("bytes").as_ref(),
            &[1, 2, 255]
        );
    }

    #[test]
    fn upload_bytes_reads_the_json_array_of_the_postmessage_fallback() {
        let body = InvokeBody::Json(serde_json::json!([1, 2, 255]));
        assert_eq!(
            upload_bytes(&body, 16).expect("bytes").as_ref(),
            &[1, 2, 255]
        );
    }

    #[test]
    fn upload_bytes_rejects_anything_but_bytes() {
        for value in [
            serde_json::json!({ "batchId": "b" }),
            serde_json::json!([1, 256]),
            serde_json::json!([1, -1]),
            serde_json::json!([1, "2"]),
        ] {
            let err = upload_bytes(&InvokeBody::Json(value), 16).expect_err("not bytes");
            assert!(matches!(err, AppError::Internal(_)));
        }
    }

    #[test]
    fn upload_bytes_refuses_a_json_array_longer_than_the_limit() {
        // The second array has a bad item as well: it is refused for its length, which shows the
        // length is checked before any item is read.
        for value in [
            serde_json::json!([1, 2, 3, 4, 5]),
            serde_json::json!([1, 2, 3, 4, "x"]),
        ] {
            let err = upload_bytes(&InvokeBody::Json(value), 4).expect_err("too long");
            assert!(
                matches!(&err, AppError::Validation(key) if key == "validation.attachmentTooLarge"),
                "{err:?}"
            );
        }
    }

    #[test]
    fn upload_bytes_takes_an_array_at_the_limit() {
        let body = InvokeBody::Json(serde_json::json!([1, 2, 3, 4]));
        assert_eq!(
            upload_bytes(&body, 4).expect("bytes").as_ref(),
            &[1, 2, 3, 4]
        );
    }

    #[test]
    fn upload_bytes_refuses_a_raw_body_over_the_limit() {
        let err = upload_bytes(&InvokeBody::Raw(vec![0; 5]), 4).expect_err("too long");
        assert!(
            matches!(&err, AppError::Validation(key) if key == "validation.attachmentTooLarge"),
            "{err:?}"
        );
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

    #[test]
    fn photo_batches_list_with_both_contents_newest_first() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date, description) \
               VALUES ('old', 'prop-1', 100, 'Old'), ('new', 'prop-1', 300, 'New'), \
                      ('mid', 'prop-1', 200, NULL), ('other', 'prop-2', 400, 'Other');",
        )
        .expect("batches");
        let full = attachment_store::put_bytes(&conn, b"full")
            .expect("blob")
            .id;
        let thumb = attachment_store::put_bytes(&conn, b"thumb")
            .expect("blob")
            .id;
        for (id, batch, at) in [("p2", "new", 20), ("p1", "new", 10), ("q1", "old", 5)] {
            conn.execute(
                "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, batch, full, thumb, at],
            )
            .expect("photo");
        }

        let batches = list_batches(&conn, "prop-1").expect("list");
        let order: Vec<_> = batches.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(order, ["new", "mid", "old", "batch-1"]);
        let photos: Vec<_> = batches[0].photos.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(photos, ["p1", "p2"], "oldest photo first");
        assert!(batches[1].photos.is_empty());
        assert_eq!(batches[0].description.as_deref(), Some("New"));
        let photo = &batches[0].photos[0];
        assert_eq!(
            (photo.content, photo.thumb_content),
            (
                crate::models::AttachmentContent::Here,
                crate::models::AttachmentContent::Here
            )
        );
    }
}
