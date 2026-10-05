# Milestones ("Co vás čeká") Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gather contract dates, payments, data-upkeep reminders and crossed watchlist targets from every domain into a dashboard card and a top-bar indicator, with "Vyřízeno" / "Odložit", and make watchlist targets direction-aware.

**Architecture:** A Rust service `services/milestones/` (one builder per domain + a combiner that adds the stage, drops dismissed/snoozed occurrences stored in a new `milestone_states` table, and orders the list) behind three thin commands. Migration `005_milestones` also adds `watched_stocks.target_direction` and `bank_accounts.interest_rate_valid_until`. The React side reads one query (`["milestones"]`), refreshed by every successful mutation through the query client's mutation cache.

**Tech Stack:** Tauri 2, Rust (rusqlite, chrono, specta, serde), React 19 + TypeScript, TanStack Query, react-i18next, Vitest, sonner, lucide-react, Radix (shadcn) primitives.

**Spec:** `docs/specs/2026-10-05-milestones-design.md`.

## Global Constraints

- Never call `invoke()` outside `src/lib/tauri-api.ts`.
- `shared/schema.ts` is the wire contract: every Rust field change is mirrored there; regenerate `shared/generated-types.ts` with `cd src-tauri && cargo test generate_bindings -- --ignored` and commit it.
- New Rust struct fields use per-field `#[serde(rename = "camelCase")]` (never `rename_all` on structs).
- Commands are thin (validate → delegate → map); logic lives in `src-tauri/src/services/` taking `&Connection`; errors are `AppError`, never `Result<_, String>`; no `.unwrap()` outside tests.
- Migrations are append-only: `005_milestones` is new; never edit 001–004.
- Every user-facing string goes into BOTH `src/i18n/locales/en/` and `src/i18n/locales/cs/`.
- Money is TEXT, timestamps are unix seconds INTEGER, days are UTC midnights (ADR 0008).
- Design system: tokens only (`text-ink-*`, `bg-well`, `border-line-soft`, …), no raw colours; green/red-brown only for gain/loss.
- Logging: never log names, tickers or amounts above `debug`.
- Rust service tests use `Connection::open_in_memory()` with a hand-written schema, never the `Database` struct.
- Before every commit: `git branch --show-current` must print `feature/milestones` (other sessions can switch branches in this checkout).
- Commits are Conventional Commits and end with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Pre-commit runs lint-staged + `npm run typecheck` + `cargo fmt --check`; run `cd src-tauri && cargo fmt` before committing Rust.

---

### Task 1: Migration 005 (target direction, promo rate end, milestone states)

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (new const `MIGRATION_005`, Vec entry, new test module)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `docs/architecture/database.md` (catalog rows)

**Interfaces:**
- Produces: columns `watched_stocks.target_direction TEXT` (`'below'|'above'|NULL`), `bank_accounts.interest_rate_valid_until INTEGER`, table `milestone_states(key TEXT PRIMARY KEY, state TEXT NOT NULL CHECK (state IN ('done','snoozed')), until_day INTEGER, updated_at INTEGER NOT NULL DEFAULT (unixepoch()))`.

- [ ] **Step 1: Write the failing migration test.** In `src-tauri/src/db/migrations.rs`, find the test module that defines `fn database_before_004()` and add a sibling module right after it, with the same nesting and the same `use super::*;` line:

```rust
mod migration_005 {
    use super::*;

    /// A database as it was before `005`: every migration that comes earlier in the chain.
    fn database_before_005() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let earlier: Vec<(&str, &str)> = all_migrations()
            .into_iter()
            .filter(|(name, _)| *name < "005_milestones")
            .collect();
        run_chain(&conn, &earlier).expect("earlier migrations");
        conn
    }

    fn direction(conn: &Connection, ticker: &str) -> Option<String> {
        conn.query_row(
            "SELECT target_direction FROM watched_stocks WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .expect("watched row")
    }

    #[test]
    fn existing_targets_get_a_direction_from_the_stored_price() {
        let conn = database_before_005();
        conn.execute_batch(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date) VALUES
                ('sd1', 'CAT', '845', 'USD', 1700000000),
                ('sd2', 'AAPL', '200', 'USD', 1700000000);
             INSERT INTO watched_stocks (id, ticker, target_price) VALUES
                ('w1', 'CAT', '600'),
                ('w2', 'AAPL', '250'),
                ('w3', 'NOPRICE', '10'),
                ('w4', 'NOTARGET', NULL);",
        )
        .expect("rows written by an earlier version");

        run_migrations(&conn).expect("migrate to the current chain");

        // Below the price: waiting for a dip. Above it, or no price known: the old meaning.
        assert_eq!(direction(&conn, "CAT").as_deref(), Some("below"));
        assert_eq!(direction(&conn, "AAPL").as_deref(), Some("above"));
        assert_eq!(direction(&conn, "NOPRICE").as_deref(), Some("above"));
        assert_eq!(direction(&conn, "NOTARGET"), None);
    }

    #[test]
    fn promo_rate_end_and_milestone_states_exist() {
        let conn = database_before_005();
        run_migrations(&conn).expect("migrate");
        conn.execute(
            "INSERT INTO bank_accounts (id, name, interest_rate_valid_until) VALUES ('a1', 'Spořák', 1790000000)",
            [],
        )
        .expect("the promo rate end column exists");
        conn.execute(
            "INSERT INTO milestone_states (key, state, until_day) VALUES ('backup_stale', 'snoozed', 1790000000)",
            [],
        )
        .expect("a snoozed state");
        assert!(
            conn.execute(
                "INSERT INTO milestone_states (key, state) VALUES ('x', 'forgotten')",
                [],
            )
            .is_err(),
            "unknown states are rejected"
        );
        assert_eq!(
            applied_migration_names(&conn).expect("applied"),
            known_migration_names()
        );
    }
}
```

- [ ] **Step 2: Run it and watch it fail.**

Run: `cd src-tauri && cargo test migration_005`
Expected: FAIL — `no such column: target_direction` (the migration does not exist yet).

- [ ] **Step 3: Add the migration.** After `const MIGRATION_004` add:

```rust
/// `005_milestones`: the direction a watchlist target waits for, the end of an account's
/// promotional rate, and the "done" / "snoozed" state of milestone occurrences
/// (spec 2026-10-05-milestones-design).
const MIGRATION_005: &str = r#"
-- 'below' = waiting for the price to fall to the target (buy / buy more),
-- 'above' = waiting for it to rise (sell); NULL while there is no target.
ALTER TABLE watched_stocks ADD COLUMN target_direction TEXT CHECK (target_direction IN ('below', 'above'));

-- Existing targets: below the stored price means a dip; above it, or no price known,
-- keeps the meaning every earlier version used ("reached" = price at or above).
UPDATE watched_stocks
SET target_direction = CASE
    WHEN (SELECT CAST(sd.original_price AS REAL) FROM stock_data sd WHERE sd.ticker = watched_stocks.ticker)
         > CAST(target_price AS REAL) THEN 'below'
    ELSE 'above'
END
WHERE target_price IS NOT NULL;

-- UTC day an account's promotional interest rate ends; NULL when unknown.
ALTER TABLE bank_accounts ADD COLUMN interest_rate_valid_until INTEGER;

-- One row per milestone occurrence the user marked done (for good) or snoozed
-- (until `until_day`). The key names the occurrence, e.g.
-- 'insurance_anniversary:<policy id>:<day>', so next year's anniversary is a new key.
CREATE TABLE IF NOT EXISTS milestone_states (
    key TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('done', 'snoozed')),
    until_day INTEGER,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
"#;
```

and append `("005_milestones", MIGRATION_005),` to the `Vec` in `all_migrations()` after the `004` entry.

- [ ] **Step 4: Regenerate the schema snapshot and run the tests.**

Run: `cd src-tauri && cargo test regenerate_schema_snapshot -- --ignored && cargo test migration`
Expected: PASS. `git diff src/db/schema.snapshot` shows only the two new columns and the new table.

- [ ] **Step 5: Update `docs/architecture/database.md`.** In the table catalog: amend the `watched_stocks` row (notable columns: `target_direction` `below`/`above`, NULL without a target), the `bank_accounts` row (`interest_rate_valid_until`: UTC day the promotional rate ends), and add a `milestone_states` row ("Done/snoozed state of milestone occurrences, keyed by occurrence key; `until_day` for snoozes"). Follow the format of the neighbouring rows; no line numbers or file sizes.

- [ ] **Step 6: Commit.**

```bash
git branch --show-current
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/db/migrations.rs src-tauri/src/db/schema.snapshot docs/architecture/database.md
git commit -m "feat(db): migration 005 for target direction, promo rate end and milestone states

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Watchlist target direction — backend and MCP

**Files:**
- Modify: `src-tauri/src/models/stock_monitor.rs`
- Modify: `src-tauri/src/services/stock_monitor.rs` (+ tests)
- Modify: `src-tauri/src/commands/stock_monitor.rs`
- Modify: `src-tauri/src/services/mcp/stock_monitor.rs` (+ tests)
- Modify: `src-tauri/src/services/mcp/mod.rs` (tool description + call)
- Modify: `shared/generated-types.ts` (regenerated)
- Modify: `src/i18n/locales/en/common.json`, `src/i18n/locales/cs/common.json` (validation key)
- Modify: every Rust test schema that creates `watched_stocks` (find them with `grep -rn "CREATE TABLE watched_stocks" src-tauri/src`)

**Interfaces:**
- Consumes: column `watched_stocks.target_direction` (Task 1).
- Produces:
  - `pub const TARGET_BELOW: &str = "below"; pub const TARGET_ABOVE: &str = "above";` and `pub fn validate_target_direction(direction: &Option<String>) -> Result<()>` in `models/stock_monitor.rs`.
  - `pub fn infer_target_direction(target: f64, current_price: Option<f64>) -> &'static str` in `services/stock_monitor.rs`.
  - `pub fn set_target_price(conn: &Connection, ticker: &str, target_price: Option<String>, target_direction: Option<String>) -> Result<WatchedStock>`.
  - Field `target_direction: Option<String>` (`"targetDirection"`) on `WatchedStock`, `WatchedStockRow`, `StockMonitorDetail`.
  - Command `set_watched_target_price(ticker, target_price, target_direction)`; JS args `{ ticker, targetPrice, targetDirection }`.

- [ ] **Step 1: Add the column to every test schema.** For each hit of `grep -rn "CREATE TABLE watched_stocks" src-tauri/src`, add `target_direction TEXT,` after `target_price TEXT,`.

- [ ] **Step 2: Write the failing service tests.** Add to the `#[cfg(test)] mod tests` of `src-tauri/src/services/stock_monitor.rs` (it already has `setup_test_db()` with `stock_data`). Replace the existing calls `set_target_price(&conn, X, Y)` in that file with `set_target_price(&conn, X, Y, None)` (the new parameter), then add:

```rust
    fn with_price(conn: &Connection, ticker: &str, price: &str) {
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency) VALUES (?1, ?2, ?3, 'USD')",
            rusqlite::params![format!("sd-{ticker}"), ticker, price],
        )
        .expect("price row");
    }

    #[test]
    fn a_target_below_the_price_waits_for_a_dip() {
        let conn = setup_test_db();
        follow_stock(&conn, &InsertWatchedStock { ticker: "CAT".into() }).unwrap();
        with_price(&conn, "CAT", "845");
        let ws = set_target_price(&conn, "CAT", Some("600".into()), None).unwrap();
        assert_eq!(ws.target_direction.as_deref(), Some("below"));
    }

    #[test]
    fn a_target_above_the_price_or_without_a_price_waits_for_a_rise() {
        let conn = setup_test_db();
        follow_stock(&conn, &InsertWatchedStock { ticker: "CAT".into() }).unwrap();
        let ws = set_target_price(&conn, "CAT", Some("600".into()), None).unwrap();
        assert_eq!(ws.target_direction.as_deref(), Some("above"), "no price known");
        with_price(&conn, "CAT", "500");
        let ws = set_target_price(&conn, "CAT", Some("600".into()), None).unwrap();
        assert_eq!(ws.target_direction.as_deref(), Some("above"));
    }

    #[test]
    fn an_explicit_direction_wins_and_clearing_the_target_clears_it() {
        let conn = setup_test_db();
        follow_stock(&conn, &InsertWatchedStock { ticker: "CAT".into() }).unwrap();
        with_price(&conn, "CAT", "845");
        let ws = set_target_price(&conn, "CAT", Some("600".into()), Some("above".into())).unwrap();
        assert_eq!(ws.target_direction.as_deref(), Some("above"));
        let rows = list_watched_stocks(&conn).unwrap();
        assert_eq!(rows[0].target_direction.as_deref(), Some("above"));
        let detail = get_stock_monitor_detail(&conn, "CAT").unwrap();
        assert_eq!(detail.target_direction.as_deref(), Some("above"));
        let ws = set_target_price(&conn, "CAT", None, Some("below".into())).unwrap();
        assert_eq!(ws.target_price, None);
        assert_eq!(ws.target_direction, None);
    }

    #[test]
    fn an_unknown_direction_is_rejected() {
        let conn = setup_test_db();
        follow_stock(&conn, &InsertWatchedStock { ticker: "CAT".into() }).unwrap();
        let err = set_target_price(&conn, "CAT", Some("600".into()), Some("sideways".into()))
            .unwrap_err();
        assert!(matches!(err, AppError::Validation(ref k) if k == "validation.targetDirectionInvalid"));
    }
```

- [ ] **Step 3: Run them and watch them fail.**

Run: `cd src-tauri && cargo test stock_monitor`
Expected: FAIL to compile (`set_target_price` takes 3 arguments; no field `target_direction`).

- [ ] **Step 4: Implement the model changes.** In `src-tauri/src/models/stock_monitor.rs`:
  - Add to `WatchedStock`, `WatchedStockRow` and `StockMonitorDetail`, right after `target_price`:

```rust
    /// `below` (waiting for a dip) or `above` (waiting for a rise); None without a target.
    #[serde(rename = "targetDirection")]
    pub target_direction: Option<String>,
```

  - Add after `validate_target_price`:

```rust
/// Target waiting for the price to fall to it.
pub const TARGET_BELOW: &str = "below";
/// Target waiting for the price to rise to it.
pub const TARGET_ABOVE: &str = "above";

/// A target direction is `below`, `above` or absent.
pub fn validate_target_direction(direction: &Option<String>) -> Result<()> {
    match direction.as_deref() {
        None | Some(TARGET_BELOW) | Some(TARGET_ABOVE) => Ok(()),
        Some(_) => Err(AppError::Validation(
            "validation.targetDirectionInvalid".to_string(),
        )),
    }
}
```

- [ ] **Step 5: Implement the service changes.** In `src-tauri/src/services/stock_monitor.rs`:
  - Import `validate_target_direction, TARGET_ABOVE, TARGET_BELOW` from `crate::models::stock_monitor`.
  - `get_watched`: select `target_direction` after `target_price` and map it (shift the following indexes).
  - `list_watched_stocks`: select `w.target_direction` after `w.target_price` and map it (shift indexes).
  - `get_stock_monitor_detail`: `target_direction: watched.as_ref().and_then(|w| w.target_direction.clone()),`.
  - Add and use:

```rust
/// `below` when the target sits under the current price (waiting for a dip to buy),
/// otherwise `above` (waiting for a rise; also when no price is known yet).
pub fn infer_target_direction(target: f64, current_price: Option<f64>) -> &'static str {
    match current_price {
        Some(price) if price > 0.0 && target < price => TARGET_BELOW,
        _ => TARGET_ABOVE,
    }
}

pub fn set_target_price(
    conn: &Connection,
    ticker: &str,
    target_price: Option<String>,
    target_direction: Option<String>,
) -> Result<WatchedStock> {
    validate_target_price(&target_price)?;
    validate_target_direction(&target_direction)?;
    let ticker = ticker.trim().to_uppercase();
    let normalized = target_price.map(|t| t.trim().to_string());
    let direction = match &normalized {
        None => None,
        Some(target) => Some(match target_direction {
            Some(explicit) => explicit,
            None => {
                let price: Option<f64> = conn
                    .query_row(
                        "SELECT original_price FROM stock_data WHERE ticker = ?1",
                        [&ticker],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten()
                    .and_then(|p| p.trim().parse().ok());
                // validate_target_price guarantees a positive decimal.
                let target: f64 = target.parse().unwrap_or(0.0);
                infer_target_direction(target, price).to_string()
            }
        }),
    };
    let n = conn.execute(
        "UPDATE watched_stocks SET target_price = ?1, target_direction = ?2, updated_at = unixepoch()
         WHERE ticker = ?3",
        rusqlite::params![normalized, direction, ticker],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("watched stock {ticker}")));
    }
    get_watched(conn, &ticker)?
        .ok_or_else(|| AppError::Internal("watched stock missing after update".to_string()))
}
```

