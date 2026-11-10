# Device sync, Moony side — design

Date: 2026-11-06. Status: **approved for implementation** on 2026-11-06 ("implement it"), with
pricing, limits, tiers and attachments left for later (§16a). Two independent reviews
on 2026-11-06 (a builder check and an adversarial convergence/crypto review) are folded in; the
biggest change they caused is §7.3 (same ID instead of after-the-fact merges) and D22 (join only
from the synced data). The server half
exists: `../Moony-relay` (commit 25d1f23, not deployed) and its protocol in
`Moony-relay/DESIGN.md`. This spec is the Moony client half, and lists the few relay additions it
needs (§12). It is written as the hand-off brief for whoever implements it: an implementer should
not have to go back to the owner except for the questions in §16.

Decisions are numbered D1–D27. The ones marked *(my call)* were made by Claude while writing the
spec and are listed again in §16 so the owner can check them.

## 1. Why

Moony keeps one encrypted profile per device. The phone app (2026-10-16 spec, §11) and the
desktop share the schema, and today the only way to move data between them is a backup archive
(backup = transfer). People who use both see two diverging copies.

Paid sync is the planned revenue line. Moony's whole pitch is privacy (no cloud, no telemetry),
so sync has to keep that promise: the operator of the relay — Filip — must not be able to read a
single amount, name or note. That rules out the usual "upload the database to our server" design
and drives everything below.

## 2. Goals, non-goals, anti-goals

**Goals**

- G1. Changes on one device appear on the person's other devices (desktop and phone, any mix,
  up to 5 vaults per account and any number of devices per vault) within seconds when online,
  and after reconnecting when offline. Offline edits on two devices merge without losing either
  edit unless both changed the same field (then the later one wins).
- G2. End-to-end encryption. The relay sees opaque ciphertext, sizes and timing (§13). Keys
  never leave the person's devices except through pairing and the Recovery Kit, both under the
  person's control.
- G3. Integrity. A relay that drops, reorders, replays or alters data is detected; it can at most
  delay or withhold updates (§13).
- G4. No change to how the app is written. The ~900 write statements in `services/` stay as
  they are; sync captures changes below them (D4). UI and MCP writes sync alike.
- G5. Zero cost for people who don't sync: no triggers, no journal growth, no network calls.
- G6. A new device is usable within a minute: the snapshot carries the caches and history, not
  just the facts.
- G7. A person who loses every device can restore everything with the Recovery Kit.

**Non-goals (v1)**

- Shared vaults between different people (households with separate logins). One vault = one
  person's profile on their own devices.
- Web access, a server-side API, server-side search or backups. The relay is storage and
  forwarding only.
- Real-time collaborative editing of the same field (character-level merge of notes).
- Syncing derived data: history snapshots, prices, exchange rates, caches (§5).
- Store purchases on the phone (Apple 3.1.3(b) question still open, §16).
- Push notifications (WebSocket wake-up). Clients poll (§9.4).

**Anti-goals (a failure even if it "works")**

- Any plaintext financial data, file name, category name or device name reaching the relay or
  Polar.
- Silent data loss: a merge that drops a row or an edit without it being the documented
  last-writer-wins outcome.
- A sync problem that locks a person out of their local data. Local data always opens and works
  with sync broken, paused, lapsed or switched off.
- Telemetry of any kind, including "anonymous" sync statistics.

## 3. Shape of the solution

```
 device A                                  relay (Cloudflare)                device B
 ┌────────────────────────────┐            ┌────────────────────┐            ┌──────────────┐
 │ services/* write SQL       │            │ VaultDO: ops log   │            │              │
 │   │ TEMP triggers (capture)│  push      │  (ciphertext only) │   pull     │ apply: merge │
 │   ▼                        │──────────▶ │ R2: snapshots,     │──────────▶ │ by clocks,   │
 │ sync_clock + sync_outbox   │  envelopes │      blobs         │  envelopes │ then refresh │
 │   │ engine: read rows,     │            └────────────────────┘            │ derived data │
 │   │ encrypt, push          │                                              └──────────────┘
 └────────────────────────────┘
```

- Every synced row change is captured by SQLite triggers into a local journal (D4).
- The sync engine turns the journal into **changesets**, encrypts each into an **envelope**
  (one relay "op") and pushes it. It pulls other devices' envelopes in relay order, decrypts,
  verifies and merges them by per-column clocks (D9–D14).
- After merging, derived data is recomputed by the same code the app already uses: mirrors,
  history, invariant repair (§7).
- A new device starts from an encrypted **snapshot** and then pulls envelopes (§8).
- Attachments travel separately as encrypted **blobs** (§10).

## 4. Owner-level decisions this spec makes

| # | Decision |
|---|---|
| D1 | Sync is a paid add-on on top of the free app; the free app is unchanged. Turning sync on is opt-in per profile in Settings → Sync. |
| D2 | One profile = one vault. Each device keeps its own password, its own master key and its own local recovery key; the vault key is a separate secret shared by the devices of that vault. A password change on one device does not touch the others. |
| D3 | The server is Moony-relay as designed (Cloudflare, EU, Polar, 16-digit account numbers, no email). |

## 5. What syncs

### 5.1 Table classes

Every table of the main schema belongs to exactly one class. The class list lives in one Rust
registry (`services/sync/registry.rs`, D6). A test fails when a table of the migrated schema is
missing from the registry, so a new migration cannot add a table without deciding.

| Class | Meaning | Tables |
|---|---|---|
| **Synced** | Captured, pushed, merged | `bank_accounts`, `bank_account_zones`, `bank_balance_records`, `bank_transactions`, `csv_import_batches`, `institutions`, `transaction_categories`, `categorization_rules`, `learned_payees`, `budget_goal_versions`, `spending_plans`, `stock_investments`, `investment_transactions`, `investment_dividends`, `stock_import_batches`, `stock_splits`, `watched_stocks`, `stock_price_overrides`, `dividend_overrides`, `stock_tags`, `stock_tag_groups`, `stock_investment_tags`, `crypto_investments`, `crypto_transactions`, `crypto_price_overrides`, `bonds`, `loans`, `loan_events`, `real_estate`, `real_estate_one_time_costs`, `real_estate_valuations`, `real_estate_loans`, `real_estate_insurances`, `real_estate_photo_batches`, `real_estate_photos`, `real_estate_documents`, `insurance_policies`, `insurance_documents`, `other_assets`, `other_asset_transactions`, `other_asset_valuations`, `cashflow_items`, `cashflow_plan_exclusions`, `projection_settings`, `milestone_states`, plus the synced parts of `user_profile` and `app_config` (§5.2) and the new `sync_devices` (§11) |
| **Seed** | Not merged; copied once into a new device's snapshot so it starts warm, then maintained locally | `portfolio_metrics_history`, `stock_value_history`, `crypto_value_history`, `exchange_rates`, `exchange_rate_history`, `stock_data`, `crypto_prices`, `dividend_data`, `stock_dividend_events`, `stock_dividend_coverage` |
| **Derived** | Not captured; rebuilt locally from synced rows after every merge | `budget_goals`: whether a row exists and its amount follow `budget_goal_versions` and today's month (the inline logic in `budgeting.rs`), and no table references it (checked 2026-11-06) |
| **Local** | Never leaves the device | `_migrations`, `attachment_leftovers`, `loan_legacy_anchors`, `mcp_pending_refresh`, and the sync tables of §6.2 |

D5 *(my call)*: **history and fetched data are not synced.** Every device fetches prices and
rates itself and rebuilds history from the synced dated facts (ADR 0016), exactly as it does
today after an import. Syncing them would double the traffic for data any device can recompute,
and two devices taking a live snapshot on the same day would conflict for nothing. The known
cost: `portfolio_metrics_history` live rows of a day only one device saw stay on that device, so
the "undated remainder" rule (ADR 0016) can differ slightly between devices for undated items.
The snapshot narrows that gap for new devices.

### 5.2 Columns that do not sync, and columns that are recomputed (superseded, see §20)

Some synced tables carry columns that are device-local or derived. The registry gives each such
column one of two classes:

- **Local**: not captured, never written by a merge.
- **Recomputed**: written by the app's own recalculation from other synced rows. Not captured
  either; after a merge the engine reruns the recalculation (§7.1), so every device derives the
  same value from the same merged facts. Where the recalculation leaves a typed value alone (a
  position without transactions keeps the quantity typed in), that typed value has to sync: the
  registry marks those columns **recomputed-or-typed**. Capture records them only when the row
  has no source rows (no transactions), and the convergence tests compare them on every row.

