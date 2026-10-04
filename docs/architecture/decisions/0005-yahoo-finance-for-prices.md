# 0005. Yahoo Finance for stock prices

Date: 2026-07-07

## Status

Accepted

## Context

Stock quotes originally came from Marketstack. Its free tier limits (request
quota, end-of-day-only data) were too restrictive for refreshing prices
across a whole portfolio at will, and paying for an API contradicts the
free, local-first nature of the app.

## Decision

**Yahoo Finance replaced Marketstack as the stock price source** in
`src-tauri/src/services/price_api.rs` — quotes, dividends, and ticker search
go through the `yahoo_finance_api` crate, which handles Yahoo's cookie/crumb
authentication. No API key is required. The Marketstack and Finnhub remnants
(the `api_key_marketstack` / `api_key_finnhub` config entries, the settings
fields and their locale strings) were removed; only the optional CoinGecko key
remains.

## Consequences

- **Yahoo's API is unofficial and unversioned; breakage risk is accepted.**
  When Yahoo changes something, price refresh stops working until the
  integration is fixed or the provider is swapped — but no user data is
  lost; valuations just go stale.
- **Manual override tables are the fallback:** `stock_price_overrides` and
  `crypto_price_overrides` let the user pin prices by hand, so the portfolio
  stays usable through an outage.
- No key management or request-quota bookkeeping is needed for stock prices.