- [ ] **Step 6: Update the command.** In `src-tauri/src/commands/stock_monitor.rs`:

```rust
#[tauri::command]
pub async fn set_watched_target_price(
    db: State<'_, Database>,
    ticker: String,
    target_price: Option<String>,
    target_direction: Option<String>,
) -> Result<WatchedStock> {
    db.with_conn(|conn| {
        stock_monitor::set_target_price(
            conn,
            &ticker,
            target_price.clone(),
            target_direction.clone(),
        )
    })
}
```

- [ ] **Step 7: Update the MCP tool.** In `src-tauri/src/services/mcp/stock_monitor.rs`:
  - Add to `WatchlistTargetPriceArgs`:

```rust
    #[serde(default)]
    #[schemars(
        description = "\"below\" when waiting for the price to fall to the target (buy or buy more), \"above\" when waiting for it to rise (sell). Omit to infer it from the current price."
    )]
    pub direction: Option<String>,
```

  - `watchlist_set_target_price(conn, ticker, target_price, direction: Option<String>)` passes `direction` to `watchlist::set_target_price` and adds `"targetDirection": watched.target_direction,` to its JSON.
  - Add `"targetDirection": r.target_direction,` next to `"targetPrice"` in `watchlist_list`.
  - Existing tests: pass `None` as the new argument. Add a test:

```rust
    #[test]
    fn target_direction_is_stored_and_listed() {
        let conn = setup_test_db();
        let set = watchlist_set_target_price(&conn, "CAT", Some("600".into()), Some("below".into()))
            .expect("set target");
        assert_eq!(set["targetDirection"], "below");
        let list = watchlist_list(&conn).expect("list");
        assert_eq!(list["watchlist"][0]["targetDirection"], "below");
    }
```

  - In `src-tauri/src/services/mcp/mod.rs`, the `watchlist_set_target_price` tool: pass `args.direction.clone()` and replace the description with: `"Set the user's own target price for a stock in Stock Monitor, or clear it by passing targetPrice: null. direction says what the target waits for: \"below\" (a dip to buy or buy more) or \"above\" (a rise to sell); omitted, it is inferred from the current price. Moony lists a crossed target among the user's milestones; it places no order. Given in the stock's own currency as a decimal string. Follows the ticker automatically when it is not followed yet. Call this only after presenting the target to the user and receiving their confirmation."`

- [ ] **Step 8: Add the validation message.** In `src/i18n/locales/en/common.json` under `validation` add `"targetDirectionInvalid": "Choose whether the target waits for a fall or a rise."`; in `cs/common.json` add `"targetDirectionInvalid": "Vyberte, zda cíl čeká na pokles, nebo na růst."`.

- [ ] **Step 9: Run tests, regenerate bindings.**

Run: `cd src-tauri && cargo test stock_monitor && cargo test generate_bindings -- --ignored && cargo clippy -- -D warnings`
Expected: PASS; `shared/generated-types.ts` gains `targetDirection` on the three types.

- [ ] **Step 10: Commit.**

```bash
git branch --show-current
cd src-tauri && cargo fmt && cd ..
git add src-tauri shared/generated-types.ts src/i18n/locales/en/common.json src/i18n/locales/cs/common.json
git commit -m "feat(stock-monitor): targets wait for a dip or a rise

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Watchlist target direction — frontend

**Files:**
- Modify: `shared/schema.ts` (types)
- Modify: `src/lib/tauri-api.ts` (`stockMonitorApi.setTargetPrice`)
- Modify: `src/utils/stock-monitor.ts`, `src/utils/stock-monitor.test.ts`
- Modify: `src/hooks/use-stock-monitor-mutations.ts`
- Modify: `src/components/stock-monitor/TargetPriceDialog.tsx`
- Modify: `src/pages/StockMonitor.tsx`, `src/pages/StockMonitorDetail.tsx`
- Modify: `src/i18n/locales/en/stockMonitor.json`, `src/i18n/locales/cs/stockMonitor.json`

**Interfaces:**
- Consumes: command `set_watched_target_price` with `{ ticker, targetPrice, targetDirection }` (Task 2).
- Produces: `export type TargetDirection = 'below' | 'above';` in `shared/schema.ts`; `inferTargetDirection(target: number, current: number | null): TargetDirection` and `targetReached(current: number | null, target: number | null, direction: TargetDirection | null): boolean` in `src/utils/stock-monitor.ts`.

- [ ] **Step 1: Mirror the types.** In `shared/schema.ts`, above `export interface WatchedStock`, add:

```ts
/** What a watchlist target waits for: a fall to it (`below`) or a rise to it (`above`). */
export type TargetDirection = 'below' | 'above';
```

and add `targetDirection: TargetDirection | null;` right after `targetPrice` in `WatchedStock`, `WatchedStockRow` and `StockMonitorDetail`.

- [ ] **Step 2: Write the failing util tests.** Append to `src/utils/stock-monitor.test.ts` (add `inferTargetDirection, targetReached` to its import from `./stock-monitor`):

```ts
describe('inferTargetDirection', () => {
  it('waits for a dip when the target is under the price', () => {
    expect(inferTargetDirection(600, 845)).toBe('below');
  });
  it('waits for a rise when the target is above the price or no price is known', () => {
    expect(inferTargetDirection(900, 845)).toBe('above');
    expect(inferTargetDirection(600, null)).toBe('above');
  });
});

describe('targetReached', () => {
  it('a dip target is reached at or under the target', () => {
    expect(targetReached(845, 600, 'below')).toBe(false);
    expect(targetReached(600, 600, 'below')).toBe(true);
    expect(targetReached(598, 600, 'below')).toBe(true);
  });
  it('a rise target is reached at or over the target', () => {
    expect(targetReached(845, 900, 'above')).toBe(false);
    expect(targetReached(905, 900, 'above')).toBe(true);
  });
  it('a target without a direction keeps the old meaning (at or above)', () => {
    expect(targetReached(845, 600, null)).toBe(true);
  });
  it('is never reached without both numbers', () => {
    expect(targetReached(null, 600, 'below')).toBe(false);
    expect(targetReached(600, null, 'below')).toBe(false);
  });
});
```

- [ ] **Step 3: Run them and watch them fail.**

Run: `npx vitest run src/utils/stock-monitor.test.ts`
Expected: FAIL — `inferTargetDirection is not a function` / not exported.

- [ ] **Step 4: Implement the utils.** In `src/utils/stock-monitor.ts` add `import type { TargetDirection } from '@shared/schema';` and, after `targetDistance`:

```ts
/** What a new target waits for: a target under the current price waits for a dip. */
export function inferTargetDirection(target: number, current: number | null): TargetDirection {
  return current !== null && isFinite(current) && current > 0 && target < current
    ? 'below'
    : 'above';
}

/**
 * True once the price has crossed the target in its direction: at or under a
 * `below` target, at or over an `above` one (no direction = the old "above").
 */
export function targetReached(
  current: number | null,
  target: number | null,
  direction: TargetDirection | null
): boolean {
  if (current === null || target === null || !isFinite(current) || !isFinite(target)) return false;
  if (target <= 0) return false;
  return direction === 'below' ? current <= target : current >= target;
}
```

Also update the doc comment of `targetDistance`: replace "No reached/crossed semantics — the target is a personal reference point, nothing more." with "Whether the target is reached depends on its direction, see `targetReached`."

- [ ] **Step 5: Run the util tests.**

Run: `npx vitest run src/utils/stock-monitor.test.ts`
Expected: PASS.

- [ ] **Step 6: Wire the API and the mutation.**
  - `src/lib/tauri-api.ts`: import `TargetDirection` in the `@shared/schema` type import and change

```ts
  setTargetPrice: (ticker: string, targetPrice: string | null, targetDirection: TargetDirection | null) =>
    tauriInvoke<WatchedStock>('set_watched_target_price', { ticker, targetPrice, targetDirection }),
```

  - `src/hooks/use-stock-monitor-mutations.ts`: the `targetPriceMutation`'s `mutationFn` takes `{ ticker, targetPrice, targetDirection }: { ticker: string; targetPrice: string | null; targetDirection: TargetDirection | null }` and calls `stockMonitorApi.setTargetPrice(ticker, targetPrice, targetDirection)`.

- [ ] **Step 7: Add the dialog strings.** In `stockMonitor.json` under `target`:
  - cs: `"directionBelow": "Čekám na pokles – koupit / přikoupit"`, `"directionAbove": "Čekám na růst – prodat"`, `"explainBelow": "Upozorním, až cena klesne na {{price}} ({{pct}} od dnešní ceny)."`, `"explainAbove": "Upozorním, až cena vystoupá na {{price}} ({{pct}} od dnešní ceny)."`, `"explainBelowNoPrice": "Upozorním, až cena klesne na {{price}}."`, `"explainAboveNoPrice": "Upozorním, až cena vystoupá na {{price}}."`, `"directionLabel": "Na co cíl čeká"`.
  - en: `"directionBelow": "Waiting for a dip – buy / buy more"`, `"directionAbove": "Waiting for a rise – sell"`, `"explainBelow": "Moony will remind you when the price falls to {{price}} ({{pct}} from today's price)."`, `"explainAbove": "Moony will remind you when the price rises to {{price}} ({{pct}} from today's price)."`, `"explainBelowNoPrice": "Moony will remind you when the price falls to {{price}}."`, `"explainAboveNoPrice": "Moony will remind you when the price rises to {{price}}."`, `"directionLabel": "What the target waits for"`.
  - Also replace the `target.description` text in both files so it no longer says the target is only informative: cs `"Cena, při které chcete jednat. Až ji akcie překročí, Moony ji ukáže mezi termíny na přehledu."`, en `"The price at which you want to act. Once the stock crosses it, Moony lists it among the milestones on the overview."`.

- [ ] **Step 8: Rework the dialog.** In `src/components/stock-monitor/TargetPriceDialog.tsx`:
  - `TargetPriceTarget` gains `targetDirection: TargetDirection | null;`.
  - `onSave` becomes `(ticker: string, targetPrice: string | null, targetDirection: TargetDirection | null) => Promise<unknown>` in both prop types.
  - In `TargetForm` add:

```tsx
  // null = follow the typed price; an existing target keeps its stored direction.
  const [direction, setDirection] = useState<TargetDirection | null>(target.targetDirection);
  const draftValue = parseFloat(draft.trim().replace(',', '.'));
  const knownCurrent = isFinite(current) && current > 0 ? current : null;
  const effective: TargetDirection =
    direction ?? inferTargetDirection(isFinite(draftValue) ? draftValue : 0, knownCurrent);
  const explain =
    isFinite(draftValue) && draftValue > 0
      ? knownCurrent !== null
        ? t(effective === 'below' ? 'target.explainBelow' : 'target.explainAbove', {
            price: formatNativePrice(draftValue, target.currency, fmt.locale),
            pct: fmt.percent(draftValue / knownCurrent - 1, 1, { signed: true }),
          })
        : t(effective === 'below' ? 'target.explainBelowNoPrice' : 'target.explainAboveNoPrice', {
            price: formatNativePrice(draftValue, target.currency, fmt.locale),
          })
      : null;
```

  - `submit` calls `onSave(target.ticker, value, value === null ? null : effective)`; `clear` calls `onSave(target.ticker, null, null)`.
  - Under the hint paragraph, inside the form before `DialogFooter`, render:

```tsx
      <fieldset className="mt-4 grid gap-2">
        <legend className="mb-1.5 text-caption font-700 text-ink-2">{t('target.directionLabel')}</legend>
        <RadioGroup
          value={effective}
          onValueChange={(v) => setDirection(v as TargetDirection)}
          className="grid gap-2"
        >
          {(['below', 'above'] as const).map((value) => (
            <label key={value} className="flex cursor-pointer items-center gap-2 text-body text-ink-2">
              <RadioGroupItem value={value} id={`target-direction-${value}`} />
              {t(value === 'below' ? 'target.directionBelow' : 'target.directionAbove')}
            </label>
          ))}
        </RadioGroup>
        {explain && <p className="m-0 text-micro font-500 text-ink-4">{explain}</p>}
      </fieldset>
```

  with imports `import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';`, `import type { TargetDirection } from '@shared/schema';` and `inferTargetDirection` from `@/utils/stock-monitor`.

- [ ] **Step 9: Make "reached" direction-aware on the pages.**
  - `src/pages/StockMonitor.tsx`:
    - `reached` (stats) becomes `withTarget.filter((x) => targetReached(x.current, x.target, x.row.targetDirection))`; `pending` becomes the complement (`!targetReached(...)`) sorted by `Math.abs(x.target / x.current - 1)` ascending.
    - In the row, `reachedTarget = targetReached(current, targetValue, row.targetDirection)`.
    - In the target cell's non-reached branch, prefix the price with a direction glyph: `<b …>{row.targetDirection === 'below' ? '↓ ' : '↑ '}{price(row.targetPrice, row.currency)}</b>`.
    - `openTarget(row)` passes `targetDirection: row.targetDirection` into the dialog target; the dialog's `onSave` becomes `(ticker, targetPrice, targetDirection) => targetPriceMutation.mutateAsync({ ticker, targetPrice, targetDirection })`.
  - `src/pages/StockMonitorDetail.tsx`:
    - The hero `deltaNote`: `hasTarget ? (targetReached(currentValue, targetValue, detail.targetDirection) ? <span className="text-gain">{t('detail.targetReached')}</span> : distance !== null ? t('detail.toTarget', { pct: fmt.percent(Math.abs(distance) / 100, 1) }) : null) : …` where `currentValue` is the parsed current price (`parseFloat(detail.currentPrice ?? '')`, `null` when not finite). Keep the existing "set target" button branch unchanged.
    - The dialog target gets `targetDirection: detail.targetDirection`, and `onSave` passes the third argument like the list page.

- [ ] **Step 10: Verify.**

Run: `npm run typecheck && npm run lint && npx vitest run src/utils/stock-monitor.test.ts`
Expected: PASS, no errors.

- [ ] **Step 11: Commit.**

```bash
git branch --show-current
git add shared/schema.ts src/lib/tauri-api.ts src/utils/stock-monitor.ts src/utils/stock-monitor.test.ts src/hooks/use-stock-monitor-mutations.ts src/components/stock-monitor/TargetPriceDialog.tsx src/pages/StockMonitor.tsx src/pages/StockMonitorDetail.tsx src/i18n/locales/en/stockMonitor.json src/i18n/locales/cs/stockMonitor.json
git commit -m "feat(stock-monitor): pick and show the direction of a target

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Promotional rate end on bank accounts

**Files:**
- Modify: `src-tauri/src/models/bank_accounts.rs`
- Modify: `src-tauri/src/services/bank_accounts.rs` (+ test schema, `minimal_insert`)
- Modify: `src-tauri/src/commands/bank_accounts.rs` (two SELECTs that build `BankAccount`)
- Modify: `src-tauri/src/services/mcp/bank_accounts.rs` (literal + test schema), `src-tauri/src/services/mcp/savings.rs` if it builds a `BankAccount`
- Modify: `src-tauri/src/bin/seed_demo.rs` (three `InsertBankAccount` literals)
- Modify: `shared/generated-types.ts` (regenerated), `shared/schema.ts`
- Modify: `src/components/bank-accounts/BankAccountFormDialog.tsx`
- Modify: `src/i18n/locales/en/bank_accounts.json`, `src/i18n/locales/cs/bank_accounts.json`

**Interfaces:**
- Consumes: column `bank_accounts.interest_rate_valid_until` (Task 1).
- Produces: `BankAccount.interest_rate_valid_until: Option<i64>` (`interestRateValidUntil`), `InsertBankAccount.interest_rate_valid_until: Option<i64>` (`#[serde(rename = "interestRateValidUntil", default)]`); TS `interestRateValidUntil: number | null` / `interestRateValidUntil?: number | null`.

