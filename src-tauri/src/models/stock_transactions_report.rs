//! Wire types of the stock transactions report (realized sales, lots, ledger, open lots).
//! Amounts are decimal TEXT in the report currency unless a field says otherwise; days are unix
//! seconds at UTC midnight.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::fx::ConversionQuality;
use crate::error::{AppError, Result};
use crate::services::fx;

/// Longest holding-period test, in months.
pub const MAX_HOLDING_TEST_MONTHS: i64 = 600;

/// Longest period of a request: 100 years of 365.25 days, in seconds.
pub const MAX_PERIOD_SECONDS: i64 = 36_525 * 86_400;

/// How the cost of a sold share is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum StockReportCostMethod {
    #[serde(rename = "fifo")]
    Fifo,
    #[serde(rename = "average")]
    Average,
}

/// What a ledger row records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum StockReportLedgerKind {
    #[serde(rename = "buy")]
    Buy,
    #[serde(rename = "sell")]
    Sell,
    #[serde(rename = "dividend")]
    Dividend,
}

/// Request of the report: the period (both days inclusive), the currency, the cost method and
/// the optional holding-period test.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockTransactionsReportRequest {
    /// Inclusive, UTC midnight.
    #[serde(rename = "fromDay")]
    pub from_day: i64,
    /// Inclusive.
    #[serde(rename = "toDay")]
    pub to_day: i64,
    #[serde(rename = "reportCurrency")]
    pub report_currency: String,
    #[serde(rename = "costMethod")]
    pub cost_method: StockReportCostMethod,
    /// `None` = test off, otherwise 1..=600.
    #[serde(rename = "holdingTestMonths")]
    pub holding_test_months: Option<i64>,
}

impl StockTransactionsReportRequest {
    /// Validate input data at the trust boundary.
    pub fn validate(&self) -> Result<()> {
        if self.from_day > self.to_day
            || self.to_day.saturating_sub(self.from_day) > MAX_PERIOD_SECONDS
        {
            return Err(AppError::Validation("validation.periodInvalid".into()));
        }
        if let Some(months) = self.holding_test_months {
            if !(1..=MAX_HOLDING_TEST_MONTHS).contains(&months) {
                return Err(AppError::Validation(
                    "validation.holdingTestMonthsInvalid".into(),
                ));
            }
        }
        match fx::normalize(&self.report_currency) {
            Some(code) if fx::is_supported(&code) => Ok(()),
            _ => Err(AppError::Validation(
                "validation.currencyUnsupported".into(),
            )),
        }
    }
}

/// Proceeds, cost and result of a set of sold shares in the report currency.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportAmounts {
    #[serde(rename = "proceeds")]
    pub proceeds: String,
    #[serde(rename = "cost")]
    pub cost: String,
    /// `proceeds - cost`.
    #[serde(rename = "result")]
    pub result: String,
    /// Buy and sale fees of the sold pieces.
    #[serde(rename = "fees")]
    pub fees: String,
    #[serde(rename = "resultAfterFees")]
    pub result_after_fees: String,
}

/// Totals of the counted sales, split by the holding-period test when it is on.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportSummary {
    #[serde(rename = "total")]
    pub total: StockReportAmounts,
    /// `null` when the test is off.
    #[serde(rename = "taxable")]
    pub taxable: Option<StockReportAmounts>,
    #[serde(rename = "exempt")]
    pub exempt: Option<StockReportAmounts>,
    /// Counted sales.
    #[serde(rename = "salesCount")]
    pub sales_count: u32,
    #[serde(rename = "salesInProfit")]
    pub sales_in_profit: u32,
    #[serde(rename = "salesInLoss")]
    pub sales_in_loss: u32,
}

/// Activity figures of the period.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportStats {
    /// Every sale of the period, uncovered parts included.
    #[serde(rename = "proceeds")]
    pub proceeds: String,
    #[serde(rename = "salesCount")]
    pub sales_count: u32,
    #[serde(rename = "soldTickers")]
    pub sold_tickers: u32,
    #[serde(rename = "bought")]
    pub bought: String,
    #[serde(rename = "buysCount")]
    pub buys_count: u32,
    #[serde(rename = "boughtTickers")]
    pub bought_tickers: u32,
    /// Fees of every trade in the period.
    #[serde(rename = "feesPaid")]
    pub fees_paid: String,
    /// Bought + proceeds.
    #[serde(rename = "tradedVolume")]
    pub traded_volume: String,
    #[serde(rename = "dividends")]
    pub dividends: String,
    #[serde(rename = "dividendsCount")]
    pub dividends_count: u32,
}

