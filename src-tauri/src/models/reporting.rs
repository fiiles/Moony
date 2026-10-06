//! Shared, booked-only report contracts. Monetary values are CZK TEXT.
use crate::models::FlowKind;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum FlowSource {
    #[serde(rename = "explicit")]
    Explicit,
    #[serde(rename = "ownAccount")]
    OwnAccount,
    #[serde(rename = "category")]
    Category,
    #[serde(rename = "direction")]
    Direction,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum FxQuality {
    #[serde(rename = "estimated")]
    Estimated,
    #[serde(rename = "historical")]
    Historical,
    #[serde(rename = "missing")]
    Missing,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct ReportQuality {
    #[serde(rename = "missingFxCount")]
    pub missing_fx_count: i32,
    #[serde(rename = "missingFxCurrencies")]
    pub missing_fx_currencies: Vec<String>,
    #[serde(rename = "estimatedFxCount")]
    pub estimated_fx_count: i32,
    #[serde(rename = "observedCompletedMonths")]
    pub observed_completed_months: Vec<i64>,
    #[serde(rename = "partialMonthCount")]
    pub partial_month_count: i32,
    #[serde(rename = "unobservedMonthCount")]
    pub unobserved_month_count: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ReportTransaction {
    pub id: String,
    #[serde(rename = "bookingDate")]
    pub booking_date: i64,
    pub amount: String,
    pub currency: String,
    pub description: Option<String>,
    #[serde(rename = "counterpartyName")]
    pub counterparty_name: Option<String>,
    #[serde(rename = "counterpartyIban")]
    pub counterparty_iban: Option<String>,
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: String,
    #[serde(rename = "bankAccountName")]
    pub bank_account_name: String,
    #[serde(rename = "txType")]
    pub tx_type: String,
    #[serde(rename = "effectiveFlowKind")]
    pub effective_flow_kind: FlowKind,
    #[serde(rename = "flowSource")]
    pub flow_source: FlowSource,
    #[serde(rename = "signedAmountCzk")]
    pub signed_amount_czk: Option<String>,
    #[serde(rename = "fxQuality")]
    pub fx_quality: FxQuality,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EffectiveTransactionFlow {
    pub id: String,
    #[serde(rename = "effectiveFlowKind")]
    pub effective_flow_kind: FlowKind,
    #[serde(rename = "flowSource")]
    pub flow_source: FlowSource,
}
