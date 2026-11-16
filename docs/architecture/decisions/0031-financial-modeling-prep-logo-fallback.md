# 0031. Asset logos: Financial Modeling Prep when Yahoo has none

Date: 2026-11-15

## Status

Accepted. Supersedes the "only from a party that already knows the ticker" rule and the "no
fallback source" decision of [ADR 0028](0028-asset-logos.md); the rest of 0028 (backend download,
encrypted Local cache, re-encoding, bundled coin icons) stays. Reverses the stock half of audit
finding REL-02 (2026-10-02), at the owner's decision of 2026-11-15.

## Context

ADR 0028 took stock logos from Yahoo Finance only. Measured on 2026-11-15, Yahoo's search returns
a logo for 25 of the 40 largest US companies (none for BRK-B, GOOGL, META, JPM, JNJ, KO, PG, XOM,
DIS and others) and for none of the European stocks and ETFs tried (SAP.DE, ASML.AS, NESN.SW,
MC.PA, CEZ.PR, VWCE.DE, CSPX.L, IWDA.AS). Yahoo only has logos for a small curated list of brands.
Most lists therefore still showed initials.

Financial Modeling Prep (FMP) serves a public logo image per ticker at
`https://financialmodelingprep.com/image-stock/<TICKER>.png`, without an API key, and accepts
Yahoo-style symbols (`BRK-B`, `CEZ.PR`, `VWCE.DE`). It had a logo for every ticker above; for an
ETF it returns the issuer's logo (Vanguard, iShares). An unknown ticker answers HTTP 404. A few
tickers get a 16×16 favicon.

FMP was the logo source until REL-02 removed it because it learns which tickers a user tracks.
The owner accepts that trade for the coverage.

## Decision

- **Yahoo first, FMP second.** `price_api::fetch_asset_logo` asks Yahoo's search as before; only
  when that ends as a clean miss (no `s.yimg.com` logo for exactly this ticker, a 4xx other than
  408/429, or an image that does not normalize) is FMP asked. A Yahoo rate limit, server error or
  network failure never falls through to FMP, so FMP learns only the tickers Yahoo has no logo for.
- **What FMP receives:** the ticker in the URL path, the IP address and the `Moony/<version>` user
  agent of the shared HTTP client. No API key, no account. Redirects are not followed.
- **Only plain symbols** (letters, digits, `.` and `-`, at most 20 characters) are sent
  (`asset_logos::fmp_logo_url`); an index (`^GSPC`) or a currency pair (`EURUSD=X`) is a miss
  without a request.
- **Favicons are misses:** an image whose longer side is under 32 px is not stored (both sources).
- **Earlier misses are looked up again once:** a miss recorded before
  `asset_logos::SOURCES_CHANGED_AT` (2026-11-16 12:00 CET, after the last Yahoo-only dev run) is due at once instead of after 30 days.
- Everything else from ADR 0028 stands: the backend builds the ticker list from holdings and
  watched stocks, at most 40 tickers per run, PNG of at most 96 px in the encrypted `asset_logos`
  table (Local, never synced), the webview never loads an image from the network, coins use the
  bundled `@web3icons/core` set.

## Alternatives considered

- **Keep Yahoo only (ADR 0028):** no new party, but most stocks and all European stocks and ETFs
  keep initials. Rejected by the owner.
- **A logo pack downloaded whole** (the same file for every user, so nobody learns the tickers):
  private, but needs a build pipeline, hosting and a licence for redistributing the images.
  Not pursued for now.
- **ETF issuer logos bundled:** private and small, but covers only ETFs.
- **FMP instead of Yahoo:** simpler, but FMP would learn every ticker, and Yahoo's brand images are
  larger (150 px). Rejected.

## Consequences

- Logo coverage rises from a curated list of US brands to most listed stocks and ETFs.
- Financial Modeling Prep becomes a fourth network party: it learns the tickers Yahoo has no logo
  for, and the IP address. `PRIVACY.md`, `README.md` and the welcome screen's network line say so.
- There is no switch to turn logos off; avoiding FMP means not tracking stocks. A setting can be
  added later if wanted.
- FMP's endpoint is unofficial for this use and can change or start requiring a key; a failure
  only means initials, as before.
