//! Stock Monitor (watchlist) models — spec 2026-08-17-stock-monitor-design
//!
//! Money values (target_price, prices) are TEXT strings per ADR 0001.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A followed stock (row in watched_stocks)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WatchedStock {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "targetPrice")]
    pub target_price: Option<String>,
    /// `below` (waiting for a dip) or `above` (waiting for a rise); None without a target.
    #[serde(rename = "targetDirection")]
    pub target_direction: Option<String>,
    pub notes: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// Overview-table row: watchlist entry enriched from the stock_data cache
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WatchedStockRow {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "targetPrice")]
    pub target_price: Option<String>,
    /// `below` (waiting for a dip) or `above` (waiting for a rise); None without a target.
    #[serde(rename = "targetDirection")]
    pub target_direction: Option<String>,
    pub notes: String,
    #[serde(rename = "shortName")]
    pub short_name: Option<String>,
    #[serde(rename = "longName")]
    pub long_name: Option<String>,
    pub currency: Option<String>,
    #[serde(rename = "currentPrice")]
    pub current_price: Option<String>,
    #[serde(rename = "previousClose")]
    pub previous_close: Option<String>,
    #[serde(rename = "fiftyTwoWeekLow")]
    pub fifty_two_week_low: Option<String>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    pub fifty_two_week_high: Option<String>,
    pub exchange: Option<String>,
    #[serde(rename = "priceFetchedAt")]
    pub price_fetched_at: Option<i64>,
    #[serde(rename = "heldInvestmentId")]
    pub held_investment_id: Option<String>,
    #[serde(rename = "isHeld")]
    pub is_held: bool,
    /// When the stock was added to the watchlist (unix seconds).
    #[serde(rename = "followedAt")]
    pub followed_at: i64,
}

/// Detail for any ticker — works for un-followed tickers too (followed=false,
/// notes empty, target None). All stock_data fields are None when the cache
/// has no row yet (e.g. offline right after a search).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockMonitorDetail {
    pub ticker: String,
    pub followed: bool,
    #[serde(rename = "targetPrice")]
    pub target_price: Option<String>,
    /// `below` (waiting for a dip) or `above` (waiting for a rise); None without a target.
    #[serde(rename = "targetDirection")]
    pub target_direction: Option<String>,
    pub notes: String,
    #[serde(rename = "shortName")]
    pub short_name: Option<String>,
    #[serde(rename = "longName")]
    pub long_name: Option<String>,
    pub currency: Option<String>,
    #[serde(rename = "currentPrice")]
    pub current_price: Option<String>,
    #[serde(rename = "previousClose")]
    pub previous_close: Option<String>,
    #[serde(rename = "fiftyTwoWeekLow")]
    pub fifty_two_week_low: Option<String>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    pub fifty_two_week_high: Option<String>,
    #[serde(rename = "marketCap")]
    pub market_cap: Option<String>,
    #[serde(rename = "peRatio")]
    pub pe_ratio: Option<String>,
    /// Raw Yahoo fraction (0.0044 = 0.44 %); multiply by 100 for display.
    #[serde(rename = "trailingDividendYield")]
    pub trailing_dividend_yield: Option<String>,
    pub exchange: Option<String>,
    #[serde(rename = "priceFetchedAt")]
    pub price_fetched_at: Option<i64>,
    #[serde(rename = "heldInvestmentId")]
    pub held_investment_id: Option<String>,
    #[serde(rename = "isHeld")]
    pub is_held: bool,
    /// When the stock was added to the watchlist (unix seconds); None when not followed.
    #[serde(rename = "followedAt")]
    pub followed_at: Option<i64>,
}

/// Chart data point (transient, never persisted — f64 like HistoricalPrice)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockPricePoint {
    pub timestamp: i64,
    pub price: f64,
    pub currency: String,
}

/// Insert payload for following a stock
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertWatchedStock {
    pub ticker: String,
}