- [ ] **Step 1: Write the failing service test.** In `src-tauri/src/services/bank_accounts.rs` tests: add `interest_rate_valid_until INTEGER,` to the hand-written `bank_accounts` table, add `interest_rate_valid_until: None,` to `minimal_insert`, then add:

```rust
    #[test]
    fn the_promo_rate_end_is_stored_and_updated() {
        let conn = setup_test_db();
        let mut data = minimal_insert("Spořák");
        data.interest_rate_valid_until = Some(1_790_000_000);
        let created = create_account(&conn, &data).expect("create");
        assert_eq!(created.interest_rate_valid_until, Some(1_790_000_000));
        assert_eq!(
            get_account_by_id(&conn, &created.id).expect("read").interest_rate_valid_until,
            Some(1_790_000_000)
        );
        data.interest_rate_valid_until = None;
        let updated = update_account(&conn, &created.id, &data).expect("update");
        assert_eq!(updated.interest_rate_valid_until, None);
    }
```

(Use the test module's existing setup function name if it differs from `setup_test_db`.)

- [ ] **Step 2: Run it and watch it fail.**

Run: `cd src-tauri && cargo test bank_accounts`
Expected: FAIL to compile (no field `interest_rate_valid_until`).

- [ ] **Step 3: Implement.**
  - `models/bank_accounts.rs`: after `termination_date` in `BankAccount`:

```rust
    /// UTC day the promotional interest rate ends; None when unknown or not promotional.
    #[serde(rename = "interestRateValidUntil")]
    pub interest_rate_valid_until: Option<i64>,
```

    and after `termination_date` in `InsertBankAccount`:

```rust
    /// Overwritten on update like `terminationDate`: the form always sends the stored value.
    #[serde(rename = "interestRateValidUntil", default)]
    pub interest_rate_valid_until: Option<i64>,
```

  - `services/bank_accounts.rs`: `get_account_by_id` selects `interest_rate_valid_until` as the 18th column (index 17) and maps it; `create_account` inserts it (`?16`) and returns it; `update_account` sets `interest_rate_valid_until = ?13` (id moves to `?14`) and returns `data.interest_rate_valid_until`.
  - `commands/bank_accounts.rs`: both SELECTs that construct `BankAccount` append `ba.interest_rate_valid_until` as the last selected column and map it by its index.
  - `services/mcp/bank_accounts.rs`: add `interest_rate_valid_until: None,` to the `InsertBankAccount` literal and the column to its test table. Check `services/mcp/savings.rs`: if it constructs `BankAccount`, select and map the column; if it only builds JSON, leave it.
  - `bin/seed_demo.rs`: add `interest_rate_valid_until: None,` to the three literals.
  - Any other compile error from `cargo build --all-targets` about missing fields: add the field the same way.

- [ ] **Step 4: Run tests, regenerate bindings.**

Run: `cd src-tauri && cargo test && cargo test generate_bindings -- --ignored && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Mirror into `shared/schema.ts`.** `BankAccount`: `interestRateValidUntil: number | null;` after `terminationDate`; `InsertBankAccount`: `interestRateValidUntil?: number | null;` after `terminationDate?`.

- [ ] **Step 6: Add the form field.** In `src/components/bank-accounts/BankAccountFormDialog.tsx`:
  - Local `UpdateBankAccountData` type: add `interestRateValidUntil?: number | null;`.
  - State: `const [rateValidUntil, setRateValidUntil] = useState('');` (ISO `yyyy-mm-dd` or empty). Edit mode: `setRateValidUntil(account.interestRateValidUntil ? isoDay(account.interestRateValidUntil) : '')`; add mode: `setRateValidUntil('')`.
  - Helpers at module level (same as `BondsFormDialog.tsx`):

```ts
const isoDay = (sec: number) => new Date(sec * 1000).toISOString().split('T')[0];
const isoToUtcDaySec = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / 1000);
};
```

  - Both submit payloads: `interestRateValidUntil: rateValidUntil ? isoToUtcDaySec(rateValidUntil) : null,`.
  - Render after the interest-rate / zones blocks (outside the `!hasZoneDesignation` condition, so zone accounts get it too):

```tsx
          <div className="grid gap-2">
            <Label htmlFor="rateValidUntil">{t('fields.rateValidUntil')}</Label>
            <Input
              id="rateValidUntil"
              type="date"
              value={rateValidUntil}
              onChange={(e) => setRateValidUntil(e.target.value)}
            />
            <p className="text-xs text-ink-3">{t('form.rateValidUntilHelp')}</p>
          </div>
```

  - Check the bank-account update path (the mutation hook and `bankAccountsApi`) forwards the whole data object; if it picks fields one by one, add `interestRateValidUntil`.
  - Strings in `bank_accounts.json`: `fields.rateValidUntil` — cs `"Sazba platí do"`, en `"Rate valid until"`; `form.rateValidUntilHelp` — cs `"Konec zvýhodněné sazby. Moony upozorní 14 dní předem."`, en `"When the promotional rate ends. Moony reminds you 14 days ahead."` (put each key in the existing `fields` / `form` objects; `fields` appears twice in the cs file — use the one `fields.interestRate` in the form reads, i.e. the object the form's `t('fields.interestRate')` resolves to).

- [ ] **Step 7: Verify.**

Run: `npm run typecheck && npm run lint && npm test`
Expected: PASS.

- [ ] **Step 8: Commit.**

```bash
git branch --show-current
cd src-tauri && cargo fmt && cd ..
git add src-tauri shared src/components/bank-accounts/BankAccountFormDialog.tsx src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/bank_accounts.json src/hooks src/lib/tauri-api.ts
git commit -m "feat(bank-accounts): remember when a promotional rate ends

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Milestones service core + insurance builder

**Files:**
- Create: `src-tauri/src/models/milestones.rs`
- Modify: `src-tauri/src/models/mod.rs` (`pub mod milestones;` + `pub use milestones::*;`)
- Create: `src-tauri/src/services/milestones/mod.rs`
- Create: `src-tauri/src/services/milestones/insurance.rs`
- Create: `src-tauri/src/services/milestones/test_support.rs`
- Create (stubs filled by Tasks 6–7): `src-tauri/src/services/milestones/{loans,bonds,accounts,upkeep,watchlist}.rs`
- Modify: `src-tauri/src/services/mod.rs` (`pub mod milestones;`)

**Interfaces:**
- Produces:
  - `models::Milestone` (fields per spec §2: `key, kind, stage, tone, source_id, title, due_day, action_day, since_day, amount, currency, reference_amount, direction, count, can_dismiss`), `MILESTONE_STATE_DONE`, `MILESTONE_STATE_SNOOZED`, `validate_milestone_state(key: &str, state: &str) -> Result<()>`, `validate_milestone_key(key: &str) -> Result<()>`.
  - `services::milestones::{list_milestones(conn, today) -> Result<Vec<Milestone>>, set_milestone_state(conn, key, state, today) -> Result<()>, clear_milestone_state(conn, key) -> Result<()>}`.
  - Crate-internal helpers for builders: `DAY`, `STAGE_NOW`, `STAGE_SOON`, `TONE_ACTION`, `TONE_INFO`, `dated_stage`, `milestone(kind, key, source_id, title)`, `dated(kind, source_id, title, due_day, remind_from, today) -> Option<Milestone>`, `money_text(f64) -> String`; each builder is `pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>>`.
  - `test_support::{setup() -> Connection, day(y, m, d) -> i64}`.

- [ ] **Step 1: Create the model.** `src-tauri/src/models/milestones.rs`:

```rust
//! Milestones ("Co vás čeká"): upcoming dates and upkeep reminders gathered from every
//! domain (spec 2026-10-05-milestones-design).

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One occurrence of something the user should act on or know about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Milestone {
    /// Stable occurrence key; the done / snoozed state is stored under it.
    pub key: String,
    /// `insurance_anniversary`, `insurance_end`, `insurance_payment`, `loan_fixation_end`,
    /// `loan_fixation_expired`, `loan_payoff`, `loan_balance_check`, `bond_maturity`,
    /// `bond_coupon`, `account_termination`, `savings_rate_end`, `balances_stale`,
    /// `backup_stale`, `valuation_stale`, `watch_target`.
    pub kind: String,
    /// `now` (in the reminder window) or `soon` (within 90 days).
    pub stage: String,
    /// `action` (counts in the top-bar indicator) or `info`.
    pub tone: String,
    /// Policy, loan, bond, account or property id; the ticker for targets.
    #[serde(rename = "sourceId")]
    pub source_id: Option<String>,
    /// Entity name; empty for the backup and balances items.
    pub title: String,
    /// UTC day of the event.
    #[serde(rename = "dueDay")]
    pub due_day: Option<i64>,
    /// Last day to act when it differs from the event (insurance notice deadline).
    #[serde(rename = "actionDay")]
    pub action_day: Option<i64>,
    /// Day of the last backup, balance update, valuation or balance check.
    #[serde(rename = "sinceDay")]
    pub since_day: Option<i64>,
    /// Payment, coupon, returned principal or target price (TEXT money).
    pub amount: Option<String>,
    pub currency: Option<String>,
    /// Current price of a crossed target.
    #[serde(rename = "referenceAmount")]
    pub reference_amount: Option<String>,
    /// `below` / `above` for targets.
    pub direction: Option<String>,
    /// Number of stale accounts.
    pub count: Option<i64>,
    /// False when only "Odložit" makes sense (the item resolves with the data).
    #[serde(rename = "canDismiss")]
    pub can_dismiss: bool,
}

/// Hidden for good (this occurrence only).
pub const MILESTONE_STATE_DONE: &str = "done";
/// Hidden for a week.
pub const MILESTONE_STATE_SNOOZED: &str = "snoozed";
const MAX_KEY_CHARS: usize = 256;

/// A milestone key is non-empty and at most 256 characters.
pub fn validate_milestone_key(key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.chars().count() > MAX_KEY_CHARS {
        return Err(AppError::Validation(
            "validation.milestoneKeyInvalid".to_string(),
        ));
    }
    Ok(())
}

/// Arguments of `set_milestone_state`: a valid key and `done` or `snoozed`.
pub fn validate_milestone_state(key: &str, state: &str) -> Result<()> {
    validate_milestone_key(key)?;
    if state != MILESTONE_STATE_DONE && state != MILESTONE_STATE_SNOOZED {
        return Err(AppError::Validation(
            "validation.milestoneStateInvalid".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_states_are_validated() {
        assert!(validate_milestone_state("backup_stale", "done").is_ok());
        assert!(validate_milestone_state("backup_stale", "snoozed").is_ok());
        assert!(validate_milestone_state("backup_stale", "gone").is_err());
        assert!(validate_milestone_state("  ", "done").is_err());
        assert!(validate_milestone_key(&"k".repeat(257)).is_err());
    }
}
```

Register it in `models/mod.rs` next to the other modules (`pub mod milestones;` and `pub use milestones::*;`, alphabetical like the neighbours).

- [ ] **Step 2: Create the test schema.** `src-tauri/src/services/milestones/test_support.rs`:

```rust
//! In-memory schema with the tables the milestone builders read.

use rusqlite::Connection;

/// UTC midnight of a calendar date, in unix seconds.
pub(crate) fn day(y: i32, m: u32, d: u32) -> i64 {
    chrono::NaiveDate::from_ymd_opt(y, m, d)
        .expect("valid date")
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc()
        .timestamp()
}

pub(crate) fn setup() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    conn.execute_batch(
        r#"
        CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE user_profile (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE insurance_policies (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL DEFAULT 'other',
            provider TEXT NOT NULL DEFAULT '',
            policy_name TEXT NOT NULL,
            policy_number TEXT NOT NULL DEFAULT '',
            start_date INTEGER NOT NULL,
            end_date INTEGER,
            payment_frequency TEXT NOT NULL,
            one_time_payment TEXT,
            one_time_payment_currency TEXT DEFAULT 'CZK',
            regular_payment TEXT NOT NULL DEFAULT '0',
            regular_payment_currency TEXT NOT NULL DEFAULT 'CZK',
            limits TEXT DEFAULT '[]',
            notes TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE loans (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            principal TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'CZK',
            interest_rate TEXT NOT NULL DEFAULT '0',
            interest_rate_validity_date INTEGER,
            monthly_payment TEXT NOT NULL DEFAULT '0',
            start_date INTEGER NOT NULL DEFAULT (unixepoch()),
            end_date INTEGER,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            balance_anchor_amount TEXT,
            balance_anchor_date INTEGER
        );
        CREATE TABLE bonds (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            isin TEXT,
            coupon_value TEXT NOT NULL,
            interest_rate TEXT NOT NULL DEFAULT '0',
            maturity_date INTEGER,
            currency TEXT NOT NULL DEFAULT 'CZK',
            quantity TEXT NOT NULL DEFAULT '1',
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE bank_accounts (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            termination_date INTEGER,
            interest_rate_valid_until INTEGER,
            exclude_from_balance INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE real_estate (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            purchase_date INTEGER,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE real_estate_valuations (
            id TEXT PRIMARY KEY,
            real_estate_id TEXT NOT NULL,
            value TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'CZK',
            valued_at INTEGER NOT NULL,
            note TEXT,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE watched_stocks (
            id TEXT PRIMARY KEY,
            ticker TEXT NOT NULL UNIQUE,
            target_price TEXT,
            target_direction TEXT,
            notes TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE stock_data (
            id TEXT PRIMARY KEY,
            ticker TEXT UNIQUE,
            original_price TEXT,
            currency TEXT
        );
        CREATE TABLE milestone_states (
            key TEXT PRIMARY KEY,
            state TEXT NOT NULL CHECK (state IN ('done', 'snoozed')),
            until_day INTEGER,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        "#,
    )
    .expect("schema");
    conn
}
```

- [ ] **Step 3: Create the module skeleton with empty builders.** Each of `loans.rs`, `bonds.rs`, `accounts.rs`, `upkeep.rs`, `watchlist.rs` starts as:

```rust
use rusqlite::Connection;

use crate::error::Result;
use crate::models::Milestone;

pub(super) fn build(_conn: &Connection, _today: i64) -> Result<Vec<Milestone>> {
    Ok(Vec::new())
}
```

(Tasks 6 and 7 replace these bodies; give each file a `//!` line naming its domain.)

`src-tauri/src/services/milestones/mod.rs`:

