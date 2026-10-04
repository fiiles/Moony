//! Loan models

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Loan
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Loan {
    pub id: String,
    pub name: String,
    pub principal: String,
    pub currency: String,
    #[serde(rename = "interestRate")]
    pub interest_rate: String,
    #[serde(rename = "interestRateValidityDate")]
    pub interest_rate_validity_date: Option<i64>,
    #[serde(rename = "monthlyPayment")]
    pub monthly_payment: String,
    #[serde(rename = "startDate")]
    pub start_date: i64,
    #[serde(rename = "endDate")]
    pub end_date: Option<i64>,
    /// Real outstanding balance entered by the user (e.g. from a statement);
    /// set together with `balance_anchor_date`, amortization continues from it.
    #[serde(rename = "balanceAnchorAmount")]
    pub balance_anchor_amount: Option<String>,
    /// UTC day the manual balance applies to.
    #[serde(rename = "balanceAnchorDate")]
    pub balance_anchor_date: Option<i64>,
    /// Read-only: amortized balance as of today's UTC day, in the loan's own
    /// currency with 2 decimals (computed on every read, never stored).
    #[serde(rename = "outstandingBalance")]
    pub outstanding_balance: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// One payment of a loan's repayment schedule (money as TEXT, 2 decimals).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LoanScheduleRow {
    /// UTC day the payment falls due.
    #[serde(rename = "dueDay")]
    pub due_day: i64,
    pub payment: String,
    pub interest: String,
    /// Negative when the payment does not cover the interest.
    #[serde(rename = "principalPart")]
    pub principal_part: String,
    #[serde(rename = "balanceAfter")]
    pub balance_after: String,
}

/// Repayment schedule and totals of one loan as of today (loan detail page).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LoanSchedule {
    #[serde(rename = "loanId")]
    pub loan_id: String,
    /// Valuation day (today, UTC).
    #[serde(rename = "asOfDay")]
    pub as_of_day: i64,
    /// Amount and day the amortization starts from.
    #[serde(rename = "anchorAmount")]
    pub anchor_amount: String,
    #[serde(rename = "anchorDay")]
    pub anchor_day: i64,
    /// True when the anchor is the user's manual balance, false for
    /// principal at the start date.
    #[serde(rename = "anchorIsManual")]
    pub anchor_is_manual: bool,
    #[serde(rename = "outstandingBalance")]
    pub outstanding_balance: String,
    /// Anchor amount minus the balance as of `asOfDay` (negative when it grew).
    #[serde(rename = "principalRepaid")]
    pub principal_repaid: String,
    /// Interest included in the payments between the anchor and `asOfDay`.
    #[serde(rename = "interestPaid")]
    pub interest_paid: String,
    #[serde(rename = "paymentsMade")]
    pub payments_made: u32,
    /// Payments left until the balance is 0; None when the schedule does not
    /// reach 0 (no payment, payment below interest, ends at `end_date`).
    #[serde(rename = "paymentsRemaining")]
    pub payments_remaining: Option<u32>,
    #[serde(rename = "payoffDay")]
    pub payoff_day: Option<i64>,
    pub rows: Vec<LoanScheduleRow>,
    /// Codes the UI translates: `noPayment`, `paymentBelowInterest`,
    /// `balanceAtEndDate`, `scheduleTruncated`.
    pub warnings: Vec<String>,
}

/// Data for creating/updating loan
#[derive(Debug, Clone, Deserialize, Type, rmcp::schemars::JsonSchema)]
pub struct InsertLoan {
    pub name: String,
    pub principal: String,
    pub currency: Option<String>,
    #[serde(rename = "interestRate")]
    pub interest_rate: Option<String>,
    #[serde(rename = "interestRateValidityDate")]
    pub interest_rate_validity_date: Option<i64>,
    #[serde(rename = "monthlyPayment")]
    pub monthly_payment: Option<String>,
    #[serde(rename = "startDate")]
    pub start_date: Option<i64>,
    #[serde(rename = "endDate")]
    pub end_date: Option<i64>,
    /// Manual outstanding balance; must come with `balance_anchor_date`.
    /// Both absent means "amortize from the principal at the start date".
    #[serde(rename = "balanceAnchorAmount")]
    pub balance_anchor_amount: Option<String>,
    #[serde(rename = "balanceAnchorDate")]
    pub balance_anchor_date: Option<i64>,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertLoan {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Name validation
        if self.name.is_empty() {
            return Err(AppError::Validation("validation.loanNameRequired".into()));
        }
        if self.name.len() > 100 {
            return Err(AppError::Validation("validation.loanNameRequired".into()));
        }

        // Principal validation
        if self.principal.is_empty() {
            return Err(AppError::Validation("validation.principalPositive".into()));
        }
        let principal: f64 = self
            .principal
            .parse()
            .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
        if !principal.is_finite() || principal <= 0.0 {
            return Err(AppError::Validation("validation.principalPositive".into()));
        }

        // Currency validation (if provided)
        if let Some(ref currency) = self.currency {
            if currency.len() != 3 {
                return Err(AppError::Validation("validation.currencyInvalid".into()));
            }
        }

        // Interest rate validation (if provided)
        if let Some(ref rate) = self.interest_rate {
            if !rate.is_empty() {
                let rate_val: f64 = rate
                    .parse()
                    .map_err(|_| AppError::Validation("validation.interestRatePositive".into()))?;
                if !(0.0..=100.0).contains(&rate_val) {
                    return Err(AppError::Validation(
                        "validation.interestRatePositive".into(),
                    ));
                }
            }
        }

        // Monthly payment validation (if provided)
        if let Some(ref payment) = self.monthly_payment {
            if !payment.is_empty() {
                let payment_val: f64 = payment
                    .parse()
                    .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
                if !payment_val.is_finite() || payment_val < 0.0 {
                    return Err(AppError::Validation(
                        "validation.monthlyPaymentPositive".into(),
                    ));
                }
            }
        }

        // Date validation
        if let (Some(start), Some(end)) = (self.start_date, self.end_date) {
            if end <= start {
                return Err(AppError::Validation("validation.endDateAfterStart".into()));
            }
        }

        // Manual balance: amount and day go together
        let anchor_amount = self.anchor_amount_text();
        if anchor_amount.is_some() != self.balance_anchor_date.is_some() {
            return Err(AppError::Validation(
                "validation.balanceAnchorIncomplete".into(),
            ));
        }
        if let Some(amount) = anchor_amount {
            let value: f64 = amount
                .parse()
                .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
            if !value.is_finite() {
                return Err(AppError::Validation("validation.invalidAmount".into()));
            }
            if value < 0.0 {
                return Err(AppError::Validation(
                    "validation.balanceAnchorNegative".into(),
                ));
            }
        }
        if let (Some(start), Some(anchor)) = (self.start_date, self.balance_anchor_date) {
            if anchor_before_start(anchor, start) {
                return Err(AppError::Validation(
                    "validation.balanceAnchorBeforeStart".into(),
                ));
            }
        }

        Ok(())
    }