| Table | Column | Class | Why |
|---|---|---|---|
| `user_profile` | `id`, `language`, `theme`, `menu_preferences`, `mcp_server_enabled`, `mcp_server_token`, `mcp_server_port`, `created_at` | Local | D7 *(my call)*: language, theme and menu are per-device preferences (the phone may follow the system language, the desktop not); MCP is desktop-only and its token is a local secret. `id` is an autoincrement integer: the table syncs as a singleton keyed `"profile"`. |
| `bank_accounts` | `balance`, `balance_as_of` | Recomputed | Mirrors of the newest `bank_balance_records` row (`refresh_balance_columns`). |
| `loans` | `balance_anchor_amount`, `balance_anchor_date` | Recomputed | Mirror of the newest `balance_check` event (`refresh_anchor_mirror`). |
| `real_estate` | `market_price`, `market_price_currency` | Recomputed | Mirror of the newest valuation (`valuations::sync_parent_price` writes both). |
| `other_assets` | `market_price` | Recomputed | Mirror of the newest valuation. |
| `stock_investments` | `quantity` | Recomputed-or-typed | `investments::recalculate_investment_metrics` (also run at every database open). |
| `crypto_investments` | `quantity`, `currency` | Recomputed-or-typed | Derived from the transactions when there are any. |
| `other_assets` | `quantity`, `average_purchase_price` | Recomputed-or-typed | Derived from the transactions when there are any. The average uses the device's own FX history, so two devices can differ in the last digits until their rate histories match; the convergence tests compare it with a tolerance. |
| `loans` | `interest_rate`, `monthly_payment` | Recomputed-or-typed | The current terms follow the newest `rate_change` event when there is one (`loan_events`, also rewritten by `reconcile_legacy_anchors`). |
| `loan_events` | `previous_rate`, `previous_monthly_payment` | Recomputed | The chain of terms before each event, recomputed from the merged events, so two rate changes added on two devices still form one consistent chain. |
| the synced tables above | `updated_at` | Recomputed | The recalculations stamp `updated_at` with the current time. On the rows they touch, `updated_at` is a local "last recalculated" time, not synced; on other synced tables it syncs normally. The plan lists the exact columns from the recalculation code. |
| `real_estate_photos`, `real_estate_documents`, `insurance_documents` | `file_path`, `thumbnail_path` | Local | Legacy plain-file paths are local by nature; a row that still has one syncs once its content is converted (§10). |

The plan makes the private refresh functions (`bank_accounts::refresh_balance_columns`,
`valuations::sync_parent_price`) callable by the engine and extracts the inline budget-goal
logic of `budgeting.rs` into one function, so a merge reruns exactly the code a local write runs.

`app_config` syncs only an allowlist of keys (D8): `stock_import_formats`,
`learned_csv_formats`, `milestones.mutedKinds`, `rule_packs_enabled`, `rule_packs_disabled_rules`.
Every other key — `fx_reference`, `history_rebuild_version`, `onboarding.*`,
`backup.lastCreatedAt` (each device has its own backups), notification preferences, anything
new — is local unless added to the list. A local default is the safe one: a key that should have
synced is a small bug, a secret that syncs is not.

Rule packs need one more step *(my call)*: when unset, the default pack follows
`user_profile.language`, which is per device (D7), so a Czech desktop and an English phone would
categorise the same statement differently. Turning sync on (and joining) therefore writes the
device's effective rule-pack setting into the two keys explicitly, so categorisation is the same
everywhere from then on.

## 6. Capturing changes

### 6.1 Triggers, created per connection (superseded, see §20)

D4: **changes are captured by SQLite triggers, not by the services.** Moony writes through ~900
SQL statements across the services, MCP and imports; wrapping each would be a rewrite and would
miss the next one written. Triggers catch every path, cascades included.

D6: the triggers are **`TEMP` triggers generated at unlock from the registry**, only when sync is
enabled for the profile. `CREATE TEMP TRIGGER … ON main.<table>` lives on the connection, not in
the schema, so:

- migrations run before the triggers exist and never produce sync traffic (§14 handles their
  effect on other devices);
- `schema.snapshot` and the migration list do not change when the trigger shape changes;
- a profile without sync has no triggers at all (G5);
- each build regenerates them from `PRAGMA table_info` of the migrated schema plus the registry,
  so they always match the columns that exist.

For each synced table the generator writes three triggers:

- `AFTER INSERT`: record a row clock (`col = '*'`) and an outbox entry.
- `AFTER UPDATE`: for each synced column, `WHEN OLD.c IS NOT NEW.c`, record a column clock; then
  one outbox entry. A change of the primary key is recorded as a delete of the old key and an
  insert of the new one.
- `AFTER DELETE`: record a tombstone and an outbox entry.

Clocks come from a Rust scalar function registered on the connection (`sync_hlc()`, §6.3). A
per-connection mode tells the triggers what to do:

| Mode | When | Clocks and tombstones | Outbox |
|---|---|---|---|
| `local` | every ordinary write | new clock from `sync_hlc()` | yes |
| `merge` | the engine applies remote changes (§7) | the merge writes clocks itself; rows deleted by an FK cascade get a tombstone with the **parent's** tombstone clock, so a later edit of the cascaded child is dropped on every device (D13) | no |
| `repair` | recalculations and repairs after a merge (§7.1, §7.4) | none | no |

The triggers are installed right after the migrations run in `Database::open_with_key`, before
the self-heals that run at every unlock (`recalculate_all_investment_metrics`,
`reconcile_legacy_anchors`), so synced columns those write are captured too.

