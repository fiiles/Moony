# Rule override and recategorizing other payments — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a user corrects a category and asks Moony to remember it, the rule that decides that payee's category is rewritten, the picker shows how many other payments the rule covers, and the user can give them the new category in the same step, reversibly. Plus one bug fix: every enabled rule pack gets its own tab on the rules page.

**Architecture:** A new Rust service module `services/categorization/recategorize.rs` answers "which payments does this rule cover" (learned scope or custom rule), breaks them down by current category, applies a category to them and restores them. The engine gains `explain()` so the preview can name the layer that decides today. Thin commands expose preview, learn-with-history, retarget-custom-rule, learned-rule history and restore. The frontend adds a footer to the category picker (ledger) and a confirm dialog on the rules page.

**Tech Stack:** Rust (rusqlite, specta), Tauri 2 commands, React 19 + TanStack Query + react-i18next + sonner toasts, Vitest, cargo test.

**Spec:** `docs/specs/2026-10-10-rule-override-recategorize-history-design.md` (prototype next to it).

**Worktree:** everything happens in `../Moony-tauri-recategorize` on branch `feat/recategorize-history`. Never touch the shared checkout `Moony-tauri`.

## Global constraints

- Rule 1: `invoke()` only inside `src/lib/tauri-api.ts`.
- Rules 2 and 3: new wire types go in `src-tauri/src/models/categorization.rs` with per-field `#[serde(rename = "camelCase")]`, are registered in `src-tauri/src/bindings.rs`, mirrored by hand in `shared/schema.ts`, and `shared/generated-types.ts` is regenerated with `cd src-tauri && cargo test generate_bindings -- --ignored`.
- Rule 4: commands stay thin; logic in services taking `&Connection`; errors are `AppError` (`src-tauri/src/error.rs`: `Database`, `Auth`, `NotFound`, `Validation`, `ExternalApi`, `Encryption`, `Internal`). `db.with_conn(|conn| ...)` takes `FnOnce(&Connection) -> Result<T>` and returns `Result<T>`.
- Rule 5: no `.unwrap()` outside tests.
- Rule 7: after writes, the frontend calls `refreshMoneyFlows(queryClient)` from `src/lib/money-flow-refresh.ts`.
- Rule 8: every user-facing string in BOTH `src/i18n/locales/en/*.json` and `src/i18n/locales/cs/*.json`. Czech plurals use `_one` / `_few` / `_other`; English `_one` / `_other`.
- Rule 10: no money arithmetic here; ids are TEXT UUIDs.
- Copy rules (`docs/design-system/README.md` §10): toasts are past-tense verbs, buttons name the outcome, sentence case, no exclamation marks.
- Provenance labels on `bank_transactions.categorization_source`: `'exact_match'` for learned rules, `'rule'` for custom rules (constants in `services/categorization/apply.rs`).
- Rust tests use `Connection::open_in_memory()` with a hand-written minimal schema, never the `Database` struct.
- Conventional commits; `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` as the last line of every commit message.
- Gates before every commit: `npm run lint && npm run typecheck && npm test` for TS changes, `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test` for Rust changes.

## File structure

| File | Responsibility |
|---|---|
| `src-tauri/src/services/categorization/recategorize.rs` (new) | Coverage of a rule (learned scope or custom rule) over `bank_transactions`, breakdown by category, apply, restore, retarget of a custom rule, the two previews. |
| `src-tauri/src/services/categorization/engine.rs` | `Layer` enum and `explain()`; `categorize()` delegates. |
| `src-tauri/src/services/categorization/hits.rs` | `learned_matches` and `engine_for` become `pub(crate)` for reuse. |
| `src-tauri/src/services/categorization/learn.rs` | `learn_scoped_and_apply` (learned row + history in one transaction). |
| `src-tauri/src/models/categorization.rs` | Wire types `CategoryCount`, `ChangedRow`, `GoverningRule`, `RecategorizationPreview`. |
| `src-tauri/src/commands/categorization.rs`, `lib.rs`, `bindings.rs` | Six thin commands, registration. |
| `shared/schema.ts`, `src/lib/tauri-api.ts` | Contract and wrappers. |
| `src/utils/recategorization.ts` (new) | `formatBreakdown`, `changedByPick` (pure, tested). |
| `src/utils/rules-link.ts` | `editRuleHref`, `parseEditRuleSearch`. |
| `src/utils/rule-hits.ts` | `packTabValue`, `packIdFromTab` (bug fix). |
| `src/components/bank-accounts/CategorySelector.tsx` | Picker footer states, learn/retarget/history flow, undo toast. |
| `src/pages/BankAccountDetail.tsx` | Passes `transactionId`. |
| `src/pages/CategorizationRules.tsx` | Pack tabs (bug fix), `?edit=` link, learned-tab history dialog. |
| `src/lib/money-flow-refresh.ts` | Invalidates `recategorization-preview`. |
| Locales `bank_accounts.json`, `categorization.json`, `common.json` (cs + en) | Strings. |
| `docs/architecture/overview.md`, `CHANGELOG.md` | Docs. |

---

### Task 0: Commit the spec, the prototype and the launch config

**Files:**
- Already in the worktree, uncommitted: `docs/specs/2026-10-10-rule-override-recategorize-history-design.md`, `docs/specs/2026-10-10-rule-override-recategorize-history-prototype.html`, `.claude/launch.json` (the `docs` server config).

- [ ] **Step 1: Commit**

