# Cashflow planning: categories as collapsible groups, items in or out of the plan

Proposed on 2026-10-10, awaiting the owner's approval. Visual reference:
`docs/specs/2026-10-10-cashflow-planning-groups-prototype.html` (scenarios A–C, design notes under
"Poznámky k návrhu"). The page keeps what the owner likes — stats, the 100 % flow bar, the two section
cards — and changes only the list inside the cards and what feeds the totals.

## Problem

- **Automatic items cannot be left out.** Loans, insurance, property costs, dividends, bond coupons
  and account interest flow into the plan whether or not the user wants them there. A mortgage paid
  from a joint account or dividend estimates the user does not trust can only be removed by deleting
  the source entity, or by creating a manual "replacement" through a select in the add dialog that
  nobody discovers.
- **The investment section is a flat list.** Thirty dividend rows sit between interest and bond rows
  with a "Kategorie" column repeating the same label thirty times. The category totals that would
  make the section readable exist in the report but are never shown.
- **Two row-action patterns.** Manual rows show pencil and trash icons; automatic rows show a module
  badge and nothing else.

## Mental model

The plan is the list of everything Moony counts for you. Every row — automatic or manual — is either
**in the plan** or **out of it**. Nothing is deleted when a row is taken out: it stays visible, muted,
with a "Mimo plán" label, and its amount simply stops contributing. The same switch exists one level up
for a whole category ("nepočítat s dividendami"). One row type, one action affordance ("···"), one
state ("Mimo plán").

## Page design

Unchanged: page head, monthly / yearly switch, "Přidat položku", the three stats, the flow card and the
two section cards with their head (title, sub, net on the right). Stats and the flow bar read the
included totals only; the expenses stat note gains "· N položek mimo plán" when any row is excluded.

### The list inside a section card

Two blocks, "Příjmy · {amount}" and "Výdaje · {amount}" (existing `.table__group` label; the amount is
the included total). Inside each block, **one row per non-empty category**; empty categories are not
rendered. The table header and the "Kategorie" and "Frekvence" columns disappear: the row is a
two-part list — name with a sub-line on the left, amount on the right — plus a "···" cell.

**Group row** (52 px, whole row clickable, `aria-expanded`, chevron that rotates 90° when open)

- Chevron · category name (12/650 ink) · sub-line 10/500 ink-4:
  `{N položek} · {M mimo plán | celá kategorie mimo plán} · {automaticky z Úvěry, Akcie | ručně}`.
- Right: included total, tabular, `+` in gain for income, `−` in ink for expenses.
- Hover / focus-within: `well` background and the "···" button.

**Item row** (44 px, indented under the chevron column)

- Name (12/650 ink) · sub-line: `automaticky z {module} · {original amount and frequency}` or
  `ručně · {frequency}`; a manual replacement keeps its "Nahrazuje {source}" line.
- Right: amount with sign, "···" on hover / focus-within.
- The FX notes stay as today: "Kurz není k dispozici" instead of the amount, "Neúplná částka" under
  a partial one.

**Out of the plan** (`is-off`): name, sub-line and amount drop to ink-4 / ink-5; an outline badge
"Mimo plán" sits left of the amount. An excluded category shows the badge on its group row only (its
items are muted without individual badges) and its total shows the amount that *would* count, muted.

**"···" menu** (`ui/dropdown-menu.tsx`, ghost `icon-sm` button named "Možnosti: {name}" for assistive
tech; visible on hover and focus-within like every other table)

| Row | Items |
|---|---|
| Category | *Nezapočítávat kategorii* / *Započítat kategorii* · *Přidat položku do kategorie* |
| Automatic item | *Nezapočítávat* / *Započítat* · *Nahradit vlastní částkou…* · *Otevřít v modulu {module}* |
| Manual item | *Nezapočítávat* / *Započítat* · *Upravit* · separator · *Smazat* (danger) |
| Any item inside an excluded category | first entry becomes the disabled line *Celá kategorie je mimo plán*; the rest as above |

*Nahradit vlastní částkou…* opens the existing add dialog with the section, category, type and
"Automatický zdroj k nahrazení" preselected — the replacement feature stays, it only gets a visible
entry point. *Přidat položku do kategorie* opens the add dialog with that category preselected.
*Otevřít v modulu* navigates to the module's list page (route from `nav-config.ts`).