impl InsertWatchedStock {
    /// Ticker: 1–20 chars, ASCII alphanumeric plus Yahoo suffix chars . - ^ =
    pub fn validate(&self) -> Result<()> {
        let t = self.ticker.trim();
        if t.is_empty()
            || t.len() > 20
            || !t
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '^' | '='))
            || !t.chars().any(|c| c.is_ascii_alphanumeric())
        {
            return Err(AppError::Validation("validation.tickerInvalid".to_string()));
        }
        Ok(())
    }
}

/// Target price: when present, must be a plain positive decimal string
/// (digits with at most one '.'), matching the money-as-TEXT convention —
/// no scientific notation, signs, or other f64-parseable exotica.
pub fn validate_target_price(target: &Option<String>) -> Result<()> {
    if let Some(t) = target {
        let t = t.trim();
        let plain_decimal = !t.is_empty()
            && t.chars().all(|c| c.is_ascii_digit() || c == '.')
            && t.chars().filter(|&c| c == '.').count() <= 1
            && t.chars().any(|c| c.is_ascii_digit());
        let positive = t
            .parse::<f64>()
            .map(|v| v > 0.0 && v.is_finite())
            .unwrap_or(false);
        if !plain_decimal || !positive {
            return Err(AppError::Validation(
                "validation.targetPricePositive".to_string(),
            ));
        }
    }
    Ok(())
}

/// Target waiting for the price to fall to it.
pub const TARGET_BELOW: &str = "below";
/// Target waiting for the price to rise to it.
pub const TARGET_ABOVE: &str = "above";

/// A target direction is `below`, `above` or absent.
pub fn validate_target_direction(direction: &Option<String>) -> Result<()> {
    match direction.as_deref() {
        None | Some(TARGET_BELOW) | Some(TARGET_ABOVE) => Ok(()),
        Some(_) => Err(AppError::Validation(
            "validation.targetDirectionInvalid".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(ticker: &str) -> InsertWatchedStock {
        InsertWatchedStock {
            ticker: ticker.to_string(),
        }
    }

    #[test]
    fn valid_tickers_pass() {
        for t in [
            "AAPL",
            "BMW.DE",
            "bmw.de",
            "^GSPC",
            "BRK-B",
            "EURUSD=X",
            " AAPL ",
            "ABCDEFGHIJKLMNOPQRST",
        ] {
            assert!(insert(t).validate().is_ok(), "expected {t} to be valid");
        }
    }

    #[test]
    fn invalid_tickers_fail() {
        for t in [
            "",
            "   ",
            "AA PL",
            "AAPL;DROP",
            "ABCDEFGHIJKLMNOPQRSTU",
            "AAPL/US",
            "..",
            "^",
            "=",
            "ÄKTIE",
        ] {
            assert!(insert(t).validate().is_err(), "expected {t} to be invalid");
        }
    }

    #[test]
    fn target_price_validation() {
        assert!(validate_target_price(&None).is_ok());
        assert!(validate_target_price(&Some("250.50".to_string())).is_ok());
        assert!(validate_target_price(&Some(" 10 ".to_string())).is_ok());
        // "10." parses as 10.0 and matches the plain-decimal grammar — documented Ok case.
        assert!(validate_target_price(&Some("10.".to_string())).is_ok());
        assert!(validate_target_price(&Some("0".to_string())).is_err());
        assert!(validate_target_price(&Some("-5".to_string())).is_err());
        assert!(validate_target_price(&Some("+5".to_string())).is_err());
        assert!(validate_target_price(&Some("abc".to_string())).is_err());
        assert!(validate_target_price(&Some("inf".to_string())).is_err());
        assert!(validate_target_price(&Some("NaN".to_string())).is_err());
        assert!(validate_target_price(&Some("2.5e2".to_string())).is_err());
        assert!(validate_target_price(&Some("1.2.3".to_string())).is_err());
    }

    #[test]
    fn target_direction_validation() {
        assert!(validate_target_direction(&None).is_ok());
        assert!(validate_target_direction(&Some(TARGET_BELOW.to_string())).is_ok());
        assert!(validate_target_direction(&Some(TARGET_ABOVE.to_string())).is_ok());
        assert!(validate_target_direction(&Some("sideways".to_string())).is_err());
        assert!(validate_target_direction(&Some(String::new())).is_err());
        assert!(validate_target_direction(&Some("Below".to_string())).is_err());
    }
}
