# Settle finished days to closing prices

Date: 2026-11-17. Status: approved by the owner (2026-11-17). Small change, short design.

## Why

Fetched prices and the history tables are not synced between devices (device sync spec D5):
each device fetches prices itself and derives history from the synced dated facts (ADR 0016).
Stocks and crypto on a past day are `quantity × price of the day` from the ticker histories.

A day the device was running keeps the price of its last refresh on that day, which is rarely
the closing price (a refresh in the morning records a price from the middle of the session).
A day the device was not running is filled later with Yahoo's daily close. So the desktop and
the phone can show different net worth for the same past day, and nothing repairs it until a
backdated write happens to rebuild that ticker.

The owner wants the past days of the history to read the same on every device.

## Goal

Once a day is over (UTC), every device values stocks and crypto on it at Yahoo's daily close,
whether or not the device was running that day.

## Non-goals

- Today. Each device shows the prices it fetched last; they agree after both refresh.
- Items no fact dates (an account without balance records, a bond without a purchase date, an
  other asset without transactions). Their past value follows the device's own live rows
  (ADR 0016, device sync D5). Dating the item removes the difference.
- A ticker Yahoo does not know keeps the value the device recorded.
- A ticker with a manual price that Yahoo knows gets Yahoo's close on past days, as every
  ticker rebuild does today. Not changed here.
- An app left running over midnight without a lock settles at its next unlock.

## Anti-goals

- Syncing history or fetched prices.
- A provider request at every unlock on a normal day: the settle runs once per finished day.

## Approaches

1. **A settled-through mark per device (chosen).** A Local `app_config` key holds the last
   day whose history is settled. The start after unlock queues the existing ticker and days
   rebuild from the day after the mark. No migration, no new fetch path: the gap repair already
   does exactly this rebuild when a day has no row.
2. **A "provisional" flag on every row written live.** Same result, but a migration of two
   Seed tables and a second way to find work at startup.
3. **Sync the history or the prices.** Against D5 (two devices writing the same day conflict,
   double traffic), and the days would still hold mid-session prices.

## Design

- **The mark.** `app_config.history_settled_through` = the UTC day start (unix seconds) of the
  last settled day. Local: it is not in `APP_CONFIG_SYNCED_KEYS`, so it never leaves the device
  and is not part of a sync snapshot. Absent or unreadable means nothing is settled.
- **Planning (`history_plan::plan_startup`).** With the mark older than yesterday the plan
  settles through yesterday: the settle starts at the day after the mark, or at the first fact
  when there is no mark, and never before the first fact. The plan starts at the earlier of
  the first missing day and the settle start: the held tickers from that day and `Days` from
  it, as the gap repair queues today. `StartupPlan.settles_through` carries yesterday when the
  mark has to move. The one-time rebuild of a version covers every day, so it settles too.
- **Storing the mark (`start_snapshot_backfill`).** The batch gets a completion. The mark is
  stored when the days were re-derived and the tickers did not all fail:
  `settle_earned(outcome, ticker_jobs) = days_ok && (ticker_jobs == 0 || failed < ticker_jobs)`.
  All tickers failing is how being offline looks: the mark stays and the next start tries
  again. Some tickers failing is a ticker the provider rejects for good (a delisted symbol):
  it must not repeat the fetch of every ticker at every unlock, so the mark moves.
  A batch with a completion fetches strictly (existing rule), so a provider failure leaves
  the rows alone instead of writing a last known price.
- **First start with this version.** No mark, so the whole history is settled once (every
  ticker held since the first fact, one request per ticker), which also brings the days
  recorded before this change to closing prices.
- **A profile without facts** has nothing to queue; the completion runs at once and stores the
  mark, so the first fact added later does not trigger a full settle.
- **Replacing by a sync snapshot** (`sync::snapshot` replace, a joining device) swaps the Seed
  tables for another device's rows, so the mark is deleted there and the next start settles
  again.
- **No UI, no strings, no migration, no wire type.** The header badge and the chart refresh
  follow the existing `history-recalculation` and `recalculation-complete` events.

## What changes for the person

Yesterday's value can move slightly at the first start of the next day, from the last price
the device fetched to the closing price. After both devices started once on the new day, their
past days read the same for stocks and crypto.

## Known limits (accepted)

- A ticker that failed while others succeeded (a rate limit hitting part of the list) stays
  unsettled for those days until a later rebuild of that ticker.
- A profile whose every held ticker is rejected by the provider never earns the mark and asks
  the provider again at every unlock.
- Two unlocks in quick succession can queue the same settle twice (the queue coalesces what is
  still pending).

## Left to the builder

Names of the helpers, the wording of the log lines, where the tests sit in the module.
