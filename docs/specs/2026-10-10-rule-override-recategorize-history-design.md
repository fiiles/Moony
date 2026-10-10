# Rule override and recategorizing other payments

Design proposal of 2026-10-10, awaiting the owner's review. When the user corrects a
category in the bank ledger and asks Moony to remember it, the rule that decides this
payee's category today is rewritten (not just shadowed), the user sees how many earlier
payments the rewritten rule covers before clicking, and can give them the new category
in the same step. The bulk change is reversible from the toast.

## Problem

- **Earlier payments keep the old category.** A correction with "Zapamatovat" replaces
  the learned rule, so the next import is right. Every other payment of the same payee
  still carries the old category: the ledger and the reports disagree with the rule the
  user just made, and the only fix is to go through those rows one by one.
- **A custom rule silently wins.** The waterfall is own accounts → custom rules → learned
  payees → rule packs. When the old category comes from a custom rule, the learned
  correction lands below it: the toast says "Kategorie naučena" and the next import still
  assigns the old category. Nothing tells the user.
- **No consequence is visible before the click.** The picker does not say what decides
  this payee's category today, nor how many payments a remembered correction covers.
- **A remembered correction can be a no-op.** With no counterparty name longer than two
  characters and no account identifier, learning quietly does nothing; the toggle is
  shown anyway.

## Principles

1. **Show the consequence before the click.** The picker names what decides the category
   today and how many other payments the rewritten rule covers, with a breakdown by their
   current category ("12× Stravování · 2× bez kategorie").
2. **One concept everywhere: "Zařadit i další platby".** It means: give the rule's
   category to every payment the rule matches today, whatever its date, before or after
   the corrected one, with no time window; only the corrected row itself is left out
   (it already has the category). No exceptions by provenance; the breakdown
   is the safeguard, and the change is undoable. (A NULL provenance does not mean a human
   chose the category: the ledger's "Automatická kategorizace" button saves through the
   plain category update and records none, so "skip manual rows" would skip rows a rule
   chose.)
3. **Rewrite the rule that actually decides.** A learned rule is replaced, as today. When a
   custom rule has precedence, remembering changes that rule's category instead of storing
   a learned rule that can never fire.
4. **Reversible.** The toast offers "Vrátit zpět" for the other payments. The rule itself
   stays rewritten; undoing it is forgetting or editing the rule, as today.
5. **Never silent.** Everything changed in bulk is counted in the toast.

## Alternatives considered

| Approach | Why not |
|---|---|
| A toast with an action "Zařadit i 14 starších" after the change | 2.6 s toasts are missed; a bulk edit from a vanishing toast is hard to find again; no breakdown before acting. |
| A confirm dialog after every remembered change | Interrupts a fix-ten-rows session with ten dialogs; the rule editor already shows this kind of consequence inline, next to the decision. Kept for the rules page, where there is no inline place. |
| Move scoped learned rules above custom rules in the waterfall | Makes the override universal without UI, but changes the documented evaluation order, the "Pořadí vyhodnocení" strip on the rules page and every user's mental model of custom rules. Surfacing the conflict is clearer than silently changing precedence. |
| Warn about the custom rule and only link to it | Leaves a known-broken state behind (a dormant learned rule, an unchanged custom rule) with a pointer; the honest action is to change the rule that decides. |

Chosen: inline in the category picker (ledger), with a confirm dialog on the rules page.

## The category picker footer (ledger)

`CategorySelector` gets a `transactionId` prop. When the popover opens with learning
enabled, it loads one preview for the row (`preview_recategorization`), which says what
decides the category today and which other payments the rewritten rule would cover.
The footer below "+ Nová kategorie" renders one of four states. The toggle is independent
of the preview, so the footer never waits for it; only the history line does.

**Learned / pack / nothing decides today (default).**

```
🧠 Zapamatovat pro tento účet a směr                               [ON]
   Tento účet · Odchozí · Albert. Spolehlivé účty platí …
☑ Zařadit i 14 dalších plateb
   12× Stravování · 2× bez kategorie
```

- The toggle and its scope line are unchanged.
- The history checkbox appears only while the toggle is on and the preview reports at
  least one other payment. It is checked by default: a remembered correction is the
  strongest signal that the old category was wrong for this payee, and the count is
  visible before the click.
