# Cashflow planning groups and plan inclusion — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The planning page lists every category as a collapsible group with its total, and any row or whole category can be left out of the plan without deleting it, persisted in the database and honoured by the page, its stats, the flow bar and the MCP `cashflow_report` tool.

**Architecture:** A new table `cashflow_plan_exclusions` stores one key per excluded row (`category:<key>`, `<sourceId>`, `item:<id>`). `cashflow_planning::get_report_in_currency` marks every item and category `included` and sums included rows only; one thin command flips a key. The React page is split into a section card with group and item rows and a "···" menu; expansion is remembered in `localStorage` through a pure helper.

**Tech Stack:** Rust (rusqlite, serde, specta), Tauri 2 commands, React 19 + TanStack Query + react-i18next + Radix dropdown (`ui/dropdown-menu.tsx`), Vitest.

Spec: `docs/specs/2026-10-10-cashflow-planning-groups-design.md`. Prototype: `docs/specs/2026-10-10-cashflow-planning-groups-prototype.html`.

## Global Constraints

- Never call `invoke()` outside `src/lib/tauri-api.ts` (rule 1).
- `shared/schema.ts` is the wire contract; follow the 7-step procedure in `docs/standards/type-contract.md` (rules 2–3): per-field `#[serde(rename = "camelCase")]`, register new types in `src-tauri/src/bindings.rs`, regenerate `shared/generated-types.ts` with `cd src-tauri && cargo test generate_bindings -- --ignored`.
- Thin commands; logic in `src-tauri/src/services/cashflow_planning.rs` taking `&Connection`; `AppError`, never `Result<_, String>`; `validate()` on the request struct, called by the command; no `.unwrap()` outside tests (rules 4–5).
- Migration is append-only: `013_cashflow_plan_exclusions` as the next Vec entry; regenerate `src-tauri/src/db/schema.snapshot` with `cargo test regenerate_schema_snapshot -- --ignored` (rule 6, `docs/playbooks/add-db-migration.md`).
- Mutations refresh through `refreshMoneyFlows(queryClient)` from `src/lib/money-flow-refresh.ts` (rule 7; it already covers `["cashflow-report"]`, `["portfolio-metrics"]`, the snapshot and `["portfolio-history"]`).
- Every user-facing string in BOTH `src/i18n/locales/en/reports.json` and `src/i18n/locales/cs/reports.json`; Czech plurals use `_one/_few/_other`, English `_one/_other` (rule 8).
- Money stays TEXT, timestamps unix epoch INTEGER (rule 10).
- Work in the sibling worktree `../Moony-tauri-cashflow-plan` on branch `feat/cashflow-plan-groups`; never commit in the shared checkout. Run `git branch --show-current` before every commit.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File structure

| File | Responsibility |
|---|---|
| `src-tauri/src/db/migrations.rs` | `MIGRATION_013` creates `cashflow_plan_exclusions` |
| `src-tauri/src/db/schema.snapshot` | regenerated |
| `src-tauri/src/models/cashflow.rs` | `SetCashflowPlanInclusion` + `validate()`, `CASHFLOW_CATEGORIES`, `PLAN_SOURCE_PREFIXES`, `included` on `CashflowReportItem` and `CashflowCategory` |
| `src-tauri/src/services/cashflow_planning.rs` | `load_plan_exclusions`, `set_plan_inclusion`, `included` + totals in `get_report_in_currency`, key cleanup in `delete_cashflow_item`, tests |
| `src-tauri/src/commands/cashflow.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs` | command, handler registration, type registration |
| `src-tauri/src/services/mcp/analytics.rs` | `included` in `cashflow_report` items |
| `shared/schema.ts`, `shared/generated-types.ts`, `src/lib/tauri-api.ts` | contract and wrapper |
| `src/utils/plan-expansion.ts` (+ `.test.ts`) | pure expansion-state helpers over a `Storage`-like object |
| `src/hooks/use-cashflow-planning-mutations.ts` | create / update / delete / setInclusion with toasts and refresh |
| `src/components/cashflow/plan-modules.ts` | category → module label key and route |
| `src/components/cashflow/PlanRowMenu.tsx` | the "···" menu for category, automatic and manual rows |
| `src/components/cashflow/PlanItemRow.tsx` | one item row (name, sub-line, FX notes, amount, badge, menu) |
| `src/components/cashflow/PlanGroupRow.tsx` | one category row (chevron, name, meta, total, badge, menu) |
| `src/components/cashflow/PlanSectionCard.tsx` | card head, income / expense blocks, footer |
| `src/components/cashflow/AddCashflowItemDialog.tsx` | `presetCategory`, `presetReplacementSourceId` |
| `src/pages/CashflowPlanning.tsx` | head, stats, flow card, dialogs, expansion state, wiring |
| `src/i18n/locales/{cs,en}/reports.json` | copy |
| `docs/architecture/database.md`, `docs/design-system/README.md`, `CHANGELOG.md` | docs |

---

### Task 1: Migration `013_cashflow_plan_exclusions`

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (const after `MIGRATION_012`, Vec entry in `all_migrations()`)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `docs/architecture/database.md` (table row next to `cashflow_items`)

**Interfaces:**
- Produces: table `cashflow_plan_exclusions(key TEXT PRIMARY KEY, created_at INTEGER NOT NULL DEFAULT (unixepoch()))`.

- [ ] **Step 1: Add the migration const and Vec entry**

After `pub(crate) const MIGRATION_012: &str = r#"…"#;` add:

```rust
/// Rows of the planning report the user leaves out of the plan without deleting
/// them. Keys: `category:<categoryKey>`, an automatic source id (`loan:<id>`,
/// `insurance:<id>`, `propertyCosts:<id>`, `account:<id>`, `stock:<ticker>`,
/// `bond:<id>`, `rental:<id>`) or `item:<manual item id>`.
pub(crate) const MIGRATION_013: &str = r#"
CREATE TABLE IF NOT EXISTS cashflow_plan_exclusions (
    key TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
"#;
```

In `all_migrations()` append `("013_cashflow_plan_exclusions", MIGRATION_013),` after the 012 entry.

- [ ] **Step 2: Run the snapshot test to see it fail**

Run: `cd src-tauri && cargo test baseline_matches_schema_snapshot`
Expected: FAIL (snapshot out of date).

- [ ] **Step 3: Regenerate the snapshot and re-run**

Run: `cd src-tauri && cargo test regenerate_schema_snapshot -- --ignored && cargo test migrations`
Expected: PASS; `git diff src-tauri/src/db/schema.snapshot` shows exactly the new table.

- [ ] **Step 4: Document the table**

In `docs/architecture/database.md`, in the table that holds the `cashflow_items` row, add:

```markdown
| `cashflow_plan_exclusions` | Planning rows the user leaves out of the plan without deleting them | `key TEXT PRIMARY KEY`: `category:<categoryKey>` (whole category, including items added later), an automatic source id (`loan:<id>`, `insurance:<id>`, `propertyCosts:<id>`, `account:<id>`, `stock:<ticker>`, `bond:<id>`, `rental:<id>`) or `item:<manual item id>`; `created_at` (migration 013). Deleting a manual item deletes its key; keys of vanished automatic sources are harmless leftovers |
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/migrations.rs src-tauri/src/db/schema.snapshot docs/architecture/database.md
git commit -m "feat(db): cashflow_plan_exclusions table (migration 013)

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Inclusion request model and exclusion store

**Files:**
- Modify: `src-tauri/src/models/cashflow.rs`
- Modify: `src-tauri/src/services/cashflow_planning.rs` (new functions + tests; `planning_db()` helper gets the table)
- Modify: `src-tauri/src/bindings.rs` (register `SetCashflowPlanInclusion`)

**Interfaces:**
- Produces: `models::SetCashflowPlanInclusion { key: String, included: bool }` with `validate()`; `models::CASHFLOW_CATEGORIES: [&str; 16]`; `models::PLAN_SOURCE_PREFIXES: [&str; 7]`; `cashflow_planning::load_plan_exclusions(conn) -> Result<HashSet<String>>`; `cashflow_planning::set_plan_inclusion(conn, data) -> Result<()>`.

- [ ] **Step 1: Write the failing model tests**

In the `tests` module of `src-tauri/src/models/cashflow.rs` add:

```rust
    #[test]
    fn plan_inclusion_keys_are_validated() {
        let ok = |key: &str| SetCashflowPlanInclusion { key: key.into(), included: false }.validate();
        assert!(ok("category:stockDividends").is_ok());
        assert!(ok("item:0f3c").is_ok());
        for prefix in PLAN_SOURCE_PREFIXES {
            assert!(ok(&format!("{prefix}:abc")).is_ok(), "{prefix}");
        }
        for bad in [
            "",
            "stockDividends",
            "category:",
            "category:unknown",
            "item:",
            "item:has space",
            "crypto:btc",
            " loan:abc",
        ] {
            assert!(ok(bad).is_err(), "accepted {bad:?}");
        }
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd src-tauri && cargo test plan_inclusion_keys_are_validated`
Expected: compile error (`SetCashflowPlanInclusion` / `PLAN_SOURCE_PREFIXES` undefined).

- [ ] **Step 3: Add the constants and the struct**

In `src-tauri/src/models/cashflow.rs`, above `impl InsertCashflowItem`:

```rust
/// Category keys of the planning report (personal first, then investments).
pub const CASHFLOW_CATEGORIES: [&str; 16] = [
    "personalIncome",
    "personalLoans",
    "personalInsurance",
    "personalRealEstateCosts",
    "subscriptions",
    "consumptionCosts",
    "otherPersonalCosts",
    "interestIncome",
    "stockDividends",
    "bondsInterest",
    "rentalIncome",
    "otherInvestmentIncome",
    "investmentLoans",
    "investmentInsurance",
    "investmentRealEstateCosts",
    "otherInvestmentCosts",
];

/// Type prefixes of automatic source ids (`<prefix>:<native id>`), shared by
/// replacements (`cashflow_items.replacement_source_id`) and plan exclusions.
pub const PLAN_SOURCE_PREFIXES: [&str; 7] = [
    "loan",
    "insurance",
    "propertyCosts",
    "account",
    "stock",
    "bond",
    "rental",
];

/// Count a planning row in or leave it out (`cashflow_plan_exclusions`).
#[derive(Debug, Clone, Deserialize, Type)]
pub struct SetCashflowPlanInclusion {
    /// `category:<categoryKey>`, an automatic source id, or `item:<manual item id>`.
    pub key: String,
    pub included: bool,
}

impl SetCashflowPlanInclusion {
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::AppError;
        let invalid = || AppError::Validation("validation.cashflowPlanKeyInvalid".into());
        let (prefix, rest) = self.key.split_once(':').ok_or_else(invalid)?;
        if rest.is_empty() || rest.chars().any(char::is_whitespace) {
            return Err(invalid());
        }
        let known = match prefix {
            "category" => CASHFLOW_CATEGORIES.contains(&rest),
            "item" => true,
            other => PLAN_SOURCE_PREFIXES.contains(&other),
        };
        if known {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
```

Replace the inline `let categories = [ … ];` array inside `InsertCashflowItem::validate` with `if !CASHFLOW_CATEGORIES.contains(&self.category.as_str()) {` so the list exists once.

- [ ] **Step 4: Run the model tests**

Run: `cd src-tauri && cargo test models::cashflow`
Expected: PASS (both the new test and `planned_items_reject_nonfinite_and_invalid_enum_values`).

- [ ] **Step 5: Write the failing store tests**

In the `tests` module of `src-tauri/src/services/cashflow_planning.rs`, extend `planning_db()` by adding this line to the `execute_batch` SQL (before the `INSERT INTO loans`):

```sql
 CREATE TABLE cashflow_plan_exclusions(key TEXT PRIMARY KEY,created_at INTEGER NOT NULL DEFAULT 0);
```

Add the tests:

```rust
    #[test]
    fn plan_inclusion_is_stored_and_idempotent() {
        let conn = planning_db();
        let off = |key: &str| SetCashflowPlanInclusion { key: key.into(), included: false };
        let on = |key: &str| SetCashflowPlanInclusion { key: key.into(), included: true };
        set_plan_inclusion(&conn, off("loan:active")).unwrap();
        set_plan_inclusion(&conn, off("loan:active")).unwrap();
        set_plan_inclusion(&conn, off("category:stockDividends")).unwrap();
        let keys = load_plan_exclusions(&conn).unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("loan:active") && keys.contains("category:stockDividends"));
        set_plan_inclusion(&conn, on("loan:active")).unwrap();
        set_plan_inclusion(&conn, on("loan:active")).unwrap();
        let keys = load_plan_exclusions(&conn).unwrap();
        assert_eq!(keys.len(), 1);
        assert!(!keys.contains("loan:active"));
    }
```

Add `use crate::models::SetCashflowPlanInclusion;` to the file's imports (top of file, next to `CashflowItem, InsertCashflowItem`).

- [ ] **Step 6: Run it to see it fail**

Run: `cd src-tauri && cargo test plan_inclusion_is_stored_and_idempotent`
Expected: compile error (`set_plan_inclusion` undefined).

- [ ] **Step 7: Implement the store**

In `src-tauri/src/services/cashflow_planning.rs`, after `delete_cashflow_item`:

```rust
/// Every key the user left out of the plan (see migration 013).
pub fn load_plan_exclusions(conn: &rusqlite::Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT key FROM cashflow_plan_exclusions")?;
    let keys = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    Ok(keys)
}

/// Leave a category, automatic source or manual item out of the plan, or count
/// it in again. Idempotent in both directions.
pub fn set_plan_inclusion(
    conn: &rusqlite::Connection,
    data: SetCashflowPlanInclusion,
) -> Result<()> {
    data.validate()?;
    if data.included {
        conn.execute(
            "DELETE FROM cashflow_plan_exclusions WHERE key = ?1",
            [&data.key],
        )?;
    } else {
        conn.execute(
            "INSERT OR IGNORE INTO cashflow_plan_exclusions (key, created_at) VALUES (?1, ?2)",
            rusqlite::params![data.key, chrono::Utc::now().timestamp()],
        )?;
    }
    Ok(())
}
```

- [ ] **Step 8: Register the type and run the tests**

In `src-tauri/src/bindings.rs`, after `types.register::<crate::models::InsertCashflowItem>();` add
`types.register::<crate::models::SetCashflowPlanInclusion>();`.

Run: `cd src-tauri && cargo test cashflow_planning && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/models/cashflow.rs src-tauri/src/services/cashflow_planning.rs src-tauri/src/bindings.rs
git commit -m "feat(planning): plan inclusion request and exclusion store

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: `included` in the planning report

**Files:**
- Modify: `src-tauri/src/models/cashflow.rs` (`included` on `CashflowReportItem`, `CashflowCategory`)
- Modify: `src-tauri/src/services/cashflow_planning.rs` (`get_report_in_currency`, `delete_cashflow_item`, tests)
- Modify: `src-tauri/src/services/mcp/analytics.rs` (test schemas gain the table; `included` in the JSON; test)
- Modify: `shared/schema.ts`, `shared/generated-types.ts`

**Interfaces:**
- Consumes: `load_plan_exclusions`, `set_plan_inclusion` (Task 2).
- Produces: `CashflowReportItem.included: bool`, `CashflowCategory.included: bool`; `CashflowCategory.total`, section and report totals = included items only; MCP item field `"included"`.

- [ ] **Step 1: Write the failing report tests**

In the `tests` module of `src-tauri/src/services/cashflow_planning.rs` add (`planning_db()` seeds the active loan `loan:active` with a 100 CZK monthly payment):

```rust
    fn manual(conn: &rusqlite::Connection, name: &str, amount: &str, category: &str, item_type: &str) -> String {
        create_cashflow_item(
            conn,
            InsertCashflowItem {
                name: name.into(),
                amount: amount.into(),
                currency: Some("CZK".into()),
                frequency: "monthly".into(),
                item_type: item_type.into(),
                category: category.into(),
                replacement_source_id: None,
            },
        )
        .unwrap()
        .id
    }

    fn find<'a>(report: &'a CashflowReport, category: &str) -> &'a crate::models::CashflowCategory {
        [&report.personal, &report.investments]
            .into_iter()
            .flat_map(|s| s.income.iter().chain(&s.expenses))
            .find(|c| c.key == category)
            .unwrap_or_else(|| panic!("category {category}"))
    }

    #[test]
    fn excluded_automatic_item_is_listed_but_not_counted() {
        let conn = planning_db();
        manual(&conn, "Netflix", "50", "subscriptions", "expense");
        set_plan_inclusion(
            &conn,
            SetCashflowPlanInclusion { key: "loan:active".into(), included: false },
        )
        .unwrap();
        let report = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        let loans = find(&report, "personalLoans");
        assert!(loans.included);
        assert_eq!(loans.items.len(), 1);
        assert!(!loans.items[0].included);
        assert_eq!(loans.items[0].amount, "100.00", "amount stays visible");
        assert_eq!(loans.total, "0.00");
        assert_eq!(report.personal.total_expenses, "50.00");
        assert_eq!(report.total_expenses, "50.00");
        assert_eq!(report.net_cashflow, "-50.00");
        assert!(report.automatic_sources.iter().any(|s| s.source_id.as_deref() == Some("loan:active")));
    }

    #[test]
    fn excluded_category_covers_manual_and_later_items() {
        let conn = planning_db();
        let first = manual(&conn, "Netflix", "50", "subscriptions", "expense");
        set_plan_inclusion(
            &conn,
            SetCashflowPlanInclusion { key: "category:subscriptions".into(), included: false },
        )
        .unwrap();
        let later = manual(&conn, "Spotify", "30", "subscriptions", "expense");
        let report = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        let subs = find(&report, "subscriptions");
        assert!(!subs.included);
        assert_eq!(subs.items.len(), 2);
        assert!(subs.items.iter().all(|i| !i.included));
        assert!(subs.items.iter().any(|i| i.id == first) && subs.items.iter().any(|i| i.id == later));
        assert_eq!(subs.total, "0.00");
        assert_eq!(report.total_expenses, "100.00", "only the loan counts");
    }

    #[test]
    fn item_key_and_category_key_exclude_independently() {
        let conn = planning_db();
        let id = manual(&conn, "Netflix", "50", "subscriptions", "expense");
        let off = |key: String| SetCashflowPlanInclusion { key, included: false };
        let on = |key: String| SetCashflowPlanInclusion { key, included: true };
        set_plan_inclusion(&conn, off(format!("item:{id}"))).unwrap();
        set_plan_inclusion(&conn, off("category:subscriptions".into())).unwrap();
        set_plan_inclusion(&conn, on("category:subscriptions".into())).unwrap();
        let report = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        let subs = find(&report, "subscriptions");
        assert!(subs.included);
        assert!(!subs.items[0].included, "its own key still excludes it");
        set_plan_inclusion(&conn, on(format!("item:{id}"))).unwrap();
        let report = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        assert!(find(&report, "subscriptions").items[0].included);
        assert_eq!(report.total_expenses, "150.00");
    }

    #[test]
    fn replacement_hides_source_and_replacement_can_be_excluded() {
        let conn = planning_db();
        let replacement = create_cashflow_item(
            &conn,
            InsertCashflowItem {
                name: "Adjusted loan".into(),
                amount: "80".into(),
                currency: Some("CZK".into()),
                frequency: "monthly".into(),
                item_type: "expense".into(),
                category: "personalLoans".into(),
                replacement_source_id: Some("loan:active".into()),
            },
        )
        .unwrap();
        set_plan_inclusion(
            &conn,
            SetCashflowPlanInclusion { key: "loan:active".into(), included: false },
        )
        .unwrap();
        let report = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        let loans = find(&report, "personalLoans");
        assert_eq!(loans.items.len(), 1, "replaced source stays absent");
        assert_eq!(loans.items[0].id, replacement.id);
        assert!(loans.items[0].included);
        assert_eq!(report.total_expenses, "80.00");
        set_plan_inclusion(
            &conn,
            SetCashflowPlanInclusion { key: format!("item:{}", replacement.id), included: false },
        )
        .unwrap();
        let report = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        assert_eq!(report.total_expenses, "0.00");
    }

    #[test]
    fn excluded_item_with_missing_rate_does_not_degrade_quality() {
        let conn = planning_db();
        conn.execute("UPDATE loans SET currency='XRS'", []).unwrap();
        let before = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        assert!(!before.quality.missing_fx_currencies.is_empty());
        set_plan_inclusion(
            &conn,
            SetCashflowPlanInclusion { key: "loan:active".into(), included: false },
        )
        .unwrap();
        let after = get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        assert!(after.quality.missing_fx_currencies.is_empty());
        let row = &find(&after, "personalLoans").items[0];
        assert!(!row.included);
        assert_eq!(row.fx_quality, crate::models::FxQuality::Missing, "the row still says so");
    }

    #[test]
    fn deleting_a_manual_item_drops_its_key() {
        let conn = planning_db();
        let id = manual(&conn, "Netflix", "50", "subscriptions", "expense");
        set_plan_inclusion(
            &conn,
            SetCashflowPlanInclusion { key: format!("item:{id}"), included: false },
        )
        .unwrap();
        delete_cashflow_item(&conn, id).unwrap();
        assert!(load_plan_exclusions(&conn).unwrap().is_empty());
    }
```

Check the existing test helpers: if the file already has a `manual`-like helper, reuse it instead of adding a duplicate; `use crate::models::CashflowReport;` is already imported at the top of the file.

- [ ] **Step 2: Run them to see them fail**

Run: `cd src-tauri && cargo test cashflow_planning::tests`
Expected: compile errors (`included` field unknown) — that is the failure.

- [ ] **Step 3: Add the fields to the models**

In `src-tauri/src/models/cashflow.rs`:

- `CashflowReportItem`: after `pub is_user_defined: bool,` add
  ```rust
      /// False when this row or its whole category is out of the plan; `amount` is still filled.
      pub included: bool,
  ```
- `CashflowCategory`: after `pub is_user_editable: bool,` add
  ```rust
      /// False when the whole category is out of the plan (`category:<key>`).
      pub included: bool,
  ```
  and change the doc of `total` to `/// Sum of the included items only.`

- [ ] **Step 4: Compute `included` and sum included rows only**

In `get_report_in_currency` (`src-tauri/src/services/cashflow_planning.rs`):

1. Right after `let manual = get_all_cashflow_items(conn)?;` add `let exclusions = load_plan_exclusions(conn)?;`.
2. Inside the `categories` closure, before `.map(|c| {` builds items, compute per category:
   ```rust
   let category_included = !exclusions.contains(&format!("category:{}", c.key));
   ```
   (place it as the first statement inside the `.map(|c| { … })` body).
3. Inside the `filter_map(|i| { … })`, after `let source = …;` add:
   ```rust
   let own_key = source
       .clone()
       .unwrap_or_else(|| format!("item:{}", i.id));
   let included = category_included && !exclusions.contains(&own_key);
   ```
   and add `included,` to the `crate::models::CashflowReportItem { … }` literal.
4. Replace the quality recording block
   ```rust
   if item.original_components.is_empty() {
       fx.record_quality(&item.original_currency);
   } else {
       for component in &item.original_components {
           fx.record_quality(&component.currency);
       }
   }
   ```
   with the same block wrapped in `if included { … }`.
5. Change the category total to count included rows only:
   ```rust
   let total: f64 = items
       .iter()
       .filter(|i| i.included)
       .map(|i| i.amount.parse::<f64>().expect("money"))
       .sum();
   ```
   and add `included: category_included,` to the `crate::models::CashflowCategory { … }` literal.

Section and report totals already sum `c.total`, so they follow.

In `delete_cashflow_item`, after the existing `DELETE FROM cashflow_items …` (keep its not-found check), add:

```rust
    conn.execute(
        "DELETE FROM cashflow_plan_exclusions WHERE key = ?1",
        [format!("item:{id}")],
    )?;
```

(`id` is the function's `String` parameter; if the function moves it earlier, bind `let key = format!("item:{id}");` before the first statement.)

- [ ] **Step 5: Fix every other constructor and test schema**

- `src-tauri/src/services/mcp/analytics.rs`: both in-memory schemas that create `cashflow_items` (around lines 392 and 686) also get
  `CREATE TABLE cashflow_plan_exclusions(key TEXT PRIMARY KEY,created_at INTEGER NOT NULL DEFAULT 0);`.
- Search for other literal constructions: `grep -rn "CashflowReportItem {" src-tauri/src` and `grep -rn "CashflowCategory {" src-tauri/src`; every literal outside `get_report_in_currency` gets `included: true,`.

Run: `cd src-tauri && cargo test cashflow_planning && cargo test mcp`
Expected: PASS.

- [ ] **Step 6: Expose `included` through MCP**

In `cashflow_report` (`src-tauri/src/services/mcp/analytics.rs`), add `"included": item.included,` to the `serde_json::json!({ … })` per item (after `"userDefined"`). In the existing MCP test that creates the `loan:active` replacement (the one asserting `result["mainCurrency"] == "CZK"`), append at its end:

```rust
        crate::services::cashflow_planning::set_plan_inclusion(
            &conn,
            crate::models::SetCashflowPlanInclusion { key: "category:subscriptions".into(), included: false },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cashflow_items(id,name,amount,currency,frequency,item_type,category) VALUES ('sub','Netflix','50','CZK','monthly','expense','subscriptions')",
            [],
        )
        .unwrap();
        let result = cashflow_report(&conn, None).expect("cashflow");
        let app = crate::services::cashflow_planning::get_report_in_currency(&conn, "monthly", "CZK").unwrap();
        let netflix = result["expenses"].as_array().unwrap().iter().find(|i| i["name"] == "Netflix").unwrap();
        assert_eq!(netflix["included"], false);
        assert_eq!(result["summary"]["totalExpenses"], app.total_expenses);
```

If that test's schema has no `id` column on `cashflow_items`, use the column list the schema has (the schema around line 686 has `id`).

Run: `cd src-tauri && cargo test mcp && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 7: Update the wire contract**

`shared/schema.ts`:

```ts
export interface CashflowReportItem {
  // …existing fields unchanged…
  isUserDefined: boolean;
  /** False when this row or its whole category is out of the plan; `amount` is still filled. */
  included: boolean;
}

export interface CashflowCategory {
  key: string;
  name: string;
  /** Sum of the included items only. */
  total: string;
  items: CashflowReportItem[];
  isUserEditable: boolean;
  /** False when the whole category is out of the plan. */
  included: boolean;
}

/** Count a planning row in or leave it out: `category:<key>`, an automatic source id, or `item:<id>`. */
export interface SetCashflowPlanInclusion {
  key: string;
  included: boolean;
}
```

Regenerate: `cd src-tauri && cargo test generate_bindings -- --ignored`, then `git diff shared/generated-types.ts` must show only `included` on the two types and the new `SetCashflowPlanInclusion`.

- [ ] **Step 8: Typecheck and commit**

Run: `npm run typecheck`
Expected: PASS (the new fields are additive; the page is rewritten in Task 7).

```bash
git add src-tauri/src/models/cashflow.rs src-tauri/src/services/cashflow_planning.rs src-tauri/src/services/mcp/analytics.rs shared/schema.ts shared/generated-types.ts
git commit -m "feat(planning): rows and categories carry included; totals count included rows only

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Command, handler, API wrapper, changelog

**Files:**
- Modify: `src-tauri/src/commands/cashflow.rs`, `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri-api.ts`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: command `set_cashflow_plan_inclusion(data)`; `cashflowApi.setInclusion(data: SetCashflowPlanInclusion): Promise<void>`.

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands/cashflow.rs`, import `SetCashflowPlanInclusion` in the `use crate::models::{…}` line and add after `delete_cashflow_item`:

```rust
#[tauri::command]
pub async fn set_cashflow_plan_inclusion(
    db: State<'_, Database>,
    data: SetCashflowPlanInclusion,
) -> Result<()> {
    data.validate()?;
    db.with_conn(|conn| cashflow_planning::set_plan_inclusion(conn, data))
}
```

In `src-tauri/src/lib.rs`, after `commands::cashflow::delete_cashflow_item,` add `commands::cashflow::set_cashflow_plan_inclusion,`.

Run: `cd src-tauri && cargo check && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 2: Add the wrapper**

In `src/lib/tauri-api.ts`, add `SetCashflowPlanInclusion` to the `@shared/schema` type import at the top (next to `CashflowItem`) and in `cashflowApi` after `deleteItem`:

```ts
  /** Count a planning row in or leave it out of the plan (category, automatic source or manual item). */
  setInclusion: (data: SetCashflowPlanInclusion) =>
    tauriInvoke<void>('set_cashflow_plan_inclusion', { data }),
```

Run: `npm run typecheck`
Expected: PASS.

- [ ] **Step 3: Changelog**

In `CHANGELOG.md` under `## [Unreleased]` → `### Added`, append:

```markdown
- **Cashflow planning: categories as groups, rows in or out of the plan.** The planning page lists
  every category as a collapsible row with its total, count and origin (the investment section opens
  as a handful of rows instead of one line per dividend). Any row — automatic or manual — and any whole
  category can be left out of the plan from its "···" menu without deleting it: it stays visible,
  muted and labelled "Mimo plán", and stops counting in the totals, stats and flow bar. A category left
  out also covers items that appear later. The menu of an automatic row also opens the existing
  "replace with my own amount" flow and the source module. The MCP tool `cashflow_report` carries
  `included` per item and its totals follow the same rule.
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/cashflow.rs src-tauri/src/lib.rs src/lib/tauri-api.ts CHANGELOG.md
git commit -m "feat(planning): set_cashflow_plan_inclusion command and API wrapper

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Copy and the expansion helper

**Files:**
- Modify: `src/i18n/locales/cs/reports.json`, `src/i18n/locales/en/reports.json`
- Create: `src/utils/plan-expansion.ts`, `src/utils/plan-expansion.test.ts`

**Interfaces:**
- Produces: translation keys listed below; `readExpanded(storage)`, `writeExpanded(storage, keys)`, `toggleKey(keys, key)`, `setKeys(keys, categoryKeys, expanded)`, `allExpanded(keys, categoryKeys)`, `PLAN_EXPANSION_STORAGE_KEY`.

- [ ] **Step 1: Add and remove translation keys**

`src/i18n/locales/cs/reports.json`, inside `planning`:

- in `table` remove `category`, `frequency`, `monthly`, `yearly`, `editInModule`; add
  ```json
  "outOfPlan": "Mimo plán",
  "groupItems_one": "{{count}} položka",
  "groupItems_few": "{{count}} položky",
  "groupItems_other": "{{count}} položek",
  "groupExcluded_one": "{{count}} mimo plán",
  "groupExcluded_few": "{{count}} mimo plán",
  "groupExcluded_other": "{{count}} mimo plán",
  "groupCategoryExcluded": "celá kategorie mimo plán",
  "autoFrom": "automaticky z {{modules}}",
  "outOfPlanCount_one": "{{count}} položka mimo plán",
  "outOfPlanCount_few": "{{count}} položky mimo plán",
  "outOfPlanCount_other": "{{count}} položek mimo plán",
  "expandAll": "Rozbalit vše",
  "collapseAll": "Sbalit vše",
  "addToCategory": "Přidat položku do kategorie"
  ```
- add a `menu` object next to `table`:
  ```json
  "menu": {
    "label": "Možnosti: {{name}}",
    "exclude": "Nezapočítávat",
    "include": "Započítat",
    "excludeCategory": "Nezapočítávat kategorii",
    "includeCategory": "Započítat kategorii",
    "categoryExcluded": "Celá kategorie je mimo plán",
    "replace": "Nahradit vlastní částkou…",
    "openModule": "Otevřít v modulu {{module}}"
  }
  ```
- in `toast` add
  ```json
  "excluded": "Položka vyřazena z plánu",
  "included": "Položka opět v plánu",
  "categoryExcluded": "Kategorie vyřazena z plánu",
  "categoryIncluded": "Kategorie opět v plánu"
  ```

`src/i18n/locales/en/reports.json`, the same shape:

- `table`: remove the same five keys; add
  ```json
  "outOfPlan": "Out of plan",
  "groupItems_one": "{{count}} item",
  "groupItems_other": "{{count}} items",
  "groupExcluded_one": "{{count}} out of plan",
  "groupExcluded_other": "{{count}} out of plan",
  "groupCategoryExcluded": "whole category out of plan",
  "autoFrom": "automatic from {{modules}}",
  "outOfPlanCount_one": "{{count}} item out of plan",
  "outOfPlanCount_other": "{{count}} items out of plan",
  "expandAll": "Expand all",
  "collapseAll": "Collapse all",
  "addToCategory": "Add an item to this category"
  ```
- `menu`:
  ```json
  "menu": {
    "label": "Options: {{name}}",
    "exclude": "Leave out",
    "include": "Count in",
    "excludeCategory": "Leave the category out",
    "includeCategory": "Count the category in",
    "categoryExcluded": "The whole category is out of the plan",
    "replace": "Replace with my own amount…",
    "openModule": "Open in {{module}}"
  }
  ```
- `toast`:
  ```json
  "excluded": "Left out of the plan",
  "included": "Counted in again",
  "categoryExcluded": "Category left out of the plan",
  "categoryIncluded": "Category counted in again"
  ```

Also add to `common.json` of both locales, under the existing validation messages object (find `"cashflowItemInvalid"` with `grep -rn cashflowItemInvalid src/i18n`), the key `"cashflowPlanKeyInvalid"`: cs `"Neplatný klíč položky plánu."`, en `"Invalid plan row key."`.

Run: `node -e "for (const l of ['cs','en']) { const r=require('./src/i18n/locales/'+l+'/reports.json'); console.log(l, Object.keys(r.planning.menu).length, Object.keys(r.planning.table).includes('category')) }"`
Expected: `cs 8 false` and `en 8 false`.

- [ ] **Step 2: Write the failing helper tests**

`src/utils/plan-expansion.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import {
  PLAN_EXPANSION_STORAGE_KEY,
  allExpanded,
  readExpanded,
  setKeys,
  toggleKey,
  writeExpanded,
} from './plan-expansion';

function memoryStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    data,
  };
}

const throwing = {
  getItem: () => {
    throw new Error('blocked');
  },
  setItem: () => {
    throw new Error('blocked');
  },
};

describe('plan-expansion', () => {
  it('reads an empty set when nothing is stored, the value is malformed or storage is unavailable', () => {
    expect(readExpanded(memoryStorage()).size).toBe(0);
    expect(readExpanded(memoryStorage({ [PLAN_EXPANSION_STORAGE_KEY]: '{bad' })).size).toBe(0);
    expect(readExpanded(memoryStorage({ [PLAN_EXPANSION_STORAGE_KEY]: '"x"' })).size).toBe(0);
    expect(readExpanded(throwing).size).toBe(0);
    expect(readExpanded(null).size).toBe(0);
  });

  it('round-trips the expanded keys', () => {
    const storage = memoryStorage();
    writeExpanded(storage, new Set(['stockDividends', 'personalLoans']));
    expect([...readExpanded(storage)].sort()).toEqual(['personalLoans', 'stockDividends']);
    expect(() => writeExpanded(throwing, new Set(['a']))).not.toThrow();
    expect(() => writeExpanded(null, new Set(['a']))).not.toThrow();
  });

  it('toggles one key without mutating the input', () => {
    const before = new Set(['a']);
    const after = toggleKey(before, 'b');
    expect([...after].sort()).toEqual(['a', 'b']);
    expect([...toggleKey(after, 'a')]).toEqual(['b']);
    expect([...before]).toEqual(['a']);
  });

  it('expands or collapses a section without touching other sections', () => {
    const keys = new Set(['other']);
    const opened = setKeys(keys, ['a', 'b'], true);
    expect([...opened].sort()).toEqual(['a', 'b', 'other']);
    const closed = setKeys(opened, ['a', 'b'], false);
    expect([...closed]).toEqual(['other']);
  });

  it('reports whether every category of a section is expanded', () => {
    expect(allExpanded(new Set(['a', 'b']), ['a', 'b'])).toBe(true);
    expect(allExpanded(new Set(['a']), ['a', 'b'])).toBe(false);
    expect(allExpanded(new Set(), [])).toBe(false);
  });
});
```

- [ ] **Step 3: Run it to see it fail**

Run: `npx vitest run src/utils/plan-expansion.test.ts`
Expected: FAIL (module not found).

- [ ] **Step 4: Implement the helper**

`src/utils/plan-expansion.ts`:

```ts
/**
 * Which planning categories are expanded, remembered per browser. Storage is a
 * convenience only: every access is guarded, and the page works without it.
 */
export const PLAN_EXPANSION_STORAGE_KEY = 'moony.cashflowPlanning.expanded';

type StorageLike = Pick<Storage, 'getItem' | 'setItem'>;

export function readExpanded(storage: StorageLike | null | undefined): Set<string> {
  try {
    const raw = storage?.getItem(PLAN_EXPANSION_STORAGE_KEY);
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed)
      ? new Set(parsed.filter((k): k is string => typeof k === 'string'))
      : new Set();
  } catch {
    return new Set();
  }
}

export function writeExpanded(storage: StorageLike | null | undefined, keys: Set<string>): void {
  try {
    storage?.setItem(PLAN_EXPANSION_STORAGE_KEY, JSON.stringify([...keys]));
  } catch {
    // Private window or blocked storage: the page simply forgets on reload.
  }
}

export function toggleKey(keys: Set<string>, key: string): Set<string> {
  const next = new Set(keys);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}

/** Expand or collapse every category of one section; other sections keep their state. */
export function setKeys(keys: Set<string>, categoryKeys: string[], expanded: boolean): Set<string> {
  const next = new Set(keys);
  for (const key of categoryKeys) {
    if (expanded) next.add(key);
    else next.delete(key);
  }
  return next;
}

export function allExpanded(keys: Set<string>, categoryKeys: string[]): boolean {
  return categoryKeys.length > 0 && categoryKeys.every((key) => keys.has(key));
}
```

- [ ] **Step 5: Run the tests and commit**

Run: `npx vitest run src/utils/plan-expansion.test.ts && npm run lint`
Expected: PASS, no lint errors.

```bash
git add src/i18n/locales/cs/reports.json src/i18n/locales/en/reports.json src/i18n/locales/cs/common.json src/i18n/locales/en/common.json src/utils/plan-expansion.ts src/utils/plan-expansion.test.ts
git commit -m "feat(planning): copy for groups and plan inclusion; expansion helper

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Mutations hook, module map, dialog presets

**Files:**
- Create: `src/hooks/use-cashflow-planning-mutations.ts`
- Create: `src/components/cashflow/plan-modules.ts`
- Modify: `src/components/cashflow/AddCashflowItemDialog.tsx`

**Interfaces:**
- Consumes: `cashflowApi.setInclusion` (Task 4), toast keys (Task 5).
- Produces:
  - `useCashflowPlanningMutations()` → `{ createMutation, updateMutation, deleteMutation, inclusionMutation }` where `inclusionMutation.mutate({ key, included, kind })`, `kind: 'item' | 'category'`.
  - `PLAN_MODULES: Record<string, { labelKey: string; url: string }>` keyed by category; `planModule(categoryKey)` returning it or `undefined`.
  - Dialog props `presetCategory?: string`, `presetReplacementSourceId?: string | null`.

- [ ] **Step 1: The hook**

`src/hooks/use-cashflow-planning-mutations.ts`:

```ts
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertCashflowItem, SetCashflowPlanInclusion } from '@shared/schema';
import { cashflowApi } from '@/lib/tauri-api';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';

export type PlanInclusionChange = SetCashflowPlanInclusion & { kind: 'item' | 'category' };

/** Planning item create / update / delete and plan inclusion (rule 7 through `refreshMoneyFlows`). */
export function useCashflowPlanningMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) =>
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });

  const createMutation = useMutation({
    mutationFn: (data: InsertCashflowItem) => cashflowApi.createItem(data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('planning.toast.added'));
    },
    onError,
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, data }: { id: string; data: InsertCashflowItem }) =>
      cashflowApi.updateItem(id, data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('planning.toast.updated'));
    },
    onError,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => cashflowApi.deleteItem(id),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('planning.toast.deleted'));
    },
    onError,
  });

  const inclusionMutation = useMutation({
    mutationFn: ({ key, included }: PlanInclusionChange) =>
      cashflowApi.setInclusion({ key, included }),
    onSuccess: async (_result, { kind, included }) => {
      await refreshMoneyFlows(queryClient);
      toast(
        t(
          kind === 'category'
            ? included
              ? 'planning.toast.categoryIncluded'
              : 'planning.toast.categoryExcluded'
            : included
              ? 'planning.toast.included'
              : 'planning.toast.excluded'
        )
      );
    },
    onError,
  });

  return { createMutation, updateMutation, deleteMutation, inclusionMutation };
}
```

- [ ] **Step 2: The module map**

`src/components/cashflow/plan-modules.ts`:

```ts
/** The module an automatic planning row comes from, by category key (design system README §11). */
export const PLAN_MODULES: Record<string, { labelKey: string; url: string }> = {
  personalLoans: { labelKey: 'nav.loans', url: '/loans' },
  investmentLoans: { labelKey: 'nav.loans', url: '/loans' },
  personalInsurance: { labelKey: 'nav.insurance', url: '/insurance' },
  investmentInsurance: { labelKey: 'nav.insurance', url: '/insurance' },
  personalRealEstateCosts: { labelKey: 'nav.realEstate', url: '/real-estate' },
  investmentRealEstateCosts: { labelKey: 'nav.realEstate', url: '/real-estate' },
  rentalIncome: { labelKey: 'nav.realEstate', url: '/real-estate' },
  stockDividends: { labelKey: 'nav.stocks', url: '/stocks' },
  bondsInterest: { labelKey: 'nav.bonds', url: '/bonds' },
  interestIncome: { labelKey: 'nav.bankAccounts', url: '/bank-accounts' },
};

export function planModule(categoryKey: string) {
  return PLAN_MODULES[categoryKey];
}
```

Verify the label keys exist: `grep -n '"loans"\|"insurance"\|"realEstate"\|"stocks"\|"bonds"\|"bankAccounts"' src/i18n/locales/cs/common.json` (they are the keys `SOURCE_BY_CATEGORY` already used in the page).

- [ ] **Step 3: Dialog presets**

In `AddCashflowItemDialog.tsx` add to `AddCashflowItemDialogProps`:

```ts
  /** Category preselected when the dialog opens for a new item (from a group row). */
  presetCategory?: string;
  /** Automatic source preselected in "Automatický zdroj k nahrazení" (from an automatic row). */
  presetReplacementSourceId?: string | null;
```

Destructure both (`presetCategory`, `presetReplacementSourceId = null`) and in the reset `useEffect`'s `else` branch (new item) replace

```ts
        setReplacementSourceId('none');
        …
        if (availableCategories.length > 0) {
          setSelectedCategory(availableCategories[0].key);
        }
```

with

```ts
        setReplacementSourceId(presetReplacementSourceId ?? 'none');
        …
        const preset = availableCategories.find((c) => c.key === presetCategory);
        if (preset) setSelectedCategory(preset.key);
        else if (availableCategories.length > 0) setSelectedCategory(availableCategories[0].key);
```

and add `presetCategory, presetReplacementSourceId` to that effect's dependency array. The category `Select`'s `onValueChange` already resets the replacement to `'none'` when the user changes the category — keep that.

- [ ] **Step 4: Typecheck, lint, commit**

Run: `npm run typecheck && npm run lint`
Expected: PASS.

```bash
git add src/hooks/use-cashflow-planning-mutations.ts src/components/cashflow/plan-modules.ts src/components/cashflow/AddCashflowItemDialog.tsx
git commit -m "feat(planning): mutations hook, module map and dialog presets

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Grouped list components and the page

**Files:**
- Create: `src/components/cashflow/PlanRowMenu.tsx`, `PlanItemRow.tsx`, `PlanGroupRow.tsx`, `PlanSectionCard.tsx`
- Modify: `src/pages/CashflowPlanning.tsx`

**Interfaces:**
- Consumes: Task 3 fields, Task 5 keys and helpers, Task 6 hook / map / dialog props.
- Produces: the page.

Visual rules (prototype): group row 52 px, item row 44 px, grid `16px 1fr auto 28px` with `gap-x-2.5`, left padding 14 px, right 10 px; hover `bg-well`; names 12/650 `text-ink`, sub-line 10/500 `text-ink-4`; excluded rows `text-ink-4` names, `text-ink-5` sub-line, amounts `text-ink-4`; badge `variant="outline"` "Mimo plán" left of the amount; chevron `ChevronRight` 14 px rotating 90° when expanded; "···" `Ellipsis` in a ghost `icon-sm` button inside `<div data-row-actions>` (the table row class already shows it on hover / focus-within — reuse `TableRow`'s hover classes by using a plain `div` with `group/row` and the same utility string, see `src/components/ui/table.tsx:68`).

- [ ] **Step 1: `PlanRowMenu.tsx`**

```tsx
import { useTranslation } from 'react-i18next';
import { Link } from 'wouter';
import { ArrowUpRight, Check, Ellipsis, EyeOff, Pencil, Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

export type PlanRowKind = 'category' | 'auto' | 'manual';

interface PlanRowMenuProps {
  name: string;
  kind: PlanRowKind;
  included: boolean;
  /** For item rows: whether the whole category is in the plan. */
  categoryIncluded?: boolean;
  module?: { label: string; url: string };
  onToggleInclusion: () => void;
  onReplace?: () => void;
  onOpenModule?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
  onAdd?: () => void;
}

/** The "···" menu of a planning row (design system README §11, cashflow planning). */
export function PlanRowMenu({
  name,
  kind,
  included,
  categoryIncluded = true,
  module,
  onToggleInclusion,
  onReplace,
  onEdit,
  onDelete,
  onAdd,
}: PlanRowMenuProps) {
  const { t } = useTranslation('reports');
  const toggleLabel =
    kind === 'category'
      ? t(included ? 'planning.menu.excludeCategory' : 'planning.menu.includeCategory')
      : t(included ? 'planning.menu.exclude' : 'planning.menu.include');
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('planning.menu.label', { name })}
          className="shrink-0 text-ink-4 hover:text-ink data-[state=open]:text-ink"
          onClick={(e) => e.stopPropagation()}
        >
          <Ellipsis />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
        {kind !== 'category' && !categoryIncluded ? (
          <DropdownMenuItem disabled>
            <EyeOff />
            {t('planning.menu.categoryExcluded')}
          </DropdownMenuItem>
        ) : (
          <DropdownMenuItem onSelect={onToggleInclusion}>
            {included ? <EyeOff /> : <Check />}
            {toggleLabel}
          </DropdownMenuItem>
        )}
        {kind === 'category' && onAdd && (
          <DropdownMenuItem onSelect={onAdd}>
            <Plus />
            {t('planning.table.addToCategory')}
          </DropdownMenuItem>
        )}
        {kind === 'auto' && onReplace && (
          <DropdownMenuItem onSelect={onReplace}>
            <Pencil />
            {t('planning.menu.replace')}
          </DropdownMenuItem>
        )}
        {kind === 'auto' && module && (
          <DropdownMenuItem asChild>
            <Link href={module.url}>
              <ArrowUpRight />
              {t('planning.menu.openModule', { module: module.label })}
            </Link>
          </DropdownMenuItem>
        )}
        {kind === 'manual' && onEdit && (
          <DropdownMenuItem onSelect={onEdit}>
            <Pencil />
            {t('addItem.editTitle')}
          </DropdownMenuItem>
        )}
        {kind === 'manual' && onDelete && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onDelete} className="text-loss focus:text-loss">
              <Trash2 />
              {t('planning.deleteItem')}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
```

Check `ui/dropdown-menu.tsx` for an existing danger styling convention (`grep -n "loss" src/components/ui/dropdown-menu.tsx`); if a `variant="danger"` or similar exists, use it instead of the class.

- [ ] **Step 2: `PlanItemRow.tsx`**

```tsx
import { useTranslation } from 'react-i18next';
import type { CashflowReportItem } from '@shared/schema';
import { Badge } from '@/components/ui/badge';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { PlanRowMenu } from '@/components/cashflow/PlanRowMenu';

export const PLAN_ROW_GRID = 'grid grid-cols-[16px_1fr_auto_28px] items-center gap-x-2.5 pl-[14px] pr-[10px]';
export const PLAN_ROW_HOVER =
  'group/row border-b border-line-soft transition-colors duration-fast hover:bg-well focus-within:bg-well [&_[data-row-actions]]:opacity-0 [&_[data-row-actions]]:transition-opacity [&_[data-row-actions]]:duration-fast [&:hover_[data-row-actions]]:opacity-100 [&:focus-within_[data-row-actions]]:opacity-100';

interface PlanItemRowProps {
  item: CashflowReportItem;
  isIncome: boolean;
  categoryIncluded: boolean;
  reportCurrency: string;
  viewType: 'monthly' | 'yearly';
  /** Name of the automatic source a manual replacement stands in for. */
  replacedSourceName?: string;
  module?: { label: string; url: string };
  onToggleInclusion: () => void;
  onReplace: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

export function PlanItemRow({
  item,
  isIncome,
  categoryIncluded,
  reportCurrency,
  viewType,
  replacedSourceName,
  module,
  onToggleInclusion,
  onReplace,
  onEdit,
  onDelete,
}: PlanItemRowProps) {
  const { t } = useTranslation('reports');
  const { signedMoney, nativeMoney } = useReportMoney(reportCurrency);
  const off = !item.included;
  const incomplete = item.fxQuality === 'missing';
  const unavailable = incomplete && item.convertedComponentCount === 0;
  const original =
    item.originalCurrency === 'MULTI'
      ? item.originalComponents
          .map((component) => {
            const frequency =
              component.frequency === 'yearly'
                ? t('planning.table.perYear')
                : component.frequency === 'monthly'
                  ? t('planning.table.perMonth')
                  : '';
            return `${nativeMoney(Number(component.amount), component.currency)}${frequency ? ` · ${frequency}` : ''}`;
          })
          .join(' · ')
      : item.originalFrequency === 'yearly'
        ? `${nativeMoney(Number(item.originalAmount), item.originalCurrency)} · ${t('planning.table.perYear')}`
        : viewType === 'yearly'
          ? `${nativeMoney(Number(item.originalAmount), item.originalCurrency)} · ${t('planning.table.perMonth')}`
          : t('planning.table.perMonth');
  const origin = item.isUserDefined
    ? t('planning.table.manual')
    : t('planning.table.auto', { module: module?.label ?? '' });
  return (
    <div className={cn(PLAN_ROW_GRID, PLAN_ROW_HOVER, 'min-h-[44px] bg-paper')} data-item-id={item.id}>
      <span aria-hidden />
      <div className="py-2">
        <b className={cn('block text-table font-650', off ? 'text-ink-4' : 'text-ink')}>{item.name}</b>
        {replacedSourceName && (
          <small className="block text-micro text-ink-4">
            {t('readiness.replacing', { name: replacedSourceName })}
          </small>
        )}
        <small className={cn('block text-micro font-500', off ? 'text-ink-5' : 'text-ink-4')}>
          {origin} · {original}
        </small>
      </div>
      <span
        className={cn(
          'inline-flex items-center gap-2.5 whitespace-nowrap text-table font-650 num',
          off ? 'text-ink-4' : isIncome ? 'text-gain' : 'text-ink'
        )}
      >
        {off && categoryIncluded && <Badge variant="outline">{t('planning.table.outOfPlan')}</Badge>}
        <span className="text-right">
          {unavailable
            ? t('readiness.fxUnavailable')
            : signedMoney(isIncome ? Number(item.amount) : -Number(item.amount))}
          {incomplete && !unavailable && (
            <small className="block text-micro text-loss">{t('readiness.partialAmount')}</small>
          )}
        </span>
      </span>
      <div data-row-actions className="flex justify-end">
        <PlanRowMenu
          name={item.name}
          kind={item.isUserDefined ? 'manual' : 'auto'}
          included={item.included}
          categoryIncluded={categoryIncluded}
          module={module}
          onToggleInclusion={onToggleInclusion}
          onReplace={onReplace}
          onEdit={onEdit}
          onDelete={onDelete}
        />
      </div>
    </div>
  );
}
```

- [ ] **Step 3: `PlanGroupRow.tsx`**

```tsx
import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import type { CashflowCategory } from '@shared/schema';
import { Badge } from '@/components/ui/badge';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { PlanRowMenu } from '@/components/cashflow/PlanRowMenu';
import { PLAN_ROW_GRID, PLAN_ROW_HOVER } from '@/components/cashflow/PlanItemRow';

interface PlanGroupRowProps {
  category: CashflowCategory;
  isIncome: boolean;
  expanded: boolean;
  reportCurrency: string;
  /** Module labels of the automatic rows in this category, e.g. ["Úvěry"]. */
  modules: string[];
  onToggleExpanded: () => void;
  onToggleInclusion: () => void;
  onAdd: () => void;
}

export function PlanGroupRow({
  category,
  isIncome,
  expanded,
  reportCurrency,
  modules,
  onToggleExpanded,
  onToggleInclusion,
  onAdd,
}: PlanGroupRowProps) {
  const { t } = useTranslation('reports');
  const { signedMoney } = useReportMoney(reportCurrency);
  const off = !category.included;
  const name = t(`categories.${category.key}`, { defaultValue: category.name });
  const excludedCount = category.items.filter((i) => !i.included).length;
  const shownTotal = off
    ? category.items.reduce((sum, i) => sum + Number(i.amount), 0)
    : Number(category.total);
  const meta = [
    t('planning.table.groupItems', { count: category.items.length }),
    off
      ? t('planning.table.groupCategoryExcluded')
      : excludedCount > 0
        ? t('planning.table.groupExcluded', { count: excludedCount })
        : null,
    modules.length > 0
      ? t('planning.table.autoFrom', { modules: modules.join(', ') })
      : t('planning.table.manual'),
  ]
    .filter(Boolean)
    .join(' · ');
  return (
    <div
      role="button"
      tabIndex={0}
      aria-expanded={expanded}
      onClick={onToggleExpanded}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onToggleExpanded();
        }
      }}
      className={cn(PLAN_ROW_GRID, PLAN_ROW_HOVER, 'min-h-[52px] cursor-pointer focus-visible:outline-none focus-visible:shadow-focus')}
      data-group={category.key}
    >
      <ChevronRight
        className={cn('size-[14px] text-ink-4 transition-transform duration-fast', expanded && 'rotate-90')}
        aria-hidden
      />
      <div className="py-2">
        <b className={cn('block text-table font-650', off ? 'text-ink-4' : 'text-ink')}>{name}</b>
        <small className={cn('block text-micro font-500', off ? 'text-ink-5' : 'text-ink-4')}>{meta}</small>
      </div>
      <span
        className={cn(
          'inline-flex items-center gap-2.5 whitespace-nowrap text-table font-650 num',
          off ? 'text-ink-4' : isIncome ? 'text-gain' : 'text-ink'
        )}
      >
        {off && <Badge variant="outline">{t('planning.table.outOfPlan')}</Badge>}
        {signedMoney(isIncome ? shownTotal : -shownTotal)}
      </span>
      <div data-row-actions className="flex justify-end">
        <PlanRowMenu
          name={name}
          kind="category"
          included={category.included}
          onToggleInclusion={onToggleInclusion}
          onAdd={onAdd}
        />
      </div>
    </div>
  );
}
```

- [ ] **Step 4: `PlanSectionCard.tsx`**

```tsx
import { useTranslation } from 'react-i18next';
import type { CashflowCategory, CashflowReport, CashflowReportItem, CashflowSection } from '@shared/schema';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { PlanGroupRow } from '@/components/cashflow/PlanGroupRow';
import { PlanItemRow } from '@/components/cashflow/PlanItemRow';
import { planModule } from '@/components/cashflow/plan-modules';
import { allExpanded } from '@/utils/plan-expansion';

