# Changelog

All notable changes to Moony are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Stock import from brokers.** A wizard (file, columns, review, done) replaces the old import
  table. It reads exports from XTB, Trading 212, Degiro and Interactive Brokers, and your own
  table made in Excel (Czech or English headers, any delimiter and encoding); other layouts are
  mapped column by column and can be remembered. Before anything is written it confirms the
  securities on Yahoo Finance, shows what happens to every row and catches duplicates (by the
  broker's trade ID, the same values, or a trade you entered by hand with a rounded price) and
  sells above the holding. Every import is recorded and can be undone. Fees are not imported.
- **Year to date.** The dashboard, the list trend cards and the stock, crypto and bank account
  charts offer the horizons 30 days · 3 months · This year · Year · All, bounded on UTC days.
- **Cash over time.** Bank accounts get a trend card with the recorded cash history, the same
  data that feeds net worth.
- **Buys and sells on the stocks trend.** The portfolio chart on Stocks marks each trading day
  (tickers and amount in the tooltip), as Crypto already did.
- **New loans on the debt trajectory.** Each loan start is marked where the debt steps up (paid-off
  loans included), and the step is drawn as a step instead of a ramp.
- **Purchase date for real estate.** An optional purchase date starts the value trace with the
  purchase and is part of the real estate CSV export; the first estimate of a new property or
  other asset is recorded when it is created.
- **Company data on the position detail.** Sector, industry, P/E, market cap and the other key
  figures of a holding are downloaded (at most once a day) and shown on the stock detail.

### Changed

- **Charts.** The net worth chart is taller (220 px) and every list-page trend card shares one
  card and a 170 px chart (Stocks, Crypto, Real estate, Other assets, Bank accounts, Loans, Bonds,
  Insurance).
- **Stocks analysis.** The tag chips filter everything below them (allocation, TWR per tag, tag
  and holdings tables); the two cards are equally tall; TWR is shown in percent with the whole
  portfolio as a dark dashed reference and its return in the card head.
- **Insurance and real estate tables** have fewer, denser columns (type and insurer, type and
  address, the purchase price under the market value) and never scroll horizontally.
- **Recent moves** on the dashboard no longer has a "Show all" link that opened bank accounts.
- **Import CSV** is a button in the head of the Stocks page instead of the footer of the add
  dialog.
- **Bank accounts total** leaves out accounts excluded from net worth, as the dashboard does; the
  excluded amount is shown next to it.

- **Amounts follow the main currency.** Gain, yield, appreciation, rent and savings totals on
  Other assets, Real estate, Cashflow, Cashflow planning and Projection were shown in CZK
  whatever main currency you chose; they now follow it. The cashflow CSV export writes its
  amounts in the main currency and its headers end in the currency code (for example
  `income_eur`). The projection's monthly contributions are entered in the main currency, and
  new other-asset forms default to it.
- **MCP server (breaking for MCP clients).** The aggregate tools report amounts in your main
  currency and name it in a top-level `mainCurrency` field. The `*_czk` / `*Czk` keys lost the
  suffix (for example `net_worth_czk` is now `net_worth`, `valueCzk` is now `value`). History
  tools convert each day at that day's exchange rate. `tag_metrics`, `portfolio_get_history`,
  `stock_value_history` and `crypto_value_history` now return an object (`{mainCurrency, tags}`
  or `{mainCurrency, history}`) instead of a bare array. Create tools default a missing
  currency to the main currency.
- **Real estate investment calculator.** The rental property calculator is now called the real
  estate investment calculator ("Investice do nemovitosti" in Czech), since it values the whole
  investment: rent and cashflow, loan repayment, price growth and the gain from a future sale.

### Fixed

- **London-listed stocks were valued in the wrong currency.** A quote was labelled with the
  currency its ticker suffix suggests, so a London share quoted in pence (BARC.L, LLOY.L) was a
  hundred times too high and a dollar ETF listed in London (CSPX.L, VWRA.L, IWDA.L, EIMI.L) was
  valued as pounds. Prices, the previous close, dividends, the charts and the 52-week range now
  use the currency Yahoo reports, and the value history of the affected holdings is rebuilt once
  in the background. If you entered the purchase prices of such a holding in pence to match the
  old display, edit them to pounds.
- **Chart tooltips froze after a buy or sell mark.** On every chart with marks the price tooltip
  kept showing the first day while the cursor moved; it now follows the cursor, and a mark's own
  tooltip appears on hover and stays inside the card at the edges.
- **Real estate and other assets value charts showed a single point** and asked the database for
  the history about 34 times a second; they now load the history once per day and horizon.
- **A first revaluation lost the original estimate.** Every property and other asset now has a
  valuation log from its creation (existing ones are seeded once with their current value), so
  the trace shows each estimate; same-day revaluations no longer scramble the line.
- **Company data on the stock detail was always empty**; it was only downloaded for watched
  stocks and never read for holdings.
- **The 52-week range overflowed its card** on the watchlist detail.
- **Stocks analysis switches did nothing visible**: the tag chips only filtered the holdings table
  at the bottom of the page; the whole-portfolio TWR line was drawn in nearly the colour of a tag.
- **Net worth history after saving a property.** Adding or editing a property now records
  today's snapshot right away, as every other change does, instead of waiting for the next one.
- **Insurance limits in another currency** were labelled as Czech crowns in the policies table
  and ranked by their raw amount (a yen limit above a euro one) there and on the policy detail.
- **MCP cashflow and budget reports.** The `cashflow_report` and `budgeting_report` tools
  returned empty lists because they read money stored as text as numbers.
- **Loading a property into the investment calculator brings its loans.** The loan amount,
  term and interest rate now come from the loans linked to the property (summed, rate and term
  weighted by principal); a property without a linked loan sets the loan to 0 instead of
  keeping the example mortgage. Prices, rent, costs and loans are converted to the main
  currency.
- **Calculator numbers in English.** The annuity and real estate calculators read
  "5,400,000" as 5.4, so amounts loaded from a loan or a property were wrong in the English
  interface; thousands separators of the interface language are now understood.
- **Real estate calculator chart amounts.** The sale milestone and the chart tooltips converted
  the calculator's amounts from CZK a second time when the main currency was not CZK.

## [0.9.0] - 2026-10-04

First public release of Moony, published as a pre-1.0 version for a round of testing on all
three platforms before 1.0.0. Moony started as a private project; this version is a fresh start
with a new repository, a new application identifier and a single database schema, so it does
not open databases written by earlier private builds (the owner's data is converted once by a
local tool). Everything below is new to the public.

### What Moony does

- **Net worth in one place.** Bank accounts, stocks, crypto, bonds, real estate, loans,
  insurance and other assets, with a net-worth history chart, asset allocation and a
  *Getting started* checklist on the dashboard.
- **Local and encrypted.** All records live in one SQLCipher (AES-256) database in your user
  profile, protected by your password and a 24-character recovery key. No account, no cloud,
  no sync. Market data is the only thing fetched from the internet (Yahoo Finance, CoinGecko,
  ECB and Frankfurter exchange rates); see `PRIVACY.md` for the exact list. Moony sends no
  telemetry: no analytics, no usage statistics and no crash reports.
- **Bank statements.** CSV import with bank presets detected from the header row (Revolut,
  Wise, N26, Fio, Česká spořitelna, Komerční banka, Air Bank, ČSOB, mBank, Raiffeisenbank,
  Moneta, Chase, Sparkasse, DKB, ING Germany and a generic UK debit/credit layout), multilingual
  column detection with a parsed preview, duplicate skipping and one-click undo of an import.
- **Categorization.** Your own rules, rules learned from your choices, and rule packs for many
  countries; a rules page with hit counts, automation rate and a live match preview.
- **Investments.** Stock and crypto positions with live prices (no API key needed for stocks),
  cost basis from transactions, dividends, tags and tag groups, a stock analysis page, a stock
  monitor watchlist that never counts toward net worth, and per-ticker value history rebuilt in
  the background.
- **Loans, real estate, insurance.** Amortized loan balances with a repayment schedule and
  loan events (extra payments, rate changes, balance checks), dated property valuations, photo
  and document attachments, recurring costs, linked loans and insurance policies.
- **Planning.** Budgets by category, actual cashflow from bank transactions, cashflow planning,
  a net-worth projection with explicit assumptions, and annuity and rental-property calculators.
- **International.** English and Czech interface; money, numbers and dates follow the UI
  language; every ECB currency plus common others, each account in its own currency and totals
  in the one you choose.
- **Data and backups.** One-click backup to a ZIP, restore that keeps your current data in a
  `pre-restore-…` folder, full JSON export and a database integrity check, all in Settings.
- **AI assistant (optional).** A built-in MCP server on `127.0.0.1`, off by default, token
  protected, with read tools and a small set of write tools.
- **Updates.** A one-click in-app update checked after unlock, verified against the project's
  updater key; the check can be switched off.
- **Security.** Webview access to the data folder is denied except for photos; strict
  content-security policy; owner-only key files; scrypt N = 2¹⁷ key derivation; constant-time
  token comparison; validated backup archives.

### Known limitations

- Installers are not signed by Apple or Microsoft; the README explains the one-time Gatekeeper
  and SmartScreen steps. The in-app updater still verifies its own signature.
- Attachments (photos and documents) are stored as plain files next to the encrypted database.
- The updater only offers versions higher than the installed one, so a machine running an
  older 1.x build from the previous private repository must install 0.9.0 by hand.

[Unreleased]: https://github.com/fiiles/Moony/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/fiiles/Moony/releases/tag/v0.9.0
