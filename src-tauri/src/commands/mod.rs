//! Tauri commands - IPC layer between frontend and Rust backend
//!
//! All commands are registered in lib.rs and called via invoke() from frontend

pub mod auth;
pub mod backup;
pub mod bank_accounts;
pub mod bonds;
pub mod budgeting;
pub mod cashflow;
pub mod categories;
pub mod categorization;
pub mod company_info;
pub mod crypto;
pub mod export;
pub mod insurance;
pub mod investments;
pub mod loans;
pub mod onboarding;
pub mod other_assets;
pub mod portfolio;
pub mod price_api;
pub mod projection;
pub mod real_estate;
pub mod stock_import;
pub mod stock_monitor;
pub mod stock_tags;
pub mod system;