```rust
//! Milestones gathered from every domain (spec 2026-10-05-milestones-design): contract
//! dates, payments, data upkeep and crossed watchlist targets. Each domain has a builder;
//! this module gives dated items their stage, drops the occurrences the user marked done or
//! still snoozes, and orders the list for the dashboard card and the top-bar indicator.

mod accounts;
mod bonds;
mod insurance;
mod loans;
#[cfg(test)]
pub(crate) mod test_support;
mod upkeep;
mod watchlist;

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::models::{Milestone, MILESTONE_STATE_DONE, MILESTONE_STATE_SNOOZED};

pub(crate) const DAY: i64 = 86_400;
/// `soon` covers events up to this many days ahead.
const SOON_HORIZON_DAYS: i64 = 90;
/// "Odložit" hides an occurrence for a week.
const SNOOZE_DAYS: i64 = 7;

pub(crate) const STAGE_NOW: &str = "now";
pub(crate) const STAGE_SOON: &str = "soon";
pub(crate) const TONE_ACTION: &str = "action";
pub(crate) const TONE_INFO: &str = "info";

type Builder = fn(&Connection, i64) -> Result<Vec<Milestone>>;

/// Stage of a dated item: `now` from `remind_from` on, `soon` while the event is at most
/// 90 days away, otherwise None (not shown yet).
pub(crate) fn dated_stage(today: i64, due_day: i64, remind_from: i64) -> Option<&'static str> {
    if today >= remind_from {
        Some(STAGE_NOW)
    } else if due_day <= today + SOON_HORIZON_DAYS * DAY {
        Some(STAGE_SOON)
    } else {
        None
    }
}

/// A milestone with every optional field empty: `now`, `action`, dismissable.
pub(crate) fn milestone(kind: &str, key: String, source_id: Option<&str>, title: &str) -> Milestone {
    Milestone {
        key,
        kind: kind.to_string(),
        stage: STAGE_NOW.to_string(),
        tone: TONE_ACTION.to_string(),
        source_id: source_id.map(str::to_string),
        title: title.to_string(),
        due_day: None,
        action_day: None,
        since_day: None,
        amount: None,
        currency: None,
        reference_amount: None,
        direction: None,
        count: None,
        can_dismiss: true,
    }
}

/// A dated milestone keyed `<kind>:<source_id>:<due_day>`; None while it is too far ahead.
pub(crate) fn dated(
    kind: &str,
    source_id: &str,
    title: &str,
    due_day: i64,
    remind_from: i64,
    today: i64,
) -> Option<Milestone> {
    let stage = dated_stage(today, due_day, remind_from)?;
    let mut m = milestone(kind, format!("{kind}:{source_id}:{due_day}"), Some(source_id), title);
    m.stage = stage.to_string();
    m.due_day = Some(due_day);
    Some(m)
}

/// Money as TEXT with two decimals (ADR 0001).
pub(crate) fn money_text(value: f64) -> String {
    format!("{:.2}", (value * 100.0).round() / 100.0)
}

/// Every milestone as of `today` (UTC day), without the occurrences marked done or still
/// snoozed, in display order.
pub fn list_milestones(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let builders: [(&str, Builder); 6] = [
        ("insurance", insurance::build),
        ("loans", loans::build),
        ("bonds", bonds::build),
        ("accounts", accounts::build),
        ("upkeep", upkeep::build),
        ("watchlist", watchlist::build),
    ];
    let mut all = Vec::new();
    for (name, build) in builders {
        match build(conn, today) {
            Ok(items) => all.extend(items),
            // One broken record must not hide every other reminder.
            Err(e) => log::warn!("[MILESTONES] {name} skipped: {e}"),
        }
    }
    let states = load_states(conn)?;
    all.retain(|m| match states.get(&m.key) {
        Some((state, _)) if state == MILESTONE_STATE_DONE => false,
        Some((_, Some(until))) => *until <= today,
        _ => true,
    });
    all.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)).then_with(|| a.title.cmp(&b.title)));
    Ok(all)
}

/// `now` first: dated items by deadline (else event day), then targets, then upkeep;
/// `soon` items by event day.
fn sort_key(m: &Milestone) -> (u8, u8, i64) {
    if m.stage == STAGE_SOON {
        return (1, 0, m.due_day.unwrap_or(i64::MAX));
    }
    let group = if m.due_day.is_some() {
        0
    } else if m.kind == "watch_target" {
        1
    } else {
        2
    };
    (0, group, m.action_day.or(m.due_day).unwrap_or(i64::MAX))
}

fn load_states(conn: &Connection) -> Result<HashMap<String, (String, Option<i64>)>> {
    let mut stmt = conn.prepare("SELECT key, state, until_day FROM milestone_states")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            (r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
}

/// Mark one occurrence done (hidden for good) or snoozed (hidden for a week from `today`).
/// Callers validate the arguments first (`validate_milestone_state`).
pub fn set_milestone_state(conn: &Connection, key: &str, state: &str, today: i64) -> Result<()> {
    let until = (state == MILESTONE_STATE_SNOOZED).then_some(today + SNOOZE_DAYS * DAY);
    conn.execute(
        "INSERT INTO milestone_states (key, state, until_day, updated_at)
         VALUES (?1, ?2, ?3, unixepoch())
         ON CONFLICT(key) DO UPDATE SET state = excluded.state, until_day = excluded.until_day,
             updated_at = excluded.updated_at",
        params![key.trim(), state, until],
    )?;
    Ok(())
}

/// Forget the state of one occurrence (the toast's "Vrátit"); an unknown key is fine.
pub fn clear_milestone_state(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM milestone_states WHERE key = ?1", [key.trim()])?;
    Ok(())
}
```

Add `pub mod milestones;` to `src-tauri/src/services/mod.rs` (alphabetical).

- [ ] **Step 4: Write the failing core + insurance tests.** Append to `mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::test_support::{day, setup};
    use super::*;

    fn policy(conn: &Connection, id: &str, start: i64, end: Option<i64>, frequency: &str, payment: &str) {
        conn.execute(
            "INSERT INTO insurance_policies (id, policy_name, start_date, end_date, payment_frequency, regular_payment)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, format!("Pojistka {id}"), start, end, frequency, payment],
        )
        .expect("policy");
    }

    fn kinds(list: &[Milestone]) -> Vec<&str> {
        list.iter().map(|m| m.kind.as_str()).collect()
    }

    #[test]
    fn dated_stage_boundaries() {
        let today = day(2026, 10, 5);
        assert_eq!(dated_stage(today, today + 10 * DAY, today), Some(STAGE_NOW));
        assert_eq!(dated_stage(today, today + 10 * DAY, today + DAY), Some(STAGE_SOON));
        assert_eq!(dated_stage(today, today + 90 * DAY, today + 50 * DAY), Some(STAGE_SOON));
        assert_eq!(dated_stage(today, today + 91 * DAY, today + 50 * DAY), None);
    }

    #[test]
    fn an_anniversary_reminds_ten_weeks_ahead_with_the_notice_deadline() {
        let conn = setup();
        // Anniversary 24 Dec 2026; notice deadline 12 Nov; reminder from 15 Oct.
        policy(&conn, "p1", day(2020, 12, 24), None, "monthly", "300");
        let before = list_milestones(&conn, day(2026, 10, 14)).unwrap();
        assert_eq!(kinds(&before), vec!["insurance_anniversary"]);
        assert_eq!(before[0].stage, STAGE_SOON);
        let on = list_milestones(&conn, day(2026, 10, 15)).unwrap();
        assert_eq!(on[0].stage, STAGE_NOW);
        assert_eq!(on[0].tone, TONE_ACTION);
        assert_eq!(on[0].due_day, Some(day(2026, 12, 24)));
        assert_eq!(on[0].action_day, Some(day(2026, 11, 12)));
        assert_eq!(on[0].key, format!("insurance_anniversary:p1:{}", day(2026, 12, 24)));
        let late = list_milestones(&conn, day(2026, 11, 13)).unwrap();
        assert_eq!(late[0].tone, TONE_INFO, "the notice period has passed");
    }

    #[test]
    fn monthly_payments_are_routine_but_yearly_ones_remind_two_weeks_ahead() {
        let conn = setup();
        policy(&conn, "m", day(2025, 3, 20), None, "monthly", "300");
        policy(&conn, "y", day(2025, 10, 20), None, "annually", "4200");
        let list = list_milestones(&conn, day(2026, 10, 6)).unwrap();
        let payment: Vec<_> = list.iter().filter(|m| m.kind == "insurance_payment").collect();
        assert_eq!(payment.len(), 1);
        assert_eq!(payment[0].source_id.as_deref(), Some("y"));
        assert_eq!(payment[0].stage, STAGE_NOW);
        assert_eq!(payment[0].amount.as_deref(), Some("4200"));
        assert_eq!(payment[0].currency.as_deref(), Some("CZK"));
    }

    #[test]
    fn an_ending_contract_reminds_sixty_days_ahead_and_has_no_anniversary_after_it() {
        let conn = setup();
        policy(&conn, "e", day(2024, 1, 15), Some(day(2026, 12, 1)), "one_time", "0");
        let list = list_milestones(&conn, day(2026, 10, 5)).unwrap();
        assert_eq!(kinds(&list), vec!["insurance_end"]);
        assert_eq!(list[0].stage, STAGE_NOW);
    }

    #[test]
    fn ended_and_inactive_policies_are_skipped() {
        let conn = setup();
        policy(&conn, "old", day(2020, 1, 1), Some(day(2026, 1, 1)), "annually", "100");
        policy(&conn, "off", day(2020, 11, 1), None, "annually", "100");
        conn.execute("UPDATE insurance_policies SET status = 'cancelled' WHERE id = 'off'", [])
            .unwrap();
        assert!(list_milestones(&conn, day(2026, 10, 5)).unwrap().is_empty());
    }

    #[test]
    fn anniversaries_clamp_to_the_month_end() {
        let conn = setup();
        // Started 29 Feb 2024: the 2027 anniversary is 28 Feb.
        policy(&conn, "leap", day(2024, 2, 29), None, "monthly", "100");
        let list = list_milestones(&conn, day(2026, 12, 20)).unwrap();
        assert_eq!(list[0].due_day, Some(day(2027, 2, 28)));
    }

    #[test]
    fn done_hides_for_good_and_snooze_for_a_week() {
        let conn = setup();
        policy(&conn, "p1", day(2020, 12, 24), None, "monthly", "300");
        let today = day(2026, 10, 20);
        let key = list_milestones(&conn, today).unwrap()[0].key.clone();

        set_milestone_state(&conn, &key, MILESTONE_STATE_SNOOZED, today).unwrap();
        assert!(list_milestones(&conn, today + 6 * DAY).unwrap().is_empty());
        assert_eq!(list_milestones(&conn, today + 7 * DAY).unwrap().len(), 1);

        set_milestone_state(&conn, &key, MILESTONE_STATE_DONE, today).unwrap();
        assert!(list_milestones(&conn, today + 30 * DAY).unwrap().is_empty());

        clear_milestone_state(&conn, &key).unwrap();
        assert_eq!(list_milestones(&conn, today).unwrap().len(), 1);
    }

    #[test]
    fn now_items_come_first_by_deadline_then_soon_items() {
        let conn = setup();
        policy(&conn, "b", day(2020, 12, 24), None, "monthly", "1"); // now, deadline 12 Nov
        policy(&conn, "a", day(2020, 12, 10), None, "monthly", "1"); // now, deadline 29 Oct
        policy(&conn, "c", day(2021, 1, 2), None, "monthly", "1"); // soon (remind from 24 Oct)
        let list = list_milestones(&conn, day(2026, 10, 20)).unwrap();
        let ids: Vec<_> = list.iter().map(|m| m.source_id.clone().unwrap()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }
}
```

- [ ] **Step 5: Run them and watch them fail.**

Run: `cd src-tauri && cargo test milestones`
Expected: FAIL to compile — `insurance::build` does not exist.

- [ ] **Step 6: Implement the insurance builder.** `src-tauri/src/services/milestones/insurance.rs`:

```rust
//! Insurance milestones: anniversaries (with the notice deadline), contract ends and
//! non-monthly payments. The date rules mirror `src/utils/insurance.ts`: payments and
//! anniversaries fall on the start's day of month, clamped to the month's end. Unlike the
//! insurance page, the first anniversary is never the start day itself.

use chrono::{DateTime, Datelike};
use rusqlite::Connection;

use super::{dated, DAY, TONE_INFO};
use crate::error::Result;
use crate::models::{Milestone, PaymentFrequency};
use crate::services::loan_amortization::{day_floor, due_day};

/// A policy can be cancelled to its anniversary with at least six weeks' notice.
const NOTICE_DAYS: i64 = 42;
/// Remind four weeks before the notice deadline (about ten weeks before the anniversary).
const ANNIVERSARY_LEAD_DAYS: i64 = 28;
const END_LEAD_DAYS: i64 = 60;
const PAYMENT_LEAD_DAYS: i64 = 14;
/// Never walk more than this many payment periods.
const MAX_PERIODS: u32 = 1_200;

struct Policy {
    id: String,
    name: String,
    start: i64,
    end: Option<i64>,
    frequency: String,
    payment: String,
    currency: String,
}

/// Whole calendar months from `from` to `to`; negative when `to` is earlier.
fn months_diff(from: i64, to: i64) -> Option<i64> {
    let a = DateTime::from_timestamp(from, 0)?.date_naive();
    let b = DateTime::from_timestamp(to, 0)?.date_naive();
    Some((i64::from(b.year()) - i64::from(a.year())) * 12 + i64::from(b.month()) - i64::from(a.month()))
}

/// Next yearly anniversary of `start` after `today`, at least one year after the start;
/// None when the contract ends on or before it.
fn next_anniversary(start: i64, end: Option<i64>, today: i64) -> Option<i64> {
    let start = day_floor(start);
    let years = (months_diff(start, today)? / 12).max(1);
    for y in years..=years + 2 {
        let due = due_day(start, u32::try_from(y * 12).ok()?);
        if due > today {
            return match end {
                Some(e) if due >= e => None,
                _ => Some(due),
            };
        }
    }
    None
}

/// Next payment after `today` of a policy paid every `period` months; None past the end.
fn next_payment_day(start: i64, end: Option<i64>, period: u32, today: i64) -> Option<i64> {
    let start = day_floor(start);
    let mut k = u32::try_from((months_diff(start, today)? / i64::from(period) - 1).max(0)).ok()?;
    for _ in 0..=MAX_PERIODS {
        let due = due_day(start, k.checked_mul(period)?);
        if due > today {
            return match end {
                Some(e) if due > e => None,
                _ => Some(due),
            };
        }
        k += 1;
    }
    None
}

/// Months between payments worth a reminder; monthly and one-time policies have none.
fn reminder_period(frequency: &str) -> Option<u32> {
    match PaymentFrequency::parse(frequency)? {
        PaymentFrequency::Quarterly => Some(3),
        PaymentFrequency::SemiAnnually => Some(6),
        PaymentFrequency::Annually => Some(12),
        PaymentFrequency::Monthly | PaymentFrequency::OneTime => None,
    }
}

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT id, policy_name, start_date, end_date, payment_frequency, regular_payment,
                regular_payment_currency
         FROM insurance_policies WHERE status = 'active'",
    )?;
    let policies = stmt
        .query_map([], |r| {
            Ok(Policy {
                id: r.get(0)?,
                name: r.get(1)?,
                start: r.get(2)?,
                end: r.get(3)?,
                frequency: r.get(4)?,
                payment: r.get(5)?,
                currency: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    for p in policies {
        let end = p.end.map(day_floor);
        if end.is_some_and(|e| e < today) {
            continue;
        }
        if let Some(anniversary) = next_anniversary(p.start, end, today) {
            let action_day = anniversary - NOTICE_DAYS * DAY;
            let remind_from = action_day - ANNIVERSARY_LEAD_DAYS * DAY;
            if let Some(mut m) =
                dated("insurance_anniversary", &p.id, &p.name, anniversary, remind_from, today)
            {
                m.action_day = Some(action_day);
                if today > action_day {
                    m.tone = TONE_INFO.to_string();
                }
                out.push(m);
            }
        }
        if let Some(e) = end {
            if let Some(m) = dated("insurance_end", &p.id, &p.name, e, e - END_LEAD_DAYS * DAY, today) {
                out.push(m);
            }
        }
        // Only to decide whether a payment reminder is worth showing.
        let amount: f64 = p.payment.trim().parse().unwrap_or(0.0);
        if let Some(period) = reminder_period(&p.frequency).filter(|_| amount > 0.0) {
            if let Some(due) = next_payment_day(p.start, end, period, today) {
                let remind_from = due - PAYMENT_LEAD_DAYS * DAY;
                if let Some(mut m) = dated("insurance_payment", &p.id, &p.name, due, remind_from, today) {
                    m.amount = Some(p.payment.trim().to_string());
                    m.currency = Some(p.currency.clone());
                    out.push(m);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::day;
    use super::*;

    // Mirrors of `src/utils/insurance.test.ts` cases for nextPaymentDay / nextAnniversary.
    #[test]
    fn a_payment_on_the_31st_clamps_to_the_month_end() {
        let start = day(2026, 1, 31);
        assert_eq!(next_payment_day(start, None, 3, day(2026, 2, 1)), Some(day(2026, 4, 30)));
    }

    #[test]
    fn no_payment_after_the_end() {
        let start = day(2025, 1, 10);
        assert_eq!(next_payment_day(start, Some(day(2026, 12, 31)), 12, day(2026, 1, 11)), None);
    }

    #[test]
    fn the_first_anniversary_is_a_year_after_the_start() {
        assert_eq!(next_anniversary(day(2026, 9, 1), None, day(2026, 8, 1)), Some(day(2027, 9, 1)));
    }
}
```

If `src/utils/insurance.test.ts` has cases for `nextPaymentDay` / `nextAnniversary` with other inputs, add one Rust test per such case with the same inputs and expected day (except cases that expect the start day itself as the anniversary — see the module doc).