- While the preview loads the line reads "Zjišťuji další platby…"; with zero matches
  the line is absent.
- The breakdown lists the three largest current categories, then "+N dalších"; rows
  without a category are "bez kategorie". Counts include every matching payment, whatever
  its current category: the target is not known until the click, and the backend changes
  only rows whose category differs. The toast reports what actually changed.

**A custom rule decides today.**

```
⚑ Zařazuje vlastní pravidlo „Supermarkety“ → Stravování
   Upravit pravidlo →
🧠 Změnit kategorii tohoto pravidla                                 [ON]
   Má přednost před naučenými volbami; platí všude, kde zasáhne.
☑ Zařadit i 27 dalších plateb
   20× Stravování · 5× Potraviny · 2× bez kategorie
```

An interactive prototype of all three scenarios and the remaining footer states sits next
to this spec: `2026-10-10-rule-override-recategorize-history-prototype.html` (serve `docs/`
with the `docs` launch config, or open it over the design-system folder).

- Remembering means changing the governing custom rule's category to the picked one. No
  learned row is stored: it could never fire while the rule exists.
- The history count is the rule's own match set (all accounts, both directions), so the
  user sees that a broad pattern reaches further than this payee.
- "Upravit pravidlo →" opens the rules page with that rule's editor
  (`/settings/categorization-rules?edit=<id>`), for anyone who would rather narrow the
  pattern than retarget it.
- With the toggle off, only the row changes, as today.

**An own account decides today.** One line, no toggle, no checkbox:
"Převod mezi vlastními účty – zařazuje se automaticky." The row can still be set by hand.

**Nothing identifies the payee.** One line, no toggle, no checkbox:
"Nelze zapamatovat – platba nemá protistranu ani číslo účtu."

State precedence: own account, then custom rule, then "nothing identifies the payee",
then the default. A custom rule can be retargeted even for a row that could not be
learned, because the rule matches on its own pattern.

### What happens on pick

Picking a category other than the current one:

1. The row is saved as today (`update_transaction_category`, no provenance label).
2. With the toggle on:
   - default state: `learn_categorization` with `applyHistory` set from the checkbox. In one
     database transaction the learned row is replaced and, when asked, every matching
     other payment whose category differs gets the new category with
     `categorization_source = 'exact_match'` and a cleared suggestion. Returns the changed
     rows with their previous category and provenance.
   - custom-rule state: `retarget_custom_rule` with the rule id, the category and
     `applyHistory`. Updates the rule's category, reloads the engine's custom layer and,
     when asked, applies the rule to every matching payment whose category differs
     (`categorization_source = 'rule'`). Same return shape.
3. `refreshMoneyFlows` (which also invalidates the new preview key), then the toast.

Toasts (past tense, bottom right; the ones with an action stay 8 s):

| Situation | Title | Description | Action |
|---|---|---|---|
| Learned, no history change | Kategorie naučena (existing) | „Albert" | — |
| Learned, N other payments changed | Naučeno · {{count}} dalších plateb zařazeno | „Albert" → Potraviny | Vrátit zpět |
| Rule retargeted, no history change | Pravidlo „Supermarkety" zařazuje jako Potraviny | — | — |
| Rule retargeted, N changed | Pravidlo „Supermarkety" zařazuje jako Potraviny · {{count}} dalších plateb zařazeno | — | Vrátit zpět |
| Checkbox on, nothing differed | the no-history toast of the same row | | |
| Undo done | Změněné platby vráceny ({{count}}) | | |

"Vrátit zpět" calls `restore_transaction_categories` with the returned rows: each gets
its previous category and provenance back; rows deleted meanwhile are skipped. The rule
stays rewritten.

The report drilldown uses the same component without an account or direction, so it
never learns today and gets no footer change.

## The rules page

