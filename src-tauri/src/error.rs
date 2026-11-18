//! Error handling for Moony Tauri application
//!
//! Unified error types that can be serialized for Tauri IPC

#![allow(dead_code)]

use serde::Serialize;
use thiserror::Error;

/// Main application error type
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("External API error: {0}")]
    ExternalApi(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// A `validation.*` key that carries interpolation values: the message is `key|{json}`. The
    /// frontend (`src/lib/api-error.ts`) splits it at the first `|`, hands the object to the
    /// translator and translates a value that is itself a key.
    pub fn validation_with(key: &str, params: serde_json::Value) -> Self {
        AppError::Validation(key.to_string() + "|" + &params.to_string())
    }

    /// Test helper: key and values of a keyed validation error.
    #[cfg(test)]
    pub(crate) fn keyed(&self) -> Option<(String, serde_json::Value)> {
        let AppError::Validation(detail) = self else {
            return None;
        };
        let (key, params) = detail.split_once('|').unwrap_or((detail.as_str(), "{}"));
        Some((key.to_string(), serde_json::from_str(params).ok()?))
    }
}

// Serialize for Tauri IPC - converts to string message
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// Convert from rusqlite errors. A write the sync guard refused (its authorizer's `SQLITE_AUTH`, or
// its `app_config` trigger's RAISE) reads as a paused change the person can act on.
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        if refused_by_sync_guard(&e) {
            return AppError::Validation(
                crate::services::sync::capture::guard::CHANGES_PAUSED.into(),
            );
        }
        AppError::Database(e.to_string())
    }
}

/// Only the sync guard sets an authorizer, so any `SQLITE_AUTH` is the guard's.
fn refused_by_sync_guard(e: &rusqlite::Error) -> bool {
    match e {
        rusqlite::Error::SqliteFailure(failure, message) => match failure.code {
            rusqlite::ErrorCode::AuthorizationForStatementDenied => true,
            rusqlite::ErrorCode::ConstraintViolation => {
                message.as_deref() == Some(crate::services::sync::capture::guard::GUARD_RAISE)
            }
            _ => false,
        },
        _ => false,
    }
}

// Convert from reqwest errors
impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::ExternalApi(e.to_string())
    }
}

// Convert from serde_json errors
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(format!("JSON error: {}", e))
    }
}

// Convert from uuid errors
impl From<uuid::Error> for AppError {
    fn from(e: uuid::Error) -> Self {
        AppError::Internal(format!("UUID error: {}", e))
    }
}

// Convert from IO errors
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Internal(format!("IO error: {}", e))
    }
}

/// Result type alias using AppError
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_keyed_validation_carries_its_values_as_json_after_a_bar() {
        let error = AppError::validation_with(
            "validation.currencyMismatch",
            json!({ "expected": "USD", "actual": "EUR" }),
        );
        assert!(error
            .to_string()
            .starts_with("Validation error: validation.currencyMismatch|{"));
        assert_eq!(
            error.keyed(),
            Some((
                "validation.currencyMismatch".to_string(),
                json!({ "expected": "USD", "actual": "EUR" })
            ))
        );
        assert_eq!(
            AppError::Validation("validation.required".into()).keyed(),
            Some(("validation.required".to_string(), json!({})))
        );
        assert_eq!(AppError::NotFound("x".into()).keyed(), None);
    }

    #[test]
    fn the_guard_reads_as_changes_paused() {
        use crate::services::sync::capture::guard::{self, CHANGES_PAUSED};
        let paused = |e: rusqlite::Error| matches!(AppError::from(e), AppError::Validation(key) if key == CHANGES_PAUSED);
        let mut conn = crate::services::sync::test_support::migrated();
        guard::impose(&mut conn).expect("impose");

        let denied = conn
            .execute(
                "INSERT INTO institutions (id, name) VALUES ('g1', 'Bank')",
                [],
            )
            .expect_err("the authorizer refuses");
        assert!(paused(denied));
        let raised = conn
            .execute(
                "INSERT INTO app_config (key, value) VALUES ('rule_packs_enabled', '[]')",
                [],
            )
            .expect_err("the trigger refuses");
        assert!(paused(raised));

        let other = conn
            .execute(
                "INSERT INTO _migrations (name) SELECT name FROM _migrations LIMIT 1",
                [],
            )
            .expect_err("a constraint of its own");
        assert!(matches!(AppError::from(other), AppError::Database(_)));
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// docs/standards/i18n.md section 5: a person sees `AppError::Validation` text, so it is a
    /// `validation.*` key, never an English sentence. `services/mcp/` is exempt: its errors answer
    /// an AI client. Only the code before the first `#[cfg(test)]` of a file is scanned.
    #[test]
    fn validation_errors_carry_keys_never_english_sentences() {
        let literal = regex::Regex::new(r#"Validation\(\s*(?:format!\(\s*)?"((?:[^"\\]|\\.)*)""#)
            .expect("regex");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        let mut offenders = Vec::new();
        for file in files {
            if file.components().any(|c| c.as_os_str() == "mcp") {
                continue;
            }
            let source = std::fs::read_to_string(&file).expect("read source");
            let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
            for caps in literal.captures_iter(production) {
                if !caps[1].starts_with("validation.") {
                    let relative = file.strip_prefix(&root).unwrap_or(&file);
                    offenders.push(format!("{}: {}", relative.display(), &caps[1]));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "replace these with a validation.* key (AppError::validation_with for values):\n{offenders:#?}"
        );
    }

    /// Ruling P13: every `validation.sync*` key the backend can emit has an English translation
    /// (`src/i18n/locales.test.ts` then holds the other nine locales to English).
    #[test]
    fn every_sync_validation_key_has_an_english_translation() {
        let key = regex::Regex::new(r"validation\.(sync[A-Za-z]+)").expect("regex");
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let this_file = manifest.join("src").join("error.rs");
        let mut files = Vec::new();
        rust_files(&manifest.join("src"), &mut files);
        let mut keys = std::collections::BTreeSet::new();
        for file in files.into_iter().filter(|f| *f != this_file) {
            let source = std::fs::read_to_string(&file).expect("read source");
            keys.extend(key.captures_iter(&source).map(|c| c[1].to_string()));
        }
        assert!(
            keys.contains("syncDeviceNameInvalid") && keys.contains("syncChangesetInvalid"),
            "the scan finds the sync keys: {keys:?}"
        );
        // Pairing protocol 2 (audit M1, M2, L8): the typed code and the acknowledgement.
        for pairing in [
            "syncPairingCodeIncomplete",
            "syncPairingCodeMismatch",
            "syncPairingCodeMismatchFinal",
            "syncPairingInterfered",
            "syncServerNeedsUpdate",
        ] {
            assert!(keys.contains(pairing), "the scan finds {pairing}");
        }

        let english: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(manifest.join("../src/i18n/locales/en/common.json"))
                .expect("read en/common.json"),
        )
        .expect("en/common.json is JSON");
        let missing: Vec<&String> = keys
            .iter()
            .filter(|k| !english["validation"][k.as_str()].is_string())
            .collect();
        assert!(
            missing.is_empty(),
            "add these to the validation block of every locale's common.json: {missing:?}"
        );
    }
}