- [ ] **Step 7: Run the tests.**

Run: `cd src-tauri && cargo test milestones && cargo clippy --all-targets -- -D warnings`
Expected: PASS (the stub builders return nothing).

- [ ] **Step 8: Commit.**

```bash
git branch --show-current
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/models src-tauri/src/services/milestones src-tauri/src/services/mod.rs
git commit -m "feat(milestones): service core with insurance anniversaries, ends and payments

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Loan and bond milestones

**Files:**
- Modify: `src-tauri/src/services/milestones/loans.rs`
- Modify: `src-tauri/src/services/milestones/bonds.rs`

**Interfaces:**
- Consumes: `dated`, `milestone`, `money_text`, `DAY`, `TONE_INFO` from `super`; `test_support::{setup, day}`; `crate::services::loans::{list_loans(conn, today) -> Result<Vec<Loan>>, loan_schedule(conn, id, today) -> Result<LoanSchedule>}` (`LoanSchedule.payoff_day: Option<i64>`); `crate::services::loan_amortization::day_floor`.
- Produces: kinds `loan_fixation_end`, `loan_fixation_expired`, `loan_payoff`, `loan_balance_check`, `bond_maturity`, `bond_coupon` per spec §3.

- [ ] **Step 1: Write the failing loan tests.** Replace `loans.rs` with the implementation from Step 3 *without* the `build` body first, or simply write the tests below at the bottom of the stub file and run them to see them fail:

```rust
#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::super::{STAGE_NOW, TONE_INFO};
    use super::*;
    use rusqlite::params;

    fn loan(conn: &Connection, id: &str, start: i64, validity: Option<i64>, payment: &str, anchor: Option<i64>) {
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, interest_rate_validity_date,
                                monthly_payment, start_date, balance_anchor_amount, balance_anchor_date)
             VALUES (?1, ?2, '1000000', '5', ?3, ?4, ?5, ?6, ?7)",
            params![id, format!("Úvěr {id}"), validity, payment, start,
                    anchor.map(|_| "900000"), anchor],
        )
        .expect("loan");
    }

    #[test]
    fn the_fixation_end_reminds_six_months_ahead() {
        let conn = setup();
        let today = day(2026, 10, 5);
        loan(&conn, "h", day(2026, 1, 1), Some(day(2027, 3, 1)), "8000", Some(day(2026, 6, 1)));
        let list = build(&conn, today).unwrap();
        let fix = list.iter().find(|m| m.kind == "loan_fixation_end").expect("fixation");
        assert_eq!(fix.stage, STAGE_NOW);
        assert_eq!(fix.due_day, Some(day(2027, 3, 1)));
        // Two months later than six months ahead: nothing yet.
        let conn2 = setup();
        loan(&conn2, "h", day(2026, 1, 1), Some(day(2027, 6, 1)), "8000", Some(day(2026, 6, 1)));
        assert!(build(&conn2, today).unwrap().iter().all(|m| m.kind != "loan_fixation_end"));
    }

    #[test]
    fn an_expired_fixation_asks_for_the_new_rate_and_cannot_be_dismissed() {
        let conn = setup();
        loan(&conn, "h", day(2026, 1, 1), Some(day(2026, 9, 1)), "8000", Some(day(2026, 6, 1)));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let expired = list.iter().find(|m| m.kind == "loan_fixation_expired").expect("expired");
        assert!(!expired.can_dismiss);
        assert_eq!(expired.due_day, Some(day(2026, 9, 1)));
    }

    #[test]
    fn a_payoff_within_a_month_is_info() {
        let conn = setup();
        // 5 000 on 1 Aug, 2 000 a month: 1 000 left after 1 Oct, paid off on 1 Nov.
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date,
                                balance_anchor_amount, balance_anchor_date)
             VALUES ('car', 'Auto', '20000', '0', '2000', ?1, '5000', ?2)",
            params![day(2026, 1, 1), day(2026, 8, 1)],
        )
        .unwrap();
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let payoff = list.iter().find(|m| m.kind == "loan_payoff").expect("payoff");
        assert_eq!(payoff.tone, TONE_INFO);
        assert_eq!(payoff.stage, STAGE_NOW);
    }

    #[test]
    fn a_balance_unchecked_for_a_year_asks_for_a_statement() {
        let conn = setup();
        loan(&conn, "old", day(2020, 1, 1), None, "8000", None);
        loan(&conn, "fresh", day(2020, 1, 1), None, "8000", Some(day(2026, 3, 1)));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let checks: Vec<_> = list.iter().filter(|m| m.kind == "loan_balance_check").collect();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].source_id.as_deref(), Some("old"));
        assert_eq!(checks[0].since_day, Some(day(2020, 1, 1)));
        assert!(!checks[0].can_dismiss);
    }

    #[test]
    fn matured_loans_are_skipped() {
        let conn = setup();
        conn.execute(
            "INSERT INTO loans (id, name, principal, monthly_payment, start_date, end_date, interest_rate_validity_date)
             VALUES ('done', 'Hotovo', '1000', '100', ?1, ?2, ?3)",
            params![day(2019, 1, 1), day(2025, 1, 1), day(2024, 1, 1)],
        )
        .unwrap();
        assert!(build(&conn, day(2026, 10, 5)).unwrap().is_empty());
    }

    #[test]
    fn a_fixation_inside_the_window_is_now_even_when_near() {
        let conn = setup();
        let today = day(2026, 10, 5);
        // Ends in 80 days; the reminder window is 180 days, so it is "now", not "soon".
        loan(&conn, "h", day(2026, 1, 1), Some(today + 80 * 86_400), "8000", Some(day(2026, 6, 1)));
        let fix = build(&conn, today)
            .unwrap()
            .into_iter()
            .find(|m| m.kind == "loan_fixation_end")
            .unwrap();
        assert_eq!(fix.stage, STAGE_NOW);
    }
}
```

Run: `cd src-tauri && cargo test milestones::loans`
Expected: FAIL (the stub returns nothing).

- [ ] **Step 2: Write the failing bond tests** at the bottom of `bonds.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::super::{STAGE_NOW, TONE_INFO};
    use super::*;
    use rusqlite::params;

    fn bond(conn: &Connection, id: &str, value: &str, qty: &str, rate: &str, maturity: Option<i64>) {
        conn.execute(
            "INSERT INTO bonds (id, name, coupon_value, quantity, interest_rate, maturity_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, format!("Dluhopis {id}"), value, qty, rate, maturity],
        )
        .expect("bond");
    }

    #[test]
    fn a_maturity_reminds_a_month_ahead_with_the_face_value() {
        let conn = setup();
        bond(&conn, "b", "10000", "10", "0", Some(day(2026, 11, 1)));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "bond_maturity");
        assert_eq!(list[0].stage, STAGE_NOW);
        assert_eq!(list[0].amount.as_deref(), Some("100000.00"));
        assert_eq!(list[0].currency.as_deref(), Some("CZK"));
    }

    #[test]
    fn a_coupon_falls_on_the_maturity_anniversary_and_is_info() {
        let conn = setup();
        bond(&conn, "b", "10000", "10", "4.5", Some(day(2030, 10, 9)));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let coupon = list.iter().find(|m| m.kind == "bond_coupon").expect("coupon");
        assert_eq!(coupon.due_day, Some(day(2026, 10, 9)));
        assert_eq!(coupon.tone, TONE_INFO);
        assert_eq!(coupon.amount.as_deref(), Some("4500.00"));
    }

    #[test]
    fn the_last_coupon_belongs_to_the_maturity() {
        assert_eq!(next_coupon_day(day(2026, 10, 9), day(2026, 10, 5)), None);
        assert_eq!(next_coupon_day(day(2027, 10, 9), day(2026, 10, 5)), Some(day(2026, 10, 9)));
        assert_eq!(next_coupon_day(day(2027, 10, 1), day(2026, 10, 5)), None);
    }

    #[test]
    fn a_29_february_maturity_pays_on_28_february_in_common_years() {
        assert_eq!(next_coupon_day(day(2028, 2, 29), day(2026, 10, 5)), Some(day(2027, 2, 28)));
    }

    #[test]
    fn matured_bonds_and_far_dates_are_skipped() {
        let conn = setup();
        bond(&conn, "old", "1000", "1", "3", Some(day(2026, 1, 1)));
        bond(&conn, "far", "1000", "1", "0", Some(day(2027, 6, 1)));
        assert!(build(&conn, day(2026, 10, 5)).unwrap().is_empty());
    }
}
```

Run: `cd src-tauri && cargo test milestones::bonds`
Expected: FAIL to compile (`next_coupon_day` missing).

- [ ] **Step 3: Implement `loans.rs`.**

```rust
//! Loan milestones: the end of a fixed rate, an expired fixation without a new rate, the
//! last payment and a yearly balance check against a statement.

use rusqlite::Connection;

use super::{dated, milestone, DAY, TONE_INFO};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;
use crate::services::loans as loans_service;

const FIXATION_LEAD_DAYS: i64 = 180;
const PAYOFF_LEAD_DAYS: i64 = 30;
const BALANCE_CHECK_DAYS: i64 = 365;

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut out = Vec::new();
    for loan in loans_service::list_loans(conn, today)? {
        let outstanding: f64 = loan.outstanding_balance.trim().parse().unwrap_or(0.0);
        let matured = outstanding <= 0.0 || loan.end_date.is_some_and(|e| day_floor(e) < today);
        if matured {
            continue;
        }

        if let Some(validity) = loan.interest_rate_validity_date.map(day_floor) {
            if validity > today {
                let remind_from = validity - FIXATION_LEAD_DAYS * DAY;
                if let Some(m) =
                    dated("loan_fixation_end", &loan.id, &loan.name, validity, remind_from, today)
                {
                    out.push(m);
                }
            } else {
                // A rate change clears the validity date, which resolves this item.
                let mut m = milestone(
                    "loan_fixation_expired",
                    format!("loan_fixation_expired:{}:{validity}", loan.id),
                    Some(&loan.id),
                    &loan.name,
                );
                m.due_day = Some(validity);
                m.can_dismiss = false;
                out.push(m);
            }
        }

        match loans_service::loan_schedule(conn, &loan.id, today) {
            Ok(schedule) => {
                if let Some(payoff) = schedule.payoff_day.filter(|d| *d > today) {
                    let remind_from = payoff - PAYOFF_LEAD_DAYS * DAY;
                    if let Some(mut m) =
                        dated("loan_payoff", &loan.id, &loan.name, payoff, remind_from, today)
                    {
                        m.tone = TONE_INFO.to_string();
                        out.push(m);
                    }
                }
            }
            Err(e) => log::warn!("[MILESTONES] loan schedule skipped: {e}"),
        }

        // Every extra payment, rate change and balance check moves the anchor.
        let checked = day_floor(loan.balance_anchor_date.unwrap_or(loan.start_date));
        if checked < today - BALANCE_CHECK_DAYS * DAY {
            let mut m = milestone(
                "loan_balance_check",
                format!("loan_balance_check:{}", loan.id),
                Some(&loan.id),
                &loan.name,
            );
            m.since_day = Some(checked);
            m.can_dismiss = false;
            out.push(m);
        }
    }
    Ok(out)
}
```

- [ ] **Step 4: Implement `bonds.rs`.**

```rust
//! Bond milestones: maturity (the principal comes back) and the yearly coupon, paid on the
//! maturity's anniversary (as on the bonds page).

use chrono::{DateTime, Datelike, NaiveDate};
use rusqlite::Connection;

use super::{dated, money_text, DAY, TONE_INFO};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;

const MATURITY_LEAD_DAYS: i64 = 30;
const COUPON_LEAD_DAYS: i64 = 7;

struct BondRow {
    id: String,
    name: String,
    coupon_value: String,
    quantity: String,
    currency: String,
    rate: String,
    maturity: i64,
}