Facts the plan's first task must verify with a probe, as the attachments work did for
SQLCipher: TEMP triggers on `main` tables fire inside cross-schema transactions; FK `ON DELETE
CASCADE` and `SET NULL` actions fire the child table's triggers; `INSERT … ON CONFLICT DO
UPDATE` fires the UPDATE trigger; triggers survive `ATTACH` of `files`. No service uses `INSERT
OR REPLACE` today (checked 2026-11-06); a lint test keeps it that way for synced tables, because
REPLACE deletes without firing DELETE triggers unless `recursive_triggers` is on.

### 6.2 Local sync tables (migration 029) (superseded, see §20)

One append-only migration adds the sync bookkeeping below (all of it **Local**), the synced
`sync_devices` table (§11.4) and the `split_basis` columns (D27). It is additive (§14).

| Table | Purpose |
|---|---|
| `sync_state` | Single row: enabled flag, relay URL, account number, vault ID, the current vault key epoch and the vault keys (§11.1), this device's ID, its next envelope sequence and last envelope hash, the relay cursor (`head` applied), the snapshot sequence restored from, the schema barrier (§14), last error and last success times. |
| `sync_clock` | `(tbl, pk, col, hlc, device)`, PK `(tbl, pk, col)`. `col = '*'` is the row clock (insert time, which is also the row's *generation*, §7.1); `col = '-'` is a tombstone. Column clocks exist only for columns changed after the insert (D10), so the table grows with edits, not with columns × rows. A synced row with no entry at all (rows that existed before sync was turned on, migration seeds) has clock 0. |
| `sync_outbox` | `(tbl, pk)` PK, `queued_at`. Which rows changed locally since the last push. Values are **not** stored here (D11). Re-queuing a row keeps its first `queued_at`. |
| `sync_heads` | `(device, seq, hash)`: the newest envelope applied from every device, for gap and replay detection (§9.3). |
| `sync_own_log` | `(seq, hash, max_hlc)` of every envelope this device sealed since the newest snapshot: lets the device recognise its own envelopes on pull and notice a clone or rollback (§9.3). |
| `sync_pending_blobs` | Blob IDs referenced by merged rows whose content is not local yet (§10). |
| `sync_unsent` | Sealed envelopes built but not yet acknowledged by the relay, in sequence order (§9.4). |

The vault keys sit inside `moony.db`, so they are encrypted at rest by the device password like
everything else and are never written to a separate file.

### 6.3 Hybrid logical clock (superseded, see §20)

D9: clocks are **hybrid logical clocks** (HLC): 48 bits of wall-clock milliseconds and a 16-bit
counter, packed into one INTEGER, compared as integers, ties broken by device ID (16 random
bytes per device install, compared as bytes). Sending advances the local clock; receiving moves
it to at least the highest clock seen. A remote clock more than 24 hours ahead of local time is
still applied (refusing would split the devices) but is logged as a warning and shown in Sync
diagnostics, because a device with a wrong clock wins every conflict until time catches up.

### 6.4 Why the outbox holds no values

D11: the outbox records *which* rows changed; the engine reads the current values when it builds
a changeset. Five edits of one note before the next push send one value. A row that was changed
and then deleted sends only its tombstone. And after a migration rewrites stored values, pending
changes go out in the new format, with no conversion of queued data (§14).

## 7. Merging remote changes

### 7.1 Rules (superseded, see §20)

Changes from other devices are applied in relay order, one changeset at a time, each in one SQL
transaction with `PRAGMA defer_foreign_keys = ON` and the triggers in `merge` mode.

- D12 **Per-column last-writer-wins.** For each column in an incoming row change, the value is
  written when its clock is newer than the local clock of that column (the column clock if one
  exists, else the row clock). The local clock is then set to the incoming one. Clocks compare as
  `(hlc, device ID)`; every clock on the wire travels with its device (§9.1).
  - **Column groups.** Some columns only make sense together and merge as one unit: the group's
    clock is the newest of its members, and the whole group is taken from the winning side.
    Capture sends every member when any one changes. Groups in v1: `quantity` + `price_per_unit`
    + `split_basis` of `investment_transactions` (and the matching price/target/amount columns +
    `split_basis` of the three split side tables, D27); every amount + its `currency` column. The
    registry declares them.
  - **Monotonic columns.** `stock_splits.status` doesn't merge by clock: `applied` and `dismissed`
    beat `pending` whatever the clocks; between `applied` and `dismissed` the newer clock wins.
    Otherwise a second device detecting the same split would put it back to `pending`, and
    applying it again would scale the shares twice.
- **Generations.** Every row change carries the row's generation (its `'*'` clock). A change
  addressed to an older generation than the local row is dropped. This matters for keys that are
  removed and added again (a tag assignment, a milestone state): an edit of the old row must not
  land on the new one on some devices and be dropped on others.
- D13 **A delete beats edits; only a newer whole row brings a row back.** A tombstone deletes
  the row when it is newer than the row's *insert* clock (`'*'`), whatever later column edits the
  row has had (FK cascades apply as usual). Comparing against the insert clock, not the newest
  column clock, is what makes devices agree: the deleting device drops a concurrent edit as a
  partial update to a missing row, so the editing device must drop the row too. An insert (a full
  row) newer than the tombstone re-creates the row. A partial update to a deleted row is dropped.
  UUID-keyed rows are never re-inserted by the app, so in practice they stay deleted; composite
  and natural keys (`stock_investment_tags`, `app_config`, `milestone_states`,
  `cashflow_plan_exclusions`, `budget_goals`…) can be removed and added again, and that works.
- D14 **A child whose parent is gone follows the foreign key's own `ON DELETE` rule**, so every
  device ends where the deleting device's cascade would have put it. The rule per foreign key comes
  from `PRAGMA foreign_key_list` into the registry:
  - `CASCADE`: the child is recorded as a tombstone (with the parent's tombstone clock) and not
    inserted. The device that added it loses it to the cascade when the delete arrives.
  - `SET NULL` (`bank_transactions.spending_plan_id`, `investment_transactions.import_batch_id`,
    `stock_tags.group_id`, `bank_balance_records.source_batch_id`…): the child is inserted with
    that column NULL.
  - `NO ACTION` (references to `transaction_categories` from transactions, rules, learned payees
    and budget versions): there is no cascade, and the app's own delete reassigns referencing rows
    first. The registry gives each such key an explicit orphan rule, applied both when a child
    arrives after its parent's tombstone and when a tombstone arrives while local children still
    point at the parent (before the delete, or the deferred FK check would fail the whole
    changeset at commit and sync would stop for good): set the column NULL when it is nullable
    (`bank_transactions.category_id` → uncategorised), otherwise drop the child (a rule or learned
    payee for a deleted category). These writes run in `repair` mode.

  A parent that is merely *missing* (never seen, not tombstoned) means an envelope gap or an
  ordering bug — the engine stops (§9.3) instead of guessing.

After each changeset the engine:

1. Re-runs the **recalculations** of every touched parent (§5.2): balance and loan-anchor
   mirrors, valuation mirrors, position quantities and averages, current loan terms, and rebuilds
   `budget_goals` from the versions. In `repair` mode, so these writes are not sent; every device
   makes them itself.
2. Runs the **invariant repairs** of the touched tables (§7.2, §7.4), also in `repair` mode. A
   repair must be a pure function of the merged rows — never of the order changes arrived in, of
   which device runs it, or of the time — so every device computes the same result without
   anything being sent.
3. Queues **history work** through the same durable queue MCP writes use
   (`mcp_refresh::enqueue` per domain with the earliest affected day; the table is renamed in
   spirit, not in the schema), so a crash between merge and recalculation loses nothing.
4. Queues missing blobs (§10).

Then it emits `sync-data-changed`, and the frontend runs the same ritual as for
`mcp-data-changed` (AGENTS.md rule 7: invalidate the domain keys, `portfolio-metrics`,
`cashflow-report`, record a snapshot, invalidate `portfolio-history`).

### 7.2 Changes whose meaning depends on the old value (superseded, see §20)

LWW is safe for "set the field to X". It is not, on its own, safe for operations that rewrite rows
relative to what is there. The ones in Moony today:

- **Stock splits** (apply, revert, dismiss in `services/stock_splits/`) rewrite the quantity and
  price of a ticker's transactions dated before the split, and rescale `stock_price_overrides`,
  `watched_stocks.target_price` and `dividend_overrides` chosen by comparing `updated_at` with the
  ex-date. Under plain LWW this breaks: a transaction added on device B before B saw A's apply is
  never rescaled, and an apply on A with a dismiss on B leaves the status and the units
  disagreeing. Today nothing records which splits a row already reflects, and a clock can't tell.

  D27 *(my call)*: migration 029 adds a nullable `split_basis` TEXT column (a JSON list of the
  split IDs whose ratio the row's values already include) to `investment_transactions`,
  `stock_price_overrides`, `watched_stocks` and `dividend_overrides`. Apply, revert and import
  maintain it. After a merge touches a ticker's splits or transactions, a deterministic repair
  makes every row match the merged `stock_splits.status`: rows missing an `applied` split are
  scaled and get its ID; rows carrying a split that is no longer `applied` are scaled back and lose
  it. Every device runs the same repair on the same merged rows, so it is not captured. The column
  is useful without sync too (it makes revert exact), which is why it is a column and not sync
  bookkeeping.
- **Budget versions**: `merge_equal_neighbours` deletes a version equal to its predecessor. It is
  a normalisation: running it after a merge on every device gives the same result, so it joins the
  repairs (§7.4) and its deletes are not captured when run there.
- **Recategorize history** ("Zařadit i další platby") and **category reassignment** set a column
  on many rows at once. That is plain LWW per row and converges.
- **Undo an import** deletes a batch and cascades to its rows. Tombstones; converges.

Any future feature that rewrites rows relative to their values has to name its sync behaviour in
its spec. This goes into `docs/standards/rust-backend.md`.

### 7.3 Unique keys: two devices create "the same" thing (superseded, see §20)

UUIDs never collide, but several tables also have a natural unique key: `stock_investments.ticker`,
`crypto_investments.ticker`, `watched_stocks.ticker`, the three override tables, `stock_tags.name`,
`stock_tag_groups.name`, `budget_goal_versions (category_id, timeframe, effective_month)`,
`projection_settings.asset_type`, `bank_balance_records (bank_account_id, as_of)`,
`stock_splits (ticker, effective_day)`, `bank_transactions (bank_account_id, transaction_id)`,
the `learned_payees` scope index, and the partial index on `cashflow_items.replacement_source_id`.
Buying AAPL on the phone and on the desktop while both are offline creates two positions with the
same ticker and different IDs; the merge then hits the UNIQUE constraint.

D15: **the same thing gets the same ID.** Merging two rows after the fact (pick a winner, repoint
the children, keep an alias for the loser) was the first draft; review showed it depends on the
order devices see things in — a tombstone of the loser deletes the winner on some devices and not
others, children of the loser arrive before the alias exists, repointed join rows collide on their
own keys — and a dishonest relay could steer that order. So the conflict is avoided instead of
resolved. The registry gives every unique index one of four rules:

| Rule | Indexes | What happens |
|---|---|---|
| **Same ID** — the key never changes after creation | `stock_investments.ticker`, `crypto_investments.ticker`, `watched_stocks.ticker`, the three override tables, `stock_splits (ticker, effective_day)`, `projection_settings.asset_type`, `bank_transactions (bank_account_id, transaction_id)` when `transaction_id` is set, the `learned_payees` scope index | The creating service derives the primary key from the natural key: UUIDv5 of a Moony namespace, the table name and the key. Two devices buying AAPL offline create the **same** row ID, so the merge is ordinary per-column LWW and the children of both point at the same parent. Rows created before this change keep their random IDs; that is safe because a second device only ever starts from the vault's data (D22), so it finds the existing row instead of creating one. |
| **Keep newest** — one row per key, the key can be edited | `bank_balance_records (bank_account_id, as_of)`, `budget_goal_versions (category_id, timeframe, effective_month)` | When an incoming insert or update would collide, the row with the newer clock stays and the other is deleted (`repair` mode). Neither table has children. Commutative, so order doesn't matter. |
| **Rename** — names people type and can change | `stock_tags.name`, `stock_tag_groups.name` (and spending-plan names, §7.4) | Both rows stay; among rows with the same name, every one but the lowest ID gets " (2)", " (3)"… appended. Applies to updates as well as inserts (a rename to a name another device just created). Children stay where they are. |
| **Unlink** — the index only forbids a second link | `cashflow_items.replacement_source_id` | Both items stay; among items linked to the same source, every one but the lowest ID loses the link (set to NULL). |

All four are repairs computed from the merged rows, run in `repair` mode on every device. To apply
an incoming row that would hit the constraint, the engine writes the outcome directly (it never
inserts first and fixes later). The creating services of the "same ID" tables change to use a
shared `natural_id()` helper; that change is useful without sync too and lands in phase 1. The
registry's test lists every UNIQUE index of the synced tables and fails on one without a rule, so a
new constraint gets a decision.

Known limit: two devices that import **the same statement without bank transaction IDs** while
offline will have duplicates, because the import's duplicate detection (`services/dedup.rs`) is a
fuzzy match, not a constraint. That is the same as importing the file twice today; the existing
"possible duplicates" review catches it. Not worth a merge rule in v1.

### 7.4 Rules the database doesn't enforce (superseded, see §20)

Some invariants live in services, not constraints, so a merge can break them:

| Invariant | Repair after merge |
|---|---|
| One tag per tag group per investment (`stock_tags.rs`) | keep the assignment with the newest clock |
| Spending plan name unique per year, case-insensitive | the "rename" rule of §7.3: every one but the lowest ID gets " (2)"… |
| One `balance_check` per loan per day | keep the newest clock |
| Budget versions: no version equal to its predecessor (`merge_equal_neighbours`) | run the normalisation |
| Stock split units match the split status (D27) | scale rows to match `split_basis` against the merged statuses |
| Migration seeds don't revive deleted rows (§14) | a seeded row whose key has a tombstone is deleted |
| At most 20 entries in `stock_import_formats` / `learned_csv_formats` | the app_config value is one JSON array, LWW as a whole (D8); no repair |

The plan audits the services for others (grep for checks that query before insert) and adds each
to the repair list with a test. The repairs are deterministic and are not captured.

## 8. Snapshots (superseded, see §20)

A snapshot is how a new device starts and how a device that was offline too long catches up.

- D16 **Content:** a logical dump, not a database file: every synced row with its clocks (each
  with its device), every tombstone, `sync_heads` (which envelopes it covers), the schema number
  and barrier (§14), every **older vault key epoch** encrypted under the current one (so the
  Recovery Kit, which holds only the current key, still opens blobs and envelopes written under
  earlier epochs; a removed device knew those keys anyway), and the seed tables as plain rows.
  Device-local data (the MCP token, local app_config keys, `sync_state`) is never in it. Format: a
  sequence of length-prefixed JSON records, deflate-compressed, then encrypted as a stream
  (§9.2).
- **The snapshot's sequence `S`** is the relay cursor the device has applied: it is taken after a
  pull that returned `more = false`, with `sync_unsent` empty, under the same lock as the dump. Not
  the head a push returned: envelopes of other devices that landed between pull and push would
  otherwise be skipped by everyone restoring the snapshot.
- **Restoring on a joining device replaces** the synced tables: their contents (migration seeds
  included) are deleted, then the snapshot is loaded. A fresh profile's seeds have no clocks and
  may differ from the vault (a seeded category deleted before sync existed left no tombstone), so
  merging them would bring back what the person deleted.
- **Restoring after `410 snapshot_required`** (a device that was offline too long) merges by the
  D12–D15 rules instead, because that device's rows came from the vault in the first place and
  carry vault clocks; its unsent changes stay in the outbox and are pushed afterwards.
- A device refuses a snapshot from below a barrier it has crossed (§14).
- **Who uploads:** any device, when more than 2,000 envelopes or 8 MiB have accumulated since the
  newest snapshot, at most once a day per device, with a random delay of up to an hour so two
  devices rarely both do it. Uploading is what lets the relay prune old envelopes (30 days and
  covered by a snapshot).
- Snapshot uploads count against the relay's 100 MiB per-request limit. A profile whose dump
  exceeds it (unlikely without attachments; ~1 KB per transaction gives ~100k transactions
  per 100 MB uncompressed, and deflate does better) fails the upload with a clear error; v1 does
  not split snapshots. §16 Q6.

## 9. The wire format

### 9.1 Changeset (plaintext) (superseded, see §20)

```json
{
  "v": 1,
  "schema": 29,
  "device": "<16 bytes, base64url>",
  "seq": 418,
  "prev": "<32-byte SHA-256 of this device's previous envelope, base64url>",
  "devs": ["<16 bytes, base64url>"],
  "rows": [
    { "t": "bank_transactions", "k": "c0a8…", "g": [1777000000000123, 0], "ins": true,
      "c": { "amount": ["-1250.00", 1777000000000123, 0], "currency": ["CZK", 1777000000000123, 0], … } },
    { "t": "stock_investment_tags", "k": ["inv-uuid", "tag-uuid"], "g": [1776000000000001, 0],
      "del": [1777000000000456, 0] }
  ]
}
```

- `k` is the primary key: a string, or an array for composite keys, or `"profile"` for the
  `user_profile` singleton.
- Every clock is `[hlc, i]`, where `i` indexes `devs`, so a tie between equal HLCs is broken by
  the device that made the change, not the one that sent it.
- `g` is the row's generation (§7.1).
- `c` maps a column to `[value, hlc, i]`. Values are SQLite values: JSON null, string, number
  (INTEGER as a JSON integer; the only REAL columns are seed tables, never sent as changes) or
  `{"b": "<base64>"}` for a BLOB.
- **Order.** Rows are cut from the outbox by first `queued_at`, then sorted so that a parent always
  travels in the same or an earlier envelope than its children (FK depth from the registry). A
  5,000-row import spread over many envelopes therefore never sends a transaction before its
  batch, which the receiver would read as a missing parent and stop.
- A changeset holds the changes of one or more local transactions, cut so that the envelope stays
  under 60 KiB (the relay limit is 64 KiB per op). A single row can be larger: `watched_stocks.notes`
  allows 100 KiB (`MAX_NOTES_BYTES` in `stock_monitor.rs`). A changeset that doesn't fit is
  uploaded as a blob (§10, same encryption) and the envelope carries only `{"blob": "<id>",
  "sha256": "…"}` in place of `rows`. Receivers fetch and verify it before applying. Nothing is
  ever truncated.

### 9.2 Envelope (what the relay stores)

D17: **XChaCha20-Poly1305 with keys derived by HKDF-SHA256**, no signatures *(my call)*.

```
envelope = "MNY1" | epoch (u32 BE) | nonce (24 random bytes) | ciphertext+tag
key      = HKDF-SHA256(vault_key[epoch], salt = vault_id, info = "moony sync ops v1")
AAD      = "MNY1" | epoch | vault_id
plaintext = deflate(changeset JSON), zero-padded to a multiple of 1 KiB
```

- Random 24-byte nonces are safe with XChaCha20 at any volume Moony will see.
- The op ID sent to the relay is `base64url(SHA-256(envelope))[0..32]`, so retries are
  idempotent and the relay can't tell the author.
- Padding to 1 KiB hides the exact size of small edits. Compressing before encrypting can leak
  through length when an attacker controls part of the input and can watch sizes; here the input
  is the person's own data and the observer is the relay, so the remaining leak is the rough size
  of a change, which the relay sees anyway.
- Snapshot and blob streams use the same cipher in the STREAM construction, 64 KiB chunks. Every
  stream gets **its own key**: a random 32-byte salt in the stream header, and
  `key = HKDF-SHA256(vault_key[epoch], salt = stream_salt, info = "moony sync snapshot v1" | "moony
  sync blob v1")`. The 24-byte nonce of a chunk is 19 random bytes from the header, a 4-byte chunk
  counter and a 1-byte last-chunk flag. AAD = `"MNY1"` | epoch | vault ID | object ID (the
  snapshot's `S`, or the blob ID), so the relay can't swap one blob or snapshot for another.
  Without the per-stream key, chunk 0 of every blob would share a key and nonce, which breaks both
  secrecy and authenticity.
- Crates: `chacha20poly1305`, `hkdf` (RustCrypto; `sha2` and `aes-gcm` are already from that
  family), `flate2`.

Why no signatures (Ed25519 per device), which an earlier research note suggested: every device
in the vault holds the vault key and can write anything, so a signature only distinguishes one
honest device of the same person from another. It doesn't stop the relay (which has no key) and
doesn't stop a stolen device (which has a key). Removing a device is done by key rotation
(§11.4), which signatures would not replace; it needs per-device *encryption* keys (X25519), not
signing keys. Leaving signatures out keeps the envelope and the snapshot free of per-device
verification state. If shared vaults between people ever come, signatures come with them.

### 9.3 Detecting a dishonest relay

The changeset carries the author device, its sequence number and the hash of its previous
envelope, all inside the AEAD. On every pull the engine checks each envelope:

- **From this device's own ID**: it must match `sync_own_log` (same seq, same hash). Then it is
  marked acknowledged (and dropped from `sync_unsent` if it was still there). An own-ID envelope
  the device never sealed means **this device is a clone or was rolled back** (an OS-level restore,
  Time Machine, Migration Assistant, a copied folder — all of which bypass `restore_backup`). The
  device re-joins automatically (§11.4) before pushing anything: pull happens before push in every
  round, so it notices before it can fork its own chain.
- **From another device**, against `sync_heads`:
  - `seq` is exactly the next one → apply, update the head.
  - `seq` below the head → a replay; skip. `seq` equal to the head with the same hash → skip.
  - `seq` equal to the head with a different hash, or a `prev` that doesn't match the head's
    hash → that device's chain forked (a clone the original hasn't noticed yet, or a lying
    relay). Only **that author** is quarantined: its further envelopes are held, everything else
    keeps syncing, and diagnostics name the device. The clone re-joins under a new ID on its own
    next pull, after which the quarantine is moot.
  - `seq` beyond the next one (a gap) → the relay dropped something. Stop; a later pull that fills
    the gap resumes.
- Decryption failure → stop. Never skip an envelope that fails to decrypt: skipping would make
  the devices diverge silently.

A stop never touches local data and never blocks the app; it stops sync until resolved.

Where the platform has one, a this-device-only keychain item holds a binding of the device ID
(iOS `ThisDeviceOnly`; the macOS data-protection keychain; Windows DPAPI): a database whose
`sync_state` device ID doesn't match it was copied or restored, and re-joins at unlock without
waiting for the relay.

What remains possible for the relay: withholding new envelopes (a device just looks out of date)
and deleting everything (the person still has every device's local data). Both are visible; both
are availability problems, not confidentiality or integrity ones. A `404` for the vault therefore
never tears sync down by itself (§11.6): it pauses sync and asks the person.

### 9.4 The engine loop

- Runs only while the profile is unlocked (the vault keys are inside the encrypted DB). On a
  phone that means no background sync; the app syncs on unlock and on returning to the
  foreground. That is a deliberate trade-off of the at-rest encryption.
- Triggers: unlock; a local write (debounced 5 s); every 5 minutes while unlocked and in the
  foreground; app foreground on phones; "Sync now".
- **Never holds the connection mutex across network I/O.** Each step reads or writes under the
  lock and releases it.
  - Push: in one transaction, read the outbox and the current values, build and seal the
    envelopes, store them in `sync_unsent` with their sequence numbers, advance the device's
    sequence and hash, and clear those outbox rows (a later edit of the same row re-adds it
    through the trigger) → release → push `sync_unsent` in order → lock → delete what the relay
    acknowledged. A crash anywhere re-pushes the same envelopes; the op ID makes that a no-op on
    the relay, and the device's chain never forks.
  - Pull: fetch a page → release → decrypt and verify → lock → merge one changeset per
    transaction.
- Back-off on errors: 30 s, doubling to 30 minutes; `402` (not paid) stops pushing and shows the
  subscription state, pulls continue while the relay allows them; `410 snapshot_required` runs
  §8's restore; `413 quota_exceeded` (ops) and `413 snapshot_too_large` stop pushing with an
  error.
- Order per round: pull until `more` is false, then push. Pulling first means a push never
  depends on stale state, and a device that is behind catches up before adding more.

## 10. Attachments

Photos, thumbnails and documents live as blobs in `attachments.db` (ADR 0017), up to 100 MB each.
They are too big for envelopes, so:

- D18 **Blob rows sync like any row; blob content travels through new relay endpoints** (§12)
  as an encrypted stream (§9.2) keyed by `blob_id`, which is already a random UUID.
- Upload: before sealing a changeset that inserts a row with a `blob_id`/`thumb_blob_id`, the
  engine uploads the blob. A blob that fails to upload leaves its row (and any row that references
  that row) in the outbox; everything else goes ahead. Other devices never see a row whose blob
  doesn't exist on the relay, and one stubborn PDF doesn't stop sync.
- Download: a merged row with a blob that isn't local goes into `sync_pending_blobs`. Thumbnails
  download right away; photos and documents download in the background on desktop, and on phones
  on Wi-Fi only or when opened *(my call)*. Until then the attachment shows a "Downloading" state
  instead of the current "missing" error. The unreferenced-blob sweep
  (`attachment_store::delete_unreferenced`) is unaffected: it deletes blobs without rows, not rows
  without blobs.
- Delete: a row tombstone deletes the local blob in the same transaction (as today); the engine
  also calls the relay's blob delete. The relay additionally deletes every blob of a vault when
  the vault is wiped.
- Integrity: `files.blobs.sha256` of the plaintext is part of the synced row (the plan adds it if
  the row doesn't carry it), so a downloaded blob is checked after decryption.
- §10.4: rows that still point at a legacy plain file (pre-0.13 profiles not yet converted) do not
  sync until the conversion gives them a blob; the conversion runs after unlock anyway.

## 11. Keys, accounts, pairing and recovery

### 11.1 Secrets

| Secret | Size | Where | Purpose |
|---|---|---|---|
| Account number | 16 digits | `sync_state`, Recovery Kit | Pays; manages vaults |
| Vault ID | 128 bits | `sync_state`, Recovery Kit | Routing |
| Vault key, per epoch | 256 bits | `sync_state` (all epochs still needed), Recovery Kit (current) | Everything encrypted |
| Vault token | 256 bits | derived: `HKDF(vault_key[epoch], vault_id, "moony sync token v1")` | Relay auth (the relay stores its SHA-256) |
| Device ID | 128 bits | `sync_state` | Clock tie-break, envelope chains |
| Device key pair | X25519 | private in `sync_state`, public in `sync_devices` | Receiving a new vault key when another device is removed (§11.4) |

D19: the vault key is **random**, not derived from a password, and the Recovery Kit carries it in
full. There is no password-derived key on the sync side at all, so nothing about sync can be
brute-forced from the relay's data, and a device password change (D2) needs no sync step.

### 11.2 Setting up sync on the first device (superseded, see §20)

Settings → Sync (desktop only in v1, D20):

1. **Create sync account** → `POST /v1/accounts` → shows the 16-digit number ("This number is your
   sync account. Anyone who has it can pay for it and see whether it is paid. It does not open
   your data.").
2. **Pay** → `POST /v1/accounts/me/checkout` → opens Polar's checkout in the system browser.
   Moony polls `GET /v1/accounts/me` every few seconds while the page is open and continues when
   `active` is true. Monthly or yearly.
3. **Save your Recovery Kit** (§11.5). Required before continuing: the person must tick that it is
   saved, after a print/save dialog.
4. Moony creates the vault (`PUT /v1/vaults/:id`), uploads the first snapshot (with seed tables),
   records the device in `sync_devices`, installs the triggers and starts the loop.

### 11.3 Adding a device: pairing (superseded, see §20)

D21: **pairing is a QR code shown on one device and scanned by another, a key exchange through a
short-lived mailbox on the relay (§12), and a six-digit code the person compares on both
screens.** The QR itself carries no secret, so someone who photographs it gets nothing, and
nobody can slip in their own credentials.

1. The device that shows the QR (S) generates an ephemeral X25519 key pair and a mailbox ID, and
   shows `moony-pair:1:<mailbox id>:<S public key>`.
2. The device that scans (C) generates its own ephemeral pair and writes its public key into the
   mailbox's first slot.
3. Both derive a shared key (X25519 → HKDF-SHA256 over both public keys and the mailbox ID) and
   both show a six-digit code derived from the same transcript.
4. The person confirms on **both** devices that the codes match. Only then does the device that
   holds the vault seal the credentials (relay URL, account number, vault ID, every vault key
   epoch) under the shared key and write them into the second slot. The joining device reads and
   decrypts them, then deletes the mailbox.

Which device shows and which scans follows the cameras: a new phone scans the QR on an existing
desktop or phone; a new desktop shows its QR and an existing phone scans it. Desktop to desktop,
or with no second device at hand, the joining device takes the recovery code from the Recovery
Kit (§11.5). Mailboxes live 10 minutes, each slot can be written once, and the whole mailbox is
read once.

The joining device then runs a fresh-profile setup with its own password (D2), restores the
snapshot (§8, replace), pulls, generates its device key pair, records itself (with its public
key) in `sync_devices`, and starts the loop.

D22 *(my call, after review)*: **in v1 a device joins only from the synced data.** A device that
already has a profile with data can join only by replacing its data with the vault's, after an
automatic local backup the person can restore from later; or it can be the first device of a new
vault. Merging two independently built profiles is where every hard case of §7 lives (rows with no
clocks on both sides, two hand-made sets of accounts and categories, random IDs for the same
ticker), and it adds little: the person picks the more complete profile and re-imports the rest.
§16 Q5 asks whether merging is worth adding later.

### 11.4 Device list, removing a device, identity clones (superseded, see §20)

- `sync_devices` is a **synced** table (it lives in the vault, encrypted, and is added by
  migration 029 like the local tables): device ID, X25519 public key, a name the person can edit
  (default "MacBook", "iPhone"), platform, app version, when it joined, and a removed flag. The
  relay never sees names. The public key is **write-once**: merge ignores any later change to it
  and diagnostics flag the attempt. "Last synced" in the device list is read from the envelopes,
  not stored in the row, so a push doesn't write a captured row and trigger another push.
- **Remove a device** (lost phone), D23. The removed device knows everything the others know
  (vault key, token, account number), so the new key has to reach the others through something it
  lacks: each device's own X25519 key pair, generated when it joins.
  1. The removal screen lists exactly which devices will receive the new key (name, platform, when
     it joined, last synced) and the person confirms. A device row planted by someone who had the
     phone before it was removed shows up here.
  2. The device doing the removal generates a new vault key (epoch n+1) and a **rekey bundle**:
     the new key sealed separately to the public key of every confirmed device, the removed device
     ID, and the epoch-n key encrypted under the new key (a back-link, so a Recovery Kit holding
     only the new key can still read a snapshot made under epoch n), all encrypted as a whole
     under epoch n.
  3. One relay call rotates the token and stores the bundle atomically
     (`POST /v1/vaults/:id/token {token, rekey}`, §12). From that moment the old token is
     **read-only** for 30 days and the relay records its head as the rotation point. No envelope
     of epoch n can be accepted after the rotation, because nothing holding only the old token can
     write. The call needs the *current* token, so two removals started at once can't both
     succeed: the second gets `403`, fetches the first one's bundle and starts again from the new
     epoch.
  4. Any other device notices on its next call: a push with the old token gets `403`, and a pull
     sees the rotation in `GET /v1/vaults/:id`. It pulls through the rotation point first, which
     acknowledges any of its own envelopes the relay had accepted (§9.3), then fetches the bundle
     (`GET …/rekey`), opens its sealed key, derives the new token, **re-seals only its still
     unacknowledged envelopes under epoch n+1** (same sequence numbers and contents) and pushes
     them. No change is lost and none is applied twice.
  5. The removing device uploads a fresh snapshot under epoch n+1 when the subscription allows
     writes; until then the others work from the existing snapshot and envelopes.
  6. Every device rejects envelopes of epoch n that sit after the rotation point, which can only
     happen if the relay misbehaves. A device that was offline for more than 30 days re-joins by
     pairing or with the Recovery Kit.

  The removed device keeps its own local data and what was synced before removal. Afterwards it
  can't decrypt anything new, can't write and can't delete the vault. It still knows the account
  number, so it can see whether the subscription is paid; that is accepted. The UI asks the person
  to save the new Recovery Kit.
- **Identity clones and rollbacks.** Restoring a backup, an OS-level restore, or copying the data
  folder creates a database whose device ID another copy also uses, or that is behind its own
  envelopes on the relay. `restore_backup` swaps files while the connection is closed and the
  restored database opens only with the archive's key, so it cannot edit `sync_state` itself: it
  writes a marker file next to the profile, and the next unlock sees it. The keychain binding and
  §9.3's own-log check catch the copies that bypass the app.

  **Re-join** is the same in every case, and needs no person: the device takes a new device ID and
  key pair, keeps its database and its outbox, and pulls from its cursor (or restores the newest
  snapshot by *merging*, §8, when the cursor is below the floor). That is safe because every row
  in such a database came from the vault and carries vault clocks; its old self's envelopes are
  applied like any other device's. Then it **re-queues every row whose clock carries its old
  device ID and is newer than the last own envelope it can confirm on the relay**, so edits it made
  after the copy or rollback are sent again under the new ID (re-sending an edit with its original
  clock is harmless). A backup restored from a profile that never synced, or from another vault,
  is not a re-join: it is a device with its own data (D22).
- **Exports.** The full JSON export skips only `_migrations`, `app_config`, `sqlite_sequence` and
  the `mcp_server_token` column today. It must also skip every sync table (`sync_state` holds the
  vault keys, the device's private key and the account number); a test asserts the export of a
  sync-enabled profile contains none of them.

### 11.5 Recovery Kit

D24: a **Sync Recovery Kit** is a one-page PDF (and the same as plain text) with the account
number, a QR, and a recovery code: vault ID (128 bits) and current vault key (256 bits) in
Crockford base32 — 77 characters plus a 3-character checksum, printed as 16 groups of five. Long,
but it is typed only when every device is lost, and the QR on the same page covers the usual case.
It is the only way back when every device is lost. It is shown at setup and after every key
rotation, and Settings → Sync can show it again (after the device password). Older epochs come
from the snapshot and the rekey back-links (§8, §11.4), so a Kit with only the current key still
opens every blob.

It is a second code next to Moony's existing local recovery key (which unlocks one device's
database). They do different jobs and must be visibly different. How to present the two together
is §16 Q3.

### 11.6 Turning sync off, subscription ends, deleting

- **Turn off on this device**: drops the triggers, deletes `sync_state`, the clocks and the
  outbox. Local data stays. Other devices keep syncing. Turning it on again later is a join
  (D22): the device either replaces its data with the vault's (after a backup) or starts a new
  vault. The confirmation says so before sync is turned off.
- **Delete synced data**: `DELETE /v1/vaults/:id` from any device, after the device password. That
  device then turns sync off. Other devices get `404`; because a relay could send that falsely,
  they pause and ask ("Your synced data is gone from the server. Turn sync off on this device?")
  instead of deleting their keys on their own. Local data on every device stays.
- **Subscription lapses**: pushes fail with `402`; devices keep working locally and keep a growing
  outbox; pulling works for 30 days; Settings → Sync shows "Sync paused — renew to continue" and
  the date the relay deletes the data (90 days after the end). Renewing resumes everything from
  the outbox.
- **Delete Account** (the app's existing full wipe) also deletes the vault when sync is on and
  the network is up, and says so in the confirmation.

## 12. Relay changes this needs

Small additions to `Moony-relay` (protocol v1.1), built before the client parts that use them:

| Endpoint | For |
|---|---|
| `PUT /v1/vaults/:id/blobs/:blobId/:part` (raw body, `Content-Length`, ≤ 32 MiB per part) and `POST /v1/vaults/:id/blobs/:blobId/complete` (part count, total size) | §10 upload. A 100 MiB document plus encryption overhead is over the Workers request limit, so blobs go up in parts; a blob exists only once completed, and incomplete parts older than a day are swept. Write entitlement; counts against a separate per-vault blob quota (D25 *(my call)*: 10 GiB) |
| `GET /v1/vaults/:id/blobs/:blobId/:part` | §10 download; read entitlement |
| `DELETE /v1/vaults/:id/blobs/:blobId` | §10 delete |
| `PUT /v1/pairing/:mailboxId/:slot` (slots 1 and 2, each write-once), `GET /v1/pairing/:mailboxId/:slot`, `DELETE /v1/pairing/:mailboxId`; 10-minute TTL, 64 KiB per slot | §11.3; no auth (the content is a public key or sealed data and the ID is random), rate-limited per IP |
| `POST /v1/vaults/:id/token` takes `{token, rekey}` and needs only `owner` access; keeps the previous token's hash as **read-only** for 30 days; `GET /v1/vaults/:id/rekey` returns the stored rekey bundle and the relay head at rotation, to the current or the previous token | §11.4 device removal. Today the old token stops working at once, rotation needs a paid subscription, and `owner` access (delete) needs only a token; after the change only the current token writes or deletes, and a person whose subscription lapsed can still lock out a lost device |

R2 keys must change before the relay is deployed. Today snapshots are stored as
`v1/<vaultId>/<seq>-<uuid>`, and the daily `sweepOrphanSnapshots` deletes *every* object under
`v1/<vaultId>/` that has no snapshot record and is over an hour old — blobs put there would be
gone within a day. New layout: snapshots under `v1/<vaultId>/snapshots/`, blob parts under
`v1/<vaultId>/blobs/<blobId>/<part>`; the sweep lists only the snapshot prefix (and a separate
sweep handles incomplete blobs); the vault wipe keeps deleting everything under `v1/<vaultId>/`.
Nothing is deployed, so no data needs moving. The cost model gets a blob line (R2 storage
$0.015/GB-month: 2 GB of photos per user is $0.03 a month).

## 13. Security properties

What the relay operator can learn: the account number and its payment link (through Polar), the
vault IDs of an account, when each device syncs (IP addresses are seen by Cloudflare's edge but
not stored by the relay), envelope sizes rounded to 1 KiB, snapshot and blob sizes. Not: any
amount, name, category, ticker, note, file name, file content, device name or count of rows.

What a stolen phone or laptop yields: nothing more than today — the vault keys are inside the
SQLCipher database, behind the device password (and biometry where enabled). Removing the device
(§11.4) stops it from following future changes.

What the Recovery Kit yields: everything synced. It is treated like the local recovery key.

Threats outside scope: a compromised device while unlocked (it can read and write everything,
synced or not); a person who loses every device and the Recovery Kit (data is gone; that is the
point).

## 14. Migrations and app versions (superseded, see §20)

Devices update at different times, so envelopes carry the author's schema number (the count of
applied main migrations).

- D26 Every migration is classified in the registry as **additive** (new nullable columns, new
  tables, new seeds) or **barrier** (rewrites the meaning of synced values, like 011 did for rates
  or 019 for loan anchors). A test fails for a migration without a class.
- **Additive:** a newer device applies envelopes from older devices as they are (missing columns
  keep their values). An older device that receives an envelope from a newer schema stops pulling
  at that envelope and shows "Update Moony on this device to keep syncing"; it keeps pushing its
  own changes, which newer devices accept.
- **Barrier:** the first device past the barrier uploads a fresh snapshot and records the barrier
  in `sync_state` (and in the next envelopes). Envelopes written below the barrier and placed after
  it in relay order are not applied by devices above it. An old device can push such envelopes
  before it pulls the barrier marker (pull and push are not atomic), and it has already cleared
  those outbox rows. So when a device itself migrates past a barrier, it **re-queues every row
  whose clock carries its own device ID and is newer than the newest snapshot it has restored or
  uploaded**; those changes go out again in the new format with their original clocks, which is
  harmless where they had already arrived. A device below the barrier stops pushing and pulling
  once it sees the marker, until updated.
- **Migration data and sync.** Migrations run on every device separately and are never captured.
  Two rules keep that safe, enforced by the registry test for every migration after 029:
  - a migration that inserts synced rows uses **deterministic IDs** (a UUIDv5 of the source row's
    ID and the migration number, or a fixed seed ID), never `randomblob()`, so every device creates
    the same row (migrations 002, 007, 013 and 019 did use random IDs; they all ran before sync
    exists, so their rows reach other devices through the snapshot);
  - a seeded row whose key already has a tombstone on this device is deleted again right after the
    migrations (§7.4), so a seed doesn't bring back what the person deleted on another device.
- The relay never needs to know any of this.

## 15. Architecture in the code (superseded, see §20)

New Rust module `src-tauri/src/services/sync/`, all logic taking `&Connection` or plain data,
network through `services/http.rs`, so it is testable without a relay:

| File | Role |
|---|---|
| `registry.rs` | Table classes, non-synced columns, app_config allowlist, unique keys, FK graph, migration classes |
| `hlc.rs` | Clock type, packing, comparisons |
| `capture.rs` | TEMP trigger generation, `sync_hlc()` / capture flag registration |
| `journal.rs` | Outbox → changesets (reading current values), marking sent |
| `merge.rs` | D12–D15, column groups, generations, tombstones, orphan rules |
| `natural_id.rs` | UUIDv5 IDs for the "same ID" tables (§7.3), used by the creating services |
| `repair.rs` | §7.1 mirrors and §7.4 invariants after merge |
| `envelope.rs` | Key derivation, envelope seal/open, STREAM for snapshots and blobs |
| `snapshot.rs` | Dump; restore by replacing (join) or merging (after `410`, re-join) |
| `blobs.rs` | Upload/download queue |
| `pairing.rs` | QR payloads, mailbox crypto, recovery code encode/decode |
| `relay.rs` | HTTP client for the relay API (the only network code) |
| `engine.rs` | The loop of §9.4, back-off, events |

Commands in `commands/sync.rs` stay thin (rule 4). Wire types go through the 7-step procedure of
`type-contract.md` (rule 2): `SyncStatus`, `SyncDevice`, `SyncAccount`, `PairingOffer`,
`RecoveryKit`. The UI is a new Settings → Sync section on both apps and a small status indicator
(synced / syncing / paused / error) in the shell; every string in all ten locales (rule 8).

A Cargo feature or runtime flag keeps the whole section hidden until the owner turns it on, so
the parts can land on main as they are finished without being visible to users.

An ADR records D4 (trigger capture), D9/D12–D15 (clocks and merge rules) and D17 (envelope).

## 16. Questions only the owner can answer

Most important first.

- **Q1. Price and what it includes.** The cost side is settled (≈$5/month up to 1,000 paying
  users). Open: monthly and yearly price, a free trial or not, and whether attachments are
  included or capped (D25 proposes a 10 GiB blob quota per vault).
- **Q2. Attachments in the first release, or data first?** Blobs add two relay endpoints, a
  download queue and UI states; leaving them for a second release ships sync weeks earlier, and
  synced photo rows would show "On another device" until then.
- **Q3. One recovery story or two codes?** Each device has its local recovery key; sync adds the
  Sync Recovery Kit. Options: keep both, clearly named; or, when sync is on, make the Sync
  Recovery Kit the main way back and present the local key as secondary.
- **Q4. Phones and buying.** v1 sells only on the desktop (D20). Whether the phone may show
  "Subscribe on moonyfinance.com" depends on the Apple 3.1.3(b)/EU rules question already open.
- **Q5. Merging an existing phone profile later.** D22 makes v1 join only from the synced data:
  someone who has used Moony on both the desktop and the phone picks one profile, and the other
  device's data is replaced (after a local backup). Is that acceptable for launch, or is merging
  two existing profiles a must-have? Merging is possible later but is the hardest part of the
  design (§7.3, review notes).
- **Q6. Very large profiles.** v1 fails snapshot upload above 100 MiB compressed. Fine for
  anything Moony has seen; chunked snapshots are a later relay change.

Calls I made that the owner should check: D5 (history and prices not synced), D7 (language, theme
and menu stay per device), D8 (app_config allowlist, rule packs written explicitly when sync is
turned on), D10 (sparse clocks), D17 (no signatures), D18 (phone downloads full photos on Wi-Fi
only), D22 (joining replaces the device's data, after a backup), D25 (10 GiB blobs), D27 (a
`split_basis` column so splits survive offline edits), and that a lapsed subscriber can still lock
out a lost device (§11.4, §12).

## 16a. Owner answers (2026-11-06) (superseded, see §20)

Filip approved the spec with "implement it, decision about limits and tiers leave for later, same
for attachments". What that means for the build:

- **Q1 (price, trial, caps) — later.** The app shows no prices: the checkout step opens Polar's
  checkout for the plan the relay offers (monthly or yearly), and Polar's page shows the amount.
  No blob quota exists because there are no blobs yet. Relay limits stay as in DESIGN.md.
- **Q2 (attachments) — later.** Phase 4 is not built. In v1 the attachment tables
  `real_estate_photo_batches`, `real_estate_photos`, `real_estate_documents` and
  `insurance_documents` are **Local** (§5.1): their rows and files stay on the device that made
  them. The relay's blob endpoints and the R2 layout change (§12) wait for phase 4; the relay's
  snapshot sweep is safe until blobs exist.
- **Oversized changesets without blobs** *(my call, follows from Q2)*: §9.1's "a changeset that
  doesn't fit is uploaded as a blob" needs blobs. Instead the relay's op limit rises from 64 KiB to
  192 KiB, and the client cuts envelopes at 60 KiB but lets a single row that alone exceeds that
  travel in its own envelope up to 180 KiB (`watched_stocks.notes` is capped at 100 KiB, every
  other row is far smaller). A row that would still not fit is refused at capture time with a
  logged error and stays in the outbox (it cannot happen with today's caps; a test pins the
  largest row).
- **Q3, Q4, Q5, Q6 — my calls, as proposed:** keep both recovery codes, clearly named ("Sync
  Recovery Kit" vs the device's recovery key); no purchase or subscribe link on the phone; D22
  join-only stands for v1; snapshots over 100 MiB fail with a clear error.
- Every other *(my call)* decision stands as written.

## 17. Phases

Each phase lands on main finished and tested, hidden behind the flag until the last.

1. **Engine without network.** Registry, migration 029 (including `split_basis`), `natural_id()`
   in the creating services, capture, journal, merge, repairs, snapshot dump/restore. Proven by
   convergence tests: three in-memory "devices", random operations through the real services,
   changesets exchanged in random orders (including orders a dishonest relay could choose), every
   synced table equal at the end (property tests).
2. **Crypto and relay client.** Envelopes, STREAM, relay client, the engine loop, account
   creation, checkout, first-device setup, Recovery Kit, Settings → Sync on the desktop. Tested
   against `wrangler dev` of Moony-relay.
3. **Second device.** Relay pairing mailbox, pairing both directions, recovery code join, phone
   UI, device list, re-join.
4. **Attachments** (or before 3, depending on Q2). Relay blob endpoints, upload/download queue,
   UI states.
5. **Removal and rotation.** Key epochs, token rotation, removing a device, new Recovery Kit.
6. **Release.** Deploy the relay, Polar products, ADR, privacy policy and README text ("what the
   server knows"), real-profile dry run on copies of the owner's desktop and phone profiles,
   turn the flag on.

## 18. Verification (superseded, see §20)

- Convergence property tests (phase 1) cover D12–D15, column groups, generations, every
  `ON DELETE` rule and orphan rule, the four unique-key rules, invariants, splits with offline
  edits and concurrent apply/dismiss, a 5,000-row import split over many envelopes, barrier
  re-queues, and re-join after a simulated rollback. Each scenario the 2026-11-06 review raised is a
  named test.
- Crypto: known-answer tests for HKDF labels, envelope layout, stream headers and recovery code
  checksum; a test that a changed byte anywhere in an envelope or stream fails to open; that two
  streams never share a key; that a blob served under another blob's ID fails; pairing with a
  substituted public key shows different codes.
- The engine against the real relay in `wrangler dev`: two desktop processes on two demo profiles
  (ports 1421/1422 per the dev-server memo) editing offline and online; the phone in the
  simulator joining by QR (the simulator can't scan, so the plan adds a debug "paste pairing
  link" field).
- Failure drills: kill the app between push and mark-sent; between merge and history refresh;
  during a snapshot upload; relay returning 402, 410, 413, 5xx; a relay fed a dropped and a
  replayed envelope (a test double).
- Performance: capture overhead on a 5,000-row statement import (target: under 20% slower);
  merge speed of 50,000 rows from a snapshot on the phone (target: under 30 s on an iPhone 15).

## 19. Alternatives considered

- **Sync the SQLCipher file** (Dropbox style). Last-writer-wins for the whole file loses edits
  from the other device, and the file is tens of MB. Rejected.
- **cr-sqlite or a CRDT library** (Automerge, Yjs). cr-sqlite does almost exactly §6–§7, but it
  is a loadable extension, it is not maintained with SQLCipher builds in mind, and it changes
  tables in place. Automerge/Yjs would mean a second data model next to SQLite. A few hundred
  lines of trigger generation and merge code, fitted to Moony's schema, is less risk.
- **Capture in the services** (every service calls `journal.record()`). ~900 statements, and
  every future one, would need it. Triggers are below them all.
- **Operation-based sync** (send "import statement X" and replay the command elsewhere). Commands
  depend on local state (dedup, rules, prices) and would replay differently. Rejected for
  state-based rows.
- **Per-device Ed25519 signatures.** §9.2.
- **Password-derived vault key** (so the person needs only a password to join). It would make the
  relay's ciphertext brute-forceable offline. A random key with QR pairing and a Recovery Kit is
  stronger and not harder in practice.

## 20. As built (phase 1, 2026-11-07)

Phase 1 (the engine without network) was built on `feat/device-sync`. ADR 0024
(`docs/architecture/decisions/0024-device-sync-capture-clocks-merge.md`) records the decisions as
built. The text above is kept as it was approved. A section marked "(superseded, see §20)" has some
text that no longer matches the build; the rest of that section still stands. Where they differ,
the code and ADR 0024 win.

- **§5.2 Column classes.** These columns are recomputed-or-typed, not recomputed: each is computed
  from its source rows while any exist and keeps a typed value when it has none
  (`registry/tables.rs`):
  - `bank_accounts.balance`/`balance_as_of`, from `bank_balance_records`;
  - `stock_investments.quantity`, from `investment_transactions`;
  - `crypto_investments.quantity`/`currency`, from `crypto_transactions`;
  - `real_estate.market_price`/`market_price_currency`, from `real_estate_valuations`;
  - `other_assets.market_price`, from `other_asset_valuations`;
  - `other_assets.quantity`/`average_purchase_price`, from `other_asset_transactions`.

  `loans.interest_rate`/`monthly_payment` are plain Synced. `loan_events.previous_rate`/
  `previous_monthly_payment` are Synced, and `loan_events::rebuild_terms_chain` re-derives every
  later carrier, after a merge and in every local write that adds or removes an event. Exactly
  four `updated_at` columns are Local (`bank_accounts`, `loans`, `real_estate`, `other_assets`),
  and `real_estate.photos` is Local. Capture still clocks a recomputed-or-typed column only when its
  row has no source rows. While a computed value hides the typed one, the typed value waits in
  `sync_typed`; it goes on the wire and comes back when the last source row goes. With capture
  installed, the value a column had when sync started counts as typed. ADR 0024.
- **§6.1 Triggers.** Each table gets insert, update and primary-key-change triggers and a
  BEFORE/AFTER delete pair, plus shadow triggers for recomputed-or-typed columns. Three connection
  functions (`sync_hlc_tick`, `sync_hlc_now`, `sync_hlc_observe`) replace `sync_hlc()`, and the
  mode lives in `temp.sync_mode`. In `local` mode the outermost delete ticks once, and every row
  its cascade reaches is tombstoned with that same clock, as `merge` mode does with the parent's
  clock. A `merge`-mode delete without a cascade clock aborts. ADR 0024, D4.
- **§6.2 Local sync tables.** `sync_pending_blobs` is not created (attachments stay Local).
  `sync_own_log` holds `min_hlc` as well as `max_hlc` (the re-join watermark, §11.4 below). The
  Local table `sync_typed (tbl, pk, col, typed, assigned)` holds typed values that a repair shows
  differently: renamed names, cleared links and recomputed-or-typed columns. `sync_state` also has
  `snapshot_hlc` and `schema_seen`, and `sync_heads` has `quarantined`. Migration 029 adds the
  split entry columns next to `split_basis` (§7.2 below). `docs/architecture/database.md` lists
  them.
- **§6.3 Clock.** The ms part saturates at `2^47 − 1` (6429-10-17), so the packed value fits a
  positive SQLite INTEGER, and the decoder refuses a clock above `i64::MAX`. A row that never had
  a clock reads as clock 0 with the all-zero device. ADR 0024, D9.
- **§7.1 Merge (D12–D14).** These rules replace parts of the text:
  - every change is a full row, so "a partial update to a deleted row" never happens;
  - delete epochs replace "a change addressed to an older generation is dropped". A key's epoch
    is its newest tombstone. An insert older than that epoch is dropped. Within one epoch, two
    creations of the same row merge per column, and the stored generation becomes the larger of
    the two. A row deleted and re-created travels with both `"del"` and `"ins"`;
  - the monotonic `stock_splits.status` rule became "a `pending` written by an insert is weak":
    a weak `pending` loses to any other status, and otherwise the newer clock wins, so a revert
    syncs;
  - D14 has three more NO ACTION keys, all with the set-NULL orphan rule:
    `bank_accounts.institution_id`, `bank_transactions.suggested_category_id` and
    `transaction_categories.parent_id`;
  - parent rules run only on the FK values that won the column merge (plus the key parts). A
    CASCADE or drop-child insert into a deleted parent epoch is recorded as a tombstone at the
    parent's tombstone clock, so its own children follow it instead of stopping the merge;
  - every `'-'` tombstone is identical on all devices, with one exception: a cascade may raise a
    key's `'-'` above its author's own earlier tombstone. No decision reads that window, and no
    decision reads the `'*'` of a dead row;
  - recalculations that write synced or recomputed columns break ties by `id`. A recalculation
    that fails falls back deterministically and does not fail the merge.

  ADR 0024, D12–D14.
- **§7.2 / D27 Splits.** Rows are no longer rewritten to current units on apply and revert. The
  values as entered sync: `entry_quantity`/`entry_price_per_unit`, `entry_price`,
  `entry_target_price` and `entry_yearly_dividend_sum`, each with `entry_basis`; the three side
  tables also have `entry_at`. The current units and `split_basis` are Recomputed:
  `stock_splits::reconcile_ticker` derives them in one scaling with one rounding, so the result
  does not depend on the path. A side row is affected by a split according to `entry_at`, never
  `updated_at`. Deleting a reverted manual split, or recording it again with another ratio, first
  re-bases the rows whose `entry_basis` names it (entry values scaled back by the old ratio, the
  ID removed, as a local entry write), so fixing a wrong manual split works as it did before.
  A basis ID with no `stock_splits` row stays and scales nothing. ADR 0024.
- **§7.3 and §7.4 Unique keys and invariants (rulings U1–U4).**
  - The "keep newest" tables became Same ID. `bank_balance_records` and `budget_goal_versions`
    take natural IDs from their keys, and moving a version to another key deletes it and
    re-creates it under the new key's ID. Keep-newest, compared by the key clock
    `max('*', key column clocks)`, remains only as a warned fallback for a second ID on one key.
    That case needs legacy random-ID rows, for example a legacy row's delete and its natural-ID
    re-creation split across two envelopes by the size cap.
  - Rename and Unlink are derived: the typed value stays in `sync_typed`, the stored column is
    recomputed from the typed values of its scope, and the wire carries the typed values.
    Category names are a fourth rename scope.
  - A collision inside one changeset is deferred and retried after the rest of the changeset;
    only a collision that remains goes to the rule.
  - §7.4 adds two rules: category names follow the rename rule, and insurance
    `(provider, policy_number)` has no repair (duplicates are kept, a known limit). The split row
    of the table now reads "derive from entry values".

  ADR 0024, D15.
- **§8 Snapshots.** A dump requires an empty `sync_outbox` as well as an empty `sync_unsent`;
  otherwise it fails with `validation.syncSnapshotPending`. Both restores go through the merge
  engine and the full repair. ADR 0024.
- **§9.1 Wire.** Each row carries `"p"` (the generation of each referenced parent). A changeset
  carries `"barrier"`, omitted when it is 0. A row that was deleted and re-created carries both
  `"del"` and `"ins"`. `"prev"` is omitted at `seq` 1. ADR 0024.
- **§11.4 Re-join.** `chain::rejoin` takes the sequence number of the envelope that judged
  `Clone`, or none for a re-join found at unlock. The confirmed own prefix ends below that
  sequence, and its last entry becomes the old ID's head. Every own clock at or above
  `W = min(max_hlc of the prefix, min_hlc of every own envelope outside it)` is re-queued. This
  replaces "newer than the last own envelope it can confirm" (ADR 0024, chains).
- **§14 Migration seeds.** A seed after 029 must also use fixed values for every synced column,
  timestamps included (`docs/standards/rust-backend.md` §12). The registry test enforces only
  the deterministic IDs. Phase 1 judges an envelope `BelowBarrier` from its `"barrier"` alone;
  skipping it only when the relay placed it after the barrier marker waits for the phase 2
  engine, which knows the relay order.
- **§15 Code layout.** `registry/`, `capture/`, `journal/`, `merge/`, `repair/` and `snapshot/`
  are directories; `chain.rs`, `changeset.rs`, `state.rs` and `natural_id.rs` are files.
  `envelope`, `blobs`, `pairing`, `relay` and `engine` belong to later phases.
- **§16a Oversized rows.** The journal refuses a row over 180 KiB; capture cannot, because it
  cannot measure the deflated size. The row stays in the outbox.
- **Deliberate user-visible changes with sync off (release notes).** Phase 1 changes a few
  results for people who never turn sync on:
  - a watch target (and a price or dividend override) is rescaled by a split according to when
    the value was entered (`entry_at`), so editing the notes no longer changes it;
  - reverting a split returns exactly the values as entered (no `…00000001` drift), and applying
    two splits rounds once;
  - a budget range that lies only in the past no longer shows as the current limit;
  - a back-dated interest-rate change re-chains the "previous" terms of every later change;
  - recalculations that met two rows in the same second now break the tie by `id`;
  - fixing a wrong manual split (revert, then record it again at another ratio or delete it)
    works as before.
- **§18 Performance.** The capture overhead on a 5,000-row statement import was measured at about
  350%: 31.9 ms rose to 143.7 ms in a release build in memory, and to about 315% with a WAL file
  (`capture_overhead_on_a_5000_row_statement_import`, `#[ignore]`). The target was under 20%. The
  absolute cost is about 0.1 s per 5,000 rows, which is accepted for phase 1; phase 2 profiles it
  on a phone. The 50,000-row iPhone merge target waits for the phone phase. ADR 0024, known
  limits.

### As built (phase 3, 2026-11-10)

Phase 3 (the second device) was built on `feat/device-sync-phase-3`. ADR 0026
(`docs/architecture/decisions/0026-device-sync-pairing.md`) records its decisions, bytes and limits;
where it differs from §11, the code and ADR 0026 win.

- **§11.3 The pairing code.** The QR text is `moony-pair:1:<mailbox>:<S public>[:<sync server>]`:
  the 4th field (the base64url of the normalized address) is written only for a server that is not
  the default, so pairing works with a self-hosted server. The mailbox lives on the holder's stored
  server when the holder shows, on the joiner's typed (or stored, or default) server when the joiner
  shows, and on the server the code names when scanning. The code's six digits protect the exchange
  only when S's key arrives by an on-screen QR, so pasting a pairing code is a developer tool only.
  Each slot (not the whole mailbox) is written once and read once.
- **§11.3 Roles and directions.** The role follows the device's own sync state (holder with sync
  on, joiner without) and is not part of the derived key. The desktop always shows the QR and the
  phone always scans it, in both directions; desktop to desktop and phone to phone join with the
  Recovery Kit. Both devices check their device password before anything is shown or written, and
  each acts only after its own person confirmed the six digits. The joining device keeps its own
  profile and password and joins by replace (D22) after a verified backup; a Kit join first checks
  the account on the server and refuses a vault whose key was rotated. A join that fails for the
  moment (offline, a sync server error, another sync change running, a half-finished setup) keeps
  the received sync details: the UI asks the backend whether they are still there and offers "Try
  again" (and "Cancel setup" when a setup blocks it), else "Start again". On the phone a scanned
  Recovery Kit QR joins only after a confirm that shows the Kit's account number and sync server.
- **§11.4 The device list.** "Last synced" is derived, not stored: this device's last successful
  sync, another device's newest clock in `sync_clock`. Rows of re-joined or turned-off devices stay
  until removal in phase 5.
- **§11.2 and §16a The phone.** Settings → Sync exists on the phone (status, devices, scanning,
  the Kit, setup). Setup on the phone never opens a checkout (Q4): a self-hosted server works fully,
  the default one waits for a subscription paid elsewhere. The iPhone binds with the Secure Enclave
  as macOS does, copies the recovery code to a private pasteboard that expires after 60 s, and a
  debug build can carry the sync flag from its build environment. Android is not built.