```bash
cd ../Moony-tauri-recategorize
git add docs/specs/2026-10-10-rule-override-recategorize-history-design.md docs/specs/2026-10-10-rule-override-recategorize-history-prototype.html .claude/launch.json
git commit -m "docs: spec and prototype for rule override and recategorizing other payments

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 1: Bug fix — one tab per enabled rule pack

Today the rules page has a single "Balíček …" tab whose label shows the first enabled pack and a `Select` inside the content switches packs. The owner expects one tab per enabled pack ("global" and "CZ" both visible).

**Files:**
- Modify: `src/utils/rule-hits.ts` (append two helpers after `packCountryName`)
- Test: `src/utils/rule-hits.test.ts`
- Modify: `src/pages/CategorizationRules.tsx` (`Tab` type, `selectedPackId` state, `activePackId`, `TabsTrigger` list, pack `TabsContent`, the `Select` in the pack card header)
- Modify: `src/i18n/locales/cs/categorization.json`, `src/i18n/locales/en/categorization.json` (`tabs.packGlobal`)
- Modify: `CHANGELOG.md` (Unreleased → Fixed)

**Interfaces:**
- Produces: `packTabValue(packId: string): string` (`'pack:' + packId`), `packIdFromTab(tab: string): string | null`.

- [ ] **Step 1: Write the failing tests**

Append to `src/utils/rule-hits.test.ts` (add `packIdFromTab, packTabValue` to the import from `./rule-hits`):

```ts
describe('pack tabs', () => {
  it('names a pack tab by its id', () => {
    expect(packTabValue('cz')).toBe('pack:cz');
    expect(packTabValue('global')).toBe('pack:global');
  });

  it('reads the pack id back from a tab value', () => {
    expect(packIdFromTab('pack:cz')).toBe('cz');
    expect(packIdFromTab('pack:global')).toBe('global');
  });

  it('gives null for the custom and learned tabs and for an empty pack id', () => {
    expect(packIdFromTab('custom')).toBeNull();
    expect(packIdFromTab('learned')).toBeNull();
    expect(packIdFromTab('pack:')).toBeNull();
  });
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npx vitest run src/utils/rule-hits.test.ts`
Expected: FAIL, `packTabValue is not a function` (or an import error).

- [ ] **Step 3: Implement the helpers**

Append to `src/utils/rule-hits.ts`:

```ts
/** Tab value of one pack's tab on the rules page ("pack:cz"). */
const PACK_TAB_PREFIX = 'pack:';

export function packTabValue(packId: string): string {
  return PACK_TAB_PREFIX + packId;
}

/** The pack id a tab value names, or `null` for the custom and learned tabs. */
export function packIdFromTab(tab: string): string | null {
  if (!tab.startsWith(PACK_TAB_PREFIX)) return null;
  const id = tab.slice(PACK_TAB_PREFIX.length);
  return id.length > 0 ? id : null;
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `npx vitest run src/utils/rule-hits.test.ts`
Expected: PASS.

- [ ] **Step 5: Rebuild the tabs in `CategorizationRules.tsx`**

Import the helpers: add `packIdFromTab, packTabValue` to the existing import from `@/utils/rule-hits`.

Replace `type Tab = 'custom' | 'learned' | 'packs';` with:

```ts
type Tab = 'custom' | 'learned' | `pack:${string}`;
```

Delete the line `const [selectedPackId, setSelectedPackId] = useState('');`.

Replace

```ts
  const activePackId = selectedPackId || enabledPacks[0]?.packId || rulePacks[0]?.packId || '';
```

with

```ts
  // The pack whose tab is open; on the other tabs the first enabled pack feeds the top-rule stat.
  const activePackId = packIdFromTab(tab) ?? enabledPacks[0]?.packId ?? rulePacks[0]?.packId ?? '';
```

After the `countryOf` definition add:

```ts
  const packLabel = (pack: { packId: string; country: string }) =>
    pack.packId === 'global' ? t('tabs.packGlobal') : t('tabs.pack', { country: countryOf(pack) });
```

Below the queries (after `const { data: rulePacks = [] } = useQuery({...})` and the `enabledPacks` line) add a guard so a tab of a pack disabled in Settings falls back:

```ts
  useEffect(() => {
    const packId = packIdFromTab(tab);
    if (packId && rulePacks.length > 0 && !enabledPacks.some((p) => p.packId === packId)) {
      setTab('custom');
    }
  }, [tab, rulePacks.length, enabledPacks]);
```

Replace the single pack trigger

```tsx
          <TabsTrigger value="packs">
            {activePack ? t('tabs.pack', { country: activeCountry }) : t('tabs.packGeneric')}{' '}
            <span className="text-ink-4">{packRules.length}</span>
          </TabsTrigger>
```

with one trigger per enabled pack:

```tsx
          {enabledPacks.map((pack) => (
            <TabsTrigger key={pack.packId} value={packTabValue(pack.packId)}>
              {packLabel(pack)} <span className="text-ink-4">{pack.ruleCount}</span>
            </TabsTrigger>
          ))}
```

Change the pack content wrapper `<TabsContent value="packs" className="mt-0">` … `</TabsContent>` so the same card renders once per enabled pack:

```tsx
        {enabledPacks.map((pack) => (
          <TabsContent key={pack.packId} value={packTabValue(pack.packId)} className="mt-0">
            {/* existing <Card variant="table"> … </Card> body, unchanged except the header below */}
          </TabsContent>
        ))}
```

Inside that card header replace the whole `{rulePacks.length > 1 && (<Select …>…</Select>)}` block with a link to Settings (the `Link` import from `wouter` already exists):

```tsx
                <Link href="/settings/categorization" className="text-caption font-650 text-ink-2 hover:underline">
                  {t('packTab.settingsLink')}
                </Link>
```

Remove the now-unused `Select, SelectContent, SelectItem, SelectTrigger, SelectValue` import if nothing else in the file uses it (check with `grep -n "Select" src/pages/CategorizationRules.tsx`; the learned tab's page-size select may still use it — keep the import if so).

- [ ] **Step 6: Add the locale key**

`src/i18n/locales/cs/categorization.json`, inside `"tabs"`: `"packGlobal": "Globální balíček"`.
`src/i18n/locales/en/categorization.json`, inside `"tabs"`: `"packGlobal": "Global pack"`.

- [ ] **Step 7: Changelog**

In `CHANGELOG.md` under `## [Unreleased]`, add (create the `### Fixed` heading if missing, after `### Changed`):

```markdown
### Fixed

- **Every enabled rule pack has its own tab** on the categorization rules page. Before, only the
  first enabled pack was visible as a tab and the others hid behind a selector inside it.
```

- [ ] **Step 8: Gates**

Run: `npm run lint && npm run typecheck && npm test`
Expected: all green.

- [ ] **Step 9: Commit**

```bash
git add src/utils/rule-hits.ts src/utils/rule-hits.test.ts src/pages/CategorizationRules.tsx src/i18n/locales/cs/categorization.json src/i18n/locales/en/categorization.json CHANGELOG.md
git commit -m "fix: one tab per enabled rule pack on the rules page

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Engine `explain()` names the deciding layer

**Files:**
- Modify: `src-tauri/src/services/categorization/engine.rs` (`categorize`, new `Layer`, `explain`)
- Modify: `src-tauri/src/services/categorization/mod.rs` (re-export `Layer`)
- Test: tests module at the bottom of `engine.rs`

**Interfaces:**
- Produces: `pub enum Layer { OwnAccount, Custom, Learned, Pack, None }` and `CategorizationEngine::explain(&self, tx: &TransactionInput) -> (Layer, CategorizationResult)`.

- [ ] **Step 1: Write the failing test**

Look at the existing tests module in `engine.rs` (`grep -n "mod tests" src-tauri/src/services/categorization/engine.rs`) and add, inside it:

```rust
    #[test]
    fn explain_names_the_layer_that_decides() {
        let pack_rule = CategorizationRule::new(
            "cz/albert".into(),
            "Albert".into(),
            super::super::types::RuleType::Contains,
            "albert".into(),
            "cat_groceries".into(),
        );
        let engine = CategorizationEngine::new(vec![pack_rule]);
        engine.set_own_ibans(vec!["CZ6508000000192000145399".into()]);
        engine.update_custom_rules(vec![CategorizationRule::new(
            "custom-1".into(),
            "Supermarkety".into(),
            super::super::types::RuleType::Contains,
            "lidl".into(),
            "cat_dining".into(),
        )]);
        engine.learn_scoped("acc-1", false, Some("Rohlik.cz"), None, "cat_groceries");

        let mut own = TransactionInput::new("t1".into(), Some("transfer".into()), None, -100.0);
        own.counterparty_iban = Some("CZ65 0800 0000 1920 0014 5399".into());
        assert_eq!(engine.explain(&own).0, Layer::OwnAccount);

        let custom = TransactionInput::new("t2".into(), Some("Lidl".into()), None, -100.0);
        assert_eq!(engine.explain(&custom).0, Layer::Custom);

        let mut learned = TransactionInput::new("t3".into(), None, Some("Rohlik.cz".into()), -100.0);
        learned.bank_account_id = Some("acc-1".into());
        assert_eq!(engine.explain(&learned).0, Layer::Learned);

        let pack = TransactionInput::new("t4".into(), Some("Albert".into()), None, -100.0);
        assert_eq!(engine.explain(&pack).0, Layer::Pack);

        let none = TransactionInput::new("t5".into(), Some("xyz".into()), None, -100.0);
        let (layer, result) = engine.explain(&none);
        assert_eq!(layer, Layer::None);
        assert!(!result.has_category());
    }
```

If the tests module imports differ (e.g. `use super::*;` only), add `use super::super::types::RuleType;` and write `RuleType::Contains`.

- [ ] **Step 2: Run the test to see it fail**

Run: `cd src-tauri && cargo test explain_names_the_layer_that_decides`
Expected: compile error, `explain` / `Layer` not found.

- [ ] **Step 3: Implement**

In `engine.rs`, above `pub struct CategorizationEngine`:

```rust
/// The waterfall layer that decided a transaction's category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The counterparty is one of the user's own accounts.
    OwnAccount,
    /// A user-defined custom rule.
    Custom,
    /// A learned payee (exact match).
    Learned,
    /// A locale rule pack.
    Pack,
    /// Nothing matched.
    None,
}
```

Replace the body of `pub fn categorize(&self, tx: &TransactionInput) -> CategorizationResult` with `self.explain(tx).1` and add, directly above it, `explain` with the exact logic the old `categorize` had, tagging each return:

```rust
    /// Categorize and say which layer decided. Flow (priority order):
    /// own account → custom rules → learned payees → locale packs → none.
    pub fn explain(&self, tx: &TransactionInput) -> (Layer, CategorizationResult) {
        if self.is_internal_transfer(
            tx.counterparty_iban.as_deref(),
            tx.counterparty_bank_code.as_deref(),
            tx.counterparty_country.as_deref(),
        ) {
            return (
                Layer::OwnAccount,
                CategorizationResult::Match {
                    category_id: "cat_internal_transfers".to_string(),
                    source: super::types::CategorizationSource::Rule {
                        rule_id: "own_account_iban".to_string(),
                        rule_name: "Internal Transfer (own account)".to_string(),
                    },
                },
            );
        }
        if let Ok(rules) = self.custom_rule_engine.read() {
            if let Some((result, _stop_processing)) = rules.apply(tx) {
                return (Layer::Custom, result);
            }
        }
        if let Ok(exact) = self.exact_match.read() {
            if let Some(result) = exact.apply(tx) {
                return (Layer::Learned, result);
            }
        }
        if let Ok(rules) = self.default_rule_engine.read() {
            if let Some((result, _stop_processing)) = rules.apply(tx) {
                return (Layer::Pack, result);
            }
        }
        (Layer::None, CategorizationResult::None)
    }

    /// Categorize a single transaction using the waterfall approach.
    pub fn categorize(&self, tx: &TransactionInput) -> CategorizationResult {
        self.explain(tx).1
    }
```

In `mod.rs` change `pub use engine::CategorizationEngine;` to `pub use engine::{CategorizationEngine, Layer};`.

- [ ] **Step 4: Run the tests**

Run: `cd src-tauri && cargo test categorization`
Expected: PASS (the old `categorize` tests too).

- [ ] **Step 5: Commit**

```bash
cd src-tauri && cargo fmt && cd ..
git add src-tauri/src/services/categorization/engine.rs src-tauri/src/services/categorization/mod.rs
git commit -m "feat(categorization): engine explains which layer decided

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Wire types and the `recategorize` service

**Files:**
- Modify: `src-tauri/src/models/categorization.rs` (four new structs at the end)
- Modify: `src-tauri/src/services/categorization/hits.rs` (`fn learned_matches` and `fn engine_for` become `pub(crate) fn`)
- Create: `src-tauri/src/services/categorization/recategorize.rs`
- Modify: `src-tauri/src/services/categorization/mod.rs` (`pub mod recategorize;`)

**Interfaces:**
- Consumes: `Layer`, `CategorizationEngine::explain` (Task 2); `learn::LearnedPayeeRow` (pub fields: `id`, `normalized_payee`, `original_payee`, `counterparty_iban`, `category_id`, `bank_account_id`, `is_credit`, `created_at`, `updated_at`); `hits::learned_matches(row: &LearnedPayeeRow, tx: &TransactionInput) -> bool`; `hits::engine_for(rule: &CategorizationRule) -> RuleEngine`; `apply::{SOURCE_RULE, SOURCE_EXACT_MATCH}`; `bank_accounts::category_exists(conn, id) -> Result<bool>`; `financial_semantics::{canonical_structured_identifier(id, bank_code, country) -> String, valid_iban(&str) -> bool}`; `tokenizer::normalize_payee_key(&str) -> String`.
- Produces (all `pub` in `recategorize.rs`):
  - `enum Matcher { Learned(LearnedPayeeRow), Custom(CategorizationRule) }`
  - `struct MatchedRow { id: String, category_id: Option<String>, source: Option<String> }`
  - `fn matching_rows(conn: &Connection, matcher: &Matcher, exclude: Option<&str>) -> Result<Vec<MatchedRow>>`
  - `fn breakdown(rows: &[MatchedRow]) -> Vec<CategoryCount>`
  - `fn apply(conn: &Connection, rows: &[MatchedRow], category_id: &str, source: &str) -> Result<Vec<ChangedRow>>`
  - `fn restore(conn: &Connection, rows: &[ChangedRow]) -> Result<usize>`
  - `fn learned_scope_for(tx: &TransactionInput) -> Option<LearnedPayeeRow>`
  - `fn load_transaction_input(conn: &Connection, id: &str) -> Result<Option<TransactionInput>>`
  - `fn load_custom_rule(conn: &Connection, id: &str) -> Result<Option<(CategorizationRule, bool)>>` (bool = is_system)
  - `fn retarget_custom_rule(conn: &Connection, rule_id: &str, category_id: &str, apply_history: bool) -> Result<Vec<ChangedRow>>`
  - `fn preview_for_transaction(conn: &Connection, engine: &CategorizationEngine, transaction_id: &str) -> Result<RecategorizationPreview>`
  - `fn preview_for_learned_rule(conn: &Connection, id: &str) -> Result<RecategorizationPreview>`
  - `fn apply_learned_rule(conn: &Connection, id: &str) -> Result<Vec<ChangedRow>>`
  - models: `CategoryCount { category_id: Option<String>, count: usize }`, `ChangedRow { id, previous_category_id: Option<String>, previous_source: Option<String> }`, `GoverningRule { id, name, category_id }`, `RecategorizationPreview { governing: String, rule: Option<GoverningRule>, learnable: bool, matches: usize, by_category: Vec<CategoryCount> }`.

- [ ] **Step 1: Add the wire types**

Append to `src-tauri/src/models/categorization.rs`:

```rust
/// One current-category bucket of the payments a rewritten rule covers.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct CategoryCount {
    /// `None` = the payments without a category.
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    #[serde(rename = "count")]
    pub count: usize,
}

/// A payment a bulk recategorization changed, with what it held before (for undo).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct ChangedRow {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "previousCategoryId")]
    pub previous_category_id: Option<String>,
    #[serde(rename = "previousSource")]
    pub previous_source: Option<String>,
}

/// The custom rule that decides a transaction's category today.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct GoverningRule {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
}

/// What rewriting the rule behind a transaction would cover (spec 2026-10-10).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct RecategorizationPreview {
    /// `ownAccount` | `custom` | `learned` | `pack` | `none` — the layer that decides today.
    #[serde(rename = "governing")]
    pub governing: String,
    /// Set when `governing` is `custom`.
    #[serde(rename = "rule")]
    pub rule: Option<GoverningRule>,
    /// False when neither a payee key nor an account identity exists (nothing to remember).
    #[serde(rename = "learnable")]
    pub learnable: bool,
    /// Other payments the rewritten rule matches, the edited row excluded.
    #[serde(rename = "matches")]
    pub matches: usize,
    #[serde(rename = "byCategory")]
    pub by_category: Vec<CategoryCount>,
}
```

- [ ] **Step 2: Open the two `hits.rs` helpers**

In `src-tauri/src/services/categorization/hits.rs` change `fn engine_for(rule: &CategorizationRule) -> RuleEngine {` to `pub(crate) fn engine_for(...)` and `fn learned_matches(row: &learn::LearnedPayeeRow, tx: &TransactionInput) -> bool {` to `pub(crate) fn learned_matches(...)`.

- [ ] **Step 3: Write the failing service tests**

Create `src-tauri/src/services/categorization/recategorize.rs` with the module doc, the `use` lines and ONLY the tests module first (the functions come in step 5):

```rust
//! Recategorizing other payments when a rule is rewritten (spec
//! 2026-10-10-rule-override-recategorize-history): which payments a learned
//! rule or a custom rule covers today, how their current categories break
//! down, giving them the rule's category, and taking that back.
//!
//! Coverage is replayed per rule, not through the waterfall: the question is
//! "which payments does THIS rule reach", the same reading `hits.rs` uses.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use super::apply::{SOURCE_EXACT_MATCH, SOURCE_RULE};
use super::engine::Layer;
use super::hits::{engine_for, learned_matches};
use super::learn::{self, LearnedPayeeRow};
use super::tokenizer::normalize_payee_key;
use super::types::{CategorizationResult, CategorizationRule, CategorizationSource, RuleType, TransactionInput};
use super::CategorizationEngine;
use crate::error::{AppError, Result};
use crate::models::categorization::{CategoryCount, ChangedRow, GoverningRule, RecategorizationPreview};
use crate::services::bank_accounts::category_exists;
use crate::services::financial_semantics::{canonical_structured_identifier, valid_iban};

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (id TEXT PRIMARY KEY, name TEXT NOT NULL);
            INSERT INTO transaction_categories VALUES ('cat_dining','Dining'),('cat_groceries','Groceries'),('cat_gifts','Gifts');
            CREATE TABLE bank_accounts (id TEXT PRIMARY KEY);
            INSERT INTO bank_accounts VALUES ('acc-1'),('acc-2');
            CREATE TABLE categorization_rules (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, rule_type TEXT NOT NULL, pattern TEXT NOT NULL,
                category_id TEXT NOT NULL, priority INTEGER NOT NULL DEFAULT 50,
                is_active INTEGER NOT NULL DEFAULT 1, stop_processing INTEGER NOT NULL DEFAULT 0,
                is_system INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL DEFAULT 0, iban_pattern TEXT
            );
            CREATE TABLE learned_payees (
                id TEXT PRIMARY KEY, normalized_payee TEXT, original_payee TEXT, counterparty_iban TEXT,
                category_id TEXT NOT NULL, bank_account_id TEXT, is_credit INTEGER,
                created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL, tx_type TEXT NOT NULL, amount TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK', description TEXT, counterparty_name TEXT, counterparty_iban TEXT,
                counterparty_bank_code TEXT, counterparty_country TEXT, booking_date INTEGER NOT NULL DEFAULT 0,
                category_id TEXT, categorization_source TEXT, suggested_category_id TEXT
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn insert_tx(conn: &Connection, id: &str, account: &str, tx_type: &str, counterparty: &str, category: Option<&str>, source: Option<&str>) {
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, description, counterparty_name, category_id, categorization_source)
             VALUES (?1, ?2, ?3, '100', 'Platba kartou', ?4, ?5, ?6)",
            params![id, account, tx_type, counterparty, category, source],
        )
        .expect("insert");
    }

    fn albert_scope() -> LearnedPayeeRow {
        LearnedPayeeRow {
            id: String::new(),
            normalized_payee: Some("albert".into()),
            original_payee: Some("Albert".into()),
            counterparty_iban: None,
            category_id: "cat_groceries".into(),
            bank_account_id: Some("acc-1".into()),
            is_credit: Some(false),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn category_of(conn: &Connection, id: &str) -> (Option<String>, Option<String>) {
        conn.query_row(
            "SELECT category_id, categorization_source FROM bank_transactions WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row")
    }

    #[test]
    fn learned_scope_matches_only_its_account_direction_and_payee() {
        let conn = setup();
        insert_tx(&conn, "t1", "acc-1", "debit", "Albert", Some("cat_dining"), None);
        insert_tx(&conn, "t2", "acc-1", "debit", "ALBERT 123456", None, None);
        insert_tx(&conn, "t3", "acc-1", "credit", "Albert", Some("cat_dining"), None);
        insert_tx(&conn, "t4", "acc-2", "debit", "Albert", Some("cat_dining"), None);
        insert_tx(&conn, "t5", "acc-1", "debit", "Lidl", Some("cat_dining"), None);
        let rows = matching_rows(&conn, &Matcher::Learned(albert_scope()), Some("t1")).unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["t2"], "same account, outgoing, payee key albert; t1 excluded");
    }

    #[test]
    fn custom_rule_spans_every_account() {
        let conn = setup();
        insert_tx(&conn, "t1", "acc-1", "debit", "Albert", Some("cat_dining"), None);
        insert_tx(&conn, "t2", "acc-2", "credit", "Albert Hypermarket", None, None);
        insert_tx(&conn, "t3", "acc-1", "debit", "Lidl", Some("cat_dining"), None);
        let rule = CategorizationRule::new("r1".into(), "Supermarkety".into(), RuleType::Contains, "albert".into(), "cat_dining".into());
        let rows = matching_rows(&conn, &Matcher::Custom(rule), None).unwrap();
        let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["t1", "t2"]);
    }

    #[test]
    fn breakdown_orders_by_count_and_keeps_the_uncategorized_bucket() {
        let rows = vec![
            MatchedRow { id: "a".into(), category_id: None, source: None },
            MatchedRow { id: "b".into(), category_id: Some("cat_dining".into()), source: None },
            MatchedRow { id: "c".into(), category_id: Some("cat_dining".into()), source: None },
            MatchedRow { id: "d".into(), category_id: Some("cat_gifts".into()), source: None },
        ];
        assert_eq!(
            breakdown(&rows),
            vec![
                CategoryCount { category_id: Some("cat_dining".into()), count: 2 },
                CategoryCount { category_id: Some("cat_gifts".into()), count: 1 },
                CategoryCount { category_id: None, count: 1 },
            ],
            "ties: named categories before the uncategorized bucket"
        );
    }

    #[test]
    fn apply_skips_rows_already_in_the_category_and_restore_round_trips() {
        let conn = setup();
        insert_tx(&conn, "t1", "acc-1", "debit", "Albert", Some("cat_dining"), Some("rule"));
        insert_tx(&conn, "t2", "acc-1", "debit", "Albert", Some("cat_groceries"), None);
        insert_tx(&conn, "t3", "acc-1", "debit", "Albert", None, None);
        conn.execute("UPDATE bank_transactions SET suggested_category_id = 'cat_gifts' WHERE id = 't3'", []).unwrap();
        let rows = matching_rows(&conn, &Matcher::Learned(albert_scope()), None).unwrap();
        let changed = apply(&conn, &rows, "cat_groceries", SOURCE_EXACT_MATCH).unwrap();
        let mut changed_ids: Vec<&str> = changed.iter().map(|c| c.id.as_str()).collect();
        changed_ids.sort();
        assert_eq!(changed_ids, vec!["t1", "t3"], "t2 already had the category");
        assert_eq!(category_of(&conn, "t1"), (Some("cat_groceries".into()), Some("exact_match".into())));
        assert_eq!(
            conn.query_row("SELECT suggested_category_id FROM bank_transactions WHERE id = 't3'", [], |r| r.get::<_, Option<String>>(0)).unwrap(),
            None,
            "a pending suggestion is cleared"
        );
        let t1 = changed.iter().find(|c| c.id == "t1").unwrap();
        assert_eq!(t1.previous_category_id.as_deref(), Some("cat_dining"));
        assert_eq!(t1.previous_source.as_deref(), Some("rule"));

        conn.execute("DELETE FROM bank_transactions WHERE id = 't3'", []).unwrap();
        let restored = restore(&conn, &changed).unwrap();
        assert_eq!(restored, 1, "a row deleted meanwhile is skipped");
        assert_eq!(category_of(&conn, "t1"), (Some("cat_dining".into()), Some("rule".into())));
    }

    #[test]
    fn apply_rejects_an_unknown_category() {
        let conn = setup();
        let err = apply(&conn, &[], "cat_missing", SOURCE_RULE).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn retarget_changes_only_the_rule_category_and_applies_when_asked() {
        let conn = setup();
        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id, priority, stop_processing, iban_pattern)
             VALUES ('r1', 'Supermarkety', 'contains', 'albert', 'cat_dining', 70, 1, NULL)",
            [],
        ).unwrap();
        insert_tx(&conn, "t1", "acc-1", "debit", "Albert", Some("cat_dining"), Some("rule"));
        insert_tx(&conn, "t2", "acc-2", "debit", "Albert", Some("cat_groceries"), None);

        let changed = retarget_custom_rule(&conn, "r1", "cat_groceries", false).unwrap();
        assert!(changed.is_empty());
        let (category, priority, stop): (String, i32, i32) = conn
            .query_row("SELECT category_id, priority, stop_processing FROM categorization_rules WHERE id = 'r1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((category.as_str(), priority, stop), ("cat_groceries", 70, 1));
        assert_eq!(category_of(&conn, "t1").0.as_deref(), Some("cat_dining"), "history untouched without apply");

        let changed = retarget_custom_rule(&conn, "r1", "cat_gifts", true).unwrap();
        let mut ids: Vec<&str> = changed.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["t1", "t2"]);
        assert_eq!(category_of(&conn, "t2"), (Some("cat_gifts".into()), Some("rule".into())));
    }

    #[test]
    fn retarget_refuses_system_rules_and_unknown_rules() {
        let conn = setup();
        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id, is_system) VALUES ('sys', 'System', 'contains', 'x', 'cat_dining', 1)",
            [],
        ).unwrap();
        assert!(matches!(retarget_custom_rule(&conn, "sys", "cat_gifts", false).unwrap_err(), AppError::Validation(_)));
        assert!(matches!(retarget_custom_rule(&conn, "nope", "cat_gifts", false).unwrap_err(), AppError::NotFound(_)));
    }

    #[test]
    fn learned_scope_for_prefers_a_valid_iban_identity_over_the_payee() {
        let mut tx = TransactionInput::new("t".into(), None, Some("Albert".into()), -10.0);
        tx.bank_account_id = Some("acc-1".into());
        let scope = learned_scope_for(&tx).unwrap();
        assert_eq!(scope.normalized_payee.as_deref(), Some("albert"));
        assert_eq!(scope.counterparty_iban, None);
        assert_eq!(scope.bank_account_id.as_deref(), Some("acc-1"));
        assert_eq!(scope.is_credit, Some(false));

        tx.counterparty_iban = Some("CZ65 0800 0000 1920 0014 5399".into());
        let scope = learned_scope_for(&tx).unwrap();
        assert_eq!(scope.counterparty_iban.as_deref(), Some("CZ6508000000192000145399"));
        assert_eq!(scope.normalized_payee, None, "an identity rule is keyed by the account alone");

        let mut nothing = TransactionInput::new("t".into(), None, Some("AB".into()), -10.0);
        nothing.bank_account_id = Some("acc-1".into());
        assert!(learned_scope_for(&nothing).is_none(), "two characters identify nobody");
        let mut no_account = TransactionInput::new("t".into(), None, Some("Albert".into()), -10.0);
        no_account.bank_account_id = None;
        assert!(learned_scope_for(&no_account).is_none());
    }

    #[test]
    fn preview_for_transaction_reports_the_layer_and_the_coverage() {
        let conn = setup();
        insert_tx(&conn, "t1", "acc-1", "debit", "Albert", Some("cat_dining"), None);
        insert_tx(&conn, "t2", "acc-1", "debit", "Albert", Some("cat_dining"), None);
        insert_tx(&conn, "t3", "acc-1", "debit", "Albert", None, None);
        let engine = CategorizationEngine::new_empty();

        let p = preview_for_transaction(&conn, &engine, "t1").unwrap();
        assert_eq!(p.governing, "none");
        assert!(p.learnable);
        assert_eq!(p.matches, 2);
        assert_eq!(
            p.by_category,
            vec![
                CategoryCount { category_id: Some("cat_dining".into()), count: 1 },
                CategoryCount { category_id: None, count: 1 },
            ]
        );

        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id) VALUES ('r1', 'Supermarkety', 'contains', 'albert', 'cat_dining')",
            [],
        ).unwrap();
        engine.update_custom_rules(vec![CategorizationRule::new("r1".into(), "Supermarkety".into(), RuleType::Contains, "albert".into(), "cat_dining".into())]);
        let p = preview_for_transaction(&conn, &engine, "t1").unwrap();
        assert_eq!(p.governing, "custom");
        assert_eq!(p.rule.as_ref().map(|r| r.name.as_str()), Some("Supermarkety"));
        assert_eq!(p.matches, 2, "the custom rule's own coverage, t1 excluded");

        engine.set_own_ibans(vec!["CZ6508000000192000145399".into()]);
        conn.execute("UPDATE bank_transactions SET counterparty_iban = 'CZ6508000000192000145399' WHERE id = 't1'", []).unwrap();
        let p = preview_for_transaction(&conn, &engine, "t1").unwrap();
        assert_eq!(p.governing, "ownAccount");
        assert!(!p.learnable);
        assert_eq!(p.matches, 0);

        assert!(matches!(preview_for_transaction(&conn, &engine, "missing").unwrap_err(), AppError::NotFound(_)));
    }

    #[test]
    fn learned_rule_preview_and_apply_use_the_stored_rule() {
        let conn = setup();
        conn.execute(
            "INSERT INTO learned_payees (id, normalized_payee, original_payee, category_id, bank_account_id, is_credit) VALUES ('l1', 'albert', 'Albert', 'cat_groceries', 'acc-1', 0)",
            [],
        ).unwrap();
        insert_tx(&conn, "t1", "acc-1", "debit", "Albert", Some("cat_dining"), None);
        insert_tx(&conn, "t2", "acc-1", "debit", "Albert", Some("cat_groceries"), None);
        let p = preview_for_learned_rule(&conn, "l1").unwrap();
        assert_eq!(p.governing, "learned");
        assert_eq!(p.matches, 2);
        let changed = apply_learned_rule(&conn, "l1").unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(category_of(&conn, "t1"), (Some("cat_groceries".into()), Some("exact_match".into())));
        assert!(matches!(preview_for_learned_rule(&conn, "missing").unwrap_err(), AppError::NotFound(_)));
    }
}
```

Add `pub mod recategorize;` to `src-tauri/src/services/categorization/mod.rs` (alphabetical, after `pub mod packs;`).

- [ ] **Step 4: Run the tests to see them fail**

Run: `cd src-tauri && cargo test recategorize`
Expected: compile errors (functions missing).

- [ ] **Step 5: Implement the service**

Insert between the `use` lines and `#[cfg(test)]` in `recategorize.rs`:

```rust
/// Which rule's coverage is asked about.
pub enum Matcher {
    /// A learned rule, stored or about to be stored, matched like the learned layer does.
    Learned(LearnedPayeeRow),
    /// A custom rule, matched like the custom layer does (every account, both directions).
    Custom(CategorizationRule),
}

/// One payment a matcher covers, with what the row holds today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedRow {
    pub id: String,
    pub category_id: Option<String>,
    pub source: Option<String>,
}

struct Candidate {
    input: TransactionInput,
    category_id: Option<String>,
    source: Option<String>,
}

const CANDIDATE_COLUMNS: &str = "id, description, counterparty_name, counterparty_iban, counterparty_bank_code, counterparty_country, amount, tx_type, bank_account_id, category_id, categorization_source";

fn candidate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    let amount: String = row.get(6)?;
    let tx_type: String = row.get(7)?;
    let value: f64 = amount.trim().parse().unwrap_or(0.0);
    let is_credit = tx_type.eq_ignore_ascii_case("credit");
    Ok(Candidate {
        input: TransactionInput {
            id: row.get(0)?,
            description: row.get(1)?,
            counterparty: row.get(2)?,
            counterparty_iban: row.get(3)?,
            counterparty_bank_code: row.get(4)?,
            counterparty_country: row.get(5)?,
            amount: if is_credit { value.abs() } else { -value.abs() },
            is_credit,
            bank_account_id: row.get(8)?,
        },
        category_id: row.get(9)?,
        source: row.get(10)?,
    })
}

/// Every transaction, or only one account's (a scoped learned rule never reaches another).
fn load_candidates(conn: &Connection, account: Option<&str>) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    match account {
        Some(account) => {
            let mut stmt = conn.prepare(&format!(
                "SELECT {CANDIDATE_COLUMNS} FROM bank_transactions WHERE bank_account_id = ?1"
            ))?;
            for row in stmt.query_map([account], candidate_from_row)? {
                out.push(row?);
            }
        }
        None => {
            let mut stmt = conn.prepare(&format!("SELECT {CANDIDATE_COLUMNS} FROM bank_transactions"))?;
            for row in stmt.query_map([], candidate_from_row)? {
                out.push(row?);
            }
        }
    }
    Ok(out)
}

/// The transaction as the engine sees it, or `None` when the id is unknown.
pub fn load_transaction_input(conn: &Connection, id: &str) -> Result<Option<TransactionInput>> {
    let candidate = conn
        .query_row(
            &format!("SELECT {CANDIDATE_COLUMNS} FROM bank_transactions WHERE id = ?1"),
            [id],
            candidate_from_row,
        )
        .optional()?;
    Ok(candidate.map(|c| c.input))
}

/// The payments `matcher` covers today, `exclude` (the row being edited) left out.
/// All time, every status.
pub fn matching_rows(conn: &Connection, matcher: &Matcher, exclude: Option<&str>) -> Result<Vec<MatchedRow>> {
    let candidates = match matcher {
        Matcher::Learned(row) => load_candidates(conn, row.bank_account_id.as_deref())?,
        Matcher::Custom(_) => load_candidates(conn, None)?,
    };
    let engine = match matcher {
        Matcher::Custom(rule) => Some(engine_for(rule)),
        Matcher::Learned(_) => None,
    };
    Ok(candidates
        .into_iter()
        .filter(|c| exclude != Some(c.input.id.as_str()))
        .filter(|c| match matcher {
            Matcher::Learned(row) => learned_matches(row, &c.input),
            Matcher::Custom(_) => engine.as_ref().is_some_and(|e| e.apply(&c.input).is_some()),
        })
        .map(|c| MatchedRow {
            id: c.input.id,
            category_id: c.category_id,
            source: c.source,
        })
        .collect())
}

/// Current categories of `rows`, largest bucket first; `None` is the uncategorized bucket.
pub fn breakdown(rows: &[MatchedRow]) -> Vec<CategoryCount> {
    let mut counts: HashMap<Option<String>, usize> = HashMap::new();
    for row in rows {
        *counts.entry(row.category_id.clone()).or_default() += 1;
    }
    let mut out: Vec<CategoryCount> = counts
        .into_iter()
        .map(|(category_id, count)| CategoryCount { category_id, count })
        .collect();
    // Largest first; on a tie named categories come before "no category", then by id.
    out.sort_by(|a, b| {
        b.count.cmp(&a.count).then_with(|| match (&a.category_id, &b.category_id) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        })
    });
    out
}

/// Give `category_id` to every row whose category differs, stamping `source`
/// and clearing a pending suggestion. Returns what each changed row held before.
pub fn apply(conn: &Connection, rows: &[MatchedRow], category_id: &str, source: &str) -> Result<Vec<ChangedRow>> {
    if !category_exists(conn, category_id)? {
        return Err(AppError::NotFound(format!("Category not found: {category_id}")));
    }
    let mut changed = Vec::new();
    for row in rows.iter().filter(|r| r.category_id.as_deref() != Some(category_id)) {
        let updated = conn.execute(
            "UPDATE bank_transactions
             SET category_id = ?1, categorization_source = ?2, suggested_category_id = NULL
             WHERE id = ?3",
            params![category_id, source, row.id],
        )?;
        if updated > 0 {
            changed.push(ChangedRow {
                id: row.id.clone(),
                previous_category_id: row.category_id.clone(),
                previous_source: row.source.clone(),
            });
        }
    }
    Ok(changed)
}

/// Put back what `apply` changed. Rows deleted meanwhile are skipped; a
/// previous category deleted meanwhile becomes "no category".
pub fn restore(conn: &Connection, rows: &[ChangedRow]) -> Result<usize> {
    let mut restored = 0;
    for row in rows {
        let category = match row.previous_category_id.as_deref() {
            Some(c) if category_exists(conn, c)? => Some(c),
            _ => None,
        };
        restored += conn.execute(
            "UPDATE bank_transactions SET category_id = ?1, categorization_source = ?2 WHERE id = ?3",
            params![category, row.previous_source, row.id],
        )?;
    }
    Ok(restored)
}

/// The learned rule a remembered correction on `tx` would write, keyed the way
/// `learn::learn_scoped` keys it: this account and direction, the canonical
/// account identity when it is a valid IBAN, else the payee key. `None` when
/// nothing identifies the payee (or the row has no account).
pub fn learned_scope_for(tx: &TransactionInput) -> Option<LearnedPayeeRow> {
    let account = tx.bank_account_id.clone()?;
    let identity = tx
        .counterparty_iban
        .as_deref()
        .map(|id| {
            canonical_structured_identifier(
                id,
                tx.counterparty_bank_code.as_deref(),
                tx.counterparty_country.as_deref(),
            )
        })
        .filter(|id| valid_iban(id));
    let payee = if identity.is_some() {
        None
    } else {
        tx.counterparty
            .as_deref()
            .map(normalize_payee_key)
            .filter(|key| key.len() > 2)
    };
    if identity.is_none() && payee.is_none() {
        return None;
    }
    Some(LearnedPayeeRow {
        id: String::new(),
        normalized_payee: payee,
        original_payee: tx.counterparty.clone(),
        counterparty_iban: identity,
        category_id: String::new(),
        bank_account_id: Some(account),
        is_credit: Some(tx.is_credit),
        created_at: 0,
        updated_at: 0,
    })
}

/// A stored custom rule and whether it is a system rule.
pub fn load_custom_rule(conn: &Connection, id: &str) -> Result<Option<(CategorizationRule, bool)>> {
    let row = conn
        .query_row(
            "SELECT id, name, rule_type, pattern, category_id, priority, is_active, stop_processing, iban_pattern, is_system
             FROM categorization_rules WHERE id = ?1",
            [id],
            |row| {
                let rule_type: String = row.get(2)?;
                Ok((
                    CategorizationRule {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        rule_type: rule_type.parse().unwrap_or(RuleType::Contains),
                        pattern: row.get(3)?,
                        category_id: row.get(4)?,
                        priority: row.get(5)?,
                        is_active: row.get::<_, i32>(6)? != 0,
                        stop_processing: row.get::<_, i32>(7)? != 0,
                        iban_pattern: row.get(8)?,
                    },
                    row.get::<_, i32>(9)? != 0,
                ))
            },
        )
        .optional()?;
    Ok(row)
}

/// Point a custom rule at another category and, when asked, give that category
/// to every payment the rule covers. One transaction; the caller reloads the engine.
pub fn retarget_custom_rule(conn: &Connection, rule_id: &str, category_id: &str, apply_history: bool) -> Result<Vec<ChangedRow>> {
    if !category_exists(conn, category_id)? {
        return Err(AppError::NotFound(format!("Category not found: {category_id}")));
    }
    let (mut rule, is_system) = load_custom_rule(conn, rule_id)?
        .ok_or_else(|| AppError::NotFound(format!("Rule not found: {rule_id}")))?;
    if is_system {
        return Err(AppError::Validation("validation.systemRule".into()));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE categorization_rules SET category_id = ?1 WHERE id = ?2 AND is_system = 0",
        params![category_id, rule_id],
    )?;
    rule.category_id = category_id.to_string();
    let changed = if apply_history {
        let rows = matching_rows(&tx, &Matcher::Custom(rule), None)?;
        apply(&tx, &rows, category_id, SOURCE_RULE)?
    } else {
        Vec::new()
    };
    tx.commit()?;
    Ok(changed)
}

fn empty_preview(governing: &str, learnable: bool) -> RecategorizationPreview {
    RecategorizationPreview {
        governing: governing.to_string(),
        rule: None,
        learnable,
        matches: 0,
        by_category: Vec::new(),
    }
}

fn preview_of(governing: &str, rule: Option<GoverningRule>, rows: &[MatchedRow]) -> RecategorizationPreview {
    RecategorizationPreview {
        governing: governing.to_string(),
        rule,
        learnable: true,
        matches: rows.len(),
        by_category: breakdown(rows),
    }
}

/// What remembering a correction on this transaction would rewrite and cover.
pub fn preview_for_transaction(conn: &Connection, engine: &CategorizationEngine, transaction_id: &str) -> Result<RecategorizationPreview> {
    let tx = load_transaction_input(conn, transaction_id)?
        .ok_or_else(|| AppError::NotFound(format!("Transaction not found: {transaction_id}")))?;
    let (layer, result) = engine.explain(&tx);
    if layer == Layer::OwnAccount {
        return Ok(empty_preview("ownAccount", false));
    }
    if layer == Layer::Custom {
        let rule_id = match &result {
            CategorizationResult::Match {
                source: CategorizationSource::Rule { rule_id, .. },
                ..
            } => Some(rule_id.clone()),
            _ => None,
        };
        let loaded = match rule_id {
            Some(id) => load_custom_rule(conn, &id)?,
            None => None,
        };
        if let Some((rule, _)) = loaded {
            let rows = matching_rows(conn, &Matcher::Custom(rule.clone()), Some(transaction_id))?;
            let governing = GoverningRule {
                id: rule.id,
                name: rule.name,
                category_id: rule.category_id,
            };
            return Ok(preview_of("custom", Some(governing), &rows));
        }
        // The engine knows a rule the table no longer has: fall through to the learned reading.
    }
    let governing = match layer {
        Layer::Learned => "learned",
        Layer::Pack => "pack",
        _ => "none",
    };
    match learned_scope_for(&tx) {
        None => Ok(empty_preview(governing, false)),
        Some(scope) => {
            let rows = matching_rows(conn, &Matcher::Learned(scope), Some(transaction_id))?;
            Ok(preview_of(governing, None, &rows))
        }
    }
}

fn stored_learned_rule(conn: &Connection, id: &str) -> Result<LearnedPayeeRow> {
    learn::list_learned_payees(conn)?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| AppError::NotFound(format!("Learned rule not found: {id}")))
}

/// What a stored learned rule covers today (the rules page).
pub fn preview_for_learned_rule(conn: &Connection, id: &str) -> Result<RecategorizationPreview> {
    let row = stored_learned_rule(conn, id)?;
    let rows = matching_rows(conn, &Matcher::Learned(row), None)?;
    Ok(preview_of("learned", None, &rows))
}

/// Give a stored learned rule's category to every payment it covers.
pub fn apply_learned_rule(conn: &Connection, id: &str) -> Result<Vec<ChangedRow>> {
    let row = stored_learned_rule(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    let rows = matching_rows(&tx, &Matcher::Learned(row.clone()), None)?;
    let changed = apply(&tx, &rows, &row.category_id, SOURCE_EXACT_MATCH)?;
    tx.commit()?;
    Ok(changed)
}
```

`LearnedPayeeRow` must derive `Clone` (it does: `#[derive(Debug, Clone)]`). If `list_learned_payees` is private, it is `pub fn` already (used by `hits.rs`).

- [ ] **Step 6: Run the tests**

Run: `cd src-tauri && cargo test recategorize`
Expected: PASS, all nine tests.

- [ ] **Step 7: Add the validation string**

`src/i18n/locales/cs/common.json`, inside `"validation"`: `"systemRule": "Systémová pravidla nelze upravit."`
`src/i18n/locales/en/common.json`, inside `"validation"`: `"systemRule": "System rules cannot be changed."`

- [ ] **Step 8: Gates and commit**

Run: `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test && cd ..`
Expected: green.

```bash
git add src-tauri/src/models/categorization.rs src-tauri/src/services/categorization/recategorize.rs src-tauri/src/services/categorization/mod.rs src-tauri/src/services/categorization/hits.rs src/i18n/locales/cs/common.json src/i18n/locales/en/common.json
git commit -m "feat(categorization): recategorize service — coverage, breakdown, apply, restore

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `learn_scoped_and_apply` — learned row and history in one transaction

**Files:**
- Modify: `src-tauri/src/services/categorization/learn.rs` (`learn_scoped` → wrapper; new `learn_scoped_and_apply`; tests)

**Interfaces:**
- Consumes: `recategorize::{Matcher, matching_rows, apply}`, `apply::SOURCE_EXACT_MATCH`.
- Produces: `pub fn learn_scoped_and_apply(conn: &Connection, engine: &CategorizationEngine, data: &InsertScopedLearning, apply_history: bool) -> Result<Vec<ChangedRow>>`; `learn_scoped` keeps its signature.

- [ ] **Step 1: Write the failing test**

In the `tests` module of `learn.rs`, next to `scoped_learning_survives_reload_without_crossing_account_or_direction`, add:

```rust
    #[test]
    fn scoped_learning_can_give_the_category_to_earlier_payments() {
        let conn = setup_test_db();
        conn.execute_batch(
            "DROP INDEX idx_learned_payees_composite;
             CREATE TABLE bank_accounts(id TEXT PRIMARY KEY); INSERT INTO bank_accounts VALUES ('a'),('b');
             INSERT INTO transaction_categories VALUES ('cat_dining'),('cat_groceries');
             CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL, tx_type TEXT NOT NULL, amount TEXT NOT NULL,
                description TEXT, counterparty_name TEXT, counterparty_iban TEXT, counterparty_bank_code TEXT,
                counterparty_country TEXT, category_id TEXT, categorization_source TEXT, suggested_category_id TEXT);
             INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, counterparty_name, category_id) VALUES
               ('t1','a','debit','100','Albert','cat_dining'),
               ('t2','a','debit','100','Albert',NULL),
               ('t3','a','debit','100','Albert','cat_groceries'),
               ('t4','b','debit','100','Albert','cat_dining'),
               ('t5','a','credit','100','Albert','cat_dining');",
        )
        .unwrap();
        let engine = CategorizationEngine::new_empty();
        let data = crate::models::categorization::InsertScopedLearning {
            bank_account_id: "a".into(),
            is_credit: false,
            payee: Some("Albert".into()),
            counterparty_iban: None,
            counterparty_bank_code: None,
            counterparty_country: None,
            category_id: "cat_groceries".into(),
        };
        let changed = learn_scoped_and_apply(&conn, &engine, &data, true).unwrap();
        let mut ids: Vec<&str> = changed.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["t1", "t2"], "same account and direction, not already groceries");
        let source: Option<String> = conn
            .query_row("SELECT categorization_source FROM bank_transactions WHERE id = 't1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(source.as_deref(), Some("exact_match"));
        let untouched: String = conn
            .query_row("SELECT category_id FROM bank_transactions WHERE id = 't4'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(untouched, "cat_dining");
        let mut tx = TransactionInput::new("new".into(), None, Some("Albert".into()), -50.0);
        tx.bank_account_id = Some("a".into());
        assert_eq!(engine.categorize(&tx).category_id(), Some("cat_groceries"), "the rule is learned too");

        let none = learn_scoped_and_apply(&conn, &engine, &data, false).unwrap();
        assert!(none.is_empty(), "without apply_history nothing but the rule changes");
    }
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cd src-tauri && cargo test scoped_learning_can_give_the_category_to_earlier_payments`
Expected: compile error, `learn_scoped_and_apply` not found.

- [ ] **Step 3: Implement**

In `learn.rs` add `use super::recategorize;` and `use super::apply::SOURCE_EXACT_MATCH;` and `use crate::models::categorization::ChangedRow;` to the imports. Rename the existing `pub fn learn_scoped(...) -> Result<()>` body into the new function and keep the old name as a wrapper:

```rust
/// Persist before touching memory; scope cannot replace a legacy broad rule.
pub fn learn_scoped(
    conn: &Connection,
    engine: &CategorizationEngine,
    data: &crate::models::categorization::InsertScopedLearning,
) -> Result<()> {
    learn_scoped_and_apply(conn, engine, data, false).map(|_| ())
}

/// `learn_scoped`, and when `apply_history` is set, in the same transaction,
/// every other payment the new rule covers gets its category (spec
/// 2026-10-10). Returns what those rows held before, for undo.
pub fn learn_scoped_and_apply(
    conn: &Connection,
    engine: &CategorizationEngine,
    data: &crate::models::categorization::InsertScopedLearning,
    apply_history: bool,
) -> Result<Vec<ChangedRow>> {
    data.validate()?;
    // … the existing validation, `canonical`, `identity`, `payee` derivation, unchanged …
    let transaction = conn.unchecked_transaction()?;
    // … the existing DELETE and INSERT, unchanged …
    let changed = if apply_history {
        let scope = LearnedPayeeRow {
            id: String::new(),
            normalized_payee: payee.clone(),
            original_payee: None,
            counterparty_iban: identity.clone(),
            category_id: data.category_id.clone(),
            bank_account_id: Some(data.bank_account_id.clone()),
            is_credit: Some(data.is_credit),
            created_at: 0,
            updated_at: 0,
        };
        let rows = recategorize::matching_rows(&transaction, &recategorize::Matcher::Learned(scope), None)?;
        recategorize::apply(&transaction, &rows, &data.category_id, SOURCE_EXACT_MATCH)?
    } else {
        Vec::new()
    };
    transaction.commit()?;
    engine.learn_scoped(
        &data.bank_account_id,
        data.is_credit,
        payee.as_deref(),
        identity.as_deref(),
        &data.category_id,
    );
    Ok(changed)
}
```

`LearnedPayeeRow` is defined further down in the same file; Rust does not care about order.

- [ ] **Step 4: Run the tests**

Run: `cd src-tauri && cargo test learn::`
Expected: PASS, including the existing `scoped_learning_failed_replacement_preserves_database_and_engine`.

- [ ] **Step 5: Gates and commit**

Run: `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test && cd ..`

```bash
git add src-tauri/src/services/categorization/learn.rs
git commit -m "feat(categorization): learning can recategorize other payments in one transaction

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Commands, registration and the TypeScript contract

**Files:**
- Modify: `src-tauri/src/commands/categorization.rs` (`learn_categorization` extended; five new commands)
- Modify: `src-tauri/src/lib.rs` (`generate_handler!` list, after `commands::categorization::update_learned_payee_category,`)
- Modify: `src-tauri/src/bindings.rs` (`collect_types()`, after the `LearnedPayeeEntry` line)
- Regenerate: `shared/generated-types.ts`
- Modify: `shared/schema.ts` (after `LearnedPayeeEntry`)
- Modify: `src/lib/tauri-api.ts` (`categorizationApi`; the type import list above it)

**Interfaces:**
- Produces (Tauri commands and wrappers):
  - `preview_recategorization { transactionId }` → `RecategorizationPreview`; wrapper `categorizationApi.previewRecategorization(transactionId)`.
  - `learn_categorization { data, applyHistory? }` → `ChangedRow[]`; wrapper `categorizationApi.learn(data, applyHistory = false)`.
  - `retarget_custom_rule { ruleId, categoryId, applyHistory }` → `ChangedRow[]`; wrapper `categorizationApi.retargetCustomRule(ruleId, categoryId, applyHistory)`.
  - `preview_learned_rule_history { id }` → `RecategorizationPreview`; wrapper `categorizationApi.previewLearnedRuleHistory(id)`.
  - `apply_learned_rule_to_history { id }` → `ChangedRow[]`; wrapper `categorizationApi.applyLearnedRuleToHistory(id)`.
  - `restore_transaction_categories { rows }` → `number`; wrapper `categorizationApi.restoreTransactionCategories(rows)`.

- [ ] **Step 1: Commands**

In `src-tauri/src/commands/categorization.rs` add to the imports: `use crate::models::categorization::{ChangedRow, RecategorizationPreview};` and `use crate::services::categorization::recategorize;`.

Replace `learn_categorization`:

```rust
/// Learn from the user's manual categorization (scoped to one own account,
/// direction and identity) and, with `apply_history`, give the category to
/// every other payment the new rule covers. Returns the changed rows for undo.
#[tauri::command]
pub async fn learn_categorization(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    data: crate::models::categorization::InsertScopedLearning,
    apply_history: Option<bool>,
) -> crate::error::Result<Vec<ChangedRow>> {
    data.validate()?;
    db.with_conn(|conn| {
        learn::learn_scoped_and_apply(conn, &state.0, &data, apply_history.unwrap_or(false))
    })
}
```

Append after `update_learned_payee_category`:

```rust
// ==================== Recategorizing other payments (spec 2026-10-10) ====================

/// What remembering a correction on this transaction would rewrite and cover.
#[tauri::command]
pub async fn preview_recategorization(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    transaction_id: String,
) -> crate::error::Result<RecategorizationPreview> {
    db.with_conn(|conn| recategorize::preview_for_transaction(conn, &state.0, &transaction_id))
}

/// Point a custom rule at another category (it has precedence over learned
/// choices) and, when asked, recategorize every payment it covers.
#[tauri::command]
pub async fn retarget_custom_rule(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    rule_id: String,
    category_id: String,
    apply_history: bool,
) -> crate::error::Result<Vec<ChangedRow>> {
    let changed = db.with_conn(|conn| {
        recategorize::retarget_custom_rule(conn, &rule_id, &category_id, apply_history)
    })?;
    reload_custom_rules(&state, &db).map_err(crate::error::AppError::Internal)?;
    log::info!("Retargeted custom rule {} to {}", rule_id, category_id);
    Ok(changed)
}

/// What a stored learned rule covers today (the rules page).
#[tauri::command]
pub async fn preview_learned_rule_history(
    db: State<'_, Database>,
    id: String,
) -> crate::error::Result<RecategorizationPreview> {
    db.with_conn(|conn| recategorize::preview_for_learned_rule(conn, &id))
}

/// Give a stored learned rule's category to every payment it covers.
#[tauri::command]
pub async fn apply_learned_rule_to_history(
    db: State<'_, Database>,
    id: String,
) -> crate::error::Result<Vec<ChangedRow>> {
    db.with_conn(|conn| recategorize::apply_learned_rule(conn, &id))
}

/// Undo a bulk recategorization: put back the previous category and provenance.
#[tauri::command]
pub async fn restore_transaction_categories(
    db: State<'_, Database>,
    rows: Vec<ChangedRow>,
) -> crate::error::Result<usize> {
    db.with_conn(|conn| recategorize::restore(conn, &rows))
}
```

- [ ] **Step 2: Register**

`src-tauri/src/lib.rs`, in `generate_handler![...]` after `commands::categorization::update_learned_payee_category,`:

```rust
            commands::categorization::preview_recategorization,
            commands::categorization::retarget_custom_rule,
            commands::categorization::preview_learned_rule_history,
            commands::categorization::apply_learned_rule_to_history,
            commands::categorization::restore_transaction_categories,
```

`src-tauri/src/bindings.rs`, after `types.register::<crate::models::categorization::LearnedPayeeEntry>();`:

```rust
    types.register::<crate::models::categorization::CategoryCount>();
    types.register::<crate::models::categorization::ChangedRow>();
    types.register::<crate::models::categorization::GoverningRule>();
    types.register::<crate::models::categorization::RecategorizationPreview>();
```

- [ ] **Step 3: Regenerate the drift detector**

Run: `cd src-tauri && cargo test generate_bindings -- --ignored && cd ..`
Expected: `shared/generated-types.ts` gains the four interfaces (`git diff --stat shared/generated-types.ts` shows a change).

- [ ] **Step 4: Mirror into `shared/schema.ts`**

After the `LearnedPayeeEntry` interface:

```ts
/** One current-category bucket of the payments a rewritten rule covers (`categoryId` null = no category). */
export interface CategoryCount {
  categoryId: string | null;
  count: number;
}

/** A payment a bulk recategorization changed, with what it held before (for undo). */
export interface ChangedRow {
  id: string;
  previousCategoryId: string | null;
  previousSource: string | null;
}

/** The custom rule that decides a transaction's category today. */
export interface GoverningRule {
  id: string;
  name: string;
  categoryId: string;
}

/** What rewriting the rule behind a transaction would cover. */
export interface RecategorizationPreview {
  /** The layer that decides today. */
  governing: 'ownAccount' | 'custom' | 'learned' | 'pack' | 'none';
  /** Set when `governing` is 'custom'. */
  rule: GoverningRule | null;
  /** False when neither a payee key nor an account identity exists. */
  learnable: boolean;
  /** Other payments the rewritten rule matches, the edited row excluded. */
  matches: number;
  byCategory: CategoryCount[];
}
```

- [ ] **Step 5: Wrappers**

In `src/lib/tauri-api.ts` add `ChangedRow, RecategorizationPreview, InsertScopedLearning` to the type import list right above `export const categorizationApi` (the one importing from `'../../shared/schema'`). Replace the `learn` wrapper and append the five new ones inside `categorizationApi`:

```ts
  /** Learn a scoped rule; with `applyHistory` the other payments it covers get the category too. */
  learn: (data: InsertScopedLearning, applyHistory = false) =>
    tauriInvoke<ChangedRow[]>('learn_categorization', { data, applyHistory }),

  // ==================== Recategorizing other payments (spec 2026-10-10) ====================

  /** What remembering a correction on this transaction would rewrite and cover. */
  previewRecategorization: (transactionId: string) =>
    tauriInvoke<RecategorizationPreview>('preview_recategorization', { transactionId }),

  /** Point the custom rule that decides today at another category; optionally recategorize its matches. */
  retargetCustomRule: (ruleId: string, categoryId: string, applyHistory: boolean) =>
    tauriInvoke<ChangedRow[]>('retarget_custom_rule', { ruleId, categoryId, applyHistory }),

  /** What a stored learned rule covers today (the rules page). */
  previewLearnedRuleHistory: (id: string) =>
    tauriInvoke<RecategorizationPreview>('preview_learned_rule_history', { id }),

  /** Give a stored learned rule's category to every payment it covers. */
  applyLearnedRuleToHistory: (id: string) =>
    tauriInvoke<ChangedRow[]>('apply_learned_rule_to_history', { id }),

  /** Undo a bulk recategorization. Returns how many rows were restored. */
  restoreTransactionCategories: (rows: ChangedRow[]) =>
    tauriInvoke<number>('restore_transaction_categories', { rows }),
```

The existing `learn: (data: import('@shared/schema').InsertScopedLearning) => tauriInvoke<void>(...)` goes away. `src/hooks/useCategorization.ts` still compiles (it ignores the result).

- [ ] **Step 6: Gates**

Run: `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test && cd .. && npm run typecheck && npm run lint`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/categorization.rs src-tauri/src/lib.rs src-tauri/src/bindings.rs shared/generated-types.ts shared/schema.ts src/lib/tauri-api.ts
git commit -m "feat(categorization): commands for previewing and recategorizing other payments

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Pure frontend helpers

**Files:**
- Create: `src/utils/recategorization.ts`, `src/utils/recategorization.test.ts`
- Modify: `src/utils/rules-link.ts`, `src/utils/rules-link.test.ts`

**Interfaces:**
- Produces: `formatBreakdown(byCategory, nameOf, labels, max = 3): string`, `changedByPick(preview, categoryId): number`, `editRuleHref(id): string`, `parseEditRuleSearch(search): string | null`.

- [ ] **Step 1: Write the failing tests**

`src/utils/recategorization.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { changedByPick, formatBreakdown } from './recategorization';

const names: Record<string, string> = { dining: 'Stravování', groceries: 'Potraviny', gifts: 'Dárky', fun: 'Zábava' };
const labels = { uncategorized: 'bez kategorie', more: (n: number) => `+${n} dalších` };
const nameOf = (id: string) => names[id] ?? id;

describe('formatBreakdown', () => {
  it('lists the largest buckets first with the uncategorized label', () => {
    expect(
      formatBreakdown(
        [
          { categoryId: null, count: 2 },
          { categoryId: 'dining', count: 12 },
        ],
        nameOf,
        labels
      )
    ).toBe('12× Stravování · 2× bez kategorie');
  });

  it('folds buckets beyond the limit into "+N dalších"', () => {
    expect(
      formatBreakdown(
        [
          { categoryId: 'dining', count: 20 },
          { categoryId: 'groceries', count: 5 },
          { categoryId: null, count: 2 },
          { categoryId: 'gifts', count: 1 },
          { categoryId: 'fun', count: 1 },
        ],
        nameOf,
        labels
      )
    ).toBe('20× Stravování · 5× Potraviny · 2× bez kategorie · +2 dalších');
  });

  it('is empty for no buckets', () => {
    expect(formatBreakdown([], nameOf, labels)).toBe('');
  });
});

describe('changedByPick', () => {
  const preview = {
    matches: 27,
    byCategory: [
      { categoryId: 'dining', count: 20 },
      { categoryId: 'groceries', count: 5 },
      { categoryId: null, count: 2 },
    ],
  };
  it('subtracts the rows already in the picked category', () => {
    expect(changedByPick(preview, 'groceries')).toBe(22);
    expect(changedByPick(preview, 'dining')).toBe(7);
    expect(changedByPick(preview, 'gifts')).toBe(27);
  });
});
```

Append to `src/utils/rules-link.test.ts` (add `editRuleHref, parseEditRuleSearch` to its import):

```ts
describe('edit rule link', () => {
  it('builds and reads the edit link', () => {
    expect(editRuleHref('abc-1')).toBe('/settings/categorization-rules?edit=abc-1');
    expect(parseEditRuleSearch('?edit=abc-1')).toBe('abc-1');
    expect(parseEditRuleSearch('edit=abc-1')).toBe('abc-1');
  });

  it('gives null without an edit parameter', () => {
    expect(parseEditRuleSearch('')).toBeNull();
    expect(parseEditRuleSearch('?new=1')).toBeNull();
    expect(parseEditRuleSearch('?edit=')).toBeNull();
  });
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npx vitest run src/utils/recategorization.test.ts src/utils/rules-link.test.ts`
Expected: FAIL (module / export missing).

- [ ] **Step 3: Implement**

`src/utils/recategorization.ts`:

```ts
import type { CategoryCount } from '@shared/schema';

interface BreakdownLabels {
  /** Label of the bucket without a category ("bez kategorie"). */
  uncategorized: string;
  /** Label for the folded tail ("+2 dalších"). */
  more: (count: number) => string;
}

/**
 * "12× Stravování · 2× bez kategorie · +1 dalších": the `max` largest buckets of a
 * recategorization preview, the rest folded into one count.
 */
export function formatBreakdown(
  byCategory: readonly CategoryCount[],
  nameOf: (categoryId: string) => string,
  labels: BreakdownLabels,
  max = 3
): string {
  const sorted = [...byCategory].sort((a, b) => b.count - a.count);
  const parts = sorted
    .slice(0, max)
    .map((b) => `${b.count}× ${b.categoryId ? nameOf(b.categoryId) : labels.uncategorized}`);
  const rest = sorted.length - max;
  if (rest > 0) parts.push(labels.more(rest));
  return parts.join(' · ');
}

/** Payments a pick would actually change: every match minus those already in `categoryId`. */
export function changedByPick(
  preview: { matches: number; byCategory: readonly CategoryCount[] },
  categoryId: string
): number {
  const same = preview.byCategory.find((b) => b.categoryId === categoryId)?.count ?? 0;
  return Math.max(0, preview.matches - same);
}
```

Append to `src/utils/rules-link.ts`:

```ts
/** Link to the rules page that opens one custom rule's editor (`?edit=<id>`). */
export function editRuleHref(ruleId: string): string {
  const query = new URLSearchParams({ edit: ruleId });
  return `${RULES_PAGE_PATH}?${query.toString()}`;
}

/** The rule id an `?edit=` link asks to open, or `null`. */
export function parseEditRuleSearch(search: string): string | null {
  const id = new URLSearchParams(search).get('edit');
  return id ? id : null;
}
```

- [ ] **Step 4: Run the tests**

Run: `npx vitest run src/utils/recategorization.test.ts src/utils/rules-link.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/utils/recategorization.ts src/utils/recategorization.test.ts src/utils/rules-link.ts src/utils/rules-link.test.ts
git commit -m "feat(categorization): breakdown formatting and edit-rule link helpers

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: The category picker footer (ledger)

**Files:**
- Modify: `src/components/bank-accounts/CategorySelector.tsx`
- Modify: `src/pages/BankAccountDetail.tsx` (the `<CategorySelector … compact />` inside the ledger table: add `transactionId={tx.id}`)
- Modify: `src/lib/money-flow-refresh.ts` (add `'recategorization-preview'` to `keys`)
- Modify: `src/i18n/locales/cs/bank_accounts.json`, `src/i18n/locales/en/bank_accounts.json` (`categorization.*`)

**Interfaces:**
- Consumes: `categorizationApi.previewRecategorization / learn / retargetCustomRule / restoreTransactionCategories` (Task 5), `formatBreakdown`, `editRuleHref` (Task 6), `RecategorizationPreview`, `ChangedRow` (schema).
- Produces: `CategorySelector` prop `transactionId?: string`.

- [ ] **Step 1: Locale strings**

Add inside `"categorization"` of `src/i18n/locales/cs/bank_accounts.json`:

```json
"historyChecking": "Zjišťuji další platby…",
"applyHistory_one": "Zařadit i {{count}} další platbu",
"applyHistory_few": "Zařadit i {{count}} další platby",
"applyHistory_other": "Zařadit i {{count}} dalších plateb",
"uncategorizedCount": "bez kategorie",
"moreCategories": "+{{count}} dalších",
"governedByRule": "Zařazuje vlastní pravidlo „{{rule}}“ → {{category}}",
"retargetRule": "Změnit kategorii tohoto pravidla",
"retargetRuleScope": "Má přednost před naučenými volbami; platí všude, kde zasáhne.",
"editRule": "Upravit pravidlo →",
"ownAccountNote": "Převod mezi vlastními účty – zařazuje se automaticky.",
"notLearnable": "Nelze zapamatovat – platba nemá protistranu ani číslo účtu.",
"learnedApplied_one": "Naučeno · {{count}} další platba zařazena",
"learnedApplied_few": "Naučeno · {{count}} další platby zařazeny",
"learnedApplied_other": "Naučeno · {{count}} dalších plateb zařazeno",
"ruleRetargeted": "Pravidlo „{{rule}}“ zařazuje jako {{category}}",
"ruleRetargetedApplied_one": "Pravidlo „{{rule}}“ zařazuje jako {{category}} · {{count}} další platba zařazena",
"ruleRetargetedApplied_few": "Pravidlo „{{rule}}“ zařazuje jako {{category}} · {{count}} další platby zařazeny",
"ruleRetargetedApplied_other": "Pravidlo „{{rule}}“ zařazuje jako {{category}} · {{count}} dalších plateb zařazeno",
"undoHistory": "Vrátit zpět",
"historyRestored": "Změněné platby vráceny ({{count}})"
```

And in `src/i18n/locales/en/bank_accounts.json`:

```json
"historyChecking": "Checking other payments…",
"applyHistory_one": "Also categorize {{count}} other payment",
"applyHistory_other": "Also categorize {{count}} other payments",
"uncategorizedCount": "no category",
"moreCategories": "+{{count}} more",
"governedByRule": "Categorized by your rule \"{{rule}}\" → {{category}}",
"retargetRule": "Change this rule's category",
"retargetRuleScope": "Takes precedence over learned choices; applies wherever it matches.",
"editRule": "Edit rule →",
"ownAccountNote": "Transfer between your own accounts – categorized automatically.",
"notLearnable": "Cannot be remembered – the payment has no counterparty or account number.",
"learnedApplied_one": "Learned · {{count}} other payment categorized",
"learnedApplied_other": "Learned · {{count}} other payments categorized",
"ruleRetargeted": "Rule \"{{rule}}\" now assigns {{category}}",
"ruleRetargetedApplied_one": "Rule \"{{rule}}\" now assigns {{category}} · {{count}} other payment categorized",
"ruleRetargetedApplied_other": "Rule \"{{rule}}\" now assigns {{category}} · {{count}} other payments categorized",
"undoHistory": "Undo",
"historyRestored": "Changed payments restored ({{count}})"
```

- [ ] **Step 2: Refresh key**

In `src/lib/money-flow-refresh.ts` add `'recategorization-preview',` to the `keys` array (after `'learnedPayees',`).

- [ ] **Step 3: Rewrite the selection flow in `CategorySelector.tsx`**

Imports to add: `useQuery` from `@tanstack/react-query` (next to `useQueryClient`), `useLocation` from `wouter`, `Checkbox` from `@/components/ui/checkbox`, `ArrowRightLeft, CircleAlert, Flag, Undo2` from `lucide-react` (keep the existing ones), `formatBreakdown` from `@/utils/recategorization`, `editRuleHref` from `@/utils/rules-link`, and types `ChangedRow, RecategorizationPreview` from `@shared/schema`.

Add the prop:

```ts
  /** The ledger row's id; enables the "other payments" preview in the picker footer. */
  transactionId?: string;
```

Inside the component, after `const [createSkipLearning, setCreateSkipLearning] = useState(false);`:

```ts
  const [, setLocation] = useLocation();
  const canLearn = enableLearning && !!bankAccountId && isCredit !== undefined;
  const preview = useQuery({
    queryKey: ['recategorization-preview', transactionId],
    queryFn: () => categorizationApi.previewRecategorization(transactionId as string),
    enabled: open && canLearn && !!transactionId,
    staleTime: 0,
  });
```

Add, after `handleDeclineSuggestion`:

```ts
  const historyToast = (title: string, description: string | undefined, changed: ChangedRow[]) => {
    if (changed.length === 0) {
      toast(title, { description });
      return;
    }
    toast(title, {
      description,
      duration: 8000,
      action: {
        label: t('categorization.undoHistory'),
        onClick: () => {
          void (async () => {
            try {
              const restored = await categorizationApi.restoreTransactionCategories(changed);
              await refreshMoneyFlows(queryClient);
              toast(t('categorization.historyRestored', { count: restored }));
            } catch (e) {
              toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
            }
          })();
        },
      },
    });
  };
```

Replace `handleSelect` with:

```ts
  const handleSelect = async (
    categoryId: string,
    options: { remember: boolean; applyHistory: boolean } = { remember: true, applyHistory: false }
  ) => {
    if (categoryId === currentCategoryId) return;

    setIsSaving(true);
    try {
      await onCategoryChange(categoryId);
    } catch {
      setIsSaving(false);
      return;
    }
    setIsSaving(false);
    setOpen(false);

    if (!options.remember || !canLearn) return;

    const info = preview.data;
    if (info && (info.governing === 'ownAccount' || (!info.learnable && info.governing !== 'custom'))) {
      return;
    }
    const pickedName = categoryName(getCategoryDisplay(categoryId, categories) ?? { id: categoryId, name: categoryId, isSystem: false, iconName: 'tag', color: '' });

    try {
      if (info?.governing === 'custom' && info.rule) {
        const changed = await categorizationApi.retargetCustomRule(info.rule.id, categoryId, options.applyHistory);
        await refreshMoneyFlows(queryClient);
        historyToast(
          changed.length > 0
            ? t('categorization.ruleRetargetedApplied', { rule: info.rule.name, category: pickedName, count: changed.length })
            : t('categorization.ruleRetargeted', { rule: info.rule.name, category: pickedName }),
          undefined,
          changed
        );
        return;
      }

      // The new rule is limited to this own account, direction and reliable identity.
      const payee =
        counterpartyName && counterpartyName.trim().length > 2 ? counterpartyName.trim() : null;
      const iban =
        counterpartyIban && counterpartyIban.trim().length > 5 ? counterpartyIban.trim() : null;
      if (!payee && !iban) return;
      const changed = await categorizationApi.learn(
        {
          payee,
          counterpartyIban: iban,
          categoryId,
          bankAccountId: bankAccountId as string,
          isCredit: isCredit as boolean,
          counterpartyBankCode: counterpartyBankCode ?? null,
          counterpartyCountry: counterpartyCountry ?? null,
        },
        options.applyHistory
      );
      await refreshMoneyFlows(queryClient);
      const learnKey = payee || iban || 'unknown';
      historyToast(
        changed.length > 0
          ? t('categorization.learnedApplied', { count: changed.length })
          : t('categorization.learned'),
        changed.length > 0 ? `"${learnKey}" → ${pickedName}` : `"${learnKey}"`,
        changed
      );
    } catch (e) {
      toast.error(tc('status.error'), { description: translateApiError(e as Error, tc) });
    }
  };
```

`getCategoryDisplay` returns `null` for an unknown id; `categoryName` takes `{ id, name, isSystem }`-like objects (check `useCategoryName` in `src/hooks/use-categories.ts` and pass what it expects; the fallback object above only needs the fields it reads).

Update the two existing callers: `onSaved={(created) => void handleSelect(created.id, createSkipLearning)}` becomes `onSaved={(created) => void handleSelect(created.id, { remember: !createSkipLearning, applyHistory: false })}`.

Every `<CategoryCommandList … onSelect={handleSelect} … />` gains `preview={preview.data} previewLoading={preview.isFetching && !preview.data} onEditRule={(ruleId) => { setOpen(false); setLocation(editRuleHref(ruleId)); }}` (three render sites: compact-with-category, compact-empty, full-size).

- [ ] **Step 4: The footer in `CategoryCommandList`**

Change its props: `onSelect: (id: string, options: { remember: boolean; applyHistory: boolean }) => void;` plus

```ts
  preview?: RecategorizationPreview;
  previewLoading?: boolean;
  onEditRule?: (ruleId: string) => void;
```

Inside: keep `rememberForFuture` (default `true`), add `const [applyHistory, setApplyHistory] = useState(true);`, `const categoryNameOf = useCategoryName();` is already `categoryName`, and `const { t: tc } = useTranslation('common');` if needed. Every `onSelect(category.id, !rememberForFuture)` becomes `onSelect(category.id, { remember: rememberForFuture, applyHistory })`, and `onCreate(!rememberForFuture)` stays.

Replace the `{enableLearning && (<div className="border-t px-3 py-2.5">…</div>)}` block with:

```tsx
      {enableLearning && (
        <div className="grid gap-2 border-t px-3 py-2.5">
          {preview?.governing === 'ownAccount' ? (
            <p className="m-0 flex items-start gap-2 text-xs text-ink-3">
              <ArrowRightLeft className="mt-px h-3.5 w-3.5 shrink-0 text-ink-4" />
              {t('categorization.ownAccountNote')}
            </p>
          ) : preview && !preview.learnable && preview.governing !== 'custom' ? (
            <p className="m-0 flex items-start gap-2 text-xs text-ink-3">
              <CircleAlert className="mt-px h-3.5 w-3.5 shrink-0 text-ink-4" />
              {t('categorization.notLearnable')}
            </p>
          ) : (
            <>
              {preview?.governing === 'custom' && preview.rule && (
                <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 rounded-md bg-well px-2.5 py-2 text-xs font-medium text-ink-2">
                  <Flag className="row-span-2 mt-px h-3.5 w-3.5 text-ink-3" />
                  <span>
                    {t('categorization.governedByRule', {
                      rule: preview.rule.name,
                      category: categoryName(
                        categories.find((c) => c.id === preview.rule?.categoryId) ?? {
                          id: preview.rule.categoryId,
                          name: preview.rule.categoryId,
                          isSystem: false,
                        }
                      ),
                    })}
                  </span>
                  {onEditRule && (
                    <button
                      type="button"
                      className="justify-self-start text-[10px] font-650 text-ink-2 hover:underline"
                      onClick={() => onEditRule(preview.rule!.id)}
                    >
                      {t('categorization.editRule')}
                    </button>
                  )}
                </div>
              )}
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-start gap-2">
                  <Brain className="mt-px h-3.5 w-3.5 shrink-0 text-ink-3" />
                  <Label htmlFor="remember-toggle" className="cursor-pointer text-xs text-ink-3">
                    {preview?.governing === 'custom'
                      ? t('categorization.retargetRule')
                      : t('categorization.rememberForFuture')}
                    <small className="block font-400">
                      {preview?.governing === 'custom'
                        ? t('categorization.retargetRuleScope')
                        : scopeLabel}
                    </small>
                  </Label>
                </div>
                <Switch
                  id="remember-toggle"
                  checked={rememberForFuture}
                  onCheckedChange={setRememberForFuture}
                  className="scale-75"
                />
              </div>
              {rememberForFuture && previewLoading && (
                <p className="m-0 text-[10px] text-ink-4" aria-live="polite">
                  {t('categorization.historyChecking')}
                </p>
              )}
              {rememberForFuture && preview && preview.matches > 0 && (
                <label className="flex cursor-pointer items-start gap-2 text-xs text-ink-2">
                  <Checkbox
                    className="mt-px"
                    checked={applyHistory}
                    onCheckedChange={(v) => setApplyHistory(v === true)}
                  />
                  <span>
                    <b className="block font-600">
                      {t('categorization.applyHistory', { count: preview.matches })}
                    </b>
                    <small className="block text-[10px] text-ink-4">
                      {formatBreakdown(
                        preview.byCategory,
                        (id) =>
                          categoryName(
                            categories.find((c) => c.id === id) ?? { id, name: id, isSystem: false }
                          ),
                        {
                          uncategorized: t('categorization.uncategorizedCount'),
                          more: (n) => t('categorization.moreCategories', { count: n }),
                        }
                      )}
                    </small>
                  </span>
                </label>
              )}
            </>
          )}
        </div>
      )}
```

Adjust the `categoryName(...)` calls to whatever shape `useCategoryName()` accepts (read `src/hooks/use-categories.ts`; if it takes a `TransactionCategory`, look the category up and fall back to the raw id string with a small inline helper `const nameOf = (id: string) => { const c = categories.find((x) => x.id === id); return c ? categoryName(c) : id; }` and use `nameOf` in the three places above).

- [ ] **Step 5: Ledger passes the id**

In `src/pages/BankAccountDetail.tsx`, in the ledger table's `<CategorySelector` (the one with `onCreateRule`), add `transactionId={tx.id}`.

- [ ] **Step 6: Gates**

Run: `npm run lint && npm run typecheck && npm test`
Expected: green. Fix type errors from the `handleSelect` signature change (the `CategoryFormDialog` `onSaved` caller) if any.

- [ ] **Step 7: Verify in the app (demo profile)**

Start the demo profile from the worktree (`cd` there first, then `npm run tauri dev` through the `moony-demo` launch config as the main session does), open Bankovní účty → an account, open a categorized row's picker: the footer shows the toggle, "Zjišťuji další platby…", then "Zařadit i N dalších plateb · …". Pick another category: the rows of that payee change, the toast carries "Vrátit zpět", undo restores them. This step is run by the main session, not a subagent; the subagent stops after the gates.

- [ ] **Step 8: Commit**

```bash
git add src/components/bank-accounts/CategorySelector.tsx src/pages/BankAccountDetail.tsx src/lib/money-flow-refresh.ts src/i18n/locales/cs/bank_accounts.json src/i18n/locales/en/bank_accounts.json
git commit -m "feat(ledger): remembered corrections show and recategorize other payments

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Rules page — learned-tab history dialog and the `?edit=` link

**Files:**
- Modify: `src/pages/CategorizationRules.tsx` (`updateLearnedCategory` mutation, new dialog state + `AlertDialog`, `?edit=` handling)
- Modify: `src/i18n/locales/cs/categorization.json`, `src/i18n/locales/en/categorization.json` (`learnedTab.history.*`, `learnedTab.categoryChanged`)

**Interfaces:**
- Consumes: `categorizationApi.previewLearnedRuleHistory / applyLearnedRuleToHistory / restoreTransactionCategories`, `changedByPick`, `formatBreakdown`, `parseEditRuleSearch`.

- [ ] **Step 1: Locale strings**

`src/i18n/locales/cs/categorization.json`, inside `"learnedTab"`:

```json
"categoryChanged": "Kategorie pravidla změněna",
"history": {
  "title": "Zařadit i další platby?",
  "description_one": "Pravidlo pro „{{payee}}“ ({{account}} · {{direction}}) nově zařazuje jako {{category}}. Zasáhne {{count}} další platbu: {{breakdown}}.",
  "description_few": "Pravidlo pro „{{payee}}“ ({{account}} · {{direction}}) nově zařazuje jako {{category}}. Zasáhne {{count}} další platby: {{breakdown}}.",
  "description_other": "Pravidlo pro „{{payee}}“ ({{account}} · {{direction}}) nově zařazuje jako {{category}}. Zasáhne {{count}} dalších plateb: {{breakdown}}.",
  "onlyNew": "Jen nové platby",
  "apply_one": "Zařadit {{count}} platbu",
  "apply_few": "Zařadit {{count}} platby",
  "apply_other": "Zařadit {{count}} plateb",
  "applied_one": "{{count}} další platba zařazena",
  "applied_few": "{{count}} další platby zařazeny",
  "applied_other": "{{count}} dalších plateb zařazeno",
  "undo": "Vrátit zpět",
  "restored": "Změněné platby vráceny ({{count}})",
  "uncategorized": "bez kategorie",
  "more": "+{{count}} dalších"
}
```

`src/i18n/locales/en/categorization.json`, inside `"learnedTab"`:

```json
"categoryChanged": "Rule category changed",
"history": {
  "title": "Also categorize other payments?",
  "description_one": "The rule for \"{{payee}}\" ({{account}} · {{direction}}) now assigns {{category}}. It covers {{count}} other payment: {{breakdown}}.",
  "description_other": "The rule for \"{{payee}}\" ({{account}} · {{direction}}) now assigns {{category}}. It covers {{count}} other payments: {{breakdown}}.",
  "onlyNew": "Only new payments",
  "apply_one": "Categorize {{count}} payment",
  "apply_other": "Categorize {{count}} payments",
  "applied_one": "{{count}} other payment categorized",
  "applied_other": "{{count}} other payments categorized",
  "undo": "Undo",
  "restored": "Changed payments restored ({{count}})",
  "uncategorized": "no category",
  "more": "+{{count}} more"
}
```

- [ ] **Step 2: The dialog**

In `CategorizationRules.tsx` import `AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle` from `@/components/ui/alert-dialog`, `changedByPick, formatBreakdown` from `@/utils/recategorization`, `parseEditRuleSearch` from `@/utils/rules-link`, `useBankAccounts` from `@/hooks/use-bank-accounts` (check the path `LearnedRow` already imports it from and reuse that), and type `ChangedRow` from `@shared/schema`.

State, next to `pendingDelete`:

```ts
  const [pendingHistory, setPendingHistory] = useState<{
    id: string;
    count: number;
    description: string;
  } | null>(null);
  const { accounts } = useBankAccounts();
```

Replace the `updateLearnedCategory` mutation:

```ts
  const updateLearnedCategory = useMutation({
    mutationFn: async ({ id, categoryId }: { id: string; categoryId: string }) => {
      await categorizationApi.updateLearnedPayeeCategory(id, categoryId);
      const preview = await categorizationApi.previewLearnedRuleHistory(id);
      return { id, categoryId, preview };
    },
    onSuccess: async ({ id, categoryId, preview }) => {
      await invalidateAll();
      toast(t('learnedTab.categoryChanged'));
      const count = changedByPick(preview, categoryId);
      if (count === 0) return;
      const rule = learnedRules.find((r) => r.id === id);
      const nameOf = (catId: string) => {
        const c = categories.find((x) => x.id === catId);
        return c ? categoryName(c) : catId;
      };
      setPendingHistory({
        id,
        count,
        description: t('learnedTab.history.description', {
          count,
          payee: rule?.normalizedPayee || rule?.originalPayee || formatIbanDisplay(rule?.counterpartyIban ?? null) || '—',
          account: accounts.find((a) => a.id === rule?.bankAccountId)?.name ?? '—',
          direction: t(rule?.isCredit ? 'incoming' : 'outgoing').toLowerCase(),
          category: nameOf(categoryId),
          breakdown: formatBreakdown(preview.byCategory, nameOf, {
            uncategorized: t('learnedTab.history.uncategorized'),
            more: (n) => t('learnedTab.history.more', { count: n }),
          }),
        }),
      });
    },
    onError,
  });

  const applyHistory = useMutation({
    mutationFn: (id: string) => categorizationApi.applyLearnedRuleToHistory(id),
    onSuccess: async (changed: ChangedRow[]) => {
      await refreshMoneyFlows(queryClient);
      await invalidateAll();
      setPendingHistory(null);
      toast(t('learnedTab.history.applied', { count: changed.length }), {
        duration: 8000,
        action: {
          label: t('learnedTab.history.undo'),
          onClick: () => {
            void (async () => {
              try {
                const restored = await categorizationApi.restoreTransactionCategories(changed);
                await refreshMoneyFlows(queryClient);
                await invalidateAll();
                toast(t('learnedTab.history.restored', { count: restored }));
              } catch (error) {
                onError(error as Error);
              }
            })();
          },
        },
      });
    },
    onError,
  });
```

`formatIbanDisplay` is already imported from `@/utils/rule-hits` (check; add if not). `refreshMoneyFlows` is imported from `@/lib/money-flow-refresh` (add if not). `onError` is the page's existing error handler.

Render, next to the existing `ConfirmDeleteDialog` at the bottom of the page:

```tsx
      <AlertDialog open={pendingHistory !== null} onOpenChange={(o) => !o && setPendingHistory(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('learnedTab.history.title')}</AlertDialogTitle>
            <AlertDialogDescription>{pendingHistory?.description}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={applyHistory.isPending}>
              {t('learnedTab.history.onlyNew')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(e) => {
                e.preventDefault();
                if (pendingHistory) applyHistory.mutate(pendingHistory.id);
              }}
              disabled={applyHistory.isPending}
            >
              {applyHistory.isPending
                ? tc('status.saving')
                : t('learnedTab.history.apply', { count: pendingHistory?.count ?? 0 })}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
```

- [ ] **Step 3: The `?edit=<id>` link**

Next to `const [linkedNewRule] = useState(() => parseNewRuleSearch(search));` add:

```ts
  const [linkedEditId] = useState(() => parseEditRuleSearch(search));
  const editHandled = useRef(false);
  useEffect(() => {
    if (!linkedEditId || editHandled.current) return;
    const rule = customRules.find((r) => r.id === linkedEditId);
    if (!rule) return;
    editHandled.current = true;
    setEditingRule(rule);
    setRulePrefill(null);
    setRuleDialogOpen(true);
    setTab('custom');
    setLocation(RULES_PAGE_PATH, { replace: true });
  }, [linkedEditId, customRules, setLocation]);
```

Import `useRef` from `react` if missing. This effect must sit after `customRules` and the dialog state are declared.

- [ ] **Step 4: Gates**

Run: `npm run lint && npm run typecheck && npm test`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src/pages/CategorizationRules.tsx src/i18n/locales/cs/categorization.json src/i18n/locales/en/categorization.json
git commit -m "feat(rules): changing a learned rule offers to recategorize other payments

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Docs, changelog and final gates

**Files:**
- Modify: `docs/architecture/overview.md` (the `categorization` row of the domain table)
- Modify: `CHANGELOG.md` (Unreleased → Added, Changed)

- [ ] **Step 1: Overview**

In the categorization row of the domain table in `docs/architecture/overview.md`, after the sentence about `learn.rs`, add:

`recategorize.rs` answers which payments a learned scope or a custom rule covers today (replayed per rule, all time), breaks them down by current category, applies a category to them with the layer's provenance label and restores them (undo); `engine.explain` names the deciding layer (own account, custom, learned, pack, none) so the ledger picker can say what a remembered correction rewrites — a custom rule with precedence is retargeted instead of shadowing a learned row.

- [ ] **Step 2: Changelog**

Under `## [Unreleased]`:

`### Added`:

```markdown
- **Remembered corrections reach other payments.** The category picker in the bank ledger says
  how many other payments of the same payee the remembered rule covers, broken down by their
  current category, and categorizes them in the same step; the toast offers "Vrátit zpět". The
  same offer appears when a learned rule's category is changed on the rules page.
```

`### Changed`:

```markdown
- **A correction under a custom rule rewrites that rule.** When one of your own rules decides a
  payee's category, remembering a correction changes the rule's category (it has precedence over
  learned choices) and the picker says so, with a link to the rule. A payment that cannot be
  remembered, or a transfer between your own accounts, now says why instead of silently learning
  nothing.
```

- [ ] **Step 3: Final gates**

```bash
npm run lint && npm run typecheck && npm run format:check && npm test
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test && cd ..
git status --short
```

Expected: all green, no stray files. Run `npm run format` first if `format:check` complains, then re-run.

- [ ] **Step 4: Commit**

```bash
git add docs/architecture/overview.md CHANGELOG.md
git commit -m "docs: recategorize service and earlier-payments changelog

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Merge (main session only)

1. In the worktree: `git log --oneline main..HEAD` shows the nine commits; `git merge-base --is-ancestor main HEAD` (fast-forwardable) — if main moved, `git merge main` in the worktree first and resolve.
2. In the shared checkout `Moony-tauri`: verify `git branch --show-current` is `main` and `git status --short` is empty. If so, `git merge --ff-only feat/recategorize-history`. If the checkout is dirty or on another branch, do not touch it: report and leave the branch for the owner.
3. `git merge-base --is-ancestor feat/recategorize-history main` must print nothing and exit 0 before the branch or the worktree is removed.
4. No push.