/// One time bucket of the activity chart.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportBucket {
    /// `YYYY-MM`, or `YYYY` when the period spans more than 24 months.
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "fromDay")]
    pub from_day: i64,
    #[serde(rename = "toDay")]
    pub to_day: i64,
    #[serde(rename = "buys")]
    pub buys: String,
    #[serde(rename = "sells")]
    pub sells: String,
    /// Result after fees of the counted sales.
    #[serde(rename = "result")]
    pub result: String,
}

/// Result of one position over the period.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportTickerResult {
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    #[serde(rename = "ticker")]
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    /// Sold and counted.
    #[serde(rename = "quantity")]
    pub quantity: String,
    #[serde(rename = "resultAfterFees")]
    pub result_after_fees: String,
    /// `null` when the test is off.
    #[serde(rename = "taxableResult")]
    pub taxable_result: Option<String>,
    #[serde(rename = "exemptResult")]
    pub exempt_result: Option<String>,
}

/// The part of a sale that comes from one buy lot.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportMatch {
    #[serde(rename = "buyDay")]
    pub buy_day: i64,
    #[serde(rename = "quantity")]
    pub quantity: String,
    /// Trade currency.
    #[serde(rename = "buyPrice")]
    pub buy_price: String,
    #[serde(rename = "buyCurrency")]
    pub buy_currency: String,
    /// Report currency per 1 unit of `buyCurrency` at `buyDay`.
    #[serde(rename = "buyRate")]
    pub buy_rate: Option<String>,
    /// Per share, report currency; only with cost method `average`.
    #[serde(rename = "averageCost")]
    pub average_cost: Option<String>,
    #[serde(rename = "proceeds")]
    pub proceeds: Option<String>,
    #[serde(rename = "cost")]
    pub cost: Option<String>,
    #[serde(rename = "fees")]
    pub fees: Option<String>,
    #[serde(rename = "resultAfterFees")]
    pub result_after_fees: Option<String>,
    /// `null` when the test is off.
    #[serde(rename = "exempt")]
    pub exempt: Option<bool>,
}

/// One sale with the lots it consumed.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportSale {
    /// Transaction id.
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    #[serde(rename = "ticker")]
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    #[serde(rename = "day")]
    pub day: i64,
    /// Whole sale.
    #[serde(rename = "quantity")]
    pub quantity: String,
    /// Trade currency.
    #[serde(rename = "price")]
    pub price: String,
    #[serde(rename = "currency")]
    pub currency: String,
    /// At the sale day.
    #[serde(rename = "rate")]
    pub rate: Option<String>,
    /// `0` when every share has a lot.
    #[serde(rename = "uncoveredQuantity")]
    pub uncovered_quantity: String,
    /// `false` when an amount could not be converted.
    #[serde(rename = "counted")]
    pub counted: bool,
    #[serde(rename = "matches")]
    pub matches: Vec<StockReportMatch>,
}

/// One row of the chronological ledger.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportLedgerRow {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "kind")]
    pub kind: StockReportLedgerKind,
    /// Ex-date for a dividend.
    #[serde(rename = "day")]
    pub day: i64,
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    #[serde(rename = "ticker")]
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    /// Dividend: shares held, `null` for a manual one.
    #[serde(rename = "quantity")]
    pub quantity: Option<String>,
    /// Per share, trade currency; `null` for a manual dividend.
    #[serde(rename = "price")]
    pub price: Option<String>,
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "rate")]
    pub rate: Option<String>,
    /// Report currency.
    #[serde(rename = "amount")]
    pub amount: Option<String>,
    /// Trade currency, `null` when none.
    #[serde(rename = "fee")]
    pub fee: Option<String>,
}

