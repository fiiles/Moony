//! Loan events: extra payments, rate changes and manual balance records that
//! explain why a loan's balance moved (redesign phase 2, loan detail time trace).
//! `balance_check` is the stored kind of a manual balance record: it holds
//! exactly from its day until the user changes or removes it.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

pub const LOAN_EVENT_KINDS: [&str; 3] = ["extra_payment", "rate_change", "balance_check"];

/// One event in a loan's life. Money and rates are TEXT like everywhere else.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LoanEvent {
    pub id: String,
    #[serde(rename = "loanId")]
    pub loan_id: String,
    /// `extra_payment` | `rate_change` | `balance_check`.
    pub kind: String,
    /// UTC day of the event (ADR 0008).
    #[serde(rename = "eventDay")]
    pub event_day: i64,
    /// Extra payment amount, or the manual balance of a `balance_check` record.
    pub amount: Option<String>,
    /// New annual rate in percent (`rate_change`).
    pub rate: Option<String>,
    /// New regular payment (`rate_change`, or an `extra_payment` the bank used to lower it).
    #[serde(rename = "monthlyPayment")]
    pub monthly_payment: Option<String>,
    /// Annual rate in force before a `rate_change`; `None` on rows written before
    /// migration 019 and on other kinds.
    #[serde(rename = "previousRate")]
    pub previous_rate: Option<String>,
    /// Regular payment in force before an event that sets a new one (`rate_change`,
    /// `extra_payment` with a new payment); `None` on legacy rows and other kinds.
    #[serde(rename = "previousMonthlyPayment")]
    pub previous_monthly_payment: Option<String>,
    pub note: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for recording a loan event.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertLoanEvent {
    #[serde(rename = "loanId")]
    pub loan_id: String,
    pub kind: String,
    #[serde(rename = "eventDay")]
    pub event_day: i64,
    pub amount: Option<String>,
    pub rate: Option<String>,
    #[serde(rename = "monthlyPayment")]
    pub monthly_payment: Option<String>,
    pub note: Option<String>,
}

fn parse_positive(value: Option<&str>, allow_zero: bool) -> Result<f64> {
    let text = value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::Validation("validation.invalidAmount".into()))?;
    let number: f64 = text
        .parse()
        .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
    if !number.is_finite() || number < 0.0 || (!allow_zero && number == 0.0) {
        return Err(AppError::Validation("validation.invalidAmount".into()));
    }
    Ok(number)
}

impl InsertLoanEvent {
    /// Validate input data at the trust boundary.
    pub fn validate(&self) -> Result<()> {
        if self.loan_id.trim().is_empty() {
            return Err(AppError::Validation("validation.loanRequired".into()));
        }
        if !LOAN_EVENT_KINDS.contains(&self.kind.as_str()) {
            return Err(AppError::Validation(
                "validation.loanEventKindInvalid".into(),
            ));
        }
        if self.event_day <= 0 {
            return Err(AppError::Validation("validation.invalidDate".into()));
        }
        match self.kind.as_str() {
            "extra_payment" => {
                parse_positive(self.amount.as_deref(), false)?;
                // The bank may have used the payment to lower the regular one.
                if self
                    .monthly_payment
                    .as_deref()
                    .is_some_and(|p| !p.trim().is_empty())
                {
                    parse_positive(self.monthly_payment.as_deref(), false)?;
                }
            }
            "balance_check" => {
                parse_positive(self.amount.as_deref(), true)?;
            }
            _ => {
                let rate = self
                    .rate
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        AppError::Validation("validation.interestRatePositive".into())
                    })?;
                let rate: f64 = rate
                    .parse()
                    .map_err(|_| AppError::Validation("validation.interestRatePositive".into()))?;
                if !(0.0..=100.0).contains(&rate) {
                    return Err(AppError::Validation(
                        "validation.interestRatePositive".into(),
                    ));
                }
                if self
                    .monthly_payment
                    .as_deref()
                    .is_some_and(|p| !p.trim().is_empty())
                {
                    parse_positive(self.monthly_payment.as_deref(), true)?;
                }
            }
        }
        Ok(())
    }
}