**Learned tab.** Changing a rule's category through the row's `CategoryPick` updates the
rule as today and, in parallel, loads the rule's history preview. Here the target is
known, so the dialog counts only the matching payments whose category differs from the
new one (`matches` minus the new category's bucket). When that count is at least one, a
confirm dialog (440 px) opens:

> **Zařadit i další platby?**
> Pravidlo pro „Albert" (Běžný účet · odchozí) nově zařazuje jako Potraviny. Zasáhne
> 14 dalších plateb: 12× Stravování · 2× bez kategorie.
>
> [Jen nové platby]  [Zařadit 14 plateb]

"Jen nové platby" keeps the rule change and leaves history alone. The primary applies
the rule (`apply_learned_rule_to_history`) and toasts "{{count}} dalších plateb
zařazeno" with "Vrátit zpět".

**Custom tab.** The editor keeps its current preview and the "Zařadit nezařazené hned"
checkbox. Overriding already categorized rows from the editor is a follow-up, so that a
new broad rule does not start rewriting history by default in this change.

## Backend

New module `services/categorization/recategorize.rs`, all functions taking
`&Connection`, errors as `AppError`:

- `enum Matcher { Learned(LearnedScope), Custom(CategorizationRule) }` where
  `LearnedScope` is the account, direction and identity-or-payee-key the learned layer
  keys on (the same key `learn_scoped` writes).
- `matching_rows(conn, &Matcher, exclude: Option<&str>) -> Vec<MatchedRow>` with
  `MatchedRow { id, category_id, source }`. Learned: rows of the account and direction,
  matched with the learned-rule matcher already used by the hit counts (made
  `pub(crate)`). Custom: every row through a single-rule engine, as the editor preview
  does. The edited row is excluded. All time, every status.
- `breakdown(&[MatchedRow]) -> Vec<CategoryCount { category_id: Option<String>, count }>`,
  largest first, "no category" sorted by its count like any other.
- `apply(conn, &[MatchedRow], category_id, source_label) -> Vec<ChangedRow>`: only rows
  whose category differs; `UPDATE bank_transactions SET category_id, categorization_source,
  suggested_category_id = NULL`. `ChangedRow { id, previous_category_id, previous_source }`.
- `restore(conn, &[ChangedRow]) -> usize`.
- `CategorizationEngine::explain(&tx) -> (Layer, CategorizationResult)` with
  `Layer ∈ { OwnAccount, Custom, Learned, Pack, None }`, so the preview names the
  deciding layer without inferring it from rule ids. `categorize` becomes
  `explain(..).1`.

Thin commands in `commands/categorization.rs`:

| Command | Input | Output |
|---|---|---|
| `preview_recategorization` | `transactionId` | `RecategorizationPreview` |
| `learn_categorization` (extended) | `data: InsertScopedLearning`, `applyHistory: bool` | `ChangedRow[]` |
| `retarget_custom_rule` | `ruleId`, `categoryId`, `applyHistory` | `ChangedRow[]` |
| `preview_learned_rule_history` | learned rule `id` | `RecategorizationPreview` |
| `apply_learned_rule_to_history` | learned rule `id` | `ChangedRow[]` |
| `restore_transaction_categories` | `rows: ChangedRow[]` | `number` restored |

```ts
interface RecategorizationPreview {
  governing: 'ownAccount' | 'custom' | 'learned' | 'pack' | 'none';
  /** The custom rule that decides today, when `governing` is 'custom'. */
  rule: { id: string; name: string; categoryId: string } | null;
  /** False when neither a payee key nor an account identity exists. */
  learnable: boolean;
  /** Other payments the rewritten rule matches, the edited row excluded. */
  matches: number;
  byCategory: { categoryId: string | null; count: number }[];
}
interface ChangedRow { id: string; previousCategoryId: string | null; previousSource: string | null }
```

Types are added to `shared/schema.ts` (the wire contract), the Rust structs carry
per-field `#[serde(rename)]`, are registered in `bindings.rs`, and `generated-types.ts`
is regenerated. No migration: no table or column changes.

The MCP layer is untouched: learning stays a UI-only, human-only path.

## Frontend

- `CategorySelector`: new `transactionId` prop (ledger passes it); a
  `useQuery(['recategorization-preview', transactionId])` enabled while the popover is
  open and learning is possible; the footer states above; `handleSelect` carries
  `remember` and `applyHistory`; the toasts with undo.
- `BankAccountDetail` passes `transactionId`; nothing else changes there.
- `CategorizationRules` learned tab: the confirm dialog and its two mutations.
- `RuleEditDialog` opens from `?edit=<id>` (the "Upravit pravidlo →" link), next to the
  existing `?new=1` prefill in `utils/rules-link.ts` (`editRuleHref`, `parseRuleSearch`).
- `utils/recategorization.ts`: `formatBreakdown(byCategory, nameOf, uncategorizedLabel,
  max = 3)` → "12× Stravování · 2× bez kategorie · +1 další". Pure, tested.
- `money-flow-refresh.ts` adds `recategorization-preview` to the invalidated keys.

## Strings (both locales)

`bank_accounts.categorization.*` (ledger picker and toasts):

| Key | cs | en |
|---|---|---|
| `historyChecking` | Zjišťuji další platby… | Checking other payments… |
| `applyHistory` (plural forms) | Zařadit i {{count}} další platbu / platby / plateb | Also categorize {{count}} other payment(s) |
| `uncategorizedCount` | bez kategorie | no category |
| `moreCategories` | +{{count}} dalších | +{{count}} more |
| `governedByRule` | Zařazuje vlastní pravidlo „{{rule}}" → {{category}} | Categorized by your rule "{{rule}}" → {{category}} |
| `retargetRule` | Změnit kategorii tohoto pravidla | Change this rule's category |
| `retargetRuleScope` | Má přednost před naučenými volbami; platí všude, kde zasáhne. | Takes precedence over learned choices; applies wherever it matches. |
| `editRule` | Upravit pravidlo → | Edit rule → |
| `ownAccountNote` | Převod mezi vlastními účty – zařazuje se automaticky. | Transfer between your own accounts – categorized automatically. |
| `notLearnable` | Nelze zapamatovat – platba nemá protistranu ani číslo účtu. | Cannot be remembered – the payment has no counterparty or account number. |
| `learnedApplied` (plural) | Naučeno · {{count}} dalších plateb zařazeno | Learned · {{count}} other payments categorized |
| `ruleRetargeted` | Pravidlo „{{rule}}" zařazuje jako {{category}} | Rule "{{rule}}" now assigns {{category}} |
| `ruleRetargetedApplied` (plural) | Pravidlo „{{rule}}" zařazuje jako {{category}} · {{count}} dalších plateb zařazeno | Rule "{{rule}}" now assigns {{category}} · {{count}} other payments categorized |
| `undoHistory` | Vrátit zpět | Undo |
| `historyRestored` (plural) | Změněné platby vráceny ({{count}}) | Changed payments restored ({{count}}) |

`categorization.learnedTab.history.*` (rules page dialog):

| Key | cs | en |
|---|---|---|
| `title` | Zařadit i další platby? | Also categorize other payments? |
| `description` (plural) | Pravidlo pro „{{payee}}" ({{account}} · {{direction}}) nově zařazuje jako {{category}}. Zasáhne {{count}} dalších plateb: {{breakdown}}. | The rule for "{{payee}}" ({{account}} · {{direction}}) now assigns {{category}}. It covers {{count}} other payments: {{breakdown}}. |
| `onlyNew` | Jen nové platby | Only new payments |
| `apply` (plural) | Zařadit {{count}} plateb | Categorize {{count}} payments |
| `applied` (plural) | {{count}} dalších plateb zařazeno | {{count}} other payments categorized |

Czech plural forms follow the existing `_one` / `_few` / `_other` pattern.

## Tests

Rust (`recategorize.rs`, in-memory schema with only the tables used):

- `matching_rows` for a learned scope honours account, direction and identity vs payee
  key, and excludes the edited row; for a custom rule it spans accounts.
- `breakdown` orders by count and keeps the "no category" bucket.
- `apply` skips rows already in the category, writes the provenance label, clears a pending
  suggestion, returns previous values; `restore` round-trips them and skips missing rows.
- `retarget` changes the rule's category and nothing else about it.
- `explain` names each layer, own account first.
- `learn_categorization` with `applyHistory` is atomic: a failing update leaves the learned
  row unchanged (mirrors the existing failed-replacement test).

TypeScript: `formatBreakdown` (ordering, "+N dalších", the uncategorized label).

Commands, hooks and components are not unit-tested (testing policy).

## Docs

- `docs/architecture/overview.md`, categorization row: the recategorize module, the
  `explain` layer, the retarget path.
- `CHANGELOG.md` Unreleased → Added: remembered corrections offer to categorize earlier
  payments; Changed: a correction under a custom rule rewrites that rule and says so.

## Out of scope

- Overriding categorized rows from the custom rule editor (follow-up, see above).
- Learning from the report drilldown.
- Any change to the waterfall order or to rule packs.
- Bulk recategorization without a rule (a ledger multi-select edit).
