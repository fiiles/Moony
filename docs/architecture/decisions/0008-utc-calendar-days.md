# 0008. Calendar days are UTC-midnight epochs; periods are bounded in UTC

Date: 2026-10-02

## Status

Accepted

## Context

Day-granular dates (bank transaction `booking_date` and `value_date`,
investment and crypto `transaction_date`, per-ticker value history rows) are
stored as INTEGER unix epochs. Every writer already produced the **UTC
midnight** of the calendar day: the CSV importer (`date_parser.rs`,
`and_hms_opt(0,0,0).and_utc()`), manual entry (`new Date("YYYY-MM-DD")`, which
JavaScript parses as UTC), MCP tools and the snapshot writer (one row per UTC
day).

Readers were inconsistent. The budgeting page computed period boundaries with
local-time constructors (`new Date(y, m + 1, 0)`), so for a user in UTC+2 the
September range ended at `2026-09-29T22:00Z` and a transaction booked on
30 September (`2026-09-30T00:00Z`) belonged to no month, quarter or year at
all; in UTC− zones the first day fell out instead. Chart markers bucketed transactions by local day while
history rows were keyed by UTC day.

Alternatives considered: storing dates as `YYYY-MM-DD` TEXT (a schema
migration across six tables and every writer, with no gain for the readers),
or comparing against `< next_period_start` on the backend only (fixes one
report but leaves every other reader free to repeat the bug).

## Decision

- **A calendar day is the UTC-midnight epoch of that day.** Writers keep
  producing it; nothing is migrated.
- **Period boundaries are computed on the UTC calendar.** The shared helper
  `src/utils/period.ts` (`getUtcPeriodRange`, `utcDayStart`, `utcDayEnd`,
  `isoDateFromUtcTimestamp`) is the only place that turns a timeframe/offset
  or an ISO date into epoch bounds. `end` is inclusive — the last second of
  the last day — matching the backend's `booking_date <= end` filters.
- **Labels and day buckets use UTC getters** (`getUTCMonth`,
  `toLocaleDateString(locale, { timeZone: 'UTC' })`,
  `Math.floor(ts / 86400) * 86400`), never local ones.

## Consequences

- Any new period picker, filter preset or day bucket must use
  `src/utils/period.ts`; a local-time `new Date(y, m, d)` for a stored date
  is a bug.
- Displaying a stored day must also format in UTC; otherwise a UTC− user sees
  the previous day. The locale-aware formatter planned for the i18n stage
  takes `timeZone: 'UTC'` for day-granular values.
- Backend period filters stay inclusive on both ends; services document the
  contract with tests (`services/budgeting.rs`).
- Users see "today" change at UTC midnight, not local midnight. Accepted: the
  app has no intraday data, and the alternative (local days) is not
  reproducible across machines in different zones sharing one database
  backup.
