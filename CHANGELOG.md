# Changelog

All notable changes to Moony are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
