//! Application-wide constants

/// Default currency for cryptocurrency prices (CoinGecko returns USD)
pub const DEFAULT_CRYPTO_CURRENCY: &str = "USD";

/// Default currency for US stock prices
pub const DEFAULT_STOCK_CURRENCY: &str = "USD";

/// Currency a missing `currency` input resolves to when an entity is stored. It mirrors the
/// DB column default of the entity tables (a storage default, not the profile's main
/// currency): code that needs the profile currency reads it from the profile instead.
pub const STORAGE_DEFAULT_CURRENCY: &str = "CZK";