export type SectionId = 'personal' | 'investments';
export type ItemType = 'income' | 'expense';

interface PlanSectionCardProps {
  id: SectionId;
  title: string;
  sub: string;
  section: CashflowSection;
  report: CashflowReport;
  viewType: 'monthly' | 'yearly';
  expanded: Set<string>;
  onToggleExpanded: (categoryKey: string) => void;
  onSetExpanded: (categoryKeys: string[], expanded: boolean) => void;
  onToggleInclusion: (key: string, kind: 'item' | 'category', included: boolean) => void;
  onAdd: (section: SectionId, type: ItemType, category?: string) => void;
  onReplace: (section: SectionId, type: ItemType, item: CashflowReportItem) => void;
  onEdit: (section: SectionId, item: CashflowReportItem, category: string) => void;
  onDelete: (item: CashflowReportItem) => void;
}

/** One planning section (personal or investments): income and expense groups, footer. */
export function PlanSectionCard({
  id,
  title,
  sub,
  section,
  report,
  viewType,
  expanded,
  onToggleExpanded,
  onSetExpanded,
  onToggleInclusion,
  onAdd,
  onReplace,
  onEdit,
  onDelete,
}: PlanSectionCardProps) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { money, signedMoney } = useReportMoney(report.reportCurrency);
  const nonEmpty = (cats: CashflowCategory[]) => cats.filter((c) => c.items.length > 0);
  const incomeCats = nonEmpty(section.income);
  const expenseCats = nonEmpty(section.expenses);
  const allKeys = [...incomeCats, ...expenseCats].map((c) => c.key);
  const everyOpen = allExpanded(expanded, allKeys);
  const incomeCount = section.income.reduce((n, c) => n + c.items.length, 0);
  const expenseCount = section.expenses.reduce((n, c) => n + c.items.length, 0);
  const offCount = [...section.income, ...section.expenses]
    .flatMap((c) => c.items)
    .filter((i) => !i.included).length;
  const sourceName = (item: CashflowReportItem) =>
    item.replacementSourceId
      ? (report.automaticSources.find((s) => s.sourceId === item.replacementSourceId)?.name ??
        t('readiness.retainedSource'))
      : undefined;

  const block = (label: string, cats: CashflowCategory[], isIncome: boolean, type: ItemType) => (
    <>
      <div className="border-b border-line-soft bg-paper px-[14px] pb-1.5 pt-[9px] text-micro font-700 uppercase tracking-[0.06em] text-ink-4">
        {label}
      </div>
      {cats.length === 0 && (
        <p className="m-0 border-b border-line-soft px-[14px] py-3.5 text-center text-table text-ink-4">
          {t(isIncome ? 'planning.table.noIncome' : 'planning.table.noExpenses')}
        </p>
      )}
      {cats.map((category) => {
        const mod = planModule(category.key);
        const modules = category.items.some((i) => !i.isUserDefined) && mod ? [tc(mod.labelKey)] : [];
        const module = mod ? { label: tc(mod.labelKey), url: mod.url } : undefined;
        const open = expanded.has(category.key);
        return (
          <div key={category.key}>
            <PlanGroupRow
              category={category}
              isIncome={isIncome}
              expanded={open}
              reportCurrency={report.reportCurrency}
              modules={modules}
              onToggleExpanded={() => onToggleExpanded(category.key)}
              onToggleInclusion={() =>
                onToggleInclusion(`category:${category.key}`, 'category', !category.included)
              }
              onAdd={() => onAdd(id, type, category.key)}
            />
            {open &&
              category.items.map((item) => (
                <PlanItemRow
                  key={item.id}
                  item={item}
                  isIncome={isIncome}
                  categoryIncluded={category.included}
                  reportCurrency={report.reportCurrency}
                  viewType={viewType}
                  replacedSourceName={sourceName(item)}
                  module={module}
                  onToggleInclusion={() =>
                    onToggleInclusion(
                      item.isUserDefined ? `item:${item.id}` : (item.sourceId ?? `item:${item.id}`),
                      'item',
                      !item.included
                    )
                  }
                  onReplace={() => onReplace(id, type, item)}
                  onEdit={() => onEdit(id, item, category.key)}
                  onDelete={() => onDelete(item)}
                />
              ))}
          </div>
        );
      })}
    </>
  );

  const net = Number(section.netCashflow);
  return (
    <Card variant="table">
      <CardHeader>
        <div>
          <CardTitle>{title}</CardTitle>
          <CardDescription>{sub}</CardDescription>
        </div>
        <div className="text-right">
          <small className="block text-micro font-600 text-ink-4">
            {t(viewType === 'yearly' ? 'planning.table.netYearly' : 'planning.table.netMonthly')}
          </small>
          <b className={cn('text-[16px] font-650 tracking-[-0.03em] num', net >= 0 ? 'text-gain' : 'text-loss')}>
            {signedMoney(net)}
          </b>
        </div>
      </CardHeader>
      <div className="border-t border-line">
        {block(t('planning.table.incomeGroup', { amount: money(Number(section.totalIncome)) }), incomeCats, true, 'income')}
        {block(t('planning.table.expenseGroup', { amount: money(Number(section.totalExpenses)) }), expenseCats, false, 'expense')}
      </div>
      <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
        <span>
          {t('planning.table.incomes', { count: incomeCount })} ·{' '}
          {t('planning.table.expenses', { count: expenseCount })}
          {offCount > 0 && <> · {t('planning.table.outOfPlanCount', { count: offCount })}</>}
        </span>
        <span className="flex gap-[18px]">
          {allKeys.length > 0 && (
            <button
              type="button"
              onClick={() => onSetExpanded(allKeys, !everyOpen)}
              className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
            >
              {t(everyOpen ? 'planning.table.collapseAll' : 'planning.table.expandAll')}
            </button>
          )}
          <button
            type="button"
            onClick={() => onAdd(id, 'expense')}
            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
          >
            {t('planning.table.addToSection')} →
          </button>
        </span>
      </div>
    </Card>
  );
}
```

- [ ] **Step 5: The page**

Rewrite `src/pages/CashflowPlanning.tsx`:

- Remove `SOURCE_BY_CATEGORY`, the inline `useMutation`s, `sectionCard`, the `Table*` imports, `Badge`, `Pencil`, `Trash2`, `Repeat`.
- Keep: query, `useReportMoney`, `autoExpenses` (now `.filter((i) => !i.isUserDefined && i.included)`), `flow` (unchanged — `c.total` already counts included rows), `freeShare`, head, loading / error states, stats, flow card, dialogs, `ConfirmDeleteDialog`.
- Add state:
  ```ts
  const [expanded, setExpanded] = useState<Set<string>>(() =>
    readExpanded(typeof window === 'undefined' ? null : window.localStorage)
  );
  const persist = (next: Set<string>) => {
    setExpanded(next);
    writeExpanded(typeof window === 'undefined' ? null : window.localStorage, next);
  };
  const [addPreset, setAddPreset] = useState<{ category?: string; replacementSourceId?: string | null }>({});
  ```
- Mutations: `const { createMutation, updateMutation, deleteMutation, inclusionMutation } = useCashflowPlanningMutations();` — the dialog closes in the `onSuccess` of a local wrapper: call `createMutation.mutate(data, { onSuccess: () => setAddDialogOpen(false) })`, `updateMutation.mutate({ id, data }, { onSuccess: () => { setEditItem(null); setEditCategory(''); } })`, `deleteMutation.mutate(id, { onSuccess: () => setDeleteItem(null) })`.
- Expenses stat note: when `offExpenseCount > 0` (count of `!included` expense items across both sections) render
  `${t('planning.stats.expensesNote', { amount: money(autoExpenses) })} · ${t('planning.table.outOfPlanCount', { count: offExpenseCount })}`.
- Handlers passed to both `PlanSectionCard`s:
  ```ts
  const openAdd = (section: SectionId, type: ItemType, category?: string, replacementSourceId: string | null = null) => {
    setAddDialogSection(section);
    setAddDialogType(type);
    setAddPreset({ category, replacementSourceId });
    setAddDialogOpen(true);
  };
  onToggleExpanded={(key) => persist(toggleKey(expanded, key))}
  onSetExpanded={(keys, open) => persist(setKeys(expanded, keys, open))}
  onToggleInclusion={(key, kind, included) => inclusionMutation.mutate({ key, kind, included })}
  onAdd={(section, type, category) => openAdd(section, type, category)}
  onReplace={(section, type, item) => openAdd(section, type, item.category, item.sourceId)}
  onEdit={(section, item, category) => { setEditItem(item); setEditCategory(category); setEditSection(section); }}
  onDelete={setDeleteItem}
  ```
- Render:
  ```tsx
  <div className="grid gap-[18px]">
    <PlanSectionCard id="personal" title={t('sections.personal')} sub={t('sections.personalDescription')} section={report.personal} report={report} viewType={viewType} expanded={expanded} … />
    <PlanSectionCard id="investments" title={t('sections.investments')} sub={t('sections.investmentsDescription')} section={report.investments} … />
  </div>
  ```
- The add dialog gets `presetCategory={addPreset.category}` and `presetReplacementSourceId={addPreset.replacementSourceId}`.

- [ ] **Step 6: Lint, typecheck, format, unit tests**

Run: `npm run lint && npm run typecheck && npm run format && npm test`
Expected: PASS. Fix every lint error (the `react-hooks` rules are strict: no `setState` in effects outside the dialog's existing exemption).

- [ ] **Step 7: Verify in the app**

Start the demo profile from the worktree on port 1421 (use a separate dev-server port whenever another dev server runs: `HOME` = the shared checkout's `demo-home`, `--config` with `devUrl` 1421 and `beforeDevCommand` `npm run dev -- --port 1421 --strictPort`), confirm the lock screen greets "Alex" before typing the demo password, open `/reports/cashflow-planning` and check:

1. every category is one collapsed row with count / origin / total; clicking expands it; reload keeps it expanded;
2. "···" on an automatic row → "Nezapočítávat" mutes the row, adds "Mimo plán", lowers the category total, section net, stats and flow bar; "Započítat" restores;
3. "···" on a category → "Nezapočítávat kategorii" mutes the group with one badge, items get the disabled line; "Započítat kategorii" restores;
4. "Nahradit vlastní částkou…" opens the add dialog with the category and source preselected; saving hides the source row and shows "Nahrazuje …";
5. manual row: Upravit / Smazat work; the item key disappears after delete (no console error);
6. "Rozbalit vše / Sbalit vše" and the footer counts;
7. the console has no errors; the MCP `cashflow_report` (via `mcp__moony__cashflow_report` if the demo profile exposes it, else skip) shows `included: false` for the excluded row.

Take a screenshot of the page for the final report.

- [ ] **Step 8: Commit**

```bash
git add src/components/cashflow/PlanRowMenu.tsx src/components/cashflow/PlanItemRow.tsx src/components/cashflow/PlanGroupRow.tsx src/components/cashflow/PlanSectionCard.tsx src/pages/CashflowPlanning.tsx
git commit -m "feat(planning): categories as collapsible groups with a row menu for plan inclusion

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Design-system docs, full verification, merge

