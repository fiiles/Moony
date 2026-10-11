//! Wire types of the stocks analysis performance report (`get_stock_performance`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::fx::ConversionQuality;
use crate::error::{AppError, Result};

/// Most series one request may ask for.
const MAX_SERIES: usize = 64;

/// One line of the performance chart: a set of held positions.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PerformanceSeriesSpec {
    /// Caller-chosen identifier, echoed in the response; unique within a request.
    pub key: String,
    /// The positions of the series. Ids of positions that are not held are ignored.
    #[serde(rename = "investmentIds")]
    pub investment_ids: Vec<String>,
    /// Compute this series in another currency (the native return of a currency group); it then
    /// has no contribution.
    #[serde(rename = "currency")]
    pub currency: Option<String>,
}

/// Request of the performance report: the period, the currency and the series to chart.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockPerformanceRequest {
    /// First day (unix seconds, UTC midnight); clamped to the first recorded day.
    #[serde(rename = "fromTs")]
    pub from_ts: i64,
    /// Last day (unix seconds, UTC midnight); clamped to the last recorded day.
    #[serde(rename = "toTs")]
    pub to_ts: i64,
    /// The currency values, flows and returns are converted to, each at its own day.
    #[serde(rename = "reportCurrency")]
    pub report_currency: String,
    #[serde(rename = "series")]
    pub series: Vec<PerformanceSeriesSpec>,
}

impl StockPerformanceRequest {
    /// Validate input data at the trust boundary.
    pub fn validate(&self) -> Result<()> {
        if self.from_ts >= self.to_ts {
            return Err(AppError::Validation("validation.periodInvalid".into()));
        }
        if self.series.len() > MAX_SERIES {
            return Err(AppError::Validation("validation.tooManySeries".into()));
        }
        let mut keys = HashSet::new();
        for spec in &self.series {
            if spec.key.trim().is_empty() || !keys.insert(spec.key.as_str()) {
                return Err(AppError::Validation("validation.seriesKeyInvalid".into()));
            }
        }
        Ok(())
    }
}

/// The chart data of one requested series, on the dates of the report.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PerformanceSeries {
    pub key: String,
    /// Cumulative time-weighted return of the series' own positions in percent per date
    /// (first 0).
    #[serde(rename = "twr")]
    pub twr: Vec<f64>,
    /// Percentage points of the whole-portfolio return the series brought, cumulative per date;
    /// `null` for a series computed in another currency.
    #[serde(rename = "contribution")]
    pub contribution: Option<Vec<f64>>,
}

/// One held position over the whole period.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PositionPerformance {
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    /// Time-weighted return of the position over the period, in percent.
    #[serde(rename = "twr")]
    pub twr: f64,
    /// Percentage points of the whole-portfolio return the position brought.
    #[serde(rename = "contribution")]
    pub contribution: f64,
    /// Value on the first date, decimal TEXT; `null` when it cannot be converted.
    #[serde(rename = "startValue")]
    pub start_value: Option<String>,
    /// Value on the last date, decimal TEXT; `null` when it cannot be converted.
    #[serde(rename = "endValue")]
    pub end_value: Option<String>,
}

/// Performance of the held positions in `currency`: the whole portfolio, the requested series
/// and every position, all on the same `dates`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockPerformance {
    /// The report currency.
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    /// Emitted days as `YYYY-MM-DD`, at most 400 plus the last day (every n-th day on long
    /// ranges).
    #[serde(rename = "dates")]
    pub dates: Vec<String>,
    /// Cumulative time-weighted return of the whole portfolio in percent per date (first 0).
    #[serde(rename = "portfolio")]
    pub portfolio: Vec<f64>,
    /// Portfolio value per date (decimal TEXT); `null` when no held position converts that day.
    #[serde(rename = "values")]
    pub values: Vec<Option<String>>,
    /// Net deposits (buys minus sells) cumulated since the first date, decimal TEXT.
    #[serde(rename = "netFlows")]
    pub net_flows: Vec<String>,
    /// Money-weighted return from the first date to each date (modified Dietz, cumulative),
    /// in percent.
    #[serde(rename = "mwr")]
    pub mwr: Vec<f64>,
    #[serde(rename = "series")]
    pub series: Vec<PerformanceSeries>,
    #[serde(rename = "positions")]
    pub positions: Vec<PositionPerformance>,
}

/// Daily returns of a market index (or any Yahoo ticker) over the period, for the comparison
/// line of the performance chart (`get_stock_benchmark`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockBenchmark {
    /// The validated ticker the prices were fetched for.
    #[serde(rename = "ticker")]
    pub ticker: String,
    /// The report currency the returns are computed in.
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    /// Days as `YYYY-MM-DD`, one per calendar day (the last close carries over non-trading
    /// days); empty when the ticker has no prices.
    #[serde(rename = "dates")]
    pub dates: Vec<String>,
    /// Cumulative return of the index in percent per date, the first date being 0.
    #[serde(rename = "twr")]
    pub twr: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(key: &str) -> PerformanceSeriesSpec {
        PerformanceSeriesSpec {
            key: key.to_string(),
            investment_ids: vec![],
            currency: None,
        }
    }

    fn request(
        from_ts: i64,
        to_ts: i64,
        series: Vec<PerformanceSeriesSpec>,
    ) -> StockPerformanceRequest {
        StockPerformanceRequest {
            from_ts,
            to_ts,
            report_currency: "CZK".to_string(),
            series,
        }
    }

    fn key_of(result: Result<()>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_bad_requests() {
        assert!(request(0, 86_400, vec![spec("a"), spec("b")])
            .validate()
            .is_ok());
        assert!(request(0, 86_400, vec![]).validate().is_ok());

        // the period must run forwards
        assert_eq!(
            key_of(request(86_400, 86_400, vec![]).validate()),
            "validation.periodInvalid"
        );
        assert_eq!(
            key_of(request(86_401, 86_400, vec![]).validate()),
            "validation.periodInvalid"
        );

        // at most 64 series
        let many: Vec<_> = (0..65).map(|i| spec(&format!("k{i}"))).collect();
        assert_eq!(
            key_of(request(0, 86_400, many).validate()),
            "validation.tooManySeries"
        );
        let limit: Vec<_> = (0..64).map(|i| spec(&format!("k{i}"))).collect();
        assert!(request(0, 86_400, limit).validate().is_ok());

        // keys are non-empty and unique
        assert_eq!(
            key_of(request(0, 86_400, vec![spec("")]).validate()),
            "validation.seriesKeyInvalid"
        );
        assert_eq!(
            key_of(request(0, 86_400, vec![spec("  ")]).validate()),
            "validation.seriesKeyInvalid"
        );
        assert_eq!(
            key_of(request(0, 86_400, vec![spec("a"), spec("a")]).validate()),
            "validation.seriesKeyInvalid"
        );
    }
}
