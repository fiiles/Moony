//! Tauri commands - IPC layer between frontend and Rust backend
//!
//! All commands are registered in lib.rs and called via invoke() from frontend

pub mod attachments;
pub mod auth;
pub mod backup;
pub mod bank_accounts;
pub mod biometric;
pub mod bonds;
pub mod budgeting;
pub mod cashflow;
pub mod categories;
pub mod categorization;
pub mod company_info;
pub mod crypto;
/// Phones only: the keychain plugin is a mobile dependency.
#[cfg(mobile)]
pub(crate) mod device_backup;
pub mod export;
pub mod insurance;
pub mod investments;
pub mod loans;
pub(crate) mod mcp_refresh;
pub mod milestones;
pub mod onboarding;
pub mod other_assets;
pub mod portfolio;
pub mod price_api;
pub mod projection;
pub mod real_estate;
/// Phones only: `RunEvent::Opened` exists on iOS and Android, and the Inbox folders on iOS.
#[cfg(mobile)]
pub mod received_files;
pub mod statement_import;
pub mod stock_import;
pub mod stock_monitor;
pub mod stock_splits;
pub mod stock_tags;
pub mod system;