**Files:**
- Modify: `docs/design-system/README.md` (§7 Plan row, §11 decision 4)
- Modify: `docs/architecture/overview.md` (only if it lists planning page components — `grep -n "CashflowPlanning" docs/architecture/overview.md`)

- [ ] **Step 1: README §7 row**

Replace the Plan row's description with:

```markdown
| Plan | Cashflow planning | Planned recurring items in personal and investment sections; every non-empty category is a collapsible row (chevron, name, "N položek · M mimo plán · automaticky z Úvěry", included total), items under it with origin and original amount in the sub-line; any row or whole category can be left out of the plan from its "···" menu (stays visible, muted, "Mimo plán" badge, not counted); monthly / yearly switch, a 100 % flow bar instead of a Sankey diagram. |
```

- [ ] **Step 2: README §11 decision 4, planning paragraph**

Replace the sentence starting `items derived from other domains (…) carry a source badge instead of row actions,` with:

```markdown
   every non-empty category is a collapsible group row with its included total (collapsed by default,
   remembered per browser); every row has one "···" menu — automatic rows (`isUserDefined = false`:
   Úvěry, Pojištění, Nemovitosti, Akcie, Dluhopisy, Bankovní účty) offer Nezapočítávat / Započítat,
   Nahradit vlastní částkou… and Otevřít v modulu, manual rows Upravit / Nezapočítávat / Smazat,
   category rows Nezapočítávat kategorii / Přidat položku do kategorie; a row left out stays listed,
   muted, with a "Mimo plán" badge and does not count in totals, stats or the flow bar (persisted in
   `cashflow_plan_exclusions`, honoured by the MCP `cashflow_report` tool too), and
```

keeping the rest of the paragraph (flow bar, links) intact.

- [ ] **Step 3: Full verification**

Run, all from the worktree:

```bash
npm run lint && npm run typecheck && npm run format:check && npm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd src-tauri && cargo test generate_bindings -- --ignored && git diff --exit-code shared/generated-types.ts
```

Expected: everything green, no diff after regeneration.

- [ ] **Step 4: Commit docs**

```bash
git add docs/design-system/README.md docs/architecture/overview.md
git commit -m "docs(planning): grouped list and plan inclusion in the design system

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

- [ ] **Step 5: Review and merge**

Dispatch a spec-compliance review and a code-quality review (Sonnet) over `git diff main...feat/cashflow-plan-groups`; fix findings; re-run Step 3. Then merge through a temporary worktree of `main` (`git worktree add <scratch>/main-merge main && git merge --no-ff feat/cashflow-plan-groups`), verify `git merge-base --is-ancestor feat/cashflow-plan-groups main`, remove the merge worktree. Do not push.

Also add the spec and prototype (`docs/specs/2026-10-10-cashflow-planning-groups-*.{md,html}`) and this plan to the feature branch in the first commit of the branch so they ship with the work.
