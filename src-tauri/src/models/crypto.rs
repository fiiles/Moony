//! Cryptocurrency models

use serde::{Deserialize, Serialize};
use specta::Type;

/// Crypto investment holding
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CryptoInvestment {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "coingeckoId")]
    pub coingecko_id: String,
    pub name: String,
    pub quantity: String,
    #[serde(rename = "averagePrice")]
    pub average_price: String,
}

/// Enriched crypto with current price
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EnrichedCryptoInvestment {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "coingeckoId")]
    pub coingecko_id: String,
    pub name: String,
    pub quantity: String,
    #[serde(rename = "averagePrice")]
    pub average_price: String,
    /// Currency of the average price (native currency from first transaction)
    #[serde(rename = "averagePriceCurrency")]
    pub average_price_currency: String,
    #[serde(rename = "currentPrice")]
    pub current_price: String,
    /// Original price in its source currency (before conversion to CZK)
    #[serde(rename = "originalPrice")]
    pub original_price: String,
    /// Currency of the current market price (e.g., USD, EUR)
    pub currency: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: Option<i64>,
    /// Whether the price is manually overridden
    #[serde(rename = "isManualPrice")]
    pub is_manual_price: bool,
}

/// Data for creating crypto investment. Quantity and average price are not
/// accepted: they derive from the transaction record (pass an initial
/// transaction on create).
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertCryptoInvestment {
    pub ticker: String,
    #[serde(rename = "coingeckoId")]
    pub coingecko_id: String,
    pub name: String,
}

/// Crypto transaction (buy/sell)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CryptoTransaction {
    pub id: String,
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub ticker: String,
    pub name: String,
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    pub price_per_unit: String,
    pub currency: String,
    #[serde(rename = "transactionDate")]
    pub transaction_date: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating crypto transaction
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertCryptoTransaction {
    #[serde(rename = "investmentId")]
    pub investment_id: Option<String>,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub ticker: String,
    pub name: String,
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    pub price_per_unit: String,
    pub currency: String,
    #[serde(rename = "transactionDate")]
    pub transaction_date: i64,
}

/// Crypto price cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoPrice {
    pub id: String,
    pub symbol: String,
    #[serde(rename = "coingeckoId")]
    pub coingecko_id: Option<String>,
    pub price: String,
    pub currency: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: i64,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertCryptoInvestment {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Ticker/symbol validation
        if self.ticker.is_empty() {
            return Err(AppError::Validation("validation.tickerRequired".into()));
        }
        if self.ticker.len() > 20 {
            return Err(AppError::Validation("validation.tickerTooLong".into()));
        }

        // CoinGecko ID validation
        if self.coingecko_id.is_empty() {
            return Err(AppError::Validation(
                "validation.coingeckoIdRequired".into(),
            ));
        }

        // Name validation
        if self.name.is_empty() {
            return Err(AppError::Validation("validation.cryptoNameRequired".into()));
        }

        Ok(())
    }
}

impl InsertCryptoTransaction {
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

        // Name validation
        if self.name.is_empty() {
            return Err(AppError::Validation("validation.cryptoNameRequired".into()));
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
