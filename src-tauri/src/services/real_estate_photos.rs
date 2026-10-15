//! Storing uploaded real estate photos.
//!
//! The webview sends each photo's bytes over IPC, so no copy of the photo is ever written outside
//! the app data dir. A photo is kept as `<uuid>.jpg` (longest edge at most 2560 px) plus a 400 px
//! `<uuid>_thumb.jpg` under `real_estate_photos/<property id>/<batch id>/`; the DB stores both
//! paths relative to the app data dir.
//!
//! Decoding and resizing take a while for a camera photo, so they run in `store_photo` without the
//! connection; `record_photo` then inserts the row.

use crate::error::{AppError, Result};
use crate::models::RealEstatePhoto;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Folder under the app data dir that holds every property's photos.
pub const PHOTOS_DIR: &str = "real_estate_photos";
/// A larger photo is scaled down to this longest edge.
const MAX_EDGE: u32 = 2560;
/// Longest edge of a thumbnail.
const THUMB_EDGE: u32 = 400;

/// A photo whose files are on disk but whose row is not written yet.
pub struct StoredPhoto {
    photo: RealEstatePhoto,
    files: [PathBuf; 2],
}

impl StoredPhoto {
    fn discard(&self) {
        for file in &self.files {
            let _ = fs::remove_file(file);
        }
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

/// Decode an uploaded image and write the photo and its thumbnail as JPEG into the batch folder.
/// Nothing is left on disk when either file fails.
pub fn store_photo(
    app_data_dir: &Path,
    real_estate_id: &str,
    batch_id: &str,
    bytes: &[u8],
) -> Result<StoredPhoto> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::Internal(format!("Failed to read the uploaded image: {}", e)))?;

    let dir = app_data_dir
        .join(PHOTOS_DIR)
        .join(real_estate_id)
        .join(batch_id);
    fs::create_dir_all(&dir)?;

    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.jpg", id);
    let thumb_name = format!("{}_thumb.jpg", id);
    let rel_dir = format!("{}/{}/{}", PHOTOS_DIR, real_estate_id, batch_id);

    let stored = StoredPhoto {
        files: [dir.join(&file_name), dir.join(&thumb_name)],
        photo: RealEstatePhoto {
            id,
            batch_id: batch_id.to_string(),
            file_path: format!("{}/{}", rel_dir, file_name),
            thumbnail_path: format!("{}/{}", rel_dir, thumb_name),
            created_at: chrono::Utc::now().timestamp(),
        },
    };

    if let Err(e) = write_jpegs(&img, &stored.files) {
        stored.discard();
        return Err(e);
    }
    Ok(stored)
}

fn write_jpegs(img: &DynamicImage, [file, thumb]: &[PathBuf; 2]) -> Result<()> {
    let save = |img: &DynamicImage, path: &Path| {
        img.save_with_format(path, ImageFormat::Jpeg)
            .map_err(|e| AppError::Internal(format!("Failed to save image: {}", e)))
    };
    if img.width() > MAX_EDGE || img.height() > MAX_EDGE {
        save(&img.resize(MAX_EDGE, MAX_EDGE, FilterType::Lanczos3), file)?;
    } else {
        save(img, file)?;
    }
    save(&img.thumbnail(THUMB_EDGE, THUMB_EDGE), thumb)
}

/// Record a stored photo in its batch. The files are removed when the row cannot be written.
pub fn record_photo(conn: &Connection, stored: StoredPhoto) -> Result<RealEstatePhoto> {
    let photo = &stored.photo;
    let inserted = conn.execute(
        "INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            photo.id,
            photo.batch_id,
            photo.file_path,
            photo.thumbnail_path,
            photo.created_at
        ],
    );
    if let Err(e) = inserted {
        stored.discard();
        return Err(e.into());
    }
    Ok(stored.photo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
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
                file_path TEXT NOT NULL,
                thumbnail_path TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date)
                VALUES ('batch-1', 'prop-1', 0);
            "#,
        )
        .expect("schema");
        conn
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("moony-photos-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 128]));
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .expect("encode png");
        out.into_inner()
    }

    fn batch_files(dir: &Path) -> Vec<String> {
        let batch = dir.join(PHOTOS_DIR).join("prop-1").join("batch-1");
        let Ok(entries) = fs::read_dir(batch) else {
            return vec![];
        };
        let mut names: Vec<String> = entries
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn batch_property_names_the_owning_property() {
        let conn = setup_test_db();
        assert_eq!(batch_property(&conn, "batch-1").expect("found"), "prop-1");
    }

    #[test]
    fn an_unknown_batch_is_not_found() {
        let conn = setup_test_db();
        let err = batch_property(&conn, "nope").expect_err("missing");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn a_large_photo_is_scaled_down_and_gets_a_thumbnail() {
        let conn = setup_test_db();
        let dir = temp_dir("large");

        let stored = store_photo(&dir, "prop-1", "batch-1", &png(3000, 1500)).expect("stored");
        let photo = record_photo(&conn, stored).expect("recorded");

        let rel = format!("{}/prop-1/batch-1/", PHOTOS_DIR);
        assert_eq!(photo.file_path, format!("{}{}.jpg", rel, photo.id));
        assert_eq!(
            photo.thumbnail_path,
            format!("{}{}_thumb.jpg", rel, photo.id)
        );
        assert_eq!(photo.batch_id, "batch-1");

        let full = image::open(dir.join(&photo.file_path)).expect("full");
        assert_eq!((full.width(), full.height()), (2560, 1280));
        let thumb = image::open(dir.join(&photo.thumbnail_path)).expect("thumb");
        assert_eq!((thumb.width(), thumb.height()), (400, 200));
        assert_eq!(
            image::guess_format(&fs::read(dir.join(&photo.file_path)).expect("read"))
                .expect("format"),
            ImageFormat::Jpeg
        );

        let row: (String, String) = conn
            .query_row(
                "SELECT file_path, thumbnail_path FROM real_estate_photos WHERE id = ?1",
                [&photo.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row");
        assert_eq!(row, (photo.file_path, photo.thumbnail_path));
    }

    #[test]
    fn a_small_photo_keeps_its_size() {
        let dir = temp_dir("small");
        let stored = store_photo(&dir, "prop-1", "batch-1", &png(800, 600)).expect("stored");
        let full = image::open(&stored.files[0]).expect("full");
        assert_eq!((full.width(), full.height()), (800, 600));
    }

    #[test]
    fn unreadable_bytes_leave_nothing_behind() {
        let dir = temp_dir("garbage");
        let err = store_photo(&dir, "prop-1", "batch-1", b"not an image").err();
        assert!(matches!(err, Some(AppError::Internal(_))));
        assert!(batch_files(&dir).is_empty());
    }

    #[test]
    fn the_files_are_removed_when_the_row_cannot_be_written() {
        let conn = setup_test_db();
        conn.execute_batch("DROP TABLE real_estate_photos")
            .expect("drop");
        let dir = temp_dir("no-row");

        let stored = store_photo(&dir, "prop-1", "batch-1", &png(10, 10)).expect("stored");
        assert_eq!(batch_files(&dir).len(), 2);

        assert!(record_photo(&conn, stored).is_err());
        assert!(batch_files(&dir).is_empty());
    }
}
