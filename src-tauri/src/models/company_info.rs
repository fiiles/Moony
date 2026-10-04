//! Company data of a stock for the position detail ("About the company").
//!
//! Read from the `stock_data` cache that `price_api::refresh_stock_metadata_yahoo` fills; money
//! and ratios are TEXT strings per ADR 0001. A figure is `None` when Yahoo never reported it or
//! reported 0 for something that does not apply (no market cap for a fund, no dividend).

use serde::{Deserialize, Serialize};
use specta::Type;

/// Company metadata stored for one ticker. Every field except `ticker` is `None` when nothing
/// is stored (no `stock_data` row yet, or Yahoo has no data for the instrument).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct StockCompanyInfo {
    pub ticker: String,
    pub sector: Option<String>,
    pub industry: Option<String>,
    #[serde(rename = "peRatio")]
    pub pe_ratio: Option<String>,
    #[serde(rename = "forwardPe")]
    pub forward_pe: Option<String>,
    /// Whole number in the listing currency.
    #[serde(rename = "marketCap")]
    pub market_cap: Option<String>,
    pub beta: Option<String>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    pub fifty_two_week_high: Option<String>,
    #[serde(rename = "fiftyTwoWeekLow")]
    pub fifty_two_week_low: Option<String>,
    /// Annual dividend per share in the listing currency.
    #[serde(rename = "dividendRate")]
    pub dividend_rate: Option<String>,
    /// Raw Yahoo fraction ("0.033100" = 3.31 %); multiply by 100 for display.
    #[serde(rename = "dividendYield")]
    pub dividend_yield: Option<String>,
    /// Yahoo instrument class ("EQUITY", "ETF", …).
    #[serde(rename = "quoteType")]
    pub quote_type: Option<String>,
    /// Currency of the prices and figures above (the listing currency).
    pub currency: Option<String>,
    /// When the metadata was last fetched (unix seconds); `None` when it never was.
    #[serde(rename = "metadataFetchedAt")]
    pub metadata_fetched_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend mirror in `shared/schema.ts` relies on these exact wire names (a missing
    /// per-field rename would silently ship snake_case).
    #[test]
    fn serializes_with_camel_case_field_names() {
        let info = StockCompanyInfo {
            ticker: "AAPL".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_value(&info).expect("serialize");
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "beta",
                "currency",
                "dividendRate",
                "dividendYield",
                "fiftyTwoWeekHigh",
                "fiftyTwoWeekLow",
                "forwardPe",
                "industry",
                "marketCap",
                "metadataFetchedAt",
                "peRatio",
                "quoteType",
                "sector",
                "ticker",
            ]
        );
    }
}
