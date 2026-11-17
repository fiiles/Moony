# 0033. Finished days settle to closing prices

Date: 2026-11-17

## Status

Accepted. Adds a rule to [ADR 0016](0016-history-from-dated-facts.md) (history days derive from
dated facts) and narrows the known cost of "history and fetched data are not synced"
([ADR 0024](0024-device-sync-capture-clocks-merge.md), device sync spec D5). Neither is superseded.

## Context

Fetched prices and the history tables are Seed data: every device fetches prices itself and
derives its history from the synced dated facts. Stocks and crypto on a past day are the quantity
the transactions leave times the day's price from the ticker histories.

Today's row is written by the running app with the price it fetched last. Once the day was over
nothing touched that row again, so a day the device was running kept a price from the middle of
the session, while a day it was not running was filled later with Yahoo's daily close. Two devices
of one person therefore showed different net worth for the same past day. A ticker rebuild did
replace such a price with the close, but only when a backdated write happened to queue one.

## Decision

- **A finished day is valued at the daily close.** Once a UTC day is over, the start after unlock
  re-derives stocks and crypto on it from Yahoo's daily closes, whether or not the device was
  running that day. It is the rebuild the gap repair already runs (held tickers, then the days),
  started one day earlier: no new fetch path and no migration.
- **A settled-through mark per device.** `app_config.history_settled_through` holds the UTC day
  start of the last settled day. It is Local (not in `APP_CONFIG_SYNCED_KEYS`): it describes this
  device's Seed tables, so it never syncs and is not part of a sync snapshot. Absent or unreadable
  means nothing is settled, and the first start settles the whole history once (every ticker held
  since the first fact, one request per ticker).
- **Planning** (`history_plan::plan_startup`): with the mark older than yesterday, the plan starts
  at the earlier of the first missing day and the day after the mark (the first fact without a
  mark). The one-time rebuild of a version covers every day and settles too.
- **Earning the mark** (`history_plan::settle_earned`): the batch's completion stores yesterday
  when the days were re-derived and the tickers did not all fail. Every ticker failing is how
  being offline looks, so the mark stays and the next start tries again. Some failing is a ticker
  the provider rejects for good, which must not repeat every ticker's fetch at every unlock, so the
  mark moves. A batch with a completion fetches strictly, so a provider failure leaves the rows
  alone.
- **A replace by a sync snapshot forgets the mark.** The Seed tables then hold another device's
  rows, whose finished days this device has not settled.

## Consequences

- After both devices started once on a new day, their past days read the same for stocks and
  crypto. Yesterday's value can move slightly at that start, from the last fetched price to the
  close.
- Today still differs between devices until both refreshed their prices.
- Not covered, as before: items no fact dates follow the device's own live rows (ADR 0016); a
  ticker Yahoo does not know keeps what the device recorded; a ticker with a manual price that
  Yahoo knows gets Yahoo's close on past days, as in every ticker rebuild.
- Known limits, accepted:
  - a ticker that failed while others succeeded stays unsettled for those days until a later
    rebuild of that ticker;
  - a profile whose every held ticker the provider rejects never earns the mark and asks again at
    every unlock;
  - an app left running over midnight without a lock settles at its next unlock;
  - two unlocks in quick succession can queue the same settle twice.
- A change that replaces the Seed history tables from outside (a new restore path) must forget
  the mark.
