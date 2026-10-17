//! Models for Moony - Rust structs matching database schema
//!
//! All monetary values are stored as strings to preserve decimal precision

#![allow(dead_code)]

pub mod attachments;
pub mod backup;
pub mod bank_accounts;
pub mod bank_transactions;
pub mod biometric;
pub mod bonds;
pub mod budgeting;
pub mod cashflow;
pub mod cashflow_actuals;
pub mod categorization;
pub mod company_info;
pub mod crypto;
pub mod currency;
pub mod frequency;
pub mod fx;
pub mod insurance;
pub mod investments;
pub mod loan_events;
pub mod loans;
pub mod milestones;
pub mod onboarding;
pub mod other_assets;
pub mod portfolio;
pub mod projection;
pub mod real_estate;
pub mod reporting;
pub mod stock_monitor;
pub mod stock_performance;
pub mod stock_tags;
pub mod user;
pub mod validation;
pub mod valuations;

// Re-export commonly used types
pub use attachments::*;
pub use backup::*;
pub use bank_accounts::*;
pub use bank_transactions::*;
pub use biometric::*;
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
pub use milestones::*;
pub use onboarding::*;
pub use other_assets::*;
pub use portfolio::*;
pub use projection::*;
pub use real_estate::*;
pub use reporting::*;
pub use stock_monitor::*;
pub use stock_performance::*;
pub use stock_tags::*;
pub use user::*;
pub use valuations::*;