**Card footer**: `{N příjmy} · {M výdajů} · {K položek mimo plán}` on the left; on the right
"Rozbalit vše" / "Sbalit vše" (toggles every group in that card) and "Přidat položku do sekce →".

**Expansion**: every group starts collapsed. Which groups are open is remembered per browser in
`localStorage` under `moony.cashflowPlanning.expanded` (array of category keys; every read and write
wrapped in try/catch, the page renders correctly without it). The investment section therefore opens
as eight rows with totals instead of forty.

**Empty states** unchanged ("V této sekci zatím nejsou plánované příjmy / výdaje").

### Copy (cs / en)

New keys in `reports.json`, both locales:

| Key | cs | en |
|---|---|---|
| `planning.table.outOfPlan` | Mimo plán | Out of plan |
| `planning.table.groupItems_one/few/other` | {{count}} položka / položky / položek | {{count}} item / items |
| `planning.table.groupExcluded_one/few/other` | {{count}} mimo plán | {{count}} out of plan |
| `planning.table.groupCategoryExcluded` | celá kategorie mimo plán | whole category out of plan |
| `planning.table.autoFrom` | automaticky z {{modules}} | automatic from {{modules}} |
| `planning.table.outOfPlanCount_one/few/other` | {{count}} položka / položky / položek mimo plán | {{count}} item / items out of plan |
| `planning.table.expandAll` / `collapseAll` | Rozbalit vše / Sbalit vše | Expand all / Collapse all |
| `planning.table.addToCategory` | Přidat položku do kategorie | Add an item to this category |
| `planning.menu.label` | Možnosti: {{name}} | Options: {{name}} |
| `planning.menu.exclude` / `include` | Nezapočítávat / Započítat | Leave out / Count in |
| `planning.menu.excludeCategory` / `includeCategory` | Nezapočítávat kategorii / Započítat kategorii | Leave the category out / Count the category in |
| `planning.menu.categoryExcluded` | Celá kategorie je mimo plán | The whole category is out of the plan |
| `planning.menu.replace` | Nahradit vlastní částkou… | Replace with my own amount… |
| `planning.menu.openModule` | Otevřít v modulu {{module}} | Open in {{module}} |
| `planning.toast.excluded` / `included` | Položka vyřazena z plánu / Položka opět v plánu | Left out of the plan / Counted in again |
| `planning.toast.categoryExcluded` / `categoryIncluded` | Kategorie vyřazena z plánu / Kategorie opět v plánu | Category left out / Category counted in again |

Removed keys: `planning.table.category`, `planning.table.frequency`, `planning.table.monthly`,
`planning.table.yearly`, `planning.table.editInModule` (the badge is gone; the module name lives in the
sub-line and the menu). `planning.table.auto` / `manual` stay for the item sub-line.

## Inclusion model

Persisted exclusions, one row per key, in a new table `cashflow_plan_exclusions`:

| Key | Meaning |
|---|---|
| `category:<categoryKey>` | the whole category, including items added later (a new stock under an excluded "Dividendy z akcií" is excluded automatically) |
| `<sourceId>` (`loan:<id>`, `insurance:<id>`, `propertyCosts:<id>`, `account:<id>`, `stock:<ticker>`, `bond:<id>`, `rental:<id>`) | one automatic item, the same id the replacement feature already uses |
| `item:<id>` | one manual item |

Rules:

- An item is **included** when neither its own key nor its category key is stored. Totals of category,
  section and report sum included items only. Excluded items keep their amount in the payload so the UI
  can show it.
- Replacement wins over exclusion as today: a replaced source is not listed at all; its replacement is
  an ordinary manual item that can itself be excluded.
- Report **quality** (missing FX) describes included items only, the same way replaced sources already
  stay out of it; an excluded row with a missing rate still shows "Kurz není k dispozici" in its row.
- `automaticSources` (the inventory the add dialog offers for replacement) is unaffected by exclusions.
- Deleting a manual item deletes its `item:` key. Keys of automatic sources that disappear (repaid loan,
  sold stock) are harmless leftovers and are not cleaned, like `milestone_states`.
- The projection does not read the planning report (`services/projection.rs` has no dependency on it),
  so exclusions change only this page and the MCP tool `cashflow_report`.

## Contract

`shared/schema.ts` (7-step procedure in `docs/standards/type-contract.md`):

