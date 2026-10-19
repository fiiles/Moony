//! Services for Moony - business logic layer
//!
//! Services contain the core business logic and external API integrations

pub mod attachment_conversion;
pub mod attachment_protocol;
pub mod attachment_store;
pub mod attachments;
pub mod auth;
pub mod backfill;
pub mod backup;
pub mod bank_accounts;
pub mod biometric_unlock;
pub mod bonds;
pub mod budgeting;
pub mod cashflow_actuals;
pub mod categories;
pub mod categorization;
pub mod company_info;
pub mod cost_basis;
pub mod crypto;
pub mod crypto_investments;
pub mod csv_import;
pub mod csv_presets;
pub mod currency;
pub mod date_parser;
pub mod dated_facts;
pub mod dedup;
pub mod document_files;
pub mod financial_semantics;
pub mod fx;
pub mod fx_backfill;
pub mod history_days;
pub mod history_plan;
pub mod history_recalc;
pub mod http;
pub mod insurance;
pub mod interest_tiers;
pub mod investments;
pub mod loan_amortization;
pub mod loan_events;
pub mod loans;
pub mod local_api;
pub mod logging;
pub mod mcp;
pub mod mcp_refresh;
pub mod milestones;
pub mod net_worth;
pub mod onboarding;
pub mod other_assets;
pub mod parsing;
pub mod portfolio_history;
pub mod position_reports;
pub mod positions;
pub mod price_api;
pub mod pricing;
pub mod projection;
pub mod projection_math;
pub mod quote_unit;
pub mod real_estate;
pub mod real_estate_photos;
pub mod received_files;
pub mod session_key;
pub mod statement_import;
pub mod stock_import;
pub mod stock_monitor;
pub mod stock_performance;
pub mod stock_tags;
pub mod temp_cleanup;
pub mod ticker_history;
pub mod valuations;

pub mod reporting;

pub mod cashflow_planning;

pub mod bank_history;