/// A buy lot that is still (partly) open.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockReportOpenLot {
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    #[serde(rename = "ticker")]
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    #[serde(rename = "buyDay")]
    pub buy_day: i64,
    /// Still open.
    #[serde(rename = "quantity")]
    pub quantity: String,
    /// As bought.
    #[serde(rename = "lotQuantity")]
    pub lot_quantity: String,
    #[serde(rename = "buyPrice")]
    pub buy_price: String,
    #[serde(rename = "buyCurrency")]
    pub buy_currency: String,
    /// First day a sale is exempt; `null` when the test is off.
    #[serde(rename = "passDay")]
    pub pass_day: Option<i64>,
    /// Today.
    #[serde(rename = "value")]
    pub value: Option<String>,
    #[serde(rename = "unrealized")]
    pub unrealized: Option<String>,
}

/// The report: summary, activity, per-ticker results, sales with lots, ledger and open lots.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockTransactionsReport {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    /// The UTC day the service used.
    #[serde(rename = "today")]
    pub today: i64,
    #[serde(rename = "firstTransactionDay")]
    pub first_transaction_day: Option<i64>,
    #[serde(rename = "summary")]
    pub summary: StockReportSummary,
    #[serde(rename = "stats")]
    pub stats: StockReportStats,
    #[serde(rename = "buckets")]
    pub buckets: Vec<StockReportBucket>,
    /// Sorted by `resultAfterFees`, largest first.
    #[serde(rename = "tickers")]
    pub tickers: Vec<StockReportTickerResult>,
    /// Newest first.
    #[serde(rename = "sales")]
    pub sales: Vec<StockReportSale>,
    /// Newest first.
    #[serde(rename = "ledger")]
    pub ledger: Vec<StockReportLedgerRow>,
    /// Oldest buy first.
    #[serde(rename = "openLots")]
    pub open_lots: Vec<StockReportOpenLot>,
    #[serde(rename = "uncoveredTickers")]
    pub uncovered_tickers: Vec<String>,
}

/// Remembered choices of the report page.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockTransactionsReportSettings {
    #[serde(rename = "costMethod")]
    pub cost_method: StockReportCostMethod,
    #[serde(rename = "holdingTestEnabled")]
    pub holding_test_enabled: bool,
    #[serde(rename = "holdingTestMonths")]
    pub holding_test_months: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        from_day: i64,
        to_day: i64,
        months: Option<i64>,
        currency: &str,
    ) -> StockTransactionsReportRequest {
        StockTransactionsReportRequest {
            from_day,
            to_day,
            report_currency: currency.into(),
            cost_method: StockReportCostMethod::Fifo,
            holding_test_months: months,
        }
    }

    fn message(result: Result<()>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_period_that_ends_before_it_starts_is_refused() {
        assert!(request(86_400, 86_400, None, "CZK").validate().is_ok());
        assert_eq!(
            message(request(86_400, 0, None, "CZK").validate()),
            "validation.periodInvalid"
        );
    }

    #[test]
    fn a_period_longer_than_a_hundred_years_is_refused() {
        assert!(request(0, MAX_PERIOD_SECONDS, None, "CZK")
            .validate()
            .is_ok());
        assert_eq!(
            message(request(0, MAX_PERIOD_SECONDS + 86_400, None, "CZK").validate()),
            "validation.periodInvalid"
        );
        // Year 0001 to 2026, as a typed custom range could send.
        assert_eq!(
            message(request(-62_135_596_800, 1_767_225_600, None, "CZK").validate()),
            "validation.periodInvalid"
        );
    }

    #[test]
    fn the_holding_test_length_must_be_one_to_six_hundred_months() {
        for months in [None, Some(1), Some(36), Some(600)] {
            assert!(
                request(0, 0, months, "CZK").validate().is_ok(),
                "{months:?}"
            );
        }
        for months in [Some(0), Some(601), Some(-3)] {
            assert_eq!(
                message(request(0, 0, months, "CZK").validate()),
                "validation.holdingTestMonthsInvalid"
            );
        }
    }

    #[test]
    fn an_unsupported_report_currency_is_refused() {
        assert_eq!(
            message(request(0, 0, None, "KWD").validate()),
            "validation.currencyUnsupported"
        );
        assert_eq!(
            message(request(0, 0, None, "").validate()),
            "validation.currencyUnsupported"
        );
    }
}
