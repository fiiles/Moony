# 0016. History days derive from dated facts; manual values hold

Date: 2026-10-13

## Status

Accepted. Nothing is superseded formally: the old rule (static classes of a past day carried from
the nearest live row) was written down only in `docs/architecture/database.md`, which now states
the new rule.

## Context

On a fresh profile imported stock transactions drew a single point, a bank balance recorded a year
back changed nothing, and crypto history needed a manual recalculation after a CoinGecko key was
entered. A rebuild only updated rows that already existed, and a fresh profile has only today's
live row. The startup backfill never extended history backwards and valued past days with today's
holdings. Savings, real estate, loans and bonds were copied from the nearest live row, so no
balance record, valuation or loan event reached the past. Most writes had no history trigger, and
the ones that did ran inside the command and swallowed their errors. The alternative, a trigger per
write on top of carried statics, leaves every past day dependent on a row that may not exist.

## Decision

- **History is derived from dated facts.** A class on day D comes from the facts dated on or
  before D, by the rule the domain's own page uses, in one Rust function per class
  (`services/history_days.rs`). Today's snapshot, the MCP tools and, through a TypeScript mirror
  pinned by a shared fixture, the loans page use the same rules.
- **A manual value holds.** A bank balance record holds until the next record. A manual loan balance
  holds until another is entered or all are removed; nothing is estimated over it. An extra payment
  in manual mode lowers it, a rate change only changes the plan, and switching the loan form back to
  the estimate deletes every manual record. "Zadat zůstatek k datu" is the one concept (stored as
  `balance_check` events); the separate balance check is gone.
- **Class rules.** An account counts nothing before its first record, and carries its current
  balance when it has none. A loan follows the estimate until its first manual balance. A property
  counts from its purchase date at its newest valuation, and not at all without one. An other asset
  is the quantity its transactions leave times the valuation of the day; one with a quantity and no
  transactions (an MCP-created asset) carries today's quantity times its price like an undated
  account. Bonds have no date and carry today's value. Stocks and crypto come from the ticker histories.
- **Facts win over live rows, with a guard.** A backfill or new row is written whole. A live row is
  rewritten only for a class whose every item the facts cover that day; undated accounts, undated other
  assets, bonds and the estimated past of an overridden loan are not covered. History starts at the earliest dated
  fact: backfill rows before it are deleted, live rows never.
- **One rebuild, once.** The first start of this version rebuilds every day, and every ticker from
  its first transaction; `app_config.history_rebuild_version` is stored once the days rebuild
  succeeded. A ticker the provider keeps rejecting is reported `failed`, logged and retried by later
  writes and the startup gap fill, so it does not rerun the rebuild at every start.
- **One queue for every dated write.** A write queues `StockTicker`, `CryptoTicker` or `Days` jobs
  (earliest affected day, coalesced per key) and returns. One worker fetches quotes, then re-derives
  the days in 60-day chunks. UI commands and the MCP follow-up share one entry point.

## Consequences

- A new write path or asset class must say which days it changes and queue a job; a new class needs
  a rule in `history_days.rs` or it is not in history.
- Dialogs return at once. A failure is logged and the ticker is reported `failed` in the
  `history-recalculation` event; charts refetch on `recalculation-complete`.
- A balance recorded a year back draws a year. A loan with manual balances stops amortizing after
  the newest one, so today's debt can be higher than before: a visible change for the release notes.
- A live row keeps a recorded class value the facts do not cover, which may be stale.
- Known limits: bonds are carried; CoinGecko's free and demo tiers serve 365 days, older crypto days
  keep the last known price; a legacy rate change without `previous_*` uses the current rate before
  it; the account page interpolates between records, history steps. A valuation dated in the future
  counts today but not in history. Other-asset valuations in a foreign currency are mislabelled in
  today's value; a follow-up.