    /// Trimmed manual balance, `None` when absent or blank.
    fn anchor_amount_text(&self) -> Option<&str> {
        self.balance_anchor_amount
            .as_deref()
            .map(str::trim)
            .filter(|amount| !amount.is_empty())
    }

    /// The manual (amount, day) pair to store, `None` when the loan amortizes
    /// from its principal. Only meaningful after `validate()` accepted both
    /// halves together.
    pub fn balance_anchor(&self) -> Option<(String, i64)> {
        match (self.anchor_amount_text(), self.balance_anchor_date) {
            (Some(amount), Some(day)) => Some((amount.to_string(), day)),
            _ => None,
        }
    }
}

/// A manual balance dated before the loan starts is meaningless.
pub fn anchor_before_start(anchor_date: i64, start_date: i64) -> bool {
    anchor_date.div_euclid(86_400) < start_date.div_euclid(86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> InsertLoan {
        InsertLoan {
            name: "Mortgage".into(),
            principal: "1000000".into(),
            currency: None,
            interest_rate: Some("4.5".into()),
            interest_rate_validity_date: None,
            monthly_payment: Some("5000".into()),
            start_date: Some(1_577_836_800), // 2020-01-01
            end_date: None,
            balance_anchor_amount: None,
            balance_anchor_date: None,
        }
    }

    fn validation_key(data: &InsertLoan) -> String {
        match data.validate() {
            Err(AppError::Validation(key)) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_loan_without_a_manual_balance_is_valid() {
        assert!(valid().validate().is_ok());
        assert!(valid().balance_anchor().is_none());
    }

    #[test]
    fn a_complete_manual_balance_is_valid_and_zero_means_paid_off() {
        let mut data = valid();
        data.balance_anchor_amount = Some(" 850000.50 ".into());
        data.balance_anchor_date = Some(1_735_689_600); // 2025-01-01
        assert!(data.validate().is_ok());
        assert_eq!(
            data.balance_anchor(),
            Some(("850000.50".to_string(), 1_735_689_600))
        );
        data.balance_anchor_amount = Some("0".into());
        assert!(data.validate().is_ok());
    }

    #[test]
    fn the_manual_balance_needs_both_the_amount_and_the_day() {
        let mut data = valid();
        data.balance_anchor_amount = Some("850000".into());
        assert_eq!(validation_key(&data), "validation.balanceAnchorIncomplete");

        let mut data = valid();
        data.balance_anchor_date = Some(1_735_689_600);
        assert_eq!(validation_key(&data), "validation.balanceAnchorIncomplete");

        // a blank amount counts as absent
        data.balance_anchor_amount = Some("  ".into());
        assert_eq!(validation_key(&data), "validation.balanceAnchorIncomplete");
    }

    #[test]
    fn the_manual_balance_must_be_a_non_negative_finite_number() {
        for (amount, key) in [
            ("-1", "validation.balanceAnchorNegative"),
            ("abc", "validation.invalidAmount"),
            ("NaN", "validation.invalidAmount"),
            ("inf", "validation.invalidAmount"),
        ] {
            let mut data = valid();
            data.balance_anchor_amount = Some(amount.into());
            data.balance_anchor_date = Some(1_735_689_600);
            assert_eq!(validation_key(&data), key, "amount {amount}");
        }
    }

    #[test]
    fn the_manual_balance_cannot_predate_the_start() {
        let mut data = valid();
        data.balance_anchor_amount = Some("900000".into());
        data.balance_anchor_date = Some(1_577_836_800 - 86_400);
        assert_eq!(validation_key(&data), "validation.balanceAnchorBeforeStart");
        // the start day itself is fine, even at a different time of day
        data.balance_anchor_date = Some(1_577_836_800 + 3_600);
        assert!(data.validate().is_ok());
    }

    #[test]
    fn non_finite_principal_and_payment_are_rejected() {
        let mut data = valid();
        data.principal = "NaN".into();
        assert_eq!(validation_key(&data), "validation.principalPositive");
        let mut data = valid();
        data.monthly_payment = Some("inf".into());
        assert_eq!(validation_key(&data), "validation.monthlyPaymentPositive");
        let mut data = valid();
        data.interest_rate = Some("NaN".into());
        assert_eq!(validation_key(&data), "validation.interestRatePositive");
    }
}
