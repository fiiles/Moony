# 0020. Yahoo Finance for crypto prices; CoinGecko removed

Date: 2026-10-25

## Status

Accepted; supersedes the CoinGecko sentence of [0005](0005-yahoo-finance-for-prices.md)

## Context

Crypto holdings were priced by two providers. CoinGecko supplied the current USD price, the
coin search and the last 365 days of history; its free tier is rate-limited and serves no
older data, so Moony carried an optional CoinGecko API key (a Settings card, a banner on the
crypto page, a stored secret) and the history rebuild paused between coins. Yahoo Finance
already supplied the days older than CoinGecko's year. The coin's identity was the CoinGecko
id, which tied every holding, the wire contract, the MCP import tool, the privacy policy and
the locales to that provider. The owner wanted CoinGecko gone and crypto priced from Yahoo
Finance like stocks.

A probe against Yahoo's chart and search endpoints showed that the Yahoo symbol cannot be
derived from the ticker. The symbol is `<TICKER>[<CoinMarketCap id>]-<QUOTE>`, and plain
`TICKER-USD` belongs to whichever token Yahoo listed first: `SUI` and `PEPE` have no plain
pair, while `ARB-USD` and `TON-USD` are other coins than Arbitrum and Toncoin. Yahoo's search
returns at most seven quotes, mixes in stocks and non-USD pairs, and finds coins reliably only
when the query is tried both as `<query> USD` and as typed. Suffixed symbols serve a full daily
history.

## Decision

- **Yahoo Finance is the only crypto price source.** The current price (one chart request per
  holding), the coin search and the daily history all come from Yahoo. No API key is needed;
  the CoinGecko code, key, settings card and banner are removed.
- **A holding stores the Yahoo symbol of its USD pair** (`crypto_investments.yahoo_symbol`,
  nullable, for example `SUI20947-USD`). The ticker stays the user's and the key of everything
  downstream of pricing.
- **Symbols are resolved lazily and never guessed.** A holding without a symbol (a migrated
  one, or one added without picking a search result) is looked up by name, then by ticker. A
  symbol is stored only when exactly one coin matches; otherwise the holding stays unresolved,
  is skipped by price refreshes and history rebuilds, and the crypto page asks the user to pick
  the listing ("Rate source…"). The migration itself needs no network and assigns no symbols.

## Consequences

- **A single provider.** Yahoo's interface is unofficial; when it breaks, stock and crypto
  prices go stale together (the risk 0005 accepted). No data is lost, and
  `crypto_price_overrides` still lets the user pin a price by hand.
- **No key management, quota bookkeeping or inter-request sleep** for crypto.
- **Unresolved holdings need one pick from the user** where Yahoo lists several coins of the
  same ticker and name. Until then the last cached price is shown and the history is not
  rebuilt for that coin.
- Crypto history older than a year now comes from the same series as the recent days. Every
  holding whose symbol is set or resolved lazily is rebuilt from its first transaction on
  Yahoo's daily closes, replacing the rows stored from CoinGecko; only a holding that stays
  unresolved keeps its old rows.
- The wire contract and the MCP tool change: `coingeckoId` becomes `yahooSymbol` (optional on
  `crypto_transactions_create`).
