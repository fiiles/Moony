//! Cashflow item models for user-defined income/expense entries

use serde::{Deserialize, Serialize};
use specta::Type;

/// User-defined cashflow item (income or expense)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowItem {
    pub id: String,
    pub name: String,
    pub amount: String,
    pub currency: String,
    pub frequency: String, // "monthly" or "yearly"
    #[serde(rename = "itemType")]
    pub item_type: String, // "income" or "expense"
    pub category: String,  // Category key e.g. "savingsInterest", "rentalIncome", etc.
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// Data for creating/updating cashflow item
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertCashflowItem {
    pub name: String,
    pub amount: String,
    pub currency: Option<String>,
    pub frequency: String, // "monthly" or "yearly"
    #[serde(rename = "itemType")]
    pub item_type: String, // "income" or "expense"
    pub category: String,  // Category key
}
