//! Stock investment models

use serde::{Deserialize, Serialize};
use specta::Type;

use super::fx::ConversionQuality;
use super::stock_tags::StockTag;

/// Stock investment holding
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockInvestment {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    pub quantity: String,
    #[serde(rename = "averagePrice")]
    pub average_price: String,
    /// Currency of the average price (determined by first transaction)
    #[serde(rename = "averagePriceCurrency")]
    pub currency: String,
}

/// A stock position: its native facts plus its figures in the response's report currency.
/// Converted amounts are decimal TEXT at the report currency's minor unit and `null` when they
/// cannot be converted (the currency is then in the response's `fxQuality`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EnrichedStockInvestment {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    pub quantity: String,
    /// Native weighted average cost (currency `averagePriceCurrency`).
    #[serde(rename = "averagePrice")]
    pub average_price: String,
    /// Currency of the average price (determined by first transaction)
    #[serde(rename = "averagePriceCurrency")]
    pub average_price_currency: String,
    /// Current price in its own currency, "0" when unpriced (see `price`).
    #[serde(rename = "originalPrice")]
    pub original_price: String,
    /// Currency of the current price (e.g., USD, EUR)
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: Option<i64>,
    #[serde(rename = "isManualPrice")]
    pub is_manual_price: bool,
    /// Yearly dividend per share in `dividendCurrency`, 0 without dividend data.
    #[serde(rename = "originalDividendYield")]
    pub original_dividend_yield: f64,
    /// Currency of the dividend per share; `null` without dividend data.
    #[serde(rename = "dividendCurrency")]
    pub dividend_currency: Option<String>,
    #[serde(rename = "isManualDividend")]
    pub is_manual_dividend: bool,
    /// Current price in its own currency; `null` when unpriced.
    #[serde(rename = "price")]
    pub price: Option<String>,
    #[serde(rename = "priceCurrency")]
    pub price_currency: Option<String>,
    #[serde(rename = "value")]
    pub value: Option<String>,
    #[serde(rename = "costBasis")]
    pub cost_basis: Option<String>,
    #[serde(rename = "unrealizedGain")]
    pub unrealized_gain: Option<String>,
    #[serde(rename = "unrealizedGainPercent")]
    pub unrealized_gain_percent: Option<f64>,
    #[serde(rename = "realizedGain")]
    pub realized_gain: Option<String>,
    /// Yearly dividend per share in `dividendCurrency`; `null` without dividend data.
    #[serde(rename = "dividendPerShare")]
    pub dividend_per_share: Option<String>,
    /// Yearly dividend of the whole position in the report currency.
    #[serde(rename = "yearlyDividend")]
    pub yearly_dividend: Option<String>,
}

/// Sums in the report currency over every known amount: `value`, `costBasis` and
/// `realizedGain` of the positions where they are known, `unrealizedGain` (and its percent) of
/// the positions whose value and cost are both known, `yearlyDividend` of the positions whose
/// value is known.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PositionTotals {
    #[serde(rename = "value")]
    pub value: String,
    #[serde(rename = "costBasis")]
    pub cost_basis: String,
    #[serde(rename = "unrealizedGain")]
    pub unrealized_gain: String,
    /// `unrealizedGain` in percent of the cost of the same positions; 0 without a cost.
    #[serde(rename = "unrealizedGainPercent")]
    pub unrealized_gain_percent: f64,
    #[serde(rename = "realizedGain")]
    pub realized_gain: String,
    #[serde(rename = "yearlyDividend")]
    pub yearly_dividend: String,
}

/// Every stock position in `currency`, with what could not be converted.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockPositions {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "items")]
    pub items: Vec<EnrichedStockInvestment>,
    #[serde(rename = "totals")]
    pub totals: PositionTotals,
}

/// One stock position in `currency`; `fxQuality` covers this position only.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockPosition {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "investment")]
    pub investment: EnrichedStockInvestment,
}

/// A stock position with its transactions and tags (one IPC call for the detail page).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InvestmentWithDetails {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "investment")]
    pub investment: EnrichedStockInvestment,
    #[serde(rename = "transactions")]
    pub transactions: Vec<InvestmentTransaction>,
    #[serde(rename = "tags")]
    pub tags: Vec<StockTag>,
}

/// Data for creating stock investment. Quantity and average price are not
/// accepted: they derive from the transaction record (pass an initial
/// transaction on create).
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertStockInvestment {
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
}

/// Investment transaction (buy/sell)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InvestmentTransaction {
    pub id: String,
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    pub price_per_unit: String,
    pub currency: String,
    #[serde(rename = "transactionDate")]
    pub transaction_date: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating transaction
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertInvestmentTransaction {
    #[serde(rename = "investmentId")]
    pub investment_id: Option<String>,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    pub price_per_unit: String,
    pub currency: String,
    #[serde(rename = "transactionDate")]
    pub transaction_date: i64,
}

