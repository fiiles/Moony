# 0028. Asset logos: only from a party that already knows the ticker, or bundled

Date: 2026-11-14

## Status

Accepted

## Context

Logos were removed on 2026-10-02 (audit finding REL-02, commit `cccd3906`): the webview loaded them from financialmodelingprep.com and coincap.io, which told two more companies every ticker a user tracks. That broke the privacy promise that only the party supplying prices learns what is tracked, so stocks and coins have shown an initials circle since. The lists look bare without a mark, and the webview must not load images from the network (the CSP stays as it is).

Yahoo Finance already receives every tracked ticker for prices, and its search endpoint can return a logo URL on Yahoo's own CDN (`enableLogoUrl=true`). For coins the same endpoint points to coinmarketcap.com, a party that learns nothing today.

## Decision

A logo may only come from a party that already receives that ticker, fetched by the backend and cached encrypted, or be bundled with the app.

- **Stocks and ETFs: Yahoo only.** `services/asset_logos.rs` and `price_api::refresh_asset_logos` ask Yahoo's search for the ticker and download the image only from `https://s.yimg.com/`, without following redirects; any other host counts as "no logo". The backend builds the list from holdings and watched stocks itself, so no name from the frontend reaches the network. At most 40 tickers per run, once after unlock and after a stock is added or followed.
- **Cached encrypted.** Images are re-encoded to PNG of at most 96 px and stored in `asset_logos` in the encrypted database. The table is sync class Local: never synced. A missing logo is retried after 30 days. The webview receives data URLs and never loads an image from the network.
- **Coins: bundled.** Icons come from `@web3icons/core` (MIT), matched by ticker; no network at all.
- **No fallback source.** A ticker Yahoo has no logo for keeps the initials circle (owner decision, 2026-11-14).

## Alternatives considered

- **Third-party logo hosts** (the removed approach, or any logo API): better coverage, but each one learns the user's tickers. Rejected, as in REL-02.
- **Everything bundled:** no network, but a logo set for all listed companies is far too large and goes stale. Fine for coins, which are a small closed set; rejected for stocks.
- **Yahoo's coin logos:** they point to coinmarketcap.com, a new party. Rejected.

## Consequences

- Stock coverage is partial: tickers Yahoo has no logo for show initials.
- The bundle grows by about 5 MB for roughly 1800 coin icons.
- Each device downloads its own stock logos; the cache table is Local and not part of device sync.
- The privacy documents name company logos and the `s.yimg.com` host as part of the Yahoo traffic.
- Any future logo source must pass the rule above, or a new ADR must replace it.
