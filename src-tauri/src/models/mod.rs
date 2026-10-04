//! Models for Moony - Rust structs matching database schema
//!
//! All monetary values are stored as strings to preserve decimal precision

#![allow(dead_code)]

pub mod backup;
pub mod bank_accounts;
pub mod bank_transactions;
pub mod bonds;
pub mod budgeting;
pub mod cashflow;
pub mod cashflow_actuals;
pub mod company_info;
pub mod crypto;
pub mod currency;
pub mod frequency;
pub mod insurance;
pub mod investments;
pub mod loan_events;
pub mod loans;
pub mod onboarding;
pub mod other_assets;
pub mod projection;
pub mod real_estate;
pub mod stock_monitor;
pub mod stock_tags;
pub mod user;
pub mod valuations;

// Re-export commonly used types
pub use backup::*;
pub use bank_accounts::*;
pub use bank_transactions::*;
pub use bonds::*;
pub use budgeting::*;
pub use cashflow::*;
pub use cashflow_actuals::*;
pub use crypto::*;
pub use frequency::*;
pub use insurance::*;
pub use investments::*;
pub use loan_events::*;
pub use loans::*;
pub use onboarding::*;
pub use other_assets::*;
pub use projection::*;
pub use real_estate::*;
pub use stock_monitor::*;
pub use stock_tags::*;
pub use user::*;
pub use valuations::*;
