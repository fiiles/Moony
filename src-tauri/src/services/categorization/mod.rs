//! Smart Categorization Engine
//!
//! This module provides deterministic transaction categorization using a waterfall approach:
//! 1. Custom Rule Engine - user-defined rules
//! 2. Exact Match - HashMap lookup for learned payees
//! 3. Own-account IBAN - internal transfer detection
//! 4. Default Rule Engine - locale rule packs

pub mod apply;
pub mod engine;
pub mod exact_match;
pub mod hits;
pub mod learn;
pub mod own_accounts;
pub mod packs;
pub mod rules;
pub mod tokenizer;
pub mod types;

// Re-export main types and engine
pub use engine::CategorizationEngine;
pub use types::{
    CategorizationResult, CategorizationRule, CategorizationSource, RuleType, TransactionInput,
};