/// `day` moved to `year`; 29 February becomes 28 February in a common year.
fn in_year(day: i64, year: i32) -> Option<i64> {
    let date = DateTime::from_timestamp(day, 0)?.date_naive();
    let shifted = NaiveDate::from_ymd_opt(year, date.month(), date.day())
        .or_else(|| NaiveDate::from_ymd_opt(year, date.month(), date.day() - 1))?;
    Some(shifted.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// Next yearly anniversary of the maturity strictly after `today` and strictly before the
/// maturity (the last coupon is paid with the principal).
pub(super) fn next_coupon_day(maturity: i64, today: i64) -> Option<i64> {
    let year = DateTime::from_timestamp(today, 0)?.date_naive().year();
    let this_year = in_year(maturity, year)?;
    let next = if this_year > today { this_year } else { in_year(maturity, year + 1)? };
    (next < maturity).then_some(next)
}

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, coupon_value, quantity, currency, interest_rate, maturity_date
         FROM bonds WHERE maturity_date IS NOT NULL",
    )?;
    let bonds = stmt
        .query_map([], |r| {
            Ok(BondRow {
                id: r.get(0)?,
                name: r.get(1)?,
                coupon_value: r.get(2)?,
                quantity: r.get(3)?,
                currency: r.get(4)?,
                rate: r.get(5)?,
                maturity: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    for b in bonds {
        let maturity = day_floor(b.maturity);
        if maturity < today {
            continue;
        }
        let value: f64 = b.coupon_value.trim().parse().unwrap_or(0.0);
        let quantity: f64 = b.quantity.trim().parse().unwrap_or(1.0);
        let face = value * quantity;
        let remind_from = maturity - MATURITY_LEAD_DAYS * DAY;
        if let Some(mut m) = dated("bond_maturity", &b.id, &b.name, maturity, remind_from, today) {
            m.amount = Some(money_text(face));
            m.currency = Some(b.currency.clone());
            out.push(m);
        }
        let rate: f64 = b.rate.trim().parse().unwrap_or(0.0);
        if rate > 0.0 && face > 0.0 {
            if let Some(coupon) = next_coupon_day(maturity, today) {
                let remind_from = coupon - COUPON_LEAD_DAYS * DAY;
                if let Some(mut m) = dated("bond_coupon", &b.id, &b.name, coupon, remind_from, today) {
                    m.tone = TONE_INFO.to_string();
                    m.amount = Some(money_text(face * rate / 100.0));
                    m.currency = Some(b.currency.clone());
                    out.push(m);
                }
            }
        }
    }
    Ok(out)
}
```

- [ ] **Step 5: Run the tests.**

Run: `cd src-tauri && cargo test milestones && cargo clippy --all-targets -- -D warnings`
Expected: PASS. If `a_payoff_within_a_month_is_info` fails because the schedule computes a different payoff day, print `loans_service::loan_schedule(...)` in the test, and adjust the fixture so the payoff lands within 30 days of 5 Oct 2026 — the assertion (info tone, `now` stage) stays.

- [ ] **Step 6: Commit.**

```bash
git branch --show-current
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/services/milestones
git commit -m "feat(milestones): loan fixation, payoff and balance check; bond maturity and coupon

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Account, upkeep and watchlist milestones

**Files:**
- Modify: `src-tauri/src/services/milestones/accounts.rs`
- Modify: `src-tauri/src/services/milestones/upkeep.rs`
- Modify: `src-tauri/src/services/milestones/watchlist.rs`

**Interfaces:**
- Consumes: helpers from `super`, `test_support`, `crate::services::onboarding::FLAG_LAST_BACKUP_AT` (`"backup.lastCreatedAt"` in `app_config`, value = unix seconds as text), `crate::models::{TARGET_BELOW, TARGET_ABOVE}`.
- Produces: kinds `account_termination`, `savings_rate_end`, `balances_stale`, `backup_stale`, `valuation_stale`, `watch_target` per spec §3.

- [ ] **Step 1: Write the failing tests.** At the bottom of each file:

`accounts.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::super::STAGE_NOW;
    use super::*;
    use rusqlite::params;

    fn account(conn: &Connection, id: &str, termination: Option<i64>, rate_end: Option<i64>, exclude: bool, updated: i64) {
        conn.execute(
            "INSERT INTO bank_accounts (id, name, termination_date, interest_rate_valid_until, exclude_from_balance, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, format!("Účet {id}"), termination, rate_end, exclude as i32, updated],
        )
        .expect("account");
    }

    #[test]
    fn termination_and_promo_rate_end_remind_ahead() {
        let conn = setup();
        let today = day(2026, 10, 5);
        account(&conn, "t", Some(day(2026, 10, 30)), None, false, today);
        account(&conn, "r", None, Some(day(2026, 10, 15)), false, today);
        let list = build(&conn, today).unwrap();
        let mut kinds: Vec<_> = list.iter().map(|m| m.kind.as_str()).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["account_termination", "savings_rate_end"]);
        assert!(list.iter().all(|m| m.stage == STAGE_NOW));
    }

    #[test]
    fn stale_balances_are_one_item_without_excluded_or_terminated_accounts() {
        let conn = setup();
        let today = day(2026, 10, 5);
        account(&conn, "a", None, None, false, day(2026, 8, 1));
        account(&conn, "b", None, None, false, day(2026, 7, 1));
        account(&conn, "fresh", None, None, false, day(2026, 9, 20));
        account(&conn, "excluded", None, None, true, day(2026, 1, 1));
        account(&conn, "closed", Some(day(2026, 5, 1)), None, false, day(2026, 1, 1));
        let list = build(&conn, today).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "balances_stale");
        assert_eq!(list[0].count, Some(2));
        assert_eq!(list[0].since_day, Some(day(2026, 7, 1)));
        assert!(!list[0].can_dismiss);
    }
}
```

`upkeep.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::*;
    use rusqlite::params;

    #[test]
    fn a_backup_older_than_thirty_days_is_stale() {
        let conn = setup();
        let today = day(2026, 10, 5);
        conn.execute("INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)", [day(2026, 1, 1)]).unwrap();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('backup.lastCreatedAt', ?1)",
            [(day(2026, 8, 24) + 3_600).to_string()],
        )
        .unwrap();
        let list = build(&conn, today).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "backup_stale");
        assert_eq!(list[0].since_day, Some(day(2026, 8, 24)));
        conn.execute("UPDATE app_config SET value = ?1", [day(2026, 9, 20).to_string()]).unwrap();
        assert!(build(&conn, today).unwrap().is_empty());
    }

    #[test]
    fn a_profile_without_a_backup_gets_two_weeks_of_grace() {
        let conn = setup();
        conn.execute("INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)", [day(2026, 9, 25)]).unwrap();
        assert!(build(&conn, day(2026, 10, 5)).unwrap().is_empty());
        let list = build(&conn, day(2026, 10, 9)).unwrap();
        assert_eq!(list[0].kind, "backup_stale");
        assert_eq!(list[0].since_day, None);
    }

    #[test]
    fn a_property_valued_more_than_a_year_ago_is_stale() {
        let conn = setup();
        conn.execute("INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)", [day(2026, 10, 1)]).unwrap();
        conn.execute(
            "INSERT INTO real_estate (id, name, purchase_date, created_at) VALUES
                ('old', 'Byt', ?1, ?1), ('new', 'Chata', NULL, ?2)",
            params![day(2020, 5, 1), day(2026, 1, 1)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO real_estate_valuations (id, real_estate_id, value, valued_at) VALUES ('v1', 'old', '1', ?1)",
            [day(2025, 3, 12)],
        )
        .unwrap();
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "valuation_stale");
        assert_eq!(list[0].source_id.as_deref(), Some("old"));
        assert_eq!(list[0].since_day, Some(day(2025, 3, 12)));
    }
}
```

`watchlist.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::super::test_support::setup;
    use super::*;

    fn watched(conn: &Connection, ticker: &str, target: &str, direction: &str, price: Option<&str>) {
        conn.execute(
            "INSERT INTO watched_stocks (id, ticker, target_price, target_direction) VALUES (?1, ?1, ?2, ?3)",
            [ticker, target, direction],
        )
        .unwrap();
        if let Some(p) = price {
            conn.execute(
                "INSERT INTO stock_data (id, ticker, original_price, currency) VALUES (?1, ?1, ?2, 'USD')",
                [ticker, p],
            )
            .unwrap();
        }
    }

    #[test]
    fn only_targets_crossed_in_their_direction_count() {
        let conn = setup();
        watched(&conn, "CAT", "600", "below", Some("845"));
        watched(&conn, "DIP", "600", "below", Some("598"));
        watched(&conn, "UP", "900", "above", Some("905"));
        watched(&conn, "WAIT", "900", "above", Some("845"));
        watched(&conn, "NOPRICE", "10", "above", None);
        let list = build(&conn, 0).unwrap();
        let tickers: Vec<_> = list.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(tickers, vec!["DIP", "UP"]);
        assert_eq!(list[0].key, "watch_target:DIP:600:below");
        assert_eq!(list[0].amount.as_deref(), Some("600"));
        assert_eq!(list[0].reference_amount.as_deref(), Some("598"));
        assert_eq!(list[0].direction.as_deref(), Some("below"));
        assert_eq!(list[0].currency.as_deref(), Some("USD"));
    }
}
```

Run: `cd src-tauri && cargo test milestones`
Expected: FAIL (stubs return nothing).

- [ ] **Step 2: Implement `accounts.rs`.**

```rust
//! Bank account milestones: a term account ending, a promotional rate ending, and one
//! reminder for balances nobody updated for a month.

use rusqlite::Connection;

use super::{dated, milestone, DAY};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;

const TERMINATION_LEAD_DAYS: i64 = 28;
const RATE_END_LEAD_DAYS: i64 = 14;
const STALE_BALANCE_DAYS: i64 = 30;

struct AccountRow {
    id: String,
    name: String,
    termination: Option<i64>,
    rate_end: Option<i64>,
    excluded: bool,
    updated_at: i64,
}

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, termination_date, interest_rate_valid_until, exclude_from_balance, updated_at
         FROM bank_accounts ORDER BY name",
    )?;
    let accounts = stmt
        .query_map([], |r| {
            Ok(AccountRow {
                id: r.get(0)?,
                name: r.get(1)?,
                termination: r.get(2)?,
                rate_end: r.get(3)?,
                excluded: r.get::<_, i32>(4)? != 0,
                updated_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    let mut stale = 0_i64;
    let mut oldest: Option<i64> = None;
    for a in accounts {
        let termination = a.termination.map(day_floor);
        if termination.is_some_and(|t| t < today) {
            continue;
        }
        if let Some(t) = termination {
            if let Some(m) = dated("account_termination", &a.id, &a.name, t, t - TERMINATION_LEAD_DAYS * DAY, today) {
                out.push(m);
            }
        }
        if let Some(r) = a
            .rate_end
            .map(day_floor)
            .filter(|r| *r >= today && !termination.is_some_and(|t| t < *r))
        {
            if let Some(m) = dated("savings_rate_end", &a.id, &a.name, r, r - RATE_END_LEAD_DAYS * DAY, today) {
                out.push(m);
            }
        }
        let updated = day_floor(a.updated_at);
        if !a.excluded && updated < today - STALE_BALANCE_DAYS * DAY {
            stale += 1;
            oldest = Some(oldest.map_or(updated, |o| o.min(updated)));
        }
    }
    if stale > 0 {
        let mut m = milestone("balances_stale", "balances_stale".to_string(), None, "");
        m.count = Some(stale);
        m.since_day = oldest;
        m.can_dismiss = false;
        out.push(m);
    }
    Ok(out)
}
```

- [ ] **Step 3: Implement `upkeep.rs`.**

```rust
//! Data upkeep milestones: a backup older than a month and a property not revalued for a year.

use rusqlite::{Connection, OptionalExtension};

use super::{milestone, DAY};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;
use crate::services::onboarding::FLAG_LAST_BACKUP_AT;

const BACKUP_STALE_DAYS: i64 = 30;
/// A new profile is not asked for its first backup straight away.
const FIRST_BACKUP_GRACE_DAYS: i64 = 14;
const VALUATION_STALE_DAYS: i64 = 365;

fn backup(conn: &Connection, today: i64) -> Result<Option<Milestone>> {
    let last: Option<i64> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [FLAG_LAST_BACKUP_AT],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.trim().parse().ok());
    let since_day = match last {
        Some(at) => {
            let last_day = day_floor(at);
            if last_day >= today - BACKUP_STALE_DAYS * DAY {
                return Ok(None);
            }
            Some(last_day)
        }
        None => {
            let created: Option<i64> =
                conn.query_row("SELECT MIN(created_at) FROM user_profile", [], |r| r.get(0))?;
            match created {
                Some(c) if day_floor(c) <= today - FIRST_BACKUP_GRACE_DAYS * DAY => None,
                _ => return Ok(None),
            }
        }
    };
    let mut m = milestone("backup_stale", "backup_stale".to_string(), None, "");
    m.since_day = since_day;
    m.can_dismiss = false;
    Ok(Some(m))
}

fn valuations(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.name,
                (SELECT MAX(v.valued_at) FROM real_estate_valuations v WHERE v.real_estate_id = r.id),
                r.purchase_date, r.created_at
         FROM real_estate r ORDER BY r.name",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (id, name, valued, purchased, created) in rows {
        let last = day_floor(valued.or(purchased).unwrap_or(created));
        if last < today - VALUATION_STALE_DAYS * DAY {
            let mut m = milestone("valuation_stale", format!("valuation_stale:{id}"), Some(&id), &name);
            m.since_day = Some(last);
            m.can_dismiss = false;
            out.push(m);
        }
    }
    Ok(out)
}

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut out = Vec::new();
    if let Some(m) = backup(conn, today)? {
        out.push(m);
    }
    out.extend(valuations(conn, today)?);
    Ok(out)
}
```

- [ ] **Step 4: Implement `watchlist.rs`.**

```rust
//! Watchlist milestones: a target the price crossed in the direction it waits for.

use rusqlite::Connection;

use super::milestone;
use crate::error::Result;
use crate::models::{Milestone, TARGET_ABOVE, TARGET_BELOW};

pub(super) fn build(conn: &Connection, _today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT w.ticker, w.target_price, w.target_direction, sd.original_price, sd.currency
         FROM watched_stocks w LEFT JOIN stock_data sd ON sd.ticker = w.ticker
         WHERE w.target_price IS NOT NULL ORDER BY w.ticker",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (ticker, target_text, direction, price_text, currency) in rows {
        let target: f64 = target_text.trim().parse().unwrap_or(0.0);
        let Some(price_text) = price_text else { continue };
        let price: f64 = price_text.trim().parse().unwrap_or(0.0);
        if target <= 0.0 || price <= 0.0 {
            continue;
        }
        // A target without a direction keeps the meaning of earlier versions.
        let direction = direction.unwrap_or_else(|| TARGET_ABOVE.to_string());
        let crossed = if direction == TARGET_BELOW { price <= target } else { price >= target };
        if !crossed {
            continue;
        }
        let key = format!("watch_target:{ticker}:{}:{direction}", target_text.trim());
        let mut m = milestone("watch_target", key, Some(&ticker), &ticker);
        m.amount = Some(target_text.trim().to_string());
        m.reference_amount = Some(price_text.trim().to_string());
        m.currency = currency;
        m.direction = Some(direction);
        out.push(m);
    }
    Ok(out)
}
```

- [ ] **Step 5: Run the tests.**

Run: `cd src-tauri && cargo test milestones && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git branch --show-current
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/services/milestones
git commit -m "feat(milestones): account dates, backup, valuations, balances and crossed targets

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Commands, wire contract, API and query hooks

**Files:**
- Create: `src-tauri/src/commands/milestones.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`
- Modify: `shared/generated-types.ts` (regenerated), `shared/schema.ts`
- Modify: `src/lib/tauri-api.ts`, `src/lib/queryClient.ts`
- Create: `src/hooks/use-milestones.ts`, `src/hooks/use-milestone-mutations.ts`
- Modify: `src/components/settings/DataSection.tsx` (refresh after a backup)
- Modify: `src/i18n/locales/en/common.json`, `src/i18n/locales/cs/common.json` (validation keys)

**Interfaces:**
- Consumes: `services::milestones::{list_milestones, set_milestone_state, clear_milestone_state}`, `models::{Milestone, validate_milestone_state, validate_milestone_key}` (Task 5).
- Produces:
  - Commands `get_milestones() -> Vec<Milestone>`, `set_milestone_state(key: String, state: String)`, `clear_milestone_state(key: String)`.
  - TS: `Milestone`, `MilestoneKind`, `MilestoneStage`, `MilestoneTone`, `MilestoneState` in `shared/schema.ts`; `milestonesApi.{list(), setState(key, state), clearState(key)}`; `useMilestones()` (query key `['milestones']`); `useMilestoneActions(): { markDone(m: Milestone): void; snooze(m: Milestone): void }`.

- [ ] **Step 1: Commands.** `src-tauri/src/commands/milestones.rs`:

```rust
//! Milestones commands ("Co vás čeká") — thin wrappers over `services::milestones`.

use tauri::State;

use crate::db::Database;
use crate::error::Result;
use crate::models::{validate_milestone_key, validate_milestone_state, Milestone};
use crate::services::loan_amortization::today_utc_day;
use crate::services::milestones;

/// Every milestone as of today, without the done and snoozed occurrences.
#[tauri::command]
pub async fn get_milestones(db: State<'_, Database>) -> Result<Vec<Milestone>> {
    db.with_conn(|conn| milestones::list_milestones(conn, today_utc_day()))
}

/// Mark one occurrence `done` or `snoozed` (for a week).
#[tauri::command]
pub async fn set_milestone_state(db: State<'_, Database>, key: String, state: String) -> Result<()> {
    validate_milestone_state(&key, &state)?;
    db.with_conn(|conn| milestones::set_milestone_state(conn, &key, &state, today_utc_day()))
}

/// Undo `set_milestone_state`.
#[tauri::command]
pub async fn clear_milestone_state(db: State<'_, Database>, key: String) -> Result<()> {
    validate_milestone_key(&key)?;
    db.with_conn(|conn| milestones::clear_milestone_state(conn, &key))
}
```

  - `commands/mod.rs`: `pub mod milestones;` (alphabetical).
  - `lib.rs` `generate_handler![]`, after the onboarding group:

```rust
            // Milestones (services::milestones)
            commands::milestones::get_milestones,
            commands::milestones::set_milestone_state,
            commands::milestones::clear_milestone_state,
```

  - `bindings.rs` `collect_types()`: `types.register::<crate::models::Milestone>();` next to `OnboardingProgress`.
  - Validation strings in `common.json` under `validation`: en `"milestoneKeyInvalid": "This reminder could not be found."`, `"milestoneStateInvalid": "This reminder could not be updated."`; cs `"milestoneKeyInvalid": "Tuto připomínku se nepodařilo najít."`, `"milestoneStateInvalid": "Tuto připomínku se nepodařilo upravit."`.

- [ ] **Step 2: Build and regenerate.**

Run: `cd src-tauri && cargo build && cargo test generate_bindings -- --ignored && cargo clippy -- -D warnings`
Expected: PASS; `shared/generated-types.ts` gains `Milestone`.

- [ ] **Step 3: Mirror the contract.** In `shared/schema.ts`, after the onboarding types:

```ts
// Milestones ("Co vás čeká") — src-tauri/src/models/milestones.rs
export type MilestoneKind =
  | 'insurance_anniversary'
  | 'insurance_end'
  | 'insurance_payment'
  | 'loan_fixation_end'
  | 'loan_fixation_expired'
  | 'loan_payoff'
  | 'loan_balance_check'
  | 'bond_maturity'
  | 'bond_coupon'
  | 'account_termination'
  | 'savings_rate_end'
  | 'balances_stale'
  | 'backup_stale'
  | 'valuation_stale'
  | 'watch_target';
export type MilestoneStage = 'now' | 'soon';
export type MilestoneTone = 'action' | 'info';
export type MilestoneState = 'done' | 'snoozed';

export interface Milestone {
  /** Stable occurrence key; the done / snoozed state is stored under it. */
  key: string;
  kind: MilestoneKind;
  stage: MilestoneStage;
  /** `action` counts in the top-bar indicator; `info` is shown on the card only. */
  tone: MilestoneTone;
  /** Policy, loan, bond, account or property id; the ticker for targets. */
  sourceId: string | null;
  /** Entity name; empty for the backup and balances items. */
  title: string;
  /** UTC day of the event. */
  dueDay: number | null;
  /** Last day to act when it differs from the event (insurance notice deadline). */
  actionDay: number | null;
  /** Day of the last backup, balance update, valuation or balance check. */
  sinceDay: number | null;
  /** Payment, coupon, returned principal or target price (TEXT money). */
  amount: string | null;
  currency: string | null;
  /** Current price of a crossed target. */
  referenceAmount: string | null;
  direction: TargetDirection | null;
  /** Number of stale accounts. */
  count: number | null;
  /** False when only "Odložit" is offered. */
  canDismiss: boolean;
}
```

- [ ] **Step 4: API wrappers.** In `src/lib/tauri-api.ts` add `Milestone, MilestoneState` to the `@shared/schema` type import and, after `onboardingApi`:

```ts
// ============================================================================
// Milestones API — "Co vás čeká" card and the top-bar indicator
// ============================================================================

export const milestonesApi = {
  list: () => tauriInvoke<Milestone[]>('get_milestones'),
  setState: (key: string, state: MilestoneState) =>
    tauriInvoke<void>('set_milestone_state', { key, state }),
  clearState: (key: string) => tauriInvoke<void>('clear_milestone_state', { key }),
};
```

and `milestones: milestonesApi,` in the combined `api` object.

- [ ] **Step 5: Refresh after every mutation.** `src/lib/queryClient.ts`:

```ts
import { MutationCache, QueryClient } from '@tanstack/react-query';

// Any successful write can add, move or resolve a milestone (an edited policy, a new
// target, a recorded loan event), so every mutation refreshes ["milestones"].
const mutationCache = new MutationCache({
  onSuccess: () => {
    void queryClient.invalidateQueries({ queryKey: ['milestones'] });
  },
});
```

and pass `mutationCache,` as the first property of the `new QueryClient({...})` options (keep the existing doc comment and defaults).

- [ ] **Step 6: Hooks.** `src/hooks/use-milestones.ts`:

```ts
import { useQuery } from '@tanstack/react-query';
import { milestonesApi } from '@/lib/tauri-api';

/**
 * Milestones for the dashboard card and the top-bar indicator. Every successful
 * mutation invalidates the key (query client); the interval catches price
 * refreshes and the change of day.
 */
export function useMilestones() {
  return useQuery({
    queryKey: ['milestones'],
    queryFn: () => milestonesApi.list(),
    staleTime: 0,
    refetchOnMount: 'always',
    refetchInterval: 5 * 60_000,
  });
}
```

`src/hooks/use-milestone-mutations.ts`:

```ts
import { useMutation } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import type { Milestone, MilestoneState } from '@shared/schema';
import { queryClient } from '@/lib/queryClient';
import { milestonesApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

/** "Vyřízeno" and "Odložit o týden", each with a "Vrátit" toast. */
export function useMilestoneActions() {
  const { t } = useTranslation('milestones');
  const { t: tc } = useTranslation('common');

  // Milestone state never affects net worth — the milestones key only.
  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['milestones'] });
  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const undo = useMutation({
    mutationFn: (key: string) => milestonesApi.clearState(key),
    onSuccess: invalidate,
    onError,
  });

  const setState = useMutation({
    mutationFn: ({ key, state }: { key: string; state: MilestoneState }) =>
      milestonesApi.setState(key, state),
    onSuccess: (_data, { key, state }) => {
      invalidate();
      toast(t(state === 'done' ? 'toast.done' : 'toast.snoozed'), {
        action: { label: t('actions.undo'), onClick: () => undo.mutate(key) },
      });
    },
    onError,
  });

  return {
    markDone: (m: Milestone) => setState.mutate({ key: m.key, state: 'done' }),
    snooze: (m: Milestone) => setState.mutate({ key: m.key, state: 'snoozed' }),
  };
}
```

(The `milestones` namespace is created in Task 9; until then `t()` returns keys, which typechecks.)

- [ ] **Step 7: Refresh after a backup.** In `src/components/settings/DataSection.tsx`, after `await dataApi.createBackup(path)` succeeds, add `void queryClient.invalidateQueries({ queryKey: ['milestones'] });` (import `queryClient` from `@/lib/queryClient` if the file does not use `useQueryClient` already; if it does, use that client).

- [ ] **Step 8: Verify.**

Run: `npm run typecheck && npm run lint && npm test`
Expected: PASS.

- [ ] **Step 9: Commit.**

```bash
git branch --show-current
cd src-tauri && cargo fmt && cd ..
git add src-tauri shared src/lib src/hooks src/components/settings/DataSection.tsx src/i18n/locales/en/common.json src/i18n/locales/cs/common.json
git commit -m "feat(milestones): commands, wire contract and query hooks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Frontend milestone utils and the `milestones` namespace

**Files:**
- Create: `src/utils/milestones.ts`, `src/utils/milestones.test.ts`
- Create: `src/i18n/locales/en/milestones.json`, `src/i18n/locales/cs/milestones.json`
- Modify: `src/i18n/index.ts` (register the namespace)

**Interfaces:**
- Consumes: `Milestone` (Task 8).
- Produces: `isAttention(m)`, `groupMilestones(list) -> { now: Milestone[]; soon: Milestone[] }`, `attentionCount(list) -> number`, `milestoneHref(m) -> string`, `daysBetween(from, to) -> number`, `referenceDay(m) -> number | null`, `relativeLabel(days) -> { key: RelativeKey; count: number }` with `type RelativeKey = 'today' | 'tomorrow' | 'inDays' | 'inMonths' | 'daysAgo' | 'monthsAgo'`; i18n namespace `milestones`.

- [ ] **Step 1: Write the failing tests.** `src/utils/milestones.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import type { Milestone } from '@shared/schema';
import {
  attentionCount,
  daysBetween,
  groupMilestones,
  milestoneHref,
  referenceDay,
  relativeLabel,
} from './milestones';

const DAY = 86_400;

function m(overrides: Partial<Milestone>): Milestone {
  return {
    key: 'k',
    kind: 'insurance_anniversary',
    stage: 'now',
    tone: 'action',
    sourceId: 'p1',
    title: 'Úrazová pojistka',
    dueDay: null,
    actionDay: null,
    sinceDay: null,
    amount: null,
    currency: null,
    referenceAmount: null,
    direction: null,
    count: null,
    canDismiss: true,
    ...overrides,
  };
}

describe('groupMilestones', () => {
  it('puts now + action items under "act now" and everything else under "soon", keeping order', () => {
    const a = m({ key: 'a' });
    const b = m({ key: 'b', stage: 'soon' });
    const c = m({ key: 'c', tone: 'info' });
    const d = m({ key: 'd' });
    const { now, soon } = groupMilestones([a, b, c, d]);
    expect(now.map((x) => x.key)).toEqual(['a', 'd']);
    expect(soon.map((x) => x.key)).toEqual(['b', 'c']);
    expect(attentionCount([a, b, c, d])).toBe(2);
  });
});

describe('milestoneHref', () => {
  it('links each kind to its page', () => {
    expect(milestoneHref(m({ kind: 'insurance_payment', sourceId: 'p1' }))).toBe('/insurance/p1');
    expect(milestoneHref(m({ kind: 'loan_balance_check', sourceId: 'l1' }))).toBe('/loans/l1');
    expect(milestoneHref(m({ kind: 'bond_coupon', sourceId: 'b1' }))).toBe('/bonds');
    expect(milestoneHref(m({ kind: 'savings_rate_end', sourceId: 'a1' }))).toBe('/bank-accounts/a1');
    expect(milestoneHref(m({ kind: 'balances_stale', sourceId: null }))).toBe('/bank-accounts');
    expect(milestoneHref(m({ kind: 'valuation_stale', sourceId: 'r1' }))).toBe('/real-estate/r1');
    expect(milestoneHref(m({ kind: 'backup_stale', sourceId: null }))).toBe('/settings/data');
    expect(milestoneHref(m({ kind: 'watch_target', sourceId: 'BRK.B' }))).toBe('/stock-monitor/BRK.B');
  });
});

describe('referenceDay', () => {
  it('measures to the deadline while acting is possible, else to the event or the last update', () => {
    expect(referenceDay(m({ dueDay: 100 * DAY, actionDay: 58 * DAY }))).toBe(58 * DAY);
    expect(referenceDay(m({ dueDay: 100 * DAY, actionDay: 58 * DAY, tone: 'info' }))).toBe(100 * DAY);
    expect(referenceDay(m({ kind: 'backup_stale', sinceDay: 7 * DAY }))).toBe(7 * DAY);
    expect(referenceDay(m({ kind: 'backup_stale' }))).toBeNull();
  });
});

describe('daysBetween and relativeLabel', () => {
  it('counts whole UTC days', () => {
    expect(daysBetween(10 * DAY + 3_600, 12 * DAY)).toBe(2);
    expect(daysBetween(12 * DAY, 10 * DAY)).toBe(-2);
  });
  it('labels today, tomorrow, days, months and the past', () => {
    expect(relativeLabel(0)).toEqual({ key: 'today', count: 0 });
    expect(relativeLabel(1)).toEqual({ key: 'tomorrow', count: 1 });
    expect(relativeLabel(38)).toEqual({ key: 'inDays', count: 38 });
    expect(relativeLabel(150)).toEqual({ key: 'inMonths', count: 5 });
    expect(relativeLabel(-41)).toEqual({ key: 'daysAgo', count: 41 });
    expect(relativeLabel(-400)).toEqual({ key: 'monthsAgo', count: 13 });
  });
});
```

Run: `npx vitest run src/utils/milestones.test.ts`
Expected: FAIL — cannot find module `./milestones`.

- [ ] **Step 2: Implement.** `src/utils/milestones.ts`:

```ts
import type { Milestone } from '@shared/schema';

const DAY = 86_400;

/** "Teď jednat": in the reminder window and asking for an action. */
export function isAttention(m: Milestone): boolean {
  return m.stage === 'now' && m.tone === 'action';
}

/** "Teď jednat" and "Brzy" (soon items and info items), each keeping the backend order. */
export function groupMilestones(list: readonly Milestone[]): {
  now: Milestone[];
  soon: Milestone[];
} {
  const now: Milestone[] = [];
  const soon: Milestone[] = [];
  for (const m of list) (isAttention(m) ? now : soon).push(m);
  return { now, soon };
}

/** The number shown in the top-bar indicator. */
export function attentionCount(list: readonly Milestone[]): number {
  return list.filter(isAttention).length;
}

/** The page a milestone row leads to. */
export function milestoneHref(m: Milestone): string {
  const id = encodeURIComponent(m.sourceId ?? '');
  switch (m.kind) {
    case 'insurance_anniversary':
    case 'insurance_end':
    case 'insurance_payment':
      return `/insurance/${id}`;
    case 'loan_fixation_end':
    case 'loan_fixation_expired':
    case 'loan_payoff':
    case 'loan_balance_check':
      return `/loans/${id}`;
    case 'bond_maturity':
    case 'bond_coupon':
      return '/bonds';
    case 'account_termination':
    case 'savings_rate_end':
      return `/bank-accounts/${id}`;
    case 'balances_stale':
      return '/bank-accounts';
    case 'valuation_stale':
      return `/real-estate/${id}`;
    case 'backup_stale':
      return '/settings/data';
    case 'watch_target':
      return `/stock-monitor/${id}`;
  }
}

/** Whole UTC days from `from` to `to` (unix seconds); negative when `to` is earlier. */
export function daysBetween(from: number, to: number): number {
  return Math.floor(to / DAY) - Math.floor(from / DAY);
}

/**
 * The day the row's relative label measures: the deadline while acting is still
 * possible, else the event, else the last backup / update / valuation.
 */
export function referenceDay(m: Milestone): number | null {
  if (m.actionDay !== null && m.tone === 'action') return m.actionDay;
  return m.dueDay ?? m.sinceDay;
}

export type RelativeKey = 'today' | 'tomorrow' | 'inDays' | 'inMonths' | 'daysAgo' | 'monthsAgo';

/** "dnes", "zítra", "za 38 dní", "za 5 měsíců", "před 41 dny", "před 13 měsíci". */
export function relativeLabel(days: number): { key: RelativeKey; count: number } {
  if (days === 0) return { key: 'today', count: 0 };
  if (days === 1) return { key: 'tomorrow', count: 1 };
  if (days < 0) {
    const ago = -days;
    return ago <= 60
      ? { key: 'daysAgo', count: ago }
      : { key: 'monthsAgo', count: Math.round(ago / 30) };
  }
  return days <= 60
    ? { key: 'inDays', count: days }
    : { key: 'inMonths', count: Math.round(days / 30) };
}
```

Run: `npx vitest run src/utils/milestones.test.ts`
Expected: PASS.

- [ ] **Step 3: Create the namespace.** `src/i18n/locales/cs/milestones.json`:

```json
{
  "card": {
    "title": "Co vás čeká",
    "now": "Teď jednat",
    "soon": "Brzy",
    "showAll": "Zobrazit vše ({{count}})",
    "showLess": "Zobrazit méně"
  },
  "status": {
    "count_one": "{{count}} k vyřízení",
    "count_few": "{{count}} k vyřízení",
    "count_other": "{{count}} k vyřízení",
    "title": "Teď jednat",
    "openOverview": "Otevřít přehled"
  },
  "actions": {
    "snooze": "Odložit o týden",
    "done": "Vyřízeno",
    "undo": "Vrátit"
  },
  "toast": {
    "done": "Označeno jako vyřízené",
    "snoozed": "Odloženo o týden"
  },
  "relative": {
    "today": "dnes",
    "tomorrow": "zítra",
    "inDays_one": "za {{count}} den",
    "inDays_few": "za {{count}} dny",
    "inDays_other": "za {{count}} dní",
    "inMonths_one": "za {{count}} měsíc",
    "inMonths_few": "za {{count}} měsíce",
    "inMonths_other": "za {{count}} měsíců",
    "daysAgo_one": "před {{count}} dnem",
    "daysAgo_few": "před {{count}} dny",
    "daysAgo_other": "před {{count}} dny",
    "monthsAgo_one": "před {{count}} měsícem",
    "monthsAgo_few": "před {{count}} měsíci",
    "monthsAgo_other": "před {{count}} měsíci"
  },
  "titles": {
    "backup_stale": "Záloha dat",
    "balances_stale": "Zůstatky účtů"
  },
  "sub": {
    "insurance_anniversary": "Výročí {{date}} · výpověď nejpozději {{deadline}}",
    "insurance_anniversary_passed": "Výročí {{date}} · lhůta pro výpověď uplynula",
    "insurance_end": "Smlouva končí {{date}}",
    "insurance_payment": "Platba {{amount}} · {{date}}",
    "loan_fixation_end": "Konec fixace {{date}} · čas porovnat nabídky",
    "loan_fixation_expired": "Fixace skončila {{date}} · doplňte novou sazbu",
    "loan_payoff": "Poslední splátka {{date}}",
    "loan_balance_check": "Zůstatek ověřen {{date}} · porovnejte s výpisem",
    "bond_maturity": "Splatnost {{date}} · vrátí se {{amount}}",
    "bond_coupon": "Kupón {{amount}} · {{date}}",
    "account_termination": "Účet končí {{date}}",
    "savings_rate_end": "Zvýhodněná sazba končí {{date}}",
    "backup_stale": "Poslední záloha {{date}}",
    "backup_never": "Zatím žádná záloha",
    "balances_stale_one": "{{count}} účet bez aktualizace přes 30 dní",
    "balances_stale_few": "{{count}} účty bez aktualizace přes 30 dní",
    "balances_stale_other": "{{count}} účtů bez aktualizace přes 30 dní",
    "valuation_stale": "Odhad hodnoty z {{date}} · přeceňte",
    "watch_target_below": "Cena {{price}} klesla na cíl {{target}}",
    "watch_target_above": "Cena {{price}} dosáhla cíle {{target}}"
  }
}
```

`src/i18n/locales/en/milestones.json`:

```json
{
  "card": {
    "title": "What's coming up",
    "now": "Act now",
    "soon": "Coming up",
    "showAll": "Show all ({{count}})",
    "showLess": "Show less"
  },
  "status": {
    "count_one": "{{count}} to handle",
    "count_other": "{{count}} to handle",
    "title": "Act now",
    "openOverview": "Open overview"
  },
  "actions": {
    "snooze": "Snooze for a week",
    "done": "Done",
    "undo": "Undo"
  },
  "toast": {
    "done": "Marked as done",
    "snoozed": "Snoozed for a week"
  },
  "relative": {
    "today": "today",
    "tomorrow": "tomorrow",
    "inDays_one": "in {{count}} day",
    "inDays_other": "in {{count}} days",
    "inMonths_one": "in {{count}} month",
    "inMonths_other": "in {{count}} months",
    "daysAgo_one": "{{count}} day ago",
    "daysAgo_other": "{{count}} days ago",
    "monthsAgo_one": "{{count}} month ago",
    "monthsAgo_other": "{{count}} months ago"
  },
  "titles": {
    "backup_stale": "Data backup",
    "balances_stale": "Account balances"
  },
  "sub": {
    "insurance_anniversary": "Anniversary {{date}} · cancel by {{deadline}}",
    "insurance_anniversary_passed": "Anniversary {{date}} · the notice period has passed",
    "insurance_end": "Contract ends {{date}}",
    "insurance_payment": "Payment {{amount}} · {{date}}",
    "loan_fixation_end": "Fixed rate ends {{date}} · time to compare offers",
    "loan_fixation_expired": "Fixed rate ended {{date}} · enter the new rate",
    "loan_payoff": "Last payment {{date}}",
    "loan_balance_check": "Balance checked {{date}} · compare with a statement",
    "bond_maturity": "Matures {{date}} · {{amount}} comes back",
    "bond_coupon": "Coupon {{amount}} · {{date}}",
    "account_termination": "Account ends {{date}}",
    "savings_rate_end": "Promotional rate ends {{date}}",
    "backup_stale": "Last backup {{date}}",
    "backup_never": "No backup yet",
    "balances_stale_one": "{{count}} account not updated for over 30 days",
    "balances_stale_other": "{{count}} accounts not updated for over 30 days",
    "valuation_stale": "Valued on {{date}} · revalue it",
    "watch_target_below": "Price {{price}} fell to the target {{target}}",
    "watch_target_above": "Price {{price}} reached the target {{target}}"
  }
}
```

Register in `src/i18n/index.ts`: import `enMilestones` / `csMilestones` next to the other imports, add `'milestones'` to `NAMESPACES`, and add `milestones: enMilestones` / `milestones: csMilestones` to the `resources` map, mirroring how `stockMonitor` is registered.

- [ ] **Step 4: Verify.**

Run: `node -e "JSON.parse(require('fs').readFileSync('src/i18n/locales/en/milestones.json'))" && node -e "JSON.parse(require('fs').readFileSync('src/i18n/locales/cs/milestones.json'))" && npm run typecheck && npx vitest run src/utils/milestones.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git branch --show-current
git add src/utils/milestones.ts src/utils/milestones.test.ts src/i18n
git commit -m "feat(milestones): grouping, links, relative labels and copy

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Dashboard card and top-bar indicator

**Files:**
- Create: `src/components/milestones/MilestoneList.tsx`
- Create: `src/components/dashboard/UpcomingCard.tsx`
- Create: `src/components/shell/MilestonesStatus.tsx`
- Modify: `src/pages/Dashboard.tsx`, `src/components/shell/TopBar.tsx`

**Interfaces:**
- Consumes: `useMilestones`, `useMilestoneActions` (Task 8); `groupMilestones`, `milestoneHref`, `referenceDay`, `daysBetween`, `relativeLabel` (Task 9); `formatNativePrice` from `@/utils/stock-monitor`; `StatusText` from `@/components/shell/DataStatus`; `SectionHead` from `@/components/shell/PageHead`; `Card, CardContent` from `@/components/ui/card`; `Popover, PopoverContent, PopoverTrigger` from `@/components/ui/popover`; `Button` (`variant="ghost" size="icon-sm"`).
- Produces: `MilestoneList({ milestones, onDone, onSnooze, onNavigate? })`, `UpcomingCard()`, `MilestonesStatus()`.

- [ ] **Step 1: The shared list.** `src/components/milestones/MilestoneList.tsx`:

```tsx
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import {
  CalendarClock,
  Check,
  Clock,
  DatabaseBackup,
  FileCheck,
  Home,
  Landmark,
  Percent,
  ScrollText,
  Shield,
  Target,
  type LucideIcon,
} from 'lucide-react';
import type { Milestone, MilestoneKind } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { formatNativePrice } from '@/utils/stock-monitor';
import { daysBetween, milestoneHref, referenceDay, relativeLabel } from '@/utils/milestones';

const ICONS: Record<MilestoneKind, LucideIcon> = {
  insurance_anniversary: Shield,
  insurance_end: Shield,
  insurance_payment: Shield,
  loan_fixation_end: Percent,
  loan_fixation_expired: Percent,
  loan_payoff: Landmark,
  loan_balance_check: FileCheck,
  bond_maturity: ScrollText,
  bond_coupon: ScrollText,
  account_termination: CalendarClock,
  savings_rate_end: Percent,
  balances_stale: Landmark,
  backup_stale: DatabaseBackup,
  valuation_stale: Home,
  watch_target: Target,
};

interface MilestoneListProps {
  milestones: readonly Milestone[];
  onDone: (m: Milestone) => void;
  onSnooze: (m: Milestone) => void;
  /** Called when a row's link is followed (closes the top-bar popover). */
  onNavigate?: () => void;
}

/**
 * Milestone rows (spec 2026-10-05 §7): icon in a `well` square, title and one
 * sub-line, the relative date right; "Odložit" and "Vyřízeno" appear on hover
 * next to the link, never inside it.
 */
export function MilestoneList({ milestones, onDone, onSnooze, onNavigate }: MilestoneListProps) {
  return (
    <ul className="m-0 list-none p-0">
      {milestones.map((m) => (
        <MilestoneRow key={m.key} m={m} onDone={onDone} onSnooze={onSnooze} onNavigate={onNavigate} />
      ))}
    </ul>
  );
}

function MilestoneRow({
  m,
  onDone,
  onSnooze,
  onNavigate,
}: {
  m: Milestone;
  onDone: (m: Milestone) => void;
  onSnooze: (m: Milestone) => void;
  onNavigate?: () => void;
}) {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  const today = utcDayFloor(Date.now() / 1000);
  const thisYear = new Date(today * 1000).getUTCFullYear();

  const date = (d: number | null) =>
    d === null
      ? ''
      : new Date(d * 1000).getUTCFullYear() === thisYear
        ? fmt.day(d, { day: 'numeric', month: 'numeric' })
        : fmt.day(d);
  const money = (amount: string | null) =>
    amount === null ? '' : fmt.money(Number(amount), m.currency ?? 'CZK');
  const price = (amount: string | null) =>
    amount === null ? '' : formatNativePrice(Number(amount), m.currency, fmt.locale);

  const sub = (() => {
    switch (m.kind) {
      case 'insurance_anniversary':
        return m.tone === 'info'
          ? t('sub.insurance_anniversary_passed', { date: date(m.dueDay) })
          : t('sub.insurance_anniversary', { date: date(m.dueDay), deadline: date(m.actionDay) });
      case 'insurance_payment':
      case 'bond_coupon':
        return t(`sub.${m.kind}`, { amount: money(m.amount), date: date(m.dueDay) });
      case 'bond_maturity':
        return t('sub.bond_maturity', { date: date(m.dueDay), amount: money(m.amount) });
      case 'backup_stale':
        return m.sinceDay === null
          ? t('sub.backup_never')
          : t('sub.backup_stale', { date: date(m.sinceDay) });
      case 'balances_stale':
        return t('sub.balances_stale', { count: m.count ?? 0 });
      case 'valuation_stale':
      case 'loan_balance_check':
        return t(`sub.${m.kind}`, { date: date(m.sinceDay) });
      case 'watch_target':
        return t(m.direction === 'below' ? 'sub.watch_target_below' : 'sub.watch_target_above', {
          price: price(m.referenceAmount),
          target: price(m.amount),
        });
      default:
        return t(`sub.${m.kind}`, { date: date(m.dueDay) });
    }
  })();

  const right = (() => {
    if (m.kind === 'watch_target') {
      const target = Number(m.amount);
      const current = Number(m.referenceAmount);
      return target > 0 && current > 0 ? fmt.percent(current / target - 1, 1, { signed: true }) : '';
    }
    const ref = referenceDay(m);
    if (ref === null) return '';
    const label = relativeLabel(daysBetween(today, ref));
    return t(`relative.${label.key}`, { count: label.count });
  })();

  const Icon = ICONS[m.kind];
  const title = m.title || t(`titles.${m.kind}`);

  return (
    <li className="group grid grid-cols-[1fr_auto] items-center gap-2 border-b border-line-soft last:border-0">
      <Link
        href={milestoneHref(m)}
        onClick={onNavigate}
        className="grid min-w-0 grid-cols-[30px_1fr_auto] items-center gap-[11px] py-[11px] text-ink hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
      >
        <i className="grid size-[29px] place-items-center rounded-r2 bg-well text-ink-2">
          <Icon className="size-[15px]" strokeWidth={1.75} aria-hidden />
        </i>
        <span className="min-w-0">
          <b className="block truncate text-table font-650">{title}</b>
          <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">{sub}</small>
        </span>
        <span className="whitespace-nowrap text-caption font-600 text-ink-3 num">{right}</span>
      </Link>
      <div className="flex w-[60px] justify-end gap-0.5 opacity-0 transition-opacity duration-fast focus-within:opacity-100 group-hover:opacity-100">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('actions.snooze')}
          title={t('actions.snooze')}
          onClick={() => onSnooze(m)}
        >
          <Clock />
        </Button>
        {m.canDismiss && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('actions.done')}
            title={t('actions.done')}
            onClick={() => onDone(m)}
          >
            <Check />
          </Button>
        )}
      </div>
    </li>
  );
}
```

If an icon name does not exist in the installed `lucide-react` version (check with `grep -l "DatabaseBackup" node_modules/lucide-react/dist/lucide-react.d.ts`), use `HardDrive` for the backup and `FileText` for `ScrollText`/`FileCheck`.

- [ ] **Step 2: The dashboard card.** `src/components/dashboard/UpcomingCard.tsx`:

```tsx
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SectionHead } from '@/components/shell/PageHead';
import { Card, CardContent } from '@/components/ui/card';
import { MilestoneList } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { groupMilestones } from '@/utils/milestones';

