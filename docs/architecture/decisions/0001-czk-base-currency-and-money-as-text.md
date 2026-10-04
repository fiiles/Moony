# 0001. CZK base currency and money as TEXT

Date: 2026-07-07

## Status

Accepted

## Context

Moony tracks assets and transactions in many currencies but every aggregate —
net worth, portfolio snapshots, analytics — needs a single base to sum in.
SQLite has no decimal type, and storing money in REAL columns invites binary
floating-point drift that compounds across thousands of transaction rows.

## Decision

- **CZK is the internal base currency.** All cross-asset aggregation happens
  in CZK; the user's display currency is a presentation concern.
- **All monetary values are stored as TEXT** (decimal strings) in SQLite,
  never REAL, so stored amounts are exact and never drift.
- **Conversion happens at display time** using ECB exchange rates: fetched and
  cached by `src-tauri/src/services/currency.rs` on the backend, applied on
  the frontend via `shared/currencies.ts` (`convertToCzK`) and the
  `CurrencyContext` formatting helpers.

## Consequences

- Money is `string` across the whole wire contract: TEXT in SQLite, `String`
  in Rust structs, `string` in `shared/schema.ts`. Typing a money field as
  `number` is a contract bug.
- Every arithmetic site must parse the string first. The prevailing pattern
  `.parse().unwrap_or(0.0)` **silently coerces malformed values to zero**,
  corrupting totals instead of failing loudly. New code must propagate a parse
  error instead (see `docs/standards/rust-backend.md`).
- Historical aggregates (snapshots, trend charts) must record or look up the
  rates of their day; converting old values at today's rate is wrong.
