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