```ts
export interface CashflowReportItem {
  // …existing fields…
  /** False when this row or its category is out of the plan; the amount is still present. */
  included: boolean;
}
export interface CashflowCategory {
  // …existing fields…
  /** False when the whole category is out of the plan. */
  included: boolean;
  /** Sum of included items (unchanged field, changed meaning). */
  total: string;
}
export interface SetCashflowPlanInclusion {
  key: string;      // 'category:<key>' | '<sourceId>' | 'item:<id>'
  included: boolean;
}
```

New command `set_cashflow_plan_inclusion(data: SetCashflowPlanInclusion) -> ()` in
`src-tauri/src/commands/cashflow.rs`, wrapped as `cashflowApi.setInclusion(data)` in
`src/lib/tauri-api.ts`. Section and report totals keep their fields and now mean "included only".

MCP `cashflow_report` (`services/mcp/analytics.rs`): each item gains `"included": bool`; `summary`
already reflects the report totals. Documented in `CHANGELOG.md` (Unreleased).

## Backend behavior (`services/cashflow_planning.rs`)

- Migration `013_cashflow_plan_exclusions`:
  `CREATE TABLE IF NOT EXISTS cashflow_plan_exclusions (key TEXT PRIMARY KEY, created_at INTEGER NOT NULL DEFAULT (unixepoch()));`
  plus the schema snapshot and the table row in `docs/architecture/database.md`.
- `SetCashflowPlanInclusion::validate()`: key non-empty, no whitespace, prefix in
  `category:` (must be a known category key), `item:`, or one of the seven source prefixes.
- `set_inclusion(conn, data)`: `included = false` inserts the key (`INSERT OR IGNORE`),
  `included = true` deletes it. Idempotent.
- `get_report_in_currency`: load the key set once; while assembling items compute `included`, feed only
  included items into category / section / report totals and into `fx.record_quality`.
- `delete_cashflow_item`: also `DELETE FROM cashflow_plan_exclusions WHERE key = 'item:' || ?1`.
- Commands stay thin; `AppError` everywhere; no `unwrap()` outside tests.

## Frontend

- `src/pages/CashflowPlanning.tsx` (588 lines) is split: the page keeps the head, stats, flow card and
  dialogs; the list moves to `src/components/cashflow/PlanSectionCard.tsx` with `PlanGroupRow.tsx`,
  `PlanItemRow.tsx` and `PlanRowMenu.tsx`. Expansion persistence is a small pure helper in
  `src/utils/plan-expansion.ts` (read, toggle, set-all) so it can be unit-tested.
- The page's inline `useMutation`s move to `src/hooks/use-cashflow-planning-mutations.ts`
  (create, update, delete, setInclusion); every one refreshes through the existing
  `refreshMoneyFlows(queryClient)` so `["cashflow-report"]` and the related keys invalidate as today.
- `AddCashflowItemDialog` gains optional `presetCategory` and `presetReplacementSourceId` props; the
  replacement select stays where it is.
- Design-system README: §7 "Plan" row and §11 decision 4 describe the grouped list, "Mimo plán" and
  the "···" menu; `SOURCE_BY_CATEGORY` becomes the single map from category to module label and route.

## Testing (per `docs/standards/testing.md`)

Rust (`cashflow_planning.rs` tests, in-memory schema extended with the new table):

- item key excludes one automatic item: listed with `included: false`, category / section / report
  totals and the flow omit it, `automaticSources` still contains it;
- category key excludes every item including a manual one and an item added later; its group reports
  `included: false`;
- item key inside an excluded category: still `included: false` after the category is included again
  until its own key is removed (precedence is "either key excludes");
- replacement and exclusion together: a replaced source stays absent, its replacement can be excluded;
- quality ignores excluded items with a missing rate;
- `set_inclusion` is idempotent both ways; `validate()` rejects unknown prefixes and categories;
- deleting a manual item removes its key;
- MCP `cashflow_report` carries `included` and its summary matches the page totals.

TypeScript: unit tests for `src/utils/plan-expansion.ts`. Components and hooks are out of scope.

## Out of scope

- Sub-grouping inside a category (dividends by sector or currency), sorting, search.
- Bulk selection; per-item overrides inside an excluded category.
- Exclusions in the projection or anywhere else that does not read the planning report.
- The yearly / monthly switch, the flow bar, the Sankey (already removed) and the Cashflow actuals page.