/// Stock data cache (price + Yahoo Finance metadata)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockData {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "originalPrice")]
    pub original_price: String,
    pub currency: String,
    #[serde(rename = "priceDate")]
    pub price_date: i64,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: i64,
    #[serde(rename = "previousClose")]
    pub previous_close: Option<String>,
    // Metadata fields
    #[serde(rename = "shortName")]
    pub short_name: Option<String>,
    #[serde(rename = "longName")]
    pub long_name: Option<String>,
    pub sector: Option<String>,
    pub industry: Option<String>,
    #[serde(rename = "peRatio")]
    pub pe_ratio: Option<String>,
    #[serde(rename = "forwardPe")]
    pub forward_pe: Option<String>,
    #[serde(rename = "marketCap")]
    pub market_cap: Option<String>,
    pub beta: Option<String>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    pub fifty_two_week_high: Option<String>,
    #[serde(rename = "fiftyTwoWeekLow")]
    pub fifty_two_week_low: Option<String>,
    #[serde(rename = "trailingDividendRate")]
    pub trailing_dividend_rate: Option<String>,
    #[serde(rename = "trailingDividendYield")]
    pub trailing_dividend_yield: Option<String>,
    #[serde(rename = "exDividendDate")]
    pub ex_dividend_date: Option<i64>,
    pub description: Option<String>,
    pub exchange: Option<String>,
    #[serde(rename = "quoteType")]
    pub quote_type: Option<String>,
    #[serde(rename = "metadataFetchedAt")]
    pub metadata_fetched_at: Option<i64>,
}

/// User price override
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockPriceOverride {
    pub id: String,
    pub ticker: String,
    pub price: String,
    pub currency: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// Dividend data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DividendData {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "yearlyDividendSum")]
    pub yearly_dividend_sum: String,
    pub currency: String,
    #[serde(rename = "lastFetchedAt")]
    pub last_fetched_at: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// User dividend override
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DividendOverride {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "yearlyDividendSum")]
    pub yearly_dividend_sum: String,
    pub currency: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// One recorded day of a ticker's position. Quantity, price and currency are the native facts;
/// `value` is `quantity × price` converted at that day's rate (common-day rule), `null` when it
/// cannot be converted.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TickerValueHistory {
    pub ticker: String,
    #[serde(rename = "recordedAt")]
    pub recorded_at: i64,
    pub quantity: String,
    pub price: String,
    pub currency: String,
    pub value: Option<String>,
    #[serde(rename = "isStale")]
    pub is_stale: bool,
}

/// A ticker's value history in a report currency, oldest day first.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TickerValueHistoryReport {
    /// The report currency of every `value`.
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: crate::models::fx::ConversionQuality,
    pub points: Vec<TickerValueHistory>,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertStockInvestment {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Ticker validation
        if self.ticker.is_empty() {
            return Err(AppError::Validation("validation.tickerRequired".into()));
        }
        if self.ticker.len() > 10 {
            return Err(AppError::Validation("validation.tickerTooLong".into()));
        }
        if !self
            .ticker
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return Err(AppError::Validation("validation.tickerInvalid".into()));
        }

        // Company name validation
        if self.company_name.is_empty() {
            return Err(AppError::Validation(
                "validation.companyNameRequired".into(),
            ));
        }

        Ok(())
    }
}

impl InsertInvestmentTransaction {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Transaction type validation
        let tx_type = self.tx_type.to_lowercase();
        if tx_type != "buy" && tx_type != "sell" {
            return Err(AppError::Validation(
                "validation.transactionTypeInvalid".into(),
            ));
        }

        // Ticker validation
        if self.ticker.is_empty() {
            return Err(AppError::Validation("validation.tickerRequired".into()));
        }

        // Quantity validation
        let qty: f64 = self
            .quantity
            .parse()
            .map_err(|_| AppError::Validation("validation.invalidQuantity".into()))?;
        if qty <= 0.0 {
            return Err(AppError::Validation("validation.quantityPositive".into()));
        }

        // Price validation
        let price: f64 = self
            .price_per_unit
            .parse()
            .map_err(|_| AppError::Validation("validation.invalidPrice".into()))?;
        if price <= 0.0 {
            return Err(AppError::Validation("validation.pricePositive".into()));
        }

        // Currency validation
        if self.currency.len() != 3 {
            return Err(AppError::Validation("validation.currencyInvalid".into()));
        }
        if !self.currency.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(AppError::Validation("validation.currencyInvalid".into()));
        }

        Ok(())
    }
}

/// A single data point in a TWR time series.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TwrDataPoint {
    /// Calendar date as "YYYY-MM-DD"
    pub date: String,
    /// Cumulative time-weighted return in percent (0.0 at start of range)
    pub twr: f64,
}

/// A complete TWR series for one tag, untagged stocks, or the whole portfolio.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TwrSeries {
    /// The tag this series belongs to. None = whole portfolio or untagged.
    pub tag: Option<StockTag>,
    /// True when this series represents stocks with no tags assigned.
    #[serde(rename = "isUntagged")]
    pub is_untagged: bool,
    pub data: Vec<TwrDataPoint>,
}

/// Time-weighted return series computed in a report currency: each day's value and cash flows
/// are converted at that day's rate. Days that could not be converted are left out of the
/// chain and counted in `fxQuality`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockTwr {
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: crate::models::fx::ConversionQuality,
    pub series: Vec<TwrSeries>,
}