/** Rows shown before "Zobrazit vše"; "Teď jednat" fills them first. */
const COLLAPSED_ROWS = 6;

/**
 * "Co vás čeká" (spec 2026-10-05 §7): contract dates, payments, upkeep and
 * crossed targets in two groups. Hidden when nothing is coming up.
 */
export function UpcomingCard() {
  const { t } = useTranslation('milestones');
  const { data: milestones = [] } = useMilestones();
  const { markDone, snooze } = useMilestoneActions();
  const [expanded, setExpanded] = useState(false);

  if (milestones.length === 0) return null;

  const { now, soon } = groupMilestones(milestones);
  const limit = expanded ? Infinity : COLLAPSED_ROWS;
  const shownNow = now.slice(0, limit);
  const shownSoon = soon.slice(0, Math.max(0, limit - shownNow.length));
  const hidden = milestones.length - shownNow.length - shownSoon.length;

  return (
    <section className="mt-9">
      <SectionHead title={t('card.title')} />
      <Card variant="flat">
        <CardContent className="pb-2 pt-4">
          {shownNow.length > 0 && (
            <div>
              <div className="text-eyebrow uppercase text-ink-4">{t('card.now')}</div>
              <MilestoneList milestones={shownNow} onDone={markDone} onSnooze={snooze} />
            </div>
          )}
          {shownSoon.length > 0 && (
            <div className={shownNow.length > 0 ? 'mt-4' : undefined}>
              <div className="text-eyebrow uppercase text-ink-4">{t('card.soon')}</div>
              <MilestoneList milestones={shownSoon} onDone={markDone} onSnooze={snooze} />
            </div>
          )}
          {(hidden > 0 || expanded) && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="mb-1 mt-2 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
            >
              {expanded ? t('card.showLess') : t('card.showAll', { count: milestones.length })}
            </button>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
```

In `src/pages/Dashboard.tsx` import it and render `<UpcomingCard />` between the closing `</Stats>` and the `<div className="mt-9 grid grid-cols-[1.15fr_0.85fr] gap-[18px]">`.

- [ ] **Step 3: The top-bar indicator.** `src/components/shell/MilestonesStatus.tsx`:

```tsx
import { useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { StatusText } from '@/components/shell/DataStatus';
import { MilestoneList } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { groupMilestones } from '@/utils/milestones';

/**
 * "2 k vyřízení" in the top bar (spec 2026-10-05 §7): the "Teď jednat" items on
 * every page, opened in a popover. Hidden when there is nothing to do.
 */
export function MilestonesStatus() {
  const { t } = useTranslation('milestones');
  const [open, setOpen] = useState(false);
  const { data: milestones = [] } = useMilestones();
  const { markDone, snooze } = useMilestoneActions();
  const { now } = groupMilestones(milestones);

  if (now.length === 0) return null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="rounded-r1 focus-visible:outline-none focus-visible:shadow-focus"
        >
          <StatusText className="text-ink-2">{t('status.count', { count: now.length })}</StatusText>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[380px] p-0">
        <div className="px-4 pt-3 text-eyebrow uppercase text-ink-4">{t('status.title')}</div>
        <div className="max-h-[420px] overflow-y-auto px-4">
          <MilestoneList
            milestones={now}
            onDone={markDone}
            onSnooze={snooze}
            onNavigate={() => setOpen(false)}
          />
        </div>
        <div className="border-t border-line-soft px-4 py-2.5">
          <Link
            href="/"
            onClick={() => setOpen(false)}
            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
          >
            {t('status.openOverview')} →
          </Link>
        </div>
      </PopoverContent>
    </Popover>
  );
}
```

In `src/components/shell/TopBar.tsx` import it and render `<MilestonesStatus />` between `<HistoryRecalculationBadge />` and `<UpdateStatus />`.

- [ ] **Step 4: Verify.**

Run: `npm run typecheck && npm run lint && npm test`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git branch --show-current
git add src/components/milestones src/components/dashboard/UpcomingCard.tsx src/components/shell/MilestonesStatus.tsx src/pages/Dashboard.tsx src/components/shell/TopBar.tsx
git commit -m "feat(milestones): dashboard card and top-bar indicator

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Living docs, full gates and verification in the app

**Files:**
- Modify: `docs/design-system/README.md` (§5 top bar, §7 Overview archetype)
- Modify: `README.md` (feature list, if it has one)

- [ ] **Step 1: Docs.**
  - `docs/design-system/README.md` §5 "Top bar status": add a sentence — "Left of the update status, a neutral `.status` 'N k vyřízení' opens a popover with the milestones to act on now; it is hidden when there are none."
  - §7 Overview row: structure becomes "Greeting H1, period segment, hero (net worth + chart), 3 stats, 'Co vás čeká' (milestones in 'Teď jednat' / 'Brzy', hidden when empty), allocation ring + recent moves."
  - `README.md`: if it lists features, add one line: "Milestones: insurance anniversaries with the notice deadline, fixed-rate ends, maturities, large payments, backup and data upkeep reminders and crossed watchlist targets, on the overview and in the top bar."

- [ ] **Step 2: Full gates.**

Run:
```bash
npm run lint && npm run typecheck && npm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd src-tauri && cargo test generate_bindings -- --ignored && git diff --exit-code ../shared/generated-types.ts
```
Expected: everything passes, no binding drift.

- [ ] **Step 3: Verify in the app** (controller, not a subagent). Start the `moony-demo` launch configuration, unlock the demo profile, and check: the migration log line for `005_milestones`; the dashboard card with both groups; the top-bar count and popover; "Odložit" / "Vyřízeno" with the "Vrátit" toast; a watchlist target set below the price shows "čeká na pokles" in the dialog and is not "dosažen"; a bank account with "Sazba platí do" in 10 days appears in the card. Take a screenshot of the dashboard for the owner.

- [ ] **Step 4: Commit the docs.**

```bash
git branch --show-current
git add docs/design-system/README.md README.md
git commit -m "docs: milestones on the overview and in the top bar

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
