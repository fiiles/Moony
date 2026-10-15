//! The `moony-file` URI scheme: the only way attachment bytes reach the webview (ADR 0017).
//!
//! A URL names an attachment by its owning row (`/<kind>/<row id>`), never by a path or a blob
//! id. Content is served only while a profile is unlocked, with `no-store` caching, and only
//! images and PDFs keep their MIME type, so an attached HTML or SVG file can never run as a page.
//! A page fetch (pdf.js) needs CORS, which is granted to the app's own origin only.

use crate::db::Database;
use crate::error::AppError;
use crate::models::AttachmentKind;
use crate::services::attachments::{self, Variant};
use std::path::Path;
use tauri::http::{Response, StatusCode};

/// Scheme name registered with Tauri.
pub const SCHEME: &str = "moony-file";

/// What a URL asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub kind: AttachmentKind,
    pub variant: Variant,
    pub id: String,
}

/// Parse a request path. `convertFileSrc` encodes the whole `<kind>/<id>` as one segment, so the
/// separator may arrive as `%2F`.
pub fn parse_route(path: &str) -> Option<Route> {
    let decoded = percent_decode(path)?;
    let mut parts = decoded.trim_start_matches('/').splitn(2, '/');
    let (kind, variant) = match parts.next()? {
        "photo" => (AttachmentKind::Photo, Variant::Full),
        "photo-thumb" => (AttachmentKind::Photo, Variant::Thumbnail),
        "real-estate-document" => (AttachmentKind::RealEstateDocument, Variant::Full),
        "insurance-document" => (AttachmentKind::InsuranceDocument, Variant::Full),
        _ => return None,
    };
    let id = parts.next()?;
    let valid = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then(|| Route {
        kind,
        variant,
        id: id.to_string(),
    })
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = input.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The `Content-Type` an attachment is served with: images (except scriptable SVG) and PDFs keep
/// their type, everything else is an opaque download.
pub fn served_content_type(mime: &str) -> &str {
    let safe = (mime.starts_with("image/") && mime != "image/svg+xml") || mime == "application/pdf";
    if safe {
        mime
    } else {
        "application/octet-stream"
    }
}

/// The origin a CORS response may name: the app's own page, in every platform's form. Debug
/// builds also serve the dev server's page (`tauri dev` loads it from localhost).
pub fn allowed_origin(origin: Option<&str>) -> Option<&str> {
    let origin = origin?;
    let app = matches!(
        origin,
        "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost"
    );
    let dev = cfg!(debug_assertions)
        && (origin.starts_with("http://localhost:") || origin.starts_with("http://127.0.0.1:"));
    (app || dev).then_some(origin)
}

/// Build the response for a load result.
pub fn response_for(
    loaded: Result<Option<(Vec<u8>, String)>, AppError>,
    origin: Option<&str>,
) -> Response<Vec<u8>> {
    let status_only = |status: StatusCode| {
        let mut builder = Response::builder()
            .status(status)
            .header("Cache-Control", "no-store");
        if let Some(o) = allowed_origin(origin) {
            builder = builder.header("Access-Control-Allow-Origin", o);
        }
        builder.body(Vec::new()).unwrap_or_default()
    };
    match loaded {
        Ok(Some((body, mime))) => {
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", served_content_type(&mime))
                .header("Cache-Control", "no-store")
                .header("X-Content-Type-Options", "nosniff")
                .header("Content-Security-Policy", "sandbox; default-src 'none'");
            if let Some(o) = allowed_origin(origin) {
                builder = builder
                    .header("Access-Control-Allow-Origin", o)
                    .header("Vary", "Origin");
            }
            builder.body(body).unwrap_or_default()
        }
        Ok(None) | Err(AppError::NotFound(_)) => status_only(StatusCode::NOT_FOUND),
        Err(AppError::Auth(_)) => status_only(StatusCode::FORBIDDEN),
        Err(AppError::Internal(msg)) if msg == "attachments.storeUnavailable" => {
            status_only(StatusCode::SERVICE_UNAVAILABLE)
        }
        Err(e) => {
            log::warn!("[ATTACHMENTS] Serving an attachment failed: {e}");
            status_only(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Load what a route names: its bytes and MIME type.
pub fn load(
    conn: &rusqlite::Connection,
    data_dir: &Path,
    route: &Route,
) -> crate::error::Result<Option<(Vec<u8>, String)>> {
    let Some(resolved) =
        attachments::resolve(conn, data_dir, route.kind, &route.id, route.variant)?
    else {
        return Ok(None);
    };
    let bytes = attachments::read_bytes(conn, &resolved)?;
    Ok(Some((bytes, resolved.mime)))
}

/// Answer one scheme request. A locked app has no connection, so `with_conn` reports `Auth` and
/// the answer is 403.
pub fn respond(
    db: &Database,
    data_dir: &Path,
    method: &str,
    path: &str,
    origin: Option<&str>,
) -> Response<Vec<u8>> {
    if method != "GET" {
        let mut builder = Response::builder().status(StatusCode::METHOD_NOT_ALLOWED);
        if let Some(o) = allowed_origin(origin) {
            builder = builder.header("Access-Control-Allow-Origin", o);
        }
        return builder.body(Vec::new()).unwrap_or_default();
    }
    let Some(route) = parse_route(path) else {
        return response_for(Ok(None), origin);
    };
    let loaded = db.with_conn(|conn| load(conn, data_dir, &route));
    response_for(loaded, origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::files_schema;
    use crate::services::attachment_store;
    use rusqlite::Connection;

    #[test]
    fn routes_parse_with_plain_or_encoded_separators() {
        let id = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60";
        assert_eq!(
            parse_route(&format!("/photo%2F{id}")),
            Some(Route {
                kind: AttachmentKind::Photo,
                variant: Variant::Full,
                id: id.into()
            })
        );
        assert_eq!(
            parse_route(&format!("/photo-thumb/{id}")).map(|r| r.variant),
            Some(Variant::Thumbnail)
        );
        assert_eq!(
            parse_route(&format!("/insurance-document%2f{id}")).map(|r| r.kind),
            Some(AttachmentKind::InsuranceDocument)
        );
        assert_eq!(
            parse_route(&format!("/real-estate-document/{id}")).map(|r| r.kind),
            Some(AttachmentKind::RealEstateDocument)
        );
    }

    #[test]
    fn anything_else_does_not_parse() {
        for path in [
            "/",
            "/photo",
            "/photo/",
            "/video/abc",
            "/photo/../moony.db",
            "/photo%2F..%2Fmoony.db",
            "/photo/a b",
            "/photo/abc%",
            "/photo/abc%zz",
            "/photo/abc/def",
        ] {
            assert_eq!(parse_route(path), None, "{path}");
        }
    }

    #[test]
    fn only_images_and_pdfs_keep_their_type() {
        assert_eq!(served_content_type("image/jpeg"), "image/jpeg");
        assert_eq!(served_content_type("application/pdf"), "application/pdf");
        assert_eq!(
            served_content_type("image/svg+xml"),
            "application/octet-stream"
        );
        assert_eq!(served_content_type("text/html"), "application/octet-stream");
        assert_eq!(
            served_content_type(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            "application/octet-stream"
        );
    }

    #[test]
    fn cors_names_only_the_app_origin() {
        assert_eq!(
            allowed_origin(Some("tauri://localhost")),
            Some("tauri://localhost")
        );
        assert_eq!(
            allowed_origin(Some("http://tauri.localhost")),
            Some("http://tauri.localhost")
        );
        assert_eq!(allowed_origin(Some("https://evil.example")), None);
        assert_eq!(allowed_origin(Some("null")), None);
        assert_eq!(allowed_origin(None), None);
    }

    fn header<'a>(r: &'a Response<Vec<u8>>, name: &str) -> Option<&'a str> {
        r.headers().get(name).and_then(|v| v.to_str().ok())
    }

    #[test]
    fn a_served_attachment_carries_the_safety_headers() {
        let r = response_for(
            Ok(Some((b"<html>".to_vec(), "text/html".into()))),
            Some("tauri://localhost"),
        );
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.body(), &b"<html>".to_vec());
        assert_eq!(header(&r, "Content-Type"), Some("application/octet-stream"));
        assert_eq!(header(&r, "Cache-Control"), Some("no-store"));
        assert_eq!(header(&r, "X-Content-Type-Options"), Some("nosniff"));
        assert_eq!(
            header(&r, "Content-Security-Policy"),
            Some("sandbox; default-src 'none'")
        );
        assert_eq!(
            header(&r, "Access-Control-Allow-Origin"),
            Some("tauri://localhost")
        );

        let foreign = response_for(
            Ok(Some((vec![], "image/png".into()))),
            Some("https://x.example"),
        );
        assert_eq!(header(&foreign, "Access-Control-Allow-Origin"), None);
    }

    #[test]
    fn errors_map_to_status_codes() {
        let status = |e| response_for(Err(e), None).status();
        assert_eq!(response_for(Ok(None), None).status(), StatusCode::NOT_FOUND);
        assert_eq!(
            status(AppError::NotFound("x".into())),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(AppError::Auth("Database is locked".into())),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(AppError::Internal("attachments.storeUnavailable".into())),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            status(AppError::Database("boom".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn load_returns_the_bytes_and_type_of_a_row() {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        files_schema::migrate(&conn).expect("files schema");
        conn.execute_batch(
            "CREATE TABLE insurance_documents (id TEXT PRIMARY KEY, file_path TEXT, blob_id TEXT,
                 mime_type TEXT, original_name TEXT);",
        )
        .expect("schema");
        let blob = attachment_store::put_bytes(&conn, b"%PDF-1.7").expect("put");
        conn.execute(
            "INSERT INTO insurance_documents (id, blob_id, mime_type) VALUES ('d-1', ?1, 'application/pdf')",
            [&blob.id],
        )
        .expect("row");
        let dir = tempfile::tempdir().expect("tempdir");
        let route = parse_route("/insurance-document/d-1").expect("route");
        assert_eq!(
            load(&conn, dir.path(), &route).expect("load"),
            Some((b"%PDF-1.7".to_vec(), "application/pdf".to_string()))
        );
        let missing = parse_route("/insurance-document/d-2").expect("route");
        assert_eq!(load(&conn, dir.path(), &missing).expect("load"), None);
    }
}
