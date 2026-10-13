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

// Convert from rusqlite errors
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Database(e.to_string())
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
}
