# Bank Account Detail Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the bank account detail page on dated balance records: a balance section (line through records, three stats) shown only for a nonzero recorded balance, a flows card (12-month bar window shifted by arrows) shown only with transactions in the last 12 months, the unchanged ledger opening on the newest month with movements, and balance history / import history / interest tiers as modals from the "···" menu.

**Architecture:** A new append-only table `bank_balance_records` is written by one Rust service function (`record_balance`) that also mirrors the newest record into the existing `bank_accounts.balance` / `balance_as_of` columns, so net worth, the account list, snapshots and the MCP server keep working unchanged. The frontend gets pure helpers in `src/utils/` (balance series and stats, monthly flows window, section visibility), one data hook, five small components, and a restructured `BankAccountDetail.tsx` whose ledger card is kept as it is.

**Tech Stack:** Tauri 2 (Rust, rusqlite/SQLCipher, specta bindings), React 19 + TypeScript, TanStack Query, Recharts (existing `MoonyLineChart` / `MoonyBarChart`), react-i18next (cs + en), Vitest, cargo test.

**Spec:** `docs/specs/2026-10-10-bank-account-detail-redesign-design.md`. Visual reference: `docs/specs/2026-10-10-bank-account-detail-prototype.html` (differs from the spec in two accepted points: both head buttons are dark primary, and the arrows shift the whole window).

## Global Constraints

- Ten rules of `AGENTS.md` apply to every task; in particular: never call `invoke()` outside `src/lib/tauri-api.ts`; `shared/schema.ts` is the wire contract (7-step procedure in `docs/standards/type-contract.md`); new Rust types get per-field `#[serde(rename = "camelCase")]`, are registered in `bindings.rs` and `shared/generated-types.ts` is regenerated with `cd src-tauri && cargo test generate_bindings -- --ignored`; business logic lives in `src-tauri/src/services/` taking `&Connection`, errors are `AppError`, no `.unwrap()` outside tests; migrations are append-only (`013_balance_records` is the next number); mutations invalidate domain keys + `["portfolio-metrics"]` + `["cashflow-report"]`, then `recordSnapshot()` + `["portfolio-history"]` (all of that is what `refreshMoneyFlows()` in `src/lib/money-flow-refresh.ts` does); every user-facing string lands in BOTH `src/i18n/locales/en/*.json` and `src/i18n/locales/cs/*.json`; check `docs/DEPRECATED.md` before copying a pattern.
- Money is TEXT, timestamps are unix epoch INTEGER (UTC day starts for dates), PKs are TEXT UUIDs; amounts are formatted through `useFormat().money(amount, currency, opts)` with the account currency, never a hard-coded "Kč".
- Copy rules (design system §10): sentence case, no exclamation marks, true minus (`useFormat` does it), "pohyb/pohyby/pohybů" for movements in the hero and the flows card, "transakce" only inside the ledger card. Buttons are verbs.
- Work in the sibling worktree `../Moony-tauri-bank-detail` on branch `feat/bank-account-detail-redesign` (Task 0). Never commit, reset or switch branches in the shared checkout `../Moony-tauri` (other sessions use it).
- Gates before every commit: `npm run lint`, `npm run typecheck`, `npm run format:check` (run `npm run format` first), `npm test`; for Rust tasks `cd src-tauri && cargo test` and `cargo clippy`.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

### Task 0: Worktree, branch and the design documents

**Files:**
- Create: `../Moony-tauri-bank-detail` (git worktree)
- Commit: `docs/specs/2026-10-10-bank-account-detail-redesign-design.md`, `docs/specs/2026-10-10-bank-account-detail-prototype.html`, `docs/plans/2026-10-10-bank-account-detail-redesign.md`

- [ ] **Step 1: Create the worktree from main**

Run from `../Moony-tauri`:

```bash
git worktree add ../Moony-tauri-bank-detail -b feat/bank-account-detail-redesign main
```

Expected: a new directory `../Moony-tauri-bank-detail` on the new branch.

- [ ] **Step 2: Copy the untracked design documents into the worktree**

```bash
cp docs/specs/2026-10-10-bank-account-detail-redesign-design.md docs/specs/2026-10-10-bank-account-detail-prototype.html ../Moony-tauri-bank-detail/docs/specs/
cp docs/plans/2026-10-10-bank-account-detail-redesign.md ../Moony-tauri-bank-detail/docs/plans/
```

- [ ] **Step 3: Install dependencies in the worktree**

```bash
cd ../Moony-tauri-bank-detail && npm ci
```

Expected: `node_modules` present; `npm run typecheck` passes on the untouched tree. (The first `cargo test` in `src-tauri` compiles everything and takes several minutes; that is expected.)

- [ ] **Step 4: Commit the documents**

```bash
cd ../Moony-tauri-bank-detail
git add docs/specs/2026-10-10-bank-account-detail-redesign-design.md docs/specs/2026-10-10-bank-account-detail-prototype.html docs/plans/2026-10-10-bank-account-detail-redesign.md
git commit -m "docs: bank account detail redesign spec, prototype and plan

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

All later tasks run inside `../Moony-tauri-bank-detail`.

---

### Task 1: Migration `013_balance_records`

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (const after `MIGRATION_012`, Vec entry in `all_migrations()`, tests at the end of the test module)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `docs/architecture/database.md` (row in "### Bank accounts" of the table catalog)

**Interfaces:**
- Produces: table `bank_balance_records(id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at)` with `UNIQUE (bank_account_id, as_of)`; every account with a dated balance has exactly one backfilled record.

- [ ] **Step 1: Write the failing migration test**

Append inside the existing `mod tests` of `src-tauri/src/db/migrations.rs` (next to `migration_012_*`):

```rust
    fn database_before_013() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let earlier: Vec<(&str, &str)> = all_migrations()
            .into_iter()
            .filter(|(name, _)| *name < "013_balance_records")
            .collect();
        run_chain(&conn, &earlier).expect("earlier migrations");
        conn
    }

    #[test]
    fn migration_013_backfills_one_record_per_dated_balance() {
        let conn = database_before_013();
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, balance, balance_as_of, updated_at)
                VALUES ('dated', 'Dated', 'EUR', '1200.50', 1767312000, 1767400000);
             INSERT INTO bank_accounts (id, name, currency, balance, balance_as_of, updated_at)
                VALUES ('undated', 'Undated', 'CZK', '999', NULL, 1767400000);",
        )
        .expect("accounts");
        run_chain(&conn, &[("013_balance_records", MIGRATION_013)]).expect("013");

        let rows: Vec<(String, String, String, i64, String, i64)> = conn
            .prepare(
                "SELECT bank_account_id, amount, currency, as_of, source, recorded_at
                 FROM bank_balance_records ORDER BY bank_account_id",
            )
            .expect("select")
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .expect("rows")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("rows");
        assert_eq!(
            rows,
            vec![(
                "dated".to_string(),
                "1200.50".to_string(),
                "EUR".to_string(),
                1767312000,
                "manual".to_string(),
                1767400000
            )]
        );
        let id: String = conn
            .query_row("SELECT id FROM bank_balance_records", [], |r| r.get(0))
            .expect("id");
        assert_eq!(id.len(), 36, "ids are lowercase UUID v4 strings: {id}");
        // One record per account and day
        assert!(conn
            .execute(
                "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of, source)
                 VALUES ('dup', 'dated', '1', 'EUR', 1767312000, 'manual')",
                [],
            )
            .is_err());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test migration_013 -- --nocapture`
Expected: compile error `cannot find value MIGRATION_013`.

- [ ] **Step 3: Add the migration const and the Vec entry**

Directly after the `MIGRATION_012` const in `src-tauri/src/db/migrations.rs` add:

```rust
/// 013: dated balance records. Every explicit balance update becomes a row; the
/// `bank_accounts.balance` / `balance_as_of` columns mirror the newest record
/// (services::bank_accounts::refresh_balance_columns). Undated balances are not
/// backfilled: they stay an undated number until the user records a dated one.
pub(crate) const MIGRATION_013: &str = r#"
CREATE TABLE IF NOT EXISTS bank_balance_records (
    id TEXT PRIMARY KEY,
    bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL,
    as_of INTEGER NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    source_batch_id TEXT REFERENCES csv_import_batches(id) ON DELETE SET NULL,
    recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (bank_account_id, as_of)
);
CREATE INDEX IF NOT EXISTS idx_bank_balance_records_account_day
    ON bank_balance_records(bank_account_id, as_of DESC);
-- The ids are lowercase UUID v4 built in SQL (same recipe as migration 008).
INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at)
SELECT lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-' || substr('89ab', 1 + (random() & 3), 1) || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))),
       id, balance, currency, balance_as_of, 'manual', NULL, updated_at
FROM bank_accounts
WHERE balance_as_of IS NOT NULL;
"#;
```

In `all_migrations()` append after the 012 line:

```rust
        ("013_balance_records", MIGRATION_013),
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test migration_013`
Expected: PASS.

- [ ] **Step 5: Regenerate the schema snapshot and run the whole Rust suite**

```bash
cd src-tauri && cargo test regenerate_schema_snapshot -- --ignored && cargo test
```

Expected: `src-tauri/src/db/schema.snapshot` diff shows exactly the new table and index; `cargo test` passes (including `baseline_matches_schema_snapshot`).

- [ ] **Step 6: Document the table**

In `docs/architecture/database.md`, under `### Bank accounts`, add a row after `bank_account_zones`:

```markdown
| `bank_balance_records` | Dated balance records of an account (migration 013): one row per explicit balance update, by hand or confirmed from a statement | FK `bank_account_id` ON DELETE CASCADE; `amount`/`currency` TEXT (account currency); `as_of` UTC day start; `source` `manual` \| `statement`; `source_batch_id` → `csv_import_batches` ON DELETE SET NULL; `UNIQUE (bank_account_id, as_of)` — recording the same day replaces the row. `bank_accounts.balance`/`balance_as_of` mirror the newest record (`services/bank_accounts.rs::refresh_balance_columns`); an undated legacy balance has no record until the first dated one is recorded. Written only through `record_balance`, `create_account` (opening balance with a date) and `delete_balance_record`. |
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/db/migrations.rs src-tauri/src/db/schema.snapshot docs/architecture/database.md
git commit -m "feat(db): bank_balance_records table with a backfill of dated balances

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Rust models `BankBalanceRecord` and `InsertBalanceRecord`

**Files:**
- Modify: `src-tauri/src/models/bank_accounts.rs` (replace `InsertBalanceAnchor`, add `BankBalanceRecord`)
- Modify: `src-tauri/src/bindings.rs:117` (type registration)

**Interfaces:**
- Produces: `crate::models::bank_accounts::BankBalanceRecord { id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at }` and `InsertBalanceRecord { amount, currency, as_of, confirm_unknown_as_of, source_batch_id }` with `validate()`.

- [ ] **Step 1: Write the failing validation test**

In the `#[cfg(test)] mod tests` of `src-tauri/src/models/bank_accounts.rs` (create the module at the end of the file if there is none) add:

```rust
    #[test]
    fn insert_balance_record_validates_amount_currency_and_day() {
        let ok = InsertBalanceRecord {
            amount: "1200.50".into(),
            currency: "EUR".into(),
            as_of: 1767312000,
            confirm_unknown_as_of: false,
            source_batch_id: None,
        };
        assert!(ok.validate().is_ok());
        for (amount, currency, as_of) in [
            ("NaN", "EUR", 1767312000),
            ("12", "EU", 1767312000),
            ("12", "EUR", 1767312001),
            ("12", "EUR", i64::MAX),
        ] {
            let bad = InsertBalanceRecord {
                amount: amount.into(),
                currency: currency.into(),
                as_of,
                confirm_unknown_as_of: false,
                source_batch_id: None,
            };
            assert!(bad.validate().is_err(), "accepted {amount} {currency} {as_of}");
        }
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test insert_balance_record_validates`
Expected: compile error `cannot find struct InsertBalanceRecord`.

- [ ] **Step 3: Replace the anchor struct with the record structs**

In `src-tauri/src/models/bank_accounts.rs` replace the whole `InsertBalanceAnchor` struct and its `impl InsertBalanceAnchor` block with:

```rust
/// A dated balance of a bank account (migration 013). The newest record is mirrored
/// into `bank_accounts.balance` / `balance_as_of`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BankBalanceRecord {
    pub id: String,
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: String,
    /// Balance in the account currency (canonical decimal string).
    pub amount: String,
    pub currency: String,
    /// UTC day start (unix seconds).
    #[serde(rename = "asOf")]
    pub as_of: i64,
    /// `manual` (entered by hand) or `statement` (confirmed from a CSV import).
    pub source: String,
    #[serde(rename = "sourceBatchId")]
    pub source_batch_id: Option<String>,
    #[serde(rename = "recordedAt")]
    pub recorded_at: i64,
}

/// Input of `record_bank_account_balance`: one dated balance. `source` is derived:
/// `statement` when a batch id is given, else `manual`.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertBalanceRecord {
    pub amount: String,
    pub currency: String,
    #[serde(rename = "asOf")]
    pub as_of: i64,
    /// Required (true) only while the account has no dated record and its undated
    /// balance is not 0: the first dated record replaces that number.
    #[serde(rename = "confirmUnknownAsOf", default)]
    pub confirm_unknown_as_of: bool,
    #[serde(rename = "sourceBatchId", default)]
    pub source_batch_id: Option<String>,
}

impl InsertBalanceRecord {
    pub fn validate(&self) -> Result<()> {
        let amount = self
            .amount
            .parse::<f64>()
            .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
        if !amount.is_finite() {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }
        if self.currency.len() != 3 || !self.currency.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(AppError::Validation("validation.currencyInvalid".into()));
        }
        validate_balance_day(self.as_of)
    }
}
```

(`validate_balance_day` already exists in the file and stays.)

- [ ] **Step 4: Register the types for the drift detector**

In `src-tauri/src/bindings.rs` replace the line

```rust
    types.register::<crate::models::bank_accounts::InsertBalanceAnchor>();
```

with

```rust
    types.register::<crate::models::bank_accounts::BankBalanceRecord>();
    types.register::<crate::models::bank_accounts::InsertBalanceRecord>();
```

- [ ] **Step 5: Run the test (expect other compile errors from the old name)**

Run: `cd src-tauri && cargo test insert_balance_record_validates`
Expected: the model test passes once the crate compiles; `services/bank_accounts.rs` and `commands/bank_accounts.rs` still reference `InsertBalanceAnchor` and fail to compile. Fix that in Task 3 and Task 4 — to keep this task compiling on its own, temporarily add at the end of `models/bank_accounts.rs`:

```rust
/// Transitional alias; removed in the service/commands tasks.
pub type InsertBalanceAnchor = InsertBalanceRecord;
```

Re-run: `cargo test insert_balance_record_validates` → PASS, `cargo test` → PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/models/bank_accounts.rs src-tauri/src/bindings.rs
git commit -m "feat(models): BankBalanceRecord and InsertBalanceRecord

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Balance record service

**Files:**
- Modify: `src-tauri/src/services/bank_accounts.rs` (`set_balance_anchor` → `record_balance`, new `list_balance_records`, `delete_balance_record`, private `refresh_balance_columns`; `create_account` writes the opening record; `update_account` ignores balance; tests)
- Modify: `src-tauri/src/models/bank_accounts.rs` (remove the transitional alias at the end of the task)

**Interfaces:**
- Consumes: `InsertBalanceRecord`, `BankBalanceRecord` (Task 2); table from Task 1.
- Produces:
  - `pub fn record_balance(conn: &rusqlite::Connection, account_id: &str, data: &InsertBalanceRecord) -> Result<BankAccount>`
  - `pub fn list_balance_records(conn: &rusqlite::Connection, account_id: &str) -> Result<Vec<BankBalanceRecord>>` (newest first)
  - `pub fn delete_balance_record(conn: &rusqlite::Connection, record_id: &str) -> Result<BankAccount>`

- [ ] **Step 1: Extend the test schema and write the failing tests**

In the `setup_test_db()` of the tests module in `src-tauri/src/services/bank_accounts.rs`, add inside the `execute_batch` SQL, after the `bank_transactions` table:

```sql
            CREATE TABLE csv_import_batches (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL
            );

            CREATE TABLE bank_balance_records (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                as_of INTEGER NOT NULL,
                source TEXT NOT NULL DEFAULT 'manual',
                source_batch_id TEXT REFERENCES csv_import_batches(id) ON DELETE SET NULL,
                recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE (bank_account_id, as_of)
            );
```

Delete the whole test `integrity_balance_anchor_safeguards_and_manual_date_semantics` (it tests the retired anchor semantics; it also creates `csv_import_batches` itself, which would now collide). Add these tests:

```rust
    fn record(amount: &str, as_of: i64) -> crate::models::bank_accounts::InsertBalanceRecord {
        crate::models::bank_accounts::InsertBalanceRecord {
            amount: amount.into(),
            currency: "EUR".into(),
            as_of,
            confirm_unknown_as_of: false,
            source_batch_id: None,
        }
    }

    const D1: i64 = 1767225600; // 2026-01-01
    const D2: i64 = 1767312000; // 2026-01-02
    const D3: i64 = 1767398400; // 2026-01-03

    #[test]
    fn record_balance_mirrors_the_newest_record_into_the_columns() {
        let conn = setup_test_db();
        let a = record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("100", Some(D2)));
        // An older day is history and leaves the columns alone
        let a = record_balance(&conn, "acc-1", &record("50", D1)).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("100", Some(D2)));
        // A newer day becomes the current balance
        let a = record_balance(&conn, "acc-1", &record("120", D3)).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("120", Some(D3)));
        let list = list_balance_records(&conn, "acc-1").unwrap();
        assert_eq!(
            list.iter().map(|r| (r.as_of, r.amount.as_str(), r.source.as_str())).collect::<Vec<_>>(),
            vec![(D3, "120", "manual"), (D2, "100", "manual"), (D1, "50", "manual")]
        );
    }

    #[test]
    fn record_balance_replaces_the_same_day() {
        let conn = setup_test_db();
        record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        let a = record_balance(&conn, "acc-1", &record("101", D2)).unwrap();
        assert_eq!(a.balance, "101");
        let list = list_balance_records(&conn, "acc-1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].amount, "101");
    }

    #[test]
    fn record_balance_rejects_wrong_currency_future_day_and_foreign_batch() {
        let conn = setup_test_db();
        let mut data = record("100", D2);
        data.currency = "CZK".into();
        assert!(record_balance(&conn, "acc-1", &data).is_err());
        let future = chrono::Utc::now().timestamp().div_euclid(86400) * 86400 + 86400;
        assert!(record_balance(&conn, "acc-1", &record("100", future)).is_err());
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, created_at, updated_at) VALUES ('acc-2', 'Other', 'EUR', 0, 0);
             INSERT INTO csv_import_batches (id, bank_account_id) VALUES ('batch-2', 'acc-2');
             INSERT INTO csv_import_batches (id, bank_account_id) VALUES ('batch-1', 'acc-1');",
        )
        .unwrap();
        let mut foreign = record("100", D2);
        foreign.source_batch_id = Some("batch-2".into());
        assert!(record_balance(&conn, "acc-1", &foreign).is_err());
        let mut own = record("100", D2);
        own.source_batch_id = Some("batch-1".into());
        record_balance(&conn, "acc-1", &own).unwrap();
        let list = list_balance_records(&conn, "acc-1").unwrap();
        assert_eq!((list[0].source.as_str(), list[0].source_batch_id.as_deref()), ("statement", Some("batch-1")));
    }

    #[test]
    fn undated_nonzero_balance_needs_confirmation_only_before_the_first_record() {
        let conn = setup_test_db();
        conn.execute("UPDATE bank_accounts SET balance='999' WHERE id='acc-1'", [])
            .unwrap();
        assert!(record_balance(&conn, "acc-1", &record("100", D2)).is_err());
        let mut confirmed = record("100", D2);
        confirmed.confirm_unknown_as_of = true;
        assert_eq!(record_balance(&conn, "acc-1", &confirmed).unwrap().balance, "100");
        // From now on the flag is not needed
        assert!(record_balance(&conn, "acc-1", &record("110", D3)).is_ok());
        // An undated balance of exactly 0 never needs confirmation
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, balance, created_at, updated_at) VALUES ('acc-3', 'Zero', 'EUR', '0', 0, 0);",
        )
        .unwrap();
        assert!(record_balance(&conn, "acc-3", &record("5", D2)).is_ok());
    }

    #[test]
    fn delete_balance_record_refreshes_and_clears_the_columns() {
        let conn = setup_test_db();
        record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        record_balance(&conn, "acc-1", &record("120", D3)).unwrap();
        let newest = list_balance_records(&conn, "acc-1").unwrap()[0].id.clone();
        let a = delete_balance_record(&conn, &newest).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("100", Some(D2)));
        let last = list_balance_records(&conn, "acc-1").unwrap()[0].id.clone();
        let a = delete_balance_record(&conn, &last).unwrap();
        assert_eq!((a.balance.as_str(), a.balance_as_of), ("0", None));
        assert!(delete_balance_record(&conn, "missing").is_err());
    }

    #[test]
    fn create_account_with_a_dated_balance_writes_the_first_record() {
        let conn = setup_test_db();
        let mut data = minimal_insert("Opening");
        data.currency = Some("EUR".into());
        data.balance = Some("250".into());
        data.balance_as_of = Some(Some(D2));
        let account = create_account(&conn, &data).unwrap();
        let list = list_balance_records(&conn, &account.id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].amount.as_str(), list[0].as_of, list[0].currency.as_str()), ("250", D2, "EUR"));
        // Without a date: undated balance, no record (unchanged behaviour)
        let mut undated = minimal_insert("Undated");
        undated.balance = Some("10".into());
        let account = create_account(&conn, &undated).unwrap();
        assert!(list_balance_records(&conn, &account.id).unwrap().is_empty());
        assert_eq!(account.balance_as_of, None);
    }

    #[test]
    fn update_account_ignores_balance_fields() {
        let conn = setup_test_db();
        record_balance(&conn, "acc-1", &record("100", D2)).unwrap();
        let mut edit = minimal_insert("Renamed");
        edit.currency = Some("EUR".into());
        edit.balance = Some("5".into());
        edit.balance_as_of = Some(None);
        let a = update_account(&conn, "acc-1", &edit).unwrap();
        assert_eq!((a.name.as_str(), a.balance.as_str(), a.balance_as_of), ("Renamed", "100", Some(D2)));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test services::bank_accounts::tests`
Expected: compile errors (`record_balance`, `list_balance_records`, `delete_balance_record` not found).

- [ ] **Step 3: Implement the service functions**

In `src-tauri/src/services/bank_accounts.rs` change the import line to

```rust
use crate::models::bank_accounts::{BankBalanceRecord, InsertBalanceRecord};
use crate::models::{BankAccount, BankTransaction, InsertBankAccount, InsertBankTransaction};
```

Replace the whole `set_balance_anchor` function with:

```rust
/// Mirror the newest dated record into `bank_accounts.balance` / `balance_as_of`;
/// without any record the account goes back to `'0'` / unknown date.
fn refresh_balance_columns(conn: &rusqlite::Connection, account_id: &str) -> Result<()> {
    let newest: Option<(String, i64)> = conn
        .query_row(
            "SELECT amount, as_of FROM bank_balance_records
             WHERE bank_account_id = ?1 ORDER BY as_of DESC LIMIT 1",
            [account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match newest {
        Some((amount, day)) => conn.execute(
            "UPDATE bank_accounts SET balance = ?1, balance_as_of = ?2, updated_at = unixepoch() WHERE id = ?3",
            params![amount, day, account_id],
        )?,
        None => conn.execute(
            "UPDATE bank_accounts SET balance = '0', balance_as_of = NULL, updated_at = unixepoch() WHERE id = ?1",
            [account_id],
        )?,
    };
    Ok(())
}

/// Record a dated balance. The same day is replaced; an older day is history and
/// leaves the current balance alone; the newest day becomes the current balance.
pub fn record_balance(
    conn: &rusqlite::Connection,
    account_id: &str,
    data: &InsertBalanceRecord,
) -> Result<BankAccount> {
    data.validate()?;
    let tx = conn.unchecked_transaction()?;
    let existing = get_account_by_id(&tx, account_id)?;
    if !existing.currency.eq_ignore_ascii_case(&data.currency) {
        return Err(AppError::Validation(
            "validation.balanceCurrencyMismatch".into(),
        ));
    }
    let has_dated: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM bank_balance_records WHERE bank_account_id = ?1)",
        [account_id],
        |r| r.get(0),
    )?;
    let undated_nonzero = existing.balance_as_of.is_none()
        && super::dedup::canonical_amount(&existing.balance) != super::dedup::canonical_amount("0");
    if !has_dated && undated_nonzero && !data.confirm_unknown_as_of {
        return Err(AppError::Validation(
            "validation.balanceAnchorUnknown".into(),
        ));
    }
    if let Some(batch) = &data.source_batch_id {
        let belongs: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM csv_import_batches WHERE id = ?1 AND bank_account_id = ?2)",
            params![batch, account_id],
            |r| r.get(0),
        )?;
        if !belongs {
            return Err(AppError::Validation(
                "validation.balanceBatchMismatch".into(),
            ));
        }
    }
    let source = if data.source_batch_id.is_some() { "statement" } else { "manual" };
    tx.execute(
        "INSERT INTO bank_balance_records
            (id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch())
         ON CONFLICT(bank_account_id, as_of) DO UPDATE SET
            amount = excluded.amount, currency = excluded.currency, source = excluded.source,
            source_batch_id = excluded.source_batch_id, recorded_at = unixepoch()",
        params![
            Uuid::new_v4().to_string(),
            account_id,
            data.amount,
            existing.currency,
            data.as_of,
            source,
            data.source_batch_id,
        ],
    )?;
    refresh_balance_columns(&tx, account_id)?;
    let account = get_account_by_id(&tx, account_id)?;
    tx.commit()?;
    Ok(account)
}

/// Every dated balance of the account, newest first.
pub fn list_balance_records(
    conn: &rusqlite::Connection,
    account_id: &str,
) -> Result<Vec<BankBalanceRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at
         FROM bank_balance_records WHERE bank_account_id = ?1 ORDER BY as_of DESC",
    )?;
    let rows = stmt
        .query_map([account_id], |r| {
            Ok(BankBalanceRecord {
                id: r.get(0)?,
                bank_account_id: r.get(1)?,
                amount: r.get(2)?,
                currency: r.get(3)?,
                as_of: r.get(4)?,
                source: r.get(5)?,
                source_batch_id: r.get(6)?,
                recorded_at: r.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Delete one record and re-mirror the newest remaining one into the columns.
pub fn delete_balance_record(conn: &rusqlite::Connection, record_id: &str) -> Result<BankAccount> {
    let tx = conn.unchecked_transaction()?;
    let account_id: String = tx
        .query_row(
            "SELECT bank_account_id FROM bank_balance_records WHERE id = ?1",
            [record_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("Balance record not found: {record_id}")))?;
    tx.execute("DELETE FROM bank_balance_records WHERE id = ?1", [record_id])?;
    refresh_balance_columns(&tx, &account_id)?;
    let account = get_account_by_id(&tx, &account_id)?;
    tx.commit()?;
    Ok(account)
}
```

In `create_account`, wrap the insert in a transaction and write the opening record. Replace from `conn.execute(` (the INSERT) up to and including `)?;` with:

```rust
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO bank_accounts (
            id, name, account_type, iban, bban, currency, balance,
            institution_id, data_source, interest_rate, has_zone_designation,
            termination_date, created_at, updated_at, exclude_from_balance,
            interest_rate_valid_until, balance_as_of
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id,
            data.name,
            account_type,
            data.iban,
            data.bban,
            currency,
            balance,
            data.institution_id,
            "manual",
            data.interest_rate,
            has_zone as i32,
            data.termination_date,
            now,
            now,
            exclude as i32,
            data.interest_rate_valid_until,
            data.balance_as_of.flatten(),
        ],
    )?;
    // A dated opening balance is the first record of the account's history
    if let Some(day) = data.balance_as_of.flatten() {
        tx.execute(
            "INSERT INTO bank_balance_records
                (id, bank_account_id, amount, currency, as_of, source, source_batch_id, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'manual', NULL, ?6)",
            params![Uuid::new_v4().to_string(), id, balance, currency, day, now],
        )?;
    }
    tx.commit()?;
```

In `update_account` delete the `changed` and `balance_as_of` computations and the `let balance = ...` line, and use the stored values instead:

```rust
    // The balance is never edited here: it mirrors the newest balance record
    // (record_balance / delete_balance_record).
    let balance = existing.balance.clone();
    let balance_as_of = existing.balance_as_of;
```

Keep the UPDATE statement as it is (it writes `balance = ?6` and `balance_as_of = ?15` with those unchanged values).

Finally remove the transitional alias `pub type InsertBalanceAnchor = InsertBalanceRecord;` from `models/bank_accounts.rs` and change `commands/bank_accounts.rs` minimally so the crate compiles: rename the command's parameter type to `InsertBalanceRecord` and the call to `bank_service::record_balance` (Task 4 finishes the commands).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test services::bank_accounts::tests && cargo clippy`
Expected: all pass, no clippy warnings in the changed code.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/bank_accounts.rs src-tauri/src/models/bank_accounts.rs src-tauri/src/commands/bank_accounts.rs
git commit -m "feat(bank): balance records service mirrors the newest record into the account

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Commands, bindings, wire contract and API wrappers

**Files:**
- Modify: `src-tauri/src/commands/bank_accounts.rs` (three commands replace `set_bank_account_balance_anchor`)
- Modify: `src-tauri/src/lib.rs:281` (handler registration)
- Modify: `shared/generated-types.ts` (regenerated)
- Modify: `shared/schema.ts` (`InsertBalanceAnchor` → `InsertBalanceRecord`, new `BankBalanceRecord`, `BalanceRecordSource`)
- Modify: `src/lib/tauri-api.ts` (wrappers)
- Modify: `docs/DEPRECATED.md` ("Retired statement balance path")

**Interfaces:**
- Produces (frontend): `bankAccountsApi.getBalanceRecords(accountId): Promise<BankBalanceRecord[]>`, `bankAccountsApi.recordBalance(accountId, data: InsertBalanceRecord): Promise<BankAccount>`, `bankAccountsApi.deleteBalanceRecord(recordId): Promise<BankAccount>`; types `BankBalanceRecord`, `InsertBalanceRecord`, `BalanceRecordSource = 'manual' | 'statement'`.

- [ ] **Step 1: Write the commands**

In `src-tauri/src/commands/bank_accounts.rs` replace the `set_bank_account_balance_anchor` command with:

```rust
/// Dated balances of an account, newest first.
#[tauri::command]
pub async fn get_balance_records(
    db: State<'_, Database>,
    account_id: String,
) -> Result<Vec<crate::models::bank_accounts::BankBalanceRecord>> {
    db.with_conn(|conn| bank_service::list_balance_records(conn, &account_id))
}

/// Record a dated balance (by hand or from a statement); returns the refreshed account.
#[tauri::command]
pub async fn record_bank_account_balance(
    db: State<'_, Database>,
    account_id: String,
    data: crate::models::bank_accounts::InsertBalanceRecord,
) -> Result<BankAccount> {
    data.validate()?;
    db.with_conn(|conn| bank_service::record_balance(conn, &account_id, &data))
}

/// Delete one balance record; returns the account with the re-mirrored balance.
#[tauri::command]
pub async fn delete_balance_record(
    db: State<'_, Database>,
    record_id: String,
) -> Result<BankAccount> {
    db.with_conn(|conn| bank_service::delete_balance_record(conn, &record_id))
}
```

In `src-tauri/src/lib.rs` replace `commands::bank_accounts::set_bank_account_balance_anchor,` with:

```rust
            commands::bank_accounts::get_balance_records,
            commands::bank_accounts::record_bank_account_balance,
            commands::bank_accounts::delete_balance_record,
```

- [ ] **Step 2: Compile, run the Rust suite and regenerate the bindings**

```bash
cd src-tauri && cargo test && cargo test generate_bindings -- --ignored
```

Expected: pass; `shared/generated-types.ts` now contains `BankBalanceRecord` and `InsertBalanceRecord` and no `InsertBalanceAnchor`.

- [ ] **Step 3: Mirror the types in `shared/schema.ts`**

Replace the `InsertBalanceAnchor` interface (next to `StatementBalance`) with:

```ts
export type BalanceRecordSource = 'manual' | 'statement';

/** A dated balance of a bank account; the newest one is mirrored into `BankAccount.balance`. */
export interface BankBalanceRecord {
  id: string;
  bankAccountId: string;
  /** Balance in the account currency. */
  amount: string;
  currency: string;
  /** UTC day start (unix seconds). */
  asOf: number;
  source: BalanceRecordSource;
  sourceBatchId: string | null;
  recordedAt: number;
}

/** Input of `record_bank_account_balance`; the source is derived from `sourceBatchId`. */
export interface InsertBalanceRecord {
  amount: string;
  currency: string;
  asOf: number;
  /** Needed only while the account has no dated record and an undated nonzero balance. */
  confirmUnknownAsOf?: boolean;
  sourceBatchId?: string | null;
}
```

Update the doc comment on `InsertBankAccount.balance`/`balanceAsOf`:

```ts
  /** Opening balance (create only; the update command ignores it — balances are recorded). */
  balance?: string;
  /** UTC day of the opening balance; with it the create writes the first balance record. */
  balanceAsOf?: number | null;
```

- [ ] **Step 4: Typed wrappers**

In `src/lib/tauri-api.ts` replace the import `InsertBalanceAnchor,` with `InsertBalanceRecord, BankBalanceRecord,` and replace the `setBalanceAnchor` entry of `bankAccountsApi` with:

```ts
  // Balance records (dated balances)
  getBalanceRecords: (accountId: string) =>
    tauriInvoke<BankBalanceRecord[]>('get_balance_records', { accountId }),
  recordBalance: (accountId: string, data: InsertBalanceRecord) =>
    tauriInvoke<BankAccount>('record_bank_account_balance', { accountId, data }),
  deleteBalanceRecord: (recordId: string) =>
    tauriInvoke<BankAccount>('delete_balance_record', { recordId }),
```

Then fix the two remaining callers so the frontend compiles: in `src/components/bank-accounts/csv-import/use-csv-import-mutations.ts` rename `useBalanceAnchor` to `useRecordBalance`, its data type to `InsertBalanceRecord` and the call to `bankAccountsApi.recordBalance(accountId, data)`; in `NextSteps.tsx` rename the import and the `useBalanceAnchor(accountId)` call accordingly (the mutate payload keeps `confirmUnknownAsOf: account.balanceAsOf == null`).

- [ ] **Step 5: Update DEPRECATED.md**

Replace the "Retired statement balance path" paragraph with:

```markdown
## Retired statement balance path

The CSV import `balanceUpdatePayload` helper and the later `setBalanceAnchor` / `set_bank_account_balance_anchor` command are gone. A balance is recorded with `bankAccountsApi.recordBalance` (`record_bank_account_balance`): every call is a dated row in `bank_balance_records`, the statement path passes `sourceBatchId`, and `confirmUnknownAsOf` is needed only while the account has no dated record and an undated nonzero balance. `update_bank_account` ignores `balance`/`balanceAsOf`; the detail page no longer reconstructs a balance line from transactions (`reconstructBalance`, `significantMovements`, `movementThreshold` were removed with it).
```

- [ ] **Step 6: Gates**

```bash
npm run typecheck && npm run lint && npm test
```

Expected: pass (no other file referenced the anchor API).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/bank_accounts.rs src-tauri/src/lib.rs shared/generated-types.ts shared/schema.ts src/lib/tauri-api.ts src/components/bank-accounts/csv-import/use-csv-import-mutations.ts src/components/bank-accounts/csv-import/NextSteps.tsx docs/DEPRECATED.md
git commit -m "feat(bank): balance record commands replace the balance anchor

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Stale-balance milestone and MCP opening balance date

**Files:**
- Modify: `src-tauri/src/services/milestones/accounts.rs`
- Modify: `src-tauri/src/services/mcp/bank_accounts.rs` (`BankAccountCreateArgs`, `bank_account_create`)

- [ ] **Step 1: Write the failing milestone test**

In the tests module of `src-tauri/src/services/milestones/accounts.rs` (look at the existing `account(...)` helper and the `balances_stale` test for the setup style) add:

```rust
    #[test]
    fn a_fresh_balance_record_keeps_an_otherwise_old_account_out_of_stale() {
        let conn = setup();
        let today = day(2026, 8, 1);
        // Edited two days ago, balance recorded a week ago: fresh
        account(&conn, "acc-old-edit", None, None, false, today - 2 * DAY);
        conn.execute(
            "UPDATE bank_accounts SET balance_as_of = ?1 WHERE id = 'acc-old-edit'",
            params![today - 7 * DAY],
        )
        .unwrap();
        // Edited yesterday, but the balance dates from two months ago: stale
        account(&conn, "acc-stale", None, None, false, today - DAY);
        conn.execute(
            "UPDATE bank_accounts SET balance_as_of = ?1 WHERE id = 'acc-stale'",
            params![today - 60 * DAY],
        )
        .unwrap();
        let list = build(&conn, today).unwrap();
        let stale: Vec<_> = list.iter().filter(|m| m.kind == "balances_stale").collect();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].count, Some(1));
        assert_eq!(stale[0].since_day, Some(today - 60 * DAY));
    }
```

(The helper is `account(conn, id, termination, rate_end, exclude, updated_at)` as in the existing tests of the file; `setup`, `day` and `DAY` are already imported there.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cd src-tauri && cargo test a_fresh_balance_record_keeps`
Expected: FAIL — `count` is `Some(2)` or the fresh account is counted, because staleness still reads `updated_at`.

- [ ] **Step 3: Use the balance date when present**

In `accounts.rs`: add `balance_as_of: Option<i64>` to `AccountRow`; select it (`SELECT id, name, termination_date, interest_rate_valid_until, exclude_from_balance, updated_at, balance_as_of FROM bank_accounts ORDER BY name`, `balance_as_of: r.get(6)?`); and replace `let updated = day_floor(a.updated_at);` with

```rust
        // A recorded balance dates the account; without one, the last edit does
        let updated = day_floor(a.balance_as_of.unwrap_or(a.updated_at));
```

Update the module doc comment's last line to "one reminder for balances nobody recorded or edited for a month".

- [ ] **Step 4: Run the milestone tests**

Run: `cd src-tauri && cargo test milestones::accounts`
Expected: PASS.

- [ ] **Step 5: MCP create accepts `balanceAsOf`**

In `src-tauri/src/services/mcp/bank_accounts.rs` add to `BankAccountCreateArgs` after `balance`:

```rust
    #[serde(rename = "balanceAsOf")]
    #[schemars(description = "UTC day (unix seconds, midnight) the opening balance applies to; with it the balance becomes the first dated balance record, without it the balance stays undated")]
    pub balance_as_of: Option<i64>,
```

and in `bank_account_create` replace `balance_as_of: None,` with `balance_as_of: args.balance_as_of.map(Some),`. If the MCP module has a test that builds `BankAccountCreateArgs` literally, add `balance_as_of: None` there.

- [ ] **Step 6: Gates and commit**

```bash
cd src-tauri && cargo test && cargo clippy
```

```bash
git add src-tauri/src/services/milestones/accounts.rs src-tauri/src/services/mcp/bank_accounts.rs
git commit -m "feat(bank): stale balances follow the record date; MCP create takes balanceAsOf

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Refresh contract and the balance records hook

**Files:**
- Modify: `src/lib/money-flow-refresh.ts` (add `'bank-balance-records'` to the key list)
- Create: `src/hooks/use-balance-records.ts`
- Modify: `src/i18n/locales/en/bank_accounts.json`, `src/i18n/locales/cs/bank_accounts.json` (toast keys)

**Interfaces:**
- Produces: `useBalanceRecords(accountId?: string): { records: BankBalanceRecord[]; isLoading: boolean }` and `useBalanceRecordMutations(accountId?: string): { recordBalance: UseMutationResult<BankAccount, Error, InsertBalanceRecord>; deleteRecord: UseMutationResult<BankAccount, Error, string> }`.

- [ ] **Step 1: Add the key to the refresh contract**

In `src/lib/money-flow-refresh.ts` add `'bank-balance-records',` right after `'bank-account-zones',`.

- [ ] **Step 2: Create the hook**

`src/hooks/use-balance-records.ts`:

```ts
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { BankBalanceRecord, InsertBalanceRecord } from '@shared/schema';
import { bankAccountsApi } from '@/lib/tauri-api';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';

/** Dated balances of one account, newest first. */
export function useBalanceRecords(accountId?: string) {
  const { data: records = [], isLoading } = useQuery({
    queryKey: ['bank-balance-records', accountId],
    queryFn: () => (accountId ? bankAccountsApi.getBalanceRecords(accountId) : []),
    enabled: !!accountId,
  });
  return { records: records as BankBalanceRecord[], isLoading };
}

/**
 * Record or delete a dated balance. Both refresh the money-flow queries, record a
 * portfolio snapshot (rule 7) and the milestones (stale balances).
 */
export function useBalanceRecordMutations(accountId?: string) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  const afterWrite = async () => {
    await refreshMoneyFlows(queryClient);
    await queryClient.invalidateQueries({ queryKey: ['milestones'] });
  };

  const recordBalance = useMutation({
    mutationFn: (data: InsertBalanceRecord) => {
      if (!accountId) throw new Error('validation.accountRequired');
      return bankAccountsApi.recordBalance(accountId, data);
    },
    onSuccess: async () => {
      await afterWrite();
      toast(t('recordDialog.success'));
    },
    onError: (error: Error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });

  const deleteRecord = useMutation({
    mutationFn: (recordId: string) => bankAccountsApi.deleteBalanceRecord(recordId),
    onSuccess: async () => {
      await afterWrite();
      toast(t('balanceHistory.deleted'));
    },
    onError: (error: Error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });

  return { recordBalance, deleteRecord };
}
```

- [ ] **Step 3: Add the two toast keys**

`en/bank_accounts.json`: add top-level objects (they grow in later tasks):

```json
  "recordDialog": { "success": "Balance recorded" },
  "balanceHistory": { "deleted": "Record deleted" }
```

`cs/bank_accounts.json`:

```json
  "recordDialog": { "success": "Zůstatek zaznamenán" },
  "balanceHistory": { "deleted": "Záznam smazán" }
```

- [ ] **Step 4: Gates and commit**

```bash
npm run typecheck && npm run lint && npm run format && npm test
git add src/lib/money-flow-refresh.ts src/hooks/use-balance-records.ts src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/bank_accounts.json
git commit -m "feat(bank): balance records query and mutations

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Pure helpers for the balance section

**Files:**
- Create: `src/utils/balance-records.ts`
- Create: `src/utils/balance-records.test.ts`

**Interfaces:**
- Produces:
  - `interface BalanceRecordLike { amount: string | number; asOf: number }`
  - `newestRecord(records) / previousRecord(records)` → `T | null`
  - `valueAt(records, day): number | null` (latest record on or before `day`)
  - `balanceSeries(records, today): BalanceSeriesPoint[]` with `{ t: number; value: number; record: boolean }`
  - `changeThisYear(records, today): { delta: number; baseDay: number } | null`
  - `averageMonthlyChange(records): { perMonth: number; months: number; sinceFirst: boolean; baseDay: number } | null`

- [ ] **Step 1: Write the failing tests**

`src/utils/balance-records.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import {
  averageMonthlyChange,
  balanceSeries,
  changeThisYear,
  newestRecord,
  previousRecord,
  valueAt,
} from './balance-records';

const DAY = 86400;
const day = (iso: string) => Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10)) / 1000;
const r = (iso: string, amount: number) => ({ asOf: day(iso), amount: String(amount) });
const TODAY = day('2026-10-10');

describe('newestRecord / previousRecord', () => {
  it('orders by day regardless of input order', () => {
    const records = [r('2026-09-01', 437800), r('2026-10-03', 450000), r('2026-08-01', 425600)];
    expect(newestRecord(records)?.asOf).toBe(day('2026-10-03'));
    expect(previousRecord(records)?.asOf).toBe(day('2026-09-01'));
    expect(newestRecord([])).toBeNull();
    expect(previousRecord([r('2026-10-03', 1)])).toBeNull();
  });
});

describe('valueAt', () => {
  it('is the latest record on or before the day, null before the first', () => {
    const records = [r('2026-01-05', 341000), r('2025-12-28', 353600), r('2026-02-01', 353000)];
    expect(valueAt(records, day('2026-01-01'))).toBe(353600);
    expect(valueAt(records, day('2026-01-05'))).toBe(341000);
    expect(valueAt(records, day('2025-12-01'))).toBeNull();
  });
});

describe('balanceSeries', () => {
  it('is empty with fewer than two records', () => {
    expect(balanceSeries([], TODAY)).toEqual([]);
    expect(balanceSeries([r('2026-10-03', 1)], TODAY)).toEqual([]);
  });
  it('covers the last year, enters at the window start and ends at the newest record', () => {
    const records = [r('2025-08-01', 300000), r('2025-12-01', 340000), r('2026-10-03', 450000)];
    const series = balanceSeries(records, TODAY);
    const start = TODAY - 365 * DAY; // 2025-10-10, between the first two records
    expect(series[0]).toEqual({ t: start, value: expect.any(Number), record: false });
    // Linear interpolation between 2025-08-01 (300000) and 2025-12-01 (340000)
    const t = (start - day('2025-08-01')) / (day('2025-12-01') - day('2025-08-01'));
    expect(series[0].value).toBe(Math.round(300000 + 40000 * t));
    expect(series.slice(1)).toEqual([
      { t: day('2025-12-01'), value: 340000, record: true },
      { t: day('2026-10-03'), value: 450000, record: true },
    ]);
  });
  it('starts at the first record when the history is shorter than a year', () => {
    const records = [r('2026-04-30', 391600), r('2026-09-30', 433560)];
    expect(balanceSeries(records, TODAY).map((p) => p.t)).toEqual([day('2026-04-30'), day('2026-09-30')]);
  });
});

describe('changeThisYear', () => {
  it('compares the newest record with the value at 1 January', () => {
    const records = [r('2025-12-28', 353600), r('2026-01-05', 341000), r('2026-10-03', 450000)];
    expect(changeThisYear(records, TODAY)).toEqual({ delta: 96400, baseDay: day('2025-12-28') });
  });
  it('is null without a record on or before 1 January', () => {
    expect(changeThisYear([r('2026-04-30', 1), r('2026-09-30', 2)], TODAY)).toBeNull();
  });
});

describe('averageMonthlyChange', () => {
  it('uses the record a year before the newest one when it exists', () => {
    const records = [r('2025-09-30', 330000), r('2025-10-01', 331000), r('2026-10-03', 450000)];
    const avg = averageMonthlyChange(records);
    expect(avg).toEqual({ perMonth: 10000, months: 12, sinceFirst: false, baseDay: day('2025-09-30') });
  });
  it('falls back to the oldest record and the elapsed months', () => {
    const records = [r('2025-11-01', 330000), r('2026-10-03', 450000)];
    expect(averageMonthlyChange(records)).toEqual({
      perMonth: Math.round(120000 / 11),
      months: 11,
      sinceFirst: true,
      baseDay: day('2025-11-01'),
    });
    expect(averageMonthlyChange([r('2026-10-03', 1)])).toBeNull();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/utils/balance-records.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`src/utils/balance-records.ts`:

```ts
/**
 * Pure helpers for the balance section of the bank account detail (spec
 * `docs/specs/2026-10-10-bank-account-detail-redesign-design.md`): the line
 * through dated balance records and the three balance stats. Days are UTC day
 * starts in unix seconds; amounts are in the account currency.
 */

export interface BalanceRecordLike {
  amount: string | number;
  /** UTC day start (unix seconds). */
  asOf: number;
}

export interface BalanceSeriesPoint {
  t: number;
  value: number;
  /** True on a record, false on the interpolated window-start point. */
  record: boolean;
}

const DAY = 86400;
const YEAR = 365 * DAY;
const AVG_MONTH_DAYS = 30.44;

const num = (v: string | number) => Number(v) || 0;

function ascending<T extends BalanceRecordLike>(records: readonly T[]): T[] {
  return [...records].sort((a, b) => a.asOf - b.asOf);
}

export function newestRecord<T extends BalanceRecordLike>(records: readonly T[]): T | null {
  const sorted = ascending(records);
  return sorted.length ? sorted[sorted.length - 1] : null;
}

export function previousRecord<T extends BalanceRecordLike>(records: readonly T[]): T | null {
  const sorted = ascending(records);
  return sorted.length > 1 ? sorted[sorted.length - 2] : null;
}

/** Latest record on or before `day` (step semantics), null before the first record. */
export function valueAt(records: readonly BalanceRecordLike[], day: number): number | null {
  let best: BalanceRecordLike | null = null;
  for (const r of records) if (r.asOf <= day && (!best || r.asOf > best.asOf)) best = r;
  return best ? num(best.amount) : null;
}

/**
 * Points of the balance line: the records of the last year (from the first record
 * when the history is shorter), entered at the window start by the interpolated value
 * when a record lies before it. Empty with fewer than two records.
 */
export function balanceSeries(
  records: readonly BalanceRecordLike[],
  today: number
): BalanceSeriesPoint[] {
  const sorted = ascending(records);
  if (sorted.length < 2) return [];
  const start = Math.max(sorted[0].asOf, today - YEAR);
  const inside = sorted.filter((r) => r.asOf >= start);
  const before = [...sorted].reverse().find((r) => r.asOf < start) ?? null;
  const points = inside.map((r) => ({ t: r.asOf, value: num(r.amount), record: true }));
  if (before && points.length && points[0].t !== start) {
    const next = inside[0];
    const share = (start - before.asOf) / (next.asOf - before.asOf);
    const value = Math.round(num(before.amount) + (num(next.amount) - num(before.amount)) * share);
    points.unshift({ t: start, value, record: false });
  }
  return points;
}

/** Newest record minus the value at 1 January of `today`'s UTC year; null without a base. */
export function changeThisYear(
  records: readonly BalanceRecordLike[],
  today: number
): { delta: number; baseDay: number } | null {
  const newest = newestRecord(records);
  if (!newest) return null;
  const jan1 = Date.UTC(new Date(today * 1000).getUTCFullYear(), 0, 1) / 1000;
  let base: BalanceRecordLike | null = null;
  for (const r of records) if (r.asOf <= jan1 && (!base || r.asOf > base.asOf)) base = r;
  if (!base) return null;
  return { delta: num(newest.amount) - num(base.amount), baseDay: base.asOf };
}

/**
 * Average change per month: against the latest record at least a year before the
 * newest one, else against the oldest record over the months elapsed (at least one).
 * Null with fewer than two records.
 */
export function averageMonthlyChange(
  records: readonly BalanceRecordLike[]
): { perMonth: number; months: number; sinceFirst: boolean; baseDay: number } | null {
  const sorted = ascending(records);
  if (sorted.length < 2) return null;
  const newest = sorted[sorted.length - 1];
  const cutoff = newest.asOf - YEAR;
  let base: BalanceRecordLike | null = null;
  for (const r of sorted) if (r.asOf <= cutoff) base = r;
  const sinceFirst = base === null;
  if (!base) base = sorted[0];
  if (base === newest) return null;
  const months = Math.max(1, Math.round((newest.asOf - base.asOf) / DAY / AVG_MONTH_DAYS));
  return {
    perMonth: Math.round((num(newest.amount) - num(base.amount)) / months),
    months,
    sinceFirst,
    baseDay: base.asOf,
  };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run src/utils/balance-records.test.ts`
Expected: PASS (if the interpolation rounding differs by 1 in the first series test, fix the expectation to use the same `Math.round` formula as the implementation, not the implementation).

- [ ] **Step 5: Commit**

```bash
npm run lint && npm run format
git add src/utils/balance-records.ts src/utils/balance-records.test.ts
git commit -m "feat(bank): balance series and stats helpers over dated records

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Flows window, section visibility and ledger default helpers

**Files:**
- Modify: `src/utils/bank-activity.ts` (add month helpers, `monthlyFlows`, `flowsWindow`, `hasRecentRows`, `detailSections`, `ledgerPresetForMonth`, `monthEnd`; the old reconstruction helpers stay until Task 15 deletes them together with their last caller, so `npm run typecheck` keeps passing on every commit)
- Modify: `src/utils/bank-activity.test.ts` (add the new tests; the old ones stay until Task 15)

**Interfaces:**
- Produces:
  - `monthOf(unixSeconds): number` (UTC month index `year*12 + month`), `monthStart(month): number`, `monthLabelDate(month): Date`
  - `interface MonthFlow { month: number; income: number; expense: number; count: number }`
  - `monthlyFlows(transactions, windowEnd): MonthFlow[]` — 12 entries ending with `windowEnd`, booked rows only
  - `newestMonthWithRows(transactions): number | null`, `oldestMonthWithRows(transactions): number | null`
  - `interface FlowsWindow { end: number; months: number[]; selected: number | null; canPrev: boolean; canNext: boolean }`
  - `flowsWindow(transactions, end, currentMonth, override?): FlowsWindow`
  - `hasRecentRows(transactions, currentMonth): boolean` (a booked row in the 12 months ending with `currentMonth`)
  - `detailSections({ balance, hasAnyTransaction, hasRecentRows }): { balance: boolean; flows: boolean; ledger: boolean }`
  - `ledgerPresetForMonth(month, currentMonth): 'thisMonth' | 'lastMonth' | 'custom'`

- [ ] **Step 1: Note the helpers Task 15 will delete**

```bash
grep -rn "reconstructBalance\|significantMovements\|movementThreshold\|movementFloor\|MOVEMENT_FLOOR_EUR\|MovementEvent\|BalancePoint" src --include='*.ts' --include='*.tsx' | grep -v "src/utils/bank-activity"
```

Expected: only `src/pages/BankAccountDetail.tsx`. Leave those helpers and their tests in place in this task (Task 15 removes them with the page rewrite).

- [ ] **Step 2: Write the failing tests**

In `src/utils/bank-activity.test.ts` add (keep the existing describe blocks):

```ts
import {
  detailSections,
  flowsWindow,
  hasRecentRows,
  ledgerPresetForMonth,
  monthOf,
  monthStart,
  monthlyFlows,
  newestMonthWithRows,
  oldestMonthWithRows,
} from './bank-activity';

const at = (iso: string) =>
  Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10)) / 1000;
const row = (
  id: string,
  type: 'credit' | 'debit',
  amount: number,
  iso: string,
  status: 'booked' | 'pending' = 'booked'
): FlowTransaction => ({ id, type, amount: String(amount), bookingDate: at(iso), status });

describe('month helpers', () => {
  it('indexes UTC months and gives their first day', () => {
    expect(monthOf(at('2026-10-10'))).toBe(2026 * 12 + 9);
    expect(monthStart(2026 * 12 + 9)).toBe(at('2026-10-01'));
    expect(monthStart(2026 * 12 + 0)).toBe(at('2026-01-01'));
  });
});

describe('monthlyFlows', () => {
  const rows = [
    row('a', 'credit', 54000, '2026-09-25'),
    row('b', 'debit', 1240, '2026-09-30'),
    row('c', 'debit', 23000, '2026-08-05'),
    row('p', 'debit', 999, '2026-09-29', 'pending'),
    row('old', 'debit', 5, '2025-01-15'),
  ];
  it('returns the 12 months ending with the window end, booked rows only', () => {
    const flows = monthlyFlows(rows, monthOf(at('2026-10-10')));
    expect(flows).toHaveLength(12);
    expect(flows[0].month).toBe(monthOf(at('2025-11-01')));
    expect(flows[11]).toEqual({ month: monthOf(at('2026-10-01')), income: 0, expense: 0, count: 0 });
    expect(flows[10]).toEqual({ month: monthOf(at('2026-09-01')), income: 54000, expense: 1240, count: 2 });
    expect(flows[9]).toEqual({ month: monthOf(at('2026-08-01')), income: 0, expense: 23000, count: 1 });
  });
  it('shifts with the window end', () => {
    const flows = monthlyFlows(rows, monthOf(at('2025-01-31')));
    expect(flows[11]).toEqual({ month: monthOf(at('2025-01-01')), income: 0, expense: 5, count: 1 });
  });
});

describe('flowsWindow', () => {
  const current = monthOf(at('2026-10-10'));
  const rows = [row('a', 'credit', 1, '2026-09-25'), row('old', 'debit', 1, '2025-03-15')];
  it('selects the newest month with rows inside the window and limits the arrows', () => {
    const w = flowsWindow(rows, current, current);
    expect(w.months).toHaveLength(12);
    expect(w.selected).toBe(monthOf(at('2026-09-01')));
    expect(w.canNext).toBe(false);
    expect(w.canPrev).toBe(true);
  });
  it('stops going back when the window reaches the oldest row', () => {
    const w = flowsWindow(rows, monthOf(at('2026-02-01')), current);
    expect(w.months[0]).toBe(monthOf(at('2025-03-01')));
    expect(w.canPrev).toBe(false);
    expect(w.canNext).toBe(true);
    expect(w.selected).toBe(monthOf(at('2025-03-01')));
  });
  it('honours an override inside the window and ignores one outside', () => {
    expect(flowsWindow(rows, current, current, monthOf(at('2026-06-01'))).selected).toBe(monthOf(at('2026-06-01')));
    expect(flowsWindow(rows, current, current, monthOf(at('2024-06-01'))).selected).toBe(monthOf(at('2026-09-01')));
  });
  it('has no selection without rows in the window', () => {
    expect(flowsWindow([], current, current).selected).toBeNull();
    expect(newestMonthWithRows([])).toBeNull();
    expect(oldestMonthWithRows(rows)).toBe(monthOf(at('2025-03-01')));
  });
});

describe('hasRecentRows / detailSections / ledgerPresetForMonth', () => {
  const current = monthOf(at('2026-10-10'));
  it('counts a booked row within the 12 months ending with the current one', () => {
    expect(hasRecentRows([row('a', 'credit', 1, '2025-11-01')], current)).toBe(true);
    expect(hasRecentRows([row('a', 'credit', 1, '2025-10-31')], current)).toBe(false);
    expect(hasRecentRows([row('a', 'credit', 1, '2026-10-01', 'pending')], current)).toBe(false);
  });
  it('shows sections by data', () => {
    expect(detailSections({ balance: 450000, hasAnyTransaction: false, hasRecentRows: false })).toEqual({ balance: true, flows: false, ledger: false });
    expect(detailSections({ balance: 0, hasAnyTransaction: true, hasRecentRows: true })).toEqual({ balance: false, flows: true, ledger: true });
    expect(detailSections({ balance: 84320, hasAnyTransaction: true, hasRecentRows: true })).toEqual({ balance: true, flows: true, ledger: true });
    expect(detailSections({ balance: 0, hasAnyTransaction: true, hasRecentRows: false })).toEqual({ balance: false, flows: false, ledger: true });
    expect(detailSections({ balance: 0, hasAnyTransaction: false, hasRecentRows: false })).toEqual({ balance: false, flows: false, ledger: false });
  });
  it('maps the newest month to a ledger preset', () => {
    expect(ledgerPresetForMonth(current, current)).toBe('thisMonth');
    expect(ledgerPresetForMonth(current - 1, current)).toBe('lastMonth');
    expect(ledgerPresetForMonth(current - 2, current)).toBe('custom');
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `npx vitest run src/utils/bank-activity.test.ts`
Expected: FAIL — the new exports do not exist.

- [ ] **Step 4: Implement**

Append to the end of `src/utils/bank-activity.ts` (the module doc comment gains one sentence: "The monthly flows window of the detail page and its section rules live here too."):

```ts
const WINDOW_MONTHS = 12;

/** UTC month index of a unix timestamp: `year * 12 + monthIndex`. */
export function monthOf(unixSeconds: number): number {
  const d = new Date(unixSeconds * 1000);
  return d.getUTCFullYear() * 12 + d.getUTCMonth();
}

/** First day (UTC, unix seconds) of a month index. */
export function monthStart(month: number): number {
  return Date.UTC(Math.floor(month / 12), month % 12, 1) / 1000;
}

/** A Date on the first day of the month, for `useFormat().month`. */
export function monthLabelDate(month: number): Date {
  return new Date(monthStart(month) * 1000);
}

export interface MonthFlow {
  month: number;
  income: number;
  expense: number;
  count: number;
}

const booked = (transactions: readonly FlowTransaction[]) =>
  transactions.filter((tx) => tx.status !== 'pending');

/** Credits and debits per month for the 12 months ending with `windowEnd` (booked rows only). */
export function monthlyFlows(
  transactions: readonly FlowTransaction[],
  windowEnd: number
): MonthFlow[] {
  const first = windowEnd - (WINDOW_MONTHS - 1);
  const flows: MonthFlow[] = Array.from({ length: WINDOW_MONTHS }, (_, i) => ({
    month: first + i,
    income: 0,
    expense: 0,
    count: 0,
  }));
  for (const tx of booked(transactions)) {
    const i = monthOf(tx.bookingDate) - first;
    if (i < 0 || i >= WINDOW_MONTHS) continue;
    if (tx.type === 'credit') flows[i].income += num(tx.amount);
    else flows[i].expense += num(tx.amount);
    flows[i].count += 1;
  }
  return flows;
}

export function newestMonthWithRows(transactions: readonly FlowTransaction[]): number | null {
  const rows = booked(transactions);
  return rows.length ? Math.max(...rows.map((tx) => monthOf(tx.bookingDate))) : null;
}

export function oldestMonthWithRows(transactions: readonly FlowTransaction[]): number | null {
  const rows = booked(transactions);
  return rows.length ? Math.min(...rows.map((tx) => monthOf(tx.bookingDate))) : null;
}

export interface FlowsWindow {
  end: number;
  months: number[];
  /** Month whose net the flows card shows; null when the window has no rows. */
  selected: number | null;
  canPrev: boolean;
  canNext: boolean;
}

/**
 * The 12-month window ending with `end`, the month shown in it (an override inside
 * the window, else the newest month with rows inside it) and whether the arrows can
 * move: never past `currentMonth`, never before the oldest row.
 */
export function flowsWindow(
  transactions: readonly FlowTransaction[],
  end: number,
  currentMonth: number,
  override?: number | null
): FlowsWindow {
  const first = end - (WINDOW_MONTHS - 1);
  const months = Array.from({ length: WINDOW_MONTHS }, (_, i) => first + i);
  const inside = booked(transactions).filter((tx) => {
    const m = monthOf(tx.bookingDate);
    return m >= first && m <= end;
  });
  const newest = newestMonthWithRows(inside);
  const selected =
    override != null && override >= first && override <= end ? override : newest;
  const oldest = oldestMonthWithRows(transactions);
  return {
    end,
    months,
    selected,
    canPrev: oldest !== null && first > oldest,
    canNext: end < currentMonth,
  };
}

/** A booked row in the 12 months ending with `currentMonth`. */
export function hasRecentRows(
  transactions: readonly FlowTransaction[],
  currentMonth: number
): boolean {
  const first = currentMonth - (WINDOW_MONTHS - 1);
  return booked(transactions).some((tx) => monthOf(tx.bookingDate) >= first);
}

export interface DetailSections {
  balance: boolean;
  flows: boolean;
  ledger: boolean;
}

/** Which sections the detail page shows (spec "Section combinations"). */
export function detailSections(args: {
  balance: number;
  hasAnyTransaction: boolean;
  hasRecentRows: boolean;
}): DetailSections {
  return {
    balance: args.balance !== 0,
    flows: args.hasRecentRows,
    ledger: args.hasAnyTransaction,
  };
}

/** The ledger preset that shows `month`: a named one for this and last month, else a custom range. */
export function ledgerPresetForMonth(
  month: number,
  currentMonth: number
): 'thisMonth' | 'lastMonth' | 'custom' {
  if (month === currentMonth) return 'thisMonth';
  if (month === currentMonth - 1) return 'lastMonth';
  return 'custom';
}

/** Last day (UTC, unix seconds, 23:59:59) of a month index. */
export function monthEnd(month: number): number {
  return monthStart(month + 1) - 1;
}
```

(`num` is the existing `const num = (v) => Math.abs(Number(v) || 0)` at the top of the file.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npx vitest run src/utils/bank-activity.test.ts`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
npm run lint && npm run format
git add src/utils/bank-activity.ts src/utils/bank-activity.test.ts
git commit -m "feat(bank): monthly flows window and detail section helpers

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: `BalanceRecordDialog`

**Files:**
- Create: `src/components/bank-accounts/BalanceRecordDialog.tsx`
- Modify: `src/i18n/locales/en/bank_accounts.json`, `src/i18n/locales/cs/bank_accounts.json`

**Interfaces:**
- Consumes: `useBalanceRecordMutations` (Task 6), `BankBalanceRecord`.
- Produces: `<BalanceRecordDialog open onOpenChange account={{ id, currency, balance, balanceAsOf }} records />`.

- [ ] **Step 1: Locale keys**

`en/bank_accounts.json`, extend `recordDialog`:

```json
  "recordDialog": {
    "title": "Record balance",
    "description": "The balance is stored in the account's history for the chosen day.",
    "amount": "Amount",
    "amountPlaceholder": "e.g. 450 000",
    "asOf": "As of",
    "hint": "The balance from your statement or online banking. Imported transactions do not change it.",
    "replaces": "The record of {{date}} will be replaced.",
    "confirmUnknown": "Replace the current balance that has no date",
    "submit": "Record balance",
    "success": "Balance recorded"
  }
```

`cs/bank_accounts.json`:

```json
  "recordDialog": {
    "title": "Zaznamenat zůstatek",
    "description": "Zůstatek se uloží do historie účtu k vybranému dni.",
    "amount": "Částka",
    "amountPlaceholder": "např. 450 000",
    "asOf": "K datu",
    "hint": "Zůstatek z výpisu nebo z bankovnictví. Importované transakce ho nemění.",
    "replaces": "Záznam ze dne {{date}} bude nahrazen.",
    "confirmUnknown": "Nahradit současný zůstatek bez data",
    "submit": "Zaznamenat zůstatek",
    "success": "Zůstatek zaznamenán"
  }
```

- [ ] **Step 2: Component**

`src/components/bank-accounts/BalanceRecordDialog.tsx`:

```tsx
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { BankBalanceRecord } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useBalanceRecordMutations } from '@/hooks/use-balance-records';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { isoDateFromUtcTimestamp, todayIsoUtc, utcDayStart } from '@/utils/period';

interface BalanceRecordDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  account: { id: string; currency: string; balance: string; balanceAsOf: number | null };
  records: readonly BankBalanceRecord[];
}

/** "Zaznamenat zůstatek": one dated balance; the same day replaces the earlier record. */
export function BalanceRecordDialog({ open, onOpenChange, account, records }: BalanceRecordDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { recordBalance } = useBalanceRecordMutations(account.id);
  const [amount, setAmount] = useState('');
  const [asOf, setAsOf] = useState(todayIsoUtc());
  const [confirmUnknown, setConfirmUnknown] = useState(false);
  const [submitted, setSubmitted] = useState(false);

  // Reset the form each time the dialog opens (render-time state, no effect)
  const [openedFor, setOpenedFor] = useState<boolean>(false);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setAmount('');
      setAsOf(todayIsoUtc());
      setConfirmUnknown(false);
      setSubmitted(false);
    }
  }

  const currency = recordCurrency(account.currency);
  const replaces = records.find((r) => isoDateFromUtcTimestamp(r.asOf) === asOf) ?? null;
  const needsConfirm =
    records.length === 0 && account.balanceAsOf == null && Number(account.balance) !== 0;
  const amountError =
    amount.trim() === '' || !Number.isFinite(Number(amount)) ? tc('validation.balanceRequired') : null;

  const submit = () => {
    setSubmitted(true);
    if (amountError || !asOf) return;
    recordBalance.mutate(
      {
        amount: amount.trim(),
        currency,
        asOf: utcDayStart(asOf),
        confirmUnknownAsOf: needsConfirm ? confirmUnknown : undefined,
      },
      { onSuccess: () => onOpenChange(false) }
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('recordDialog.title')}</DialogTitle>
          <DialogDescription>{t('recordDialog.description')}</DialogDescription>
        </DialogHeader>
        <div className="grid grid-cols-[1fr_172px] gap-4">
          <div className="grid gap-2">
            <Label htmlFor="balance-record-amount">{t('recordDialog.amount')}</Label>
            <InputWrap unit={currency}>
              <Input
                id="balance-record-amount"
                type="number"
                step="0.01"
                inputMode="decimal"
                value={amount}
                placeholder={t('recordDialog.amountPlaceholder')}
                onChange={(e) => setAmount(e.target.value)}
                aria-invalid={submitted && amountError ? true : undefined}
                autoFocus
              />
            </InputWrap>
            {submitted && amountError ? (
              <p role="alert" className="text-micro font-600 text-loss">
                {amountError}
              </p>
            ) : (
              <p className="text-micro font-500 text-ink-4">{t('recordDialog.hint')}</p>
            )}
          </div>
          <div className="grid gap-2">
            <Label htmlFor="balance-record-asof">{t('recordDialog.asOf')}</Label>
            <Input
              id="balance-record-asof"
              type="date"
              value={asOf}
              max={todayIsoUtc()}
              onChange={(e) => setAsOf(e.target.value)}
            />
          </div>
        </div>
        {replaces && (
          <p className="flex items-center gap-[5px] text-micro font-500 text-ink-3">
            <Info className="size-3.5" aria-hidden />
            {t('recordDialog.replaces', { date: fmt.day(replaces.asOf) })}
          </p>
        )}
        {needsConfirm && (
          <label className="flex items-center gap-2 text-table text-ink-2">
            <Checkbox checked={confirmUnknown} onCheckedChange={(v) => setConfirmUnknown(v === true)} />
            {t('recordDialog.confirmUnknown')}
          </label>
        )}
        {submitted && recordBalance.isError && (
          <p role="alert" className="text-micro font-600 text-loss">
            {translateApiError(recordBalance.error, tc)}
          </p>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button onClick={submit} disabled={recordBalance.isPending} loading={recordBalance.isPending}>
            {t('recordDialog.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

If `Checkbox`'s `onCheckedChange` has a different name in `src/components/ui/checkbox.tsx` (it is a Radix checkbox: `onCheckedChange(checked: boolean | 'indeterminate')`), use that; if `Button` has no `loading` prop in `src/components/ui/button.tsx`, drop it and keep `disabled`.

- [ ] **Step 3: Gates and commit**

```bash
npm run lint && npm run format && npx vitest run
git add src/components/bank-accounts/BalanceRecordDialog.tsx src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/bank_accounts.json
git commit -m "feat(bank): record balance dialog

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: `BalanceHistoryDialog`

**Files:**
- Create: `src/components/bank-accounts/BalanceHistoryDialog.tsx`
- Modify: both `bank_accounts.json` locale files

**Interfaces:**
- Consumes: `useBalanceRecordMutations`, `ConfirmDeleteDialog`, `previousRecord`-style ordering (records arrive newest first from the API).
- Produces: `<BalanceHistoryDialog open onOpenChange accountId currency records />`.

- [ ] **Step 1: Locale keys**

`en/bank_accounts.json`, extend `balanceHistory`:

```json
  "balanceHistory": {
    "title": "Balance history",
    "sub_one": "{{count}} record · latest {{date}}",
    "sub_other": "{{count}} records · latest {{date}}",
    "empty": "No balance record yet.",
    "amount": "Amount",
    "change": "Change",
    "source": "Source",
    "sourceManual": "by hand",
    "sourceStatement": "from statement",
    "delete": "Delete record",
    "deleteTitle": "Delete this balance record?",
    "deleteDescription": "The record of {{amount}} as of {{date}} will be removed for good. The account balance is recalculated from the newest remaining record.",
    "deleted": "Record deleted"
  }
```

`cs/bank_accounts.json`:

```json
  "balanceHistory": {
    "title": "Historie zůstatku",
    "sub_one": "{{count}} záznam · poslední {{date}}",
    "sub_few": "{{count}} záznamy · poslední {{date}}",
    "sub_other": "{{count}} záznamů · poslední {{date}}",
    "empty": "Zatím žádný záznam zůstatku.",
    "amount": "Částka",
    "change": "Změna",
    "source": "Zdroj",
    "sourceManual": "ručně",
    "sourceStatement": "z výpisu",
    "delete": "Smazat záznam",
    "deleteTitle": "Smazat záznam zůstatku?",
    "deleteDescription": "Záznam {{amount}} ze dne {{date}} bude trvale odstraněn. Zůstatek účtu se přepočítá z nejnovějšího zbývajícího záznamu.",
    "deleted": "Záznam smazán"
  }
```

- [ ] **Step 2: Component**

`src/components/bank-accounts/BalanceHistoryDialog.tsx`:

```tsx
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Trash2 } from 'lucide-react';
import type { BankBalanceRecord } from '@shared/schema';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { useBalanceRecordMutations } from '@/hooks/use-balance-records';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';

interface BalanceHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accountId: string;
  currency: string;
  /** Newest first (as the API returns them). */
  records: readonly BankBalanceRecord[];
}

/** Every dated balance of the account with its change against the previous one. */
export function BalanceHistoryDialog({ open, onOpenChange, accountId, currency, records }: BalanceHistoryDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { deleteRecord } = useBalanceRecordMutations(accountId);
  const [pending, setPending] = useState<BankBalanceRecord | null>(null);
  const money = (v: number, signed = false) => fmt.money(v, currency, { signed, decimals: 0 });

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('balanceHistory.title')}</DialogTitle>
            <DialogDescription>
              {records.length
                ? t('balanceHistory.sub', { count: records.length, date: fmt.day(records[0].asOf) })
                : t('balanceHistory.empty')}
            </DialogDescription>
          </DialogHeader>
          {records.length > 0 && (
            <div className="-mx-[27px] max-h-[420px] overflow-y-auto border-t border-line">
              <Table>
                <TableHeader className="sticky top-0 z-[1] bg-well">
                  <TableRow>
                    <TableHead>{tc('labels.date')}</TableHead>
                    <TableHead className="text-right">{t('balanceHistory.amount')}</TableHead>
                    <TableHead className="text-right">{t('balanceHistory.change')}</TableHead>
                    <TableHead>{t('balanceHistory.source')}</TableHead>
                    <TableHead className="w-10">
                      <span className="sr-only">{tc('labels.actions')}</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {records.map((r, i) => {
                    const prev = records[i + 1];
                    const delta = prev ? Number(r.amount) - Number(prev.amount) : null;
                    return (
                      <TableRow key={r.id} className="h-[46px]">
                        <TableCell>{fmt.day(r.asOf)}</TableCell>
                        <TableCell className="text-right font-650 text-ink num">{money(Number(r.amount))}</TableCell>
                        <TableCell className={cn('text-right num', delta != null && delta > 0 && 'text-gain', delta != null && delta < 0 && 'text-loss')}>
                          {delta == null ? '—' : money(delta, true)}
                        </TableCell>
                        <TableCell>
                          <Badge variant="outline">
                            {t(r.source === 'statement' ? 'balanceHistory.sourceStatement' : 'balanceHistory.sourceManual')}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-right">
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <Button variant="ghost" size="icon-sm" className="row-actions" aria-label={tc('labels.moreActions')}>
                                <Ellipsis />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                              <DropdownMenuItem variant="danger" onSelect={() => setPending(r)}>
                                <Trash2 />
                                {t('balanceHistory.delete')}
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDeleteDialog
        open={pending !== null}
        onOpenChange={(o) => {
          if (!o) setPending(null);
        }}
        title={t('balanceHistory.deleteTitle')}
        description={
          pending
            ? t('balanceHistory.deleteDescription', { amount: money(Number(pending.amount)), date: fmt.day(pending.asOf) })
            : ''
        }
        onConfirm={() => pending && deleteRecord.mutate(pending.id, { onSuccess: () => setPending(null) })}
        isPending={deleteRecord.isPending}
        confirmLabel={t('balanceHistory.delete')}
      />
    </>
  );
}
```

Check the exact names of `DropdownMenuItem`'s `variant` prop and `Button`'s `size="icon-sm"` in `src/components/ui/` (both are used by `BankAccountDetail.tsx` today: `<DropdownMenuItem variant="danger">` and `size="icon-sm"`).

- [ ] **Step 3: Gates and commit**

```bash
npm run lint && npm run format && npx vitest run
git add src/components/bank-accounts/BalanceHistoryDialog.tsx src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/bank_accounts.json
git commit -m "feat(bank): balance history modal with record delete

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 11: `ImportHistoryDialog` and `InterestDialog`

**Files:**
- Create: `src/components/bank-accounts/ImportHistoryDialog.tsx` (the import-history table moved out of the page)
- Create: `src/components/bank-accounts/InterestDialog.tsx`
- Delete: `src/components/bank-accounts/ZonesInfoModal.tsx` if `grep -rn "ZonesInfoModal" src` finds no caller besides the file itself
- Modify: both `bank_accounts.json` locale files

**Interfaces:**
- Produces: `<ImportHistoryDialog open onOpenChange batches onUndo={(batch) => void} />` (the page keeps its `pendingUndo` confirm dialog and `deleteBatchMutation`), `<InterestDialog open onOpenChange account={{ interestRate, hasZoneDesignation, interestRateValidUntil }} zones balance currency />`.

- [ ] **Step 1: Locale keys**

`en/bank_accounts.json`, add:

```json
  "interest": {
    "title": "Interest",
    "zonedSub": "Tiered interest by balance",
    "simpleSub": "Simple interest",
    "effectiveLine": "{{rate}} effective · {{amount}} a year",
    "caption": "The effective rate is the yearly interest on the current balance divided by the balance.",
    "validUntil": "Rate valid until {{date}}",
    "rate": "Rate"
  }
```

`cs/bank_accounts.json`:

```json
  "interest": {
    "title": "Úročení",
    "zonedSub": "Pásmové úročení podle výše zůstatku",
    "simpleSub": "Jednoduché úročení",
    "effectiveLine": "{{rate}} efektivně · {{amount}} ročně",
    "caption": "Efektivní sazba je roční úrok z aktuálního zůstatku dělený zůstatkem.",
    "validUntil": "Sazba platí do {{date}}",
    "rate": "Sazba"
  }
```

- [ ] **Step 2: `ImportHistoryDialog`**

Move the markup of the `importHistoryOpen && (<Card variant="table" …>` block of `BankAccountDetail.tsx` (the table with Datum · Soubor · Importováno · Duplicity · Chyby · undo button) into:

```tsx
import { useTranslation } from 'react-i18next';
import { Undo2 } from 'lucide-react';
import type { CsvImportBatch } from '@shared/schema';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { useFormat } from '@/lib/use-format';

interface ImportHistoryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  batches: readonly CsvImportBatch[];
  /** Opens the page's undo confirmation for one batch. */
  onUndo: (batch: CsvImportBatch) => void;
}

/** Every CSV import of the account (date, file, counts) with the undo action; wide like the import wizard. */
export function ImportHistoryDialog({ open, onOpenChange, batches, onUndo }: ImportHistoryDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const newest = batches.reduce<number | null>((max, b) => (max === null || b.importedAt > max ? b.importedAt : max), null);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[840px]">
        <DialogHeader>
          <DialogTitle>{t('importHistory.title')}</DialogTitle>
          <DialogDescription>
            {newest !== null ? t('status.lastImport', { date: fmt.day(newest) }) : t('importHistory.empty')}
          </DialogDescription>
        </DialogHeader>
        {batches.length > 0 && (
          <div className="-mx-[27px] max-h-[420px] overflow-y-auto border-t border-line">
            <Table>
              <TableHeader className="sticky top-0 z-[1] bg-well">
                <TableRow>
                  <TableHead>{tc('labels.date')}</TableHead>
                  <TableHead>{t('importHistory.fileName')}</TableHead>
                  <TableHead>{t('importHistory.period')}</TableHead>
                  <TableHead className="text-right">{t('importHistory.imported')}</TableHead>
                  <TableHead className="text-right" title={t('importHistory.duplicatesHelp')}>
                    {t('importHistory.duplicates')}
                  </TableHead>
                  <TableHead className="text-right">{t('importHistory.errors')}</TableHead>
                  <TableHead className="w-44">
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {batches.map((batch) => (
                  <TableRow key={batch.id} className="h-[50px]">
                    <TableCell>{fmt.dateTime(batch.importedAt)}</TableCell>
                    <TableCell className="max-w-[220px] truncate font-650 text-ink">{batch.fileName}</TableCell>
                    <TableCell>
                      {batch.dateRange
                        ? `${fmt.day(batch.dateRange.from)} – ${fmt.day(batch.dateRange.to ?? batch.dateRange.from)}`
                        : t('readiness.unknownDate')}
                    </TableCell>
                    <TableCell className="text-right num">{batch.importedCount}</TableCell>
                    <TableCell className="text-right num">{batch.duplicateCount}</TableCell>
                    <TableCell className="text-right num">{batch.errorCount}</TableCell>
                    <TableCell className="text-right">
                      <Button variant="danger" size="sm" onClick={() => onUndo(batch)}>
                        <Undo2 />
                        {t(batch.linkedCount ? 'readiness.deleteImported' : 'readiness.deleteRecord')}
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

Add `"period": "Period"` to `importHistory` in `en/bank_accounts.json` and `"period": "Období"` in `cs/bank_accounts.json`.

- [ ] **Step 3: `InterestDialog`**

```tsx
import { useTranslation } from 'react-i18next';
import type { SavingsAccountZone } from '@shared/schema';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { calculateZonedInterest } from '@/utils/bank-account-zones';

interface InterestDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  account: { interestRate: string | null; hasZoneDesignation: boolean; interestRateValidUntil: number | null };
  zones: readonly SavingsAccountZone[];
  /** Current balance in the account currency. */
  balance: number;
  currency: string;
}

/** Interest tiers (or the simple rate) with the effective rate on the current balance. */
export function InterestDialog({ open, onOpenChange, account, zones, balance, currency }: InterestDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const zoned = account.hasZoneDesignation && zones.length > 0;
  const simpleRate = parseFloat(account.interestRate ?? '0') || 0;
  const yearly = zoned ? calculateZonedInterest(balance, zones) : balance * (simpleRate / 100);
  const effective = balance > 0 ? yearly / balance : zoned ? 0 : simpleRate / 100;
  const money = (v: number) => fmt.money(v, currency, { decimals: 0 });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('interest.title')}</DialogTitle>
          <DialogDescription>{t(zoned ? 'interest.zonedSub' : 'interest.simpleSub')}</DialogDescription>
        </DialogHeader>
        <div>
          {zoned ? (
            zones.map((zone) => (
              <div key={zone.id} className="grid grid-cols-[1fr_auto] gap-x-4 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-0">
                <span className="num">
                  {money(parseFloat(zone.fromAmount || '0'))}
                  {' – '}
                  {zone.toAmount && parseFloat(zone.toAmount) !== 0 ? money(parseFloat(zone.toAmount)) : t('zones.unlimited')}
                </span>
                <b className="font-600 text-ink num">{fmt.percent(parseFloat(zone.interestRate || '0') / 100, 2)}</b>
              </div>
            ))
          ) : (
            <div className="grid grid-cols-[1fr_auto] gap-x-4 py-[9px] text-table text-ink-3">
              <span>{t('interest.rate')}</span>
              <b className="font-600 text-ink num">{fmt.percent(simpleRate / 100, 2)}</b>
            </div>
          )}
          <p className="mt-3 text-table font-600 text-ink">
            {t('interest.effectiveLine', { rate: fmt.percent(effective, 2), amount: money(yearly) })}
          </p>
          {account.interestRateValidUntil && (
            <p className="mt-1 text-micro font-500 text-ink-4">
              {t('interest.validUntil', { date: fmt.day(account.interestRateValidUntil) })}
            </p>
          )}
          <p className="mt-3 text-micro font-500 text-ink-4">{t('interest.caption')}</p>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 4: Remove the dead zones modal if unused**

```bash
grep -rn "ZonesInfoModal" src --include='*.tsx' --include='*.ts'
```

If only `src/components/bank-accounts/ZonesInfoModal.tsx` matches, `git rm` it and add the row `| `src/components/bank-accounts/ZonesInfoModal.tsx` | Unused since the zones moved to the detail page; replaced by the Úročení modal | `src/components/bank-accounts/InterestDialog.tsx` |` to the removed-code table in `docs/DEPRECATED.md`.

- [ ] **Step 5: Gates and commit**

```bash
npm run lint && npm run format && npx vitest run
git add -A src/components/bank-accounts src/i18n/locales docs/DEPRECATED.md
git commit -m "feat(bank): import history and interest modals

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 12: Bar click on `MoonyBarChart` and the `FlowsCard`

**Files:**
- Modify: `src/components/charts/MoonyBarChart.tsx` (optional `onBarClick`)
- Create: `src/components/bank-accounts/FlowsCard.tsx`
- Modify: both `bank_accounts.json` locale files

**Interfaces:**
- Consumes: `monthlyFlows`, `flowsWindow`, `monthLabelDate` (Task 8), `MoonyBarChart`, `ChartLegend`, `HeroCard`/`HeroValue`.
- Produces: `<FlowsCard transactions currency currentMonth windowEnd onWindowEndChange selectedOverride onSelect hero formatValue notes />`.

- [ ] **Step 1: `onBarClick` on the bar chart**

In `src/components/charts/MoonyBarChart.tsx` add to `MoonyBarChartProps`:

```ts
  /** Called with the bar index when a bar (or its slot) is clicked. */
  onBarClick?: (index: number) => void;
```

destructure it in the component and pass to Recharts:

```tsx
          <BarChart
            // …existing props…
            onClick={(state) => {
              const index = (state as { activeTooltipIndex?: number } | null)?.activeTooltipIndex;
              if (onBarClick && typeof index === 'number') onBarClick(index);
            }}
            style={onBarClick ? { cursor: 'pointer' } : undefined}
          >
```

(Keep every existing prop of `<BarChart>`; only add these two.)

- [ ] **Step 2: Locale keys**

`en/bank_accounts.json`, add:

```json
  "flows": {
    "netIn": "Net · {{month}}",
    "line_one": "{{credits}} in · {{debits}} out · {{count}} movement",
    "line_other": "{{credits}} in · {{debits}} out · {{count}} movements",
    "noMoves": "no movements",
    "prev": "Previous month",
    "next": "Next month",
    "income": "Incoming",
    "expense": "Outgoing",
    "balance": "net",
    "noImport": "no import"
  }
```

`cs/bank_accounts.json`:

```json
  "flows": {
    "netIn": "Bilance · {{month}}",
    "line_one": "{{credits}} příchozí · {{debits}} odchozí · {{count}} pohyb",
    "line_few": "{{credits}} příchozí · {{debits}} odchozí · {{count}} pohyby",
    "line_other": "{{credits}} příchozí · {{debits}} odchozí · {{count}} pohybů",
    "noMoves": "bez pohybů",
    "prev": "Předchozí měsíc",
    "next": "Další měsíc",
    "income": "Příchozí",
    "expense": "Odchozí",
    "balance": "bilance",
    "noImport": "bez importu"
  }
```

- [ ] **Step 3: Component**

`src/components/bank-accounts/FlowsCard.tsx`:

```tsx
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { Card } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { MoonyBarChart, type BarGroup } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { flowsWindow, monthLabelDate, monthlyFlows, type FlowTransaction } from '@/utils/bank-activity';

interface FlowsCardProps {
  /** Booked rows in the account currency (converted by the caller). */
  transactions: readonly FlowTransaction[];
  currency: string;
  currentMonth: number;
  /** Month index the 12-month window ends with. */
  windowEnd: number;
  onWindowEndChange: (end: number) => void;
  /** A month the user clicked; null = the newest month with rows in the window. */
  selectedOverride: number | null;
  onSelect: (month: number | null) => void;
  /** True when this is the first card on the page: 42 px value and the hero material. */
  hero: boolean;
  /** Readiness notes (converted / missing currencies) rendered under the chart. */
  notes?: ReactNode;
}

/** Monthly credits and debits of the account in a 12-month window shifted by arrows. */
export function FlowsCard({ transactions, currency, currentMonth, windowEnd, onWindowEndChange, selectedOverride, onSelect, hero, notes }: FlowsCardProps) {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const win = flowsWindow(transactions, windowEnd, currentMonth, selectedOverride);
  const flows = monthlyFlows(transactions, windowEnd);
  const selected = win.selected !== null ? flows.find((f) => f.month === win.selected) ?? null : null;
  const money = (v: number, signed = false) => fmt.money(v, currency, { signed, decimals: 0 });
  const monthName = (month: number) => fmt.month(monthLabelDate(month), { month: 'long', year: 'numeric' });
  const bars: BarGroup[] = flows.map((f) => ({
    label: fmt.month(monthLabelDate(f.month), { month: 'short', year: '2-digit' }),
    a: f.income,
    b: f.expense,
    muted: win.selected !== null && f.month !== win.selected,
  }));
  const net = selected ? selected.income - selected.expense : 0;

  const arrows = (
    <div className="flex items-center gap-1">
      <Button variant="outline" size="icon-sm" aria-label={t('flows.prev')} disabled={!win.canPrev}
        onClick={() => { onSelect(null); onWindowEndChange(windowEnd - 1); }}>
        <ChevronLeft />
      </Button>
      <Button variant="outline" size="icon-sm" aria-label={t('flows.next')} disabled={!win.canNext}
        onClick={() => { onSelect(null); onWindowEndChange(windowEnd + 1); }}>
        <ChevronRight />
      </Button>
    </div>
  );

  const value = (
    <HeroValue
      compact={!hero}
      label={win.selected !== null ? t('flows.netIn', { month: monthName(win.selected) }) : t('flows.noMoves')}
      value={selected ? money(net, true) : '—'}
      tone={net > 0 ? 'gain' : net < 0 ? 'loss' : 'neutral'}
      deltaNote={
        selected && selected.count > 0
          ? t('flows.line', { credits: money(selected.income, true), debits: money(-selected.expense, true), count: selected.count })
          : t('flows.noMoves')
      }
    />
  );

  const chart = (
    <>
      <MoonyBarChart
        className="-mx-1 mt-[18px]"
        bars={bars}
        names={[t('flows.income'), t('flows.expense')]}
        height={180}
        formatValue={(v) => money(v)}
        renderExtra={(bar) => `${t('flows.balance')} ${money(bar.a - bar.b, true)}`}
        onBarClick={(i) => onSelect(flows[i]?.month ?? null)}
      />
      <ChartLegend
        items={[
          { label: t('flows.income'), swatch: { kind: 'block', color: 'var(--s1)' } },
          { label: t('flows.expense'), swatch: { kind: 'block', color: 'var(--s3)' } },
        ]}
      />
      {notes}
    </>
  );

  if (hero) {
    return (
      <HeroCard aside={arrows} chart={chart}>
        {value}
      </HeroCard>
    );
  }
  return (
    <Card className={cn('px-6 pb-4 pt-5')}>
      <div className="flex items-start justify-between gap-6">
        {value}
        {arrows}
      </div>
      {chart}
    </Card>
  );
}
```

Empty months: `MoonyBarChart` draws nothing for a 0/0 pair; the "bez importu" caption is added as a `BarMark`-free label: pass `marks={flows.map((f, index) => f.count === 0 ? { index, title: t('flows.noImport') } : null).filter(Boolean)}` only if `BarMark` renders a readable label at the baseline (check `MoonyBarChart.tsx`'s mark rendering; if it draws a ring on top of the bar, skip the marks and instead render the caption in the tooltip: `renderExtra={(bar) => bar.a === 0 && bar.b === 0 ? t('flows.noImport') : …}`). Prefer the tooltip variant when in doubt; note the choice in the commit message.

- [ ] **Step 4: Gates and commit**

```bash
npm run lint && npm run format && npx vitest run
git add src/components/charts/MoonyBarChart.tsx src/components/bank-accounts/FlowsCard.tsx src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/bank_accounts.json
git commit -m "feat(bank): flows card with a 12-month window shifted by arrows

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 13: `BalanceSection` (hero with the record line and three stats)

**Files:**
- Create: `src/components/bank-accounts/BalanceSection.tsx`
- Modify: both `bank_accounts.json` locale files

**Interfaces:**
- Consumes: `balanceSeries`, `newestRecord`, `previousRecord`, `changeThisYear`, `averageMonthlyChange` (Task 7), `calculateZonedInterest` (`src/utils/bank-account-zones.ts`), `MoonyLineChart`, `ChartLegend`, `HeroCard`, `HeroValue`, `NativeBalance`, `Stat`, `Stats`.
- Produces: `<BalanceSection account records zones today currency />`.

- [ ] **Step 1: Locale keys**

`en/bank_accounts.json`, add:

```json
  "balanceHero": {
    "label": "Balance as of {{date}}",
    "unknownDate": "Balance · unknown date",
    "since": "since {{date}}",
    "firstRecord": "first record",
    "record": "Balance record",
    "recordTip": "{{date}} · {{source}}",
    "today": "Today"
  },
  "balanceStats": {
    "changeThisYear": "Change this year",
    "vsRecordAt": "against the record of {{date}}",
    "noRecordBeforeYear": "no record before the start of the year",
    "avgPerMonth": "Average per month",
    "last12Months": "over the last 12 months",
    "sinceFirstRecord_one": "over {{count}} month since the first record",
    "sinceFirstRecord_other": "over {{count}} months since the first record",
    "singleRecord": "only one record so far",
    "yearlyInterest": "Yearly interest",
    "effectiveZoned": "{{rate}} effective · tiered interest",
    "simpleRate": "{{rate}} p.a.",
    "noInterest": "no interest"
  }
```

`cs/bank_accounts.json`:

```json
  "balanceHero": {
    "label": "Zůstatek k {{date}}",
    "unknownDate": "Zůstatek · datum neznámé",
    "since": "od {{date}}",
    "firstRecord": "první záznam",
    "record": "Záznam zůstatku",
    "recordTip": "{{date}} · {{source}}",
    "today": "Dnes"
  },
  "balanceStats": {
    "changeThisYear": "Změna letos",
    "vsRecordAt": "proti záznamu k {{date}}",
    "noRecordBeforeYear": "bez záznamu před začátkem roku",
    "avgPerMonth": "Průměrně za měsíc",
    "last12Months": "za posledních 12 měsíců",
    "sinceFirstRecord_one": "za {{count}} měsíc od prvního záznamu",
    "sinceFirstRecord_few": "za {{count}} měsíce od prvního záznamu",
    "sinceFirstRecord_other": "za {{count}} měsíců od prvního záznamu",
    "singleRecord": "zatím jediný záznam",
    "yearlyInterest": "Roční úrok",
    "effectiveZoned": "{{rate}} efektivně · pásmové úročení",
    "simpleRate": "{{rate}} p. a.",
    "noInterest": "bez úročení"
  }
```

- [ ] **Step 2: Component**

`src/components/bank-accounts/BalanceSection.tsx`:

```tsx
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { BankBalanceRecord, SavingsAccountZone } from '@shared/schema';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { NativeBalance } from '@/components/bank-accounts/NativeBalance';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import type { ChartEvent } from '@/utils/chart-scale';
import { useFormat } from '@/lib/use-format';
import { calculateZonedInterest } from '@/utils/bank-account-zones';
import { averageMonthlyChange, balanceSeries, changeThisYear, newestRecord, previousRecord } from '@/utils/balance-records';

type RecordEvent = ChartEvent & { record: BankBalanceRecord };

interface BalanceSectionProps {
  account: {
    balance: string;
    balanceAsOf: number | null;
    interestRate: string | null;
    hasZoneDesignation: boolean;
  };
  records: readonly BankBalanceRecord[];
  zones: readonly SavingsAccountZone[];
  /** UTC day start of today (unix seconds). */
  today: number;
  currency: string;
  /** Formats a value in the account currency (0 decimals). */
  formatValue: (value: number) => string;
}

/** Hero with the balance of the newest record, the line through the records of the last year, and the three balance stats. */
export function BalanceSection({ account, records, zones, today, currency, formatValue }: BalanceSectionProps) {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const balance = parseFloat(account.balance || '0');
  const newest = newestRecord(records);
  const previous = previousRecord(records);
  const signed = (v: number) => fmt.money(v, currency, { signed: true, decimals: 0 });

  const points = useMemo(() => balanceSeries(records, today).map((p) => ({ t: p.t, value: p.value })), [records, today]);
  const events = useMemo<RecordEvent[]>(
    () =>
      balanceSeries(records, today)
        .filter((p) => p.record)
        .map((p) => {
          const record = records.find((r) => r.asOf === p.t)!;
          return { id: record.id, t: p.t, type: 'mark' as const, record };
        }),
    [records, today]
  );
  // The axis runs to today while the line stops at the newest record: the gap is the staleness
  const pointsToToday = points.length && points[points.length - 1].t < today ? [...points] : points;

  const delta = newest && previous ? parseFloat(newest.amount) - parseFloat(previous.amount) : null;
  const thisYear = changeThisYear(records, today);
  const avg = averageMonthlyChange(records);
  const zoned = account.hasZoneDesignation && zones.length > 0;
  const simpleRate = parseFloat(account.interestRate ?? '0') || 0;
  const yearlyInterest = zoned ? calculateZonedInterest(balance, zones) : balance * (simpleRate / 100);
  const hasInterest = zoned || simpleRate > 0;
  const effective = balance > 0 ? yearlyInterest / balance : 0;

  return (
    <>
      <HeroCard
        chart={
          points.length >= 2 ? (
            <>
              <MoonyLineChart<RecordEvent>
                className="-mx-1 mt-[18px]"
                points={pointsToToday}
                height={180}
                formatValue={formatValue}
                renderTip={(p) => ({ title: formatValue(p.value), lines: [fmt.day(p.t)] })}
                events={events}
                renderEventTip={(cluster) => ({
                  title: formatValue(parseFloat(cluster.events[0].record.amount)),
                  lines: [
                    t('balanceHero.recordTip', {
                      date: fmt.day(cluster.events[0].t),
                      source: t(cluster.events[0].record.source === 'statement' ? 'balanceHistory.sourceStatement' : 'balanceHistory.sourceManual'),
                    }),
                  ],
                })}
                axisEnd={today}
              />
              <ChartLegend
                items={[
                  { label: t('chart.balance'), swatch: { kind: 'line' } },
                  { label: t('balanceHero.record'), swatch: { kind: 'event', type: 'mark' } },
                ]}
              />
            </>
          ) : undefined
        }
      >
        <HeroValue
          label={newest ? t('balanceHero.label', { date: fmt.day(newest.asOf) }) : account.balanceAsOf ? t('balanceHero.label', { date: fmt.day(account.balanceAsOf) }) : t('balanceHero.unknownDate')}
          value={<NativeBalance amount={balance} currency={currency} align="start" className="" secondaryClassName="ml-2 text-table font-500 text-ink-4" />}
          tone={delta === null ? 'neutral' : delta > 0 ? 'gain' : delta < 0 ? 'loss' : 'neutral'}
          delta={delta === null ? undefined : `${delta > 0 ? '↗' : delta < 0 ? '↘' : '→'} ${signed(delta)}`}
          deltaNote={delta === null ? (newest ? t('balanceHero.firstRecord') : undefined) : t('balanceHero.since', { date: fmt.day(previous!.asOf) })}
        />
      </HeroCard>
      <Stats columns={3} className="mt-[17px]">
        <Stat
          label={t('balanceStats.changeThisYear')}
          value={thisYear ? signed(thisYear.delta) : '—'}
          tone={thisYear ? (thisYear.delta > 0 ? 'gain' : thisYear.delta < 0 ? 'loss' : 'neutral') : 'neutral'}
          noteTone="neutral"
          note={thisYear ? t('balanceStats.vsRecordAt', { date: fmt.day(thisYear.baseDay) }) : t('balanceStats.noRecordBeforeYear')}
        />
        <Stat
          label={t('balanceStats.avgPerMonth')}
          value={avg ? signed(avg.perMonth) : '—'}
          tone={avg ? (avg.perMonth > 0 ? 'gain' : avg.perMonth < 0 ? 'loss' : 'neutral') : 'neutral'}
          noteTone="neutral"
          note={avg ? (avg.sinceFirst ? t('balanceStats.sinceFirstRecord', { count: avg.months }) : t('balanceStats.last12Months')) : t('balanceStats.singleRecord')}
        />
        <Stat
          label={t('balanceStats.yearlyInterest')}
          value={hasInterest ? formatValue(yearlyInterest) : '—'}
          note={hasInterest ? (zoned ? t('balanceStats.effectiveZoned', { rate: fmt.percent(effective, 2) }) : t('balanceStats.simpleRate', { rate: fmt.percent(simpleRate / 100, 2) })) : t('balanceStats.noInterest')}
        />
      </Stats>
    </>
  );
}
```

`MoonyLineChart` has no `axisEnd` prop today. In `src/components/charts/MoonyLineChart.tsx` add to `MoonyLineChartProps`:

```ts
  /**
   * Unix seconds the time axis extends to when later than the last point: a balance
   * line that stops at the newest record while the axis still ends with "Dnes".
   */
  axisEnd?: number;
```

destructure `axisEnd` in the component, and replace the line `const t1 = data[data.length - 1]?.t ?? t0;` with

```ts
  const lastT = data[data.length - 1]?.t ?? t0;
  const t1 = axisEnd !== undefined && axisEnd > lastT ? axisEnd : lastT;
```

Nothing else changes: `<XAxis domain={[t0, t1]}>` now ends at `axisEnd`, the line, the area and the end ring still stop at the last data point, and `<TimeAxis t0={t0} t1={t1} />` already labels the last tick "Dnes" when `t1` is today.

In `BalanceSection` delete the `pointsToToday` line and pass `points={points}`.

(`chart.balance` already exists in both locale files: "Zůstatek" / "Balance".)

- [ ] **Step 3: Gates and commit**

```bash
npm run lint && npm run format && npx vitest run
git add src/components/bank-accounts/BalanceSection.tsx src/components/charts/MoonyLineChart.tsx src/utils/chart-axis.ts src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/bank_accounts.json
git commit -m "feat(bank): balance section with the record line, axis to today and three stats

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 14: Controlled `AddTransactionModal` and the account form

**Files:**
- Modify: `src/components/bank-accounts/AddTransactionModal.tsx` (optional controlled `open`/`onOpenChange`)
- Modify: `src/components/bank-accounts/BankAccountFormDialog.tsx` (create: opening balance + date; edit: read-only balance line + "Zaznamenat nový zůstatek")
- Modify: both `bank_accounts.json` locale files

**Interfaces:**
- Produces: `<AddTransactionModal accountId accountCurrency open? onOpenChange? trigger? />`; `BankAccountFormDialog` gains `onRecordBalance?: () => void` and stops sending `balance` on edit; on create it sends `balance` (when filled) and `balanceAsOf`.

- [ ] **Step 1: Controlled modal**

In `AddTransactionModal.tsx` add to the props `open?: boolean; onOpenChange?: (open: boolean) => void;`, and replace `const [open, setOpen] = useState(false);` with

```ts
  const [innerOpen, setInnerOpen] = useState(false);
  const open = props.open ?? innerOpen;
  const setOpen = (next: boolean) => {
    setInnerOpen(next);
    props.onOpenChange?.(next);
  };
```

(destructure `props` accordingly; render the `DialogTrigger` only when `trigger` is given or no `open` prop is used). Every existing caller passes `trigger` and keeps working.

- [ ] **Step 2: Locale keys**

`en/bank_accounts.json`, add to `form`:

```json
    "openingBalance": "Opening balance",
    "openingBalanceHint": "Optional. Becomes the first record of the balance history.",
    "openingBalanceAsOf": "As of",
    "currentBalance": "Balance {{amount}} as of {{date}}",
    "currentBalanceUnknownDate": "Balance {{amount}} · unknown date",
    "noBalance": "No balance recorded yet",
    "recordNew": "Record a new balance"
```

`cs/bank_accounts.json`:

```json
    "openingBalance": "Počáteční zůstatek",
    "openingBalanceHint": "Nepovinné. Vytvoří první záznam historie zůstatku.",
    "openingBalanceAsOf": "K datu",
    "currentBalance": "Zůstatek {{amount}} k {{date}}",
    "currentBalanceUnknownDate": "Zůstatek {{amount}} · datum neznámé",
    "noBalance": "Zůstatek zatím nezaznamenaný",
    "recordNew": "Zaznamenat nový zůstatek"
```

Remove `form.balanceHint` and `fields.balanceManual` from both files if nothing else uses them (`grep -rn "balanceHint\|balanceManual" src`).

- [ ] **Step 3: Form changes**

In `BankAccountFormDialog.tsx`:
- Add `onRecordBalance?: () => void` to the props.
- State: `const [balance, setBalance] = useState('');` (empty by default) and `const [balanceAsOf, setBalanceAsOf] = useState(todayIsoUtc());` (import `todayIsoUtc`, `isoToUtcDaySec` is already there as `isoToUtcDaySec`).
- Add mode reset: `setBalance(''); setBalanceAsOf(todayIsoUtc());`. Edit mode: do not load the balance into the field (keep `setBalance('')`).
- Validation: `const balanceError = balance.trim() !== '' && !Number.isFinite(Number(balance)) ? tc('validation.balanceRequired') : null;` (empty is allowed now).
- Submit, create branch: send `balance: balance.trim() === '' ? undefined : balance.trim()` and `balanceAsOf: balance.trim() === '' ? undefined : isoToUtcDaySec(balanceAsOf)`. Submit, edit branch: delete the `...(balance !== account.balance ? { balance } : {})` line.
- Markup: replace the balance `<div className="grid gap-2">…</div>` with

```tsx
            {isEditMode && account ? (
              <div className="grid gap-2">
                <Label>{t('fields.balance')}</Label>
                <p className="text-table text-ink-2">
                  {Number(account.balance) === 0 && !account.balanceAsOf
                    ? t('form.noBalance')
                    : account.balanceAsOf
                      ? t('form.currentBalance', { amount: fmt.money(Number(account.balance), selectedCurrency, { decimals: 0 }), date: fmt.day(account.balanceAsOf) })
                      : t('form.currentBalanceUnknownDate', { amount: fmt.money(Number(account.balance), selectedCurrency, { decimals: 0 }) })}
                </p>
                {onRecordBalance && (
                  <Button type="button" variant="ghost" size="sm" className="justify-self-start" onClick={() => { onOpenChange(false); onRecordBalance(); }}>
                    {t('form.recordNew')}
                  </Button>
                )}
              </div>
            ) : (
              <div className="grid gap-2">
                <Label htmlFor="balance">{t('form.openingBalance')}</Label>
                <div className="grid grid-cols-[1fr_150px] gap-2">
                  <Input id="balance" type="number" step="0.01" value={balance} onChange={(e) => setBalance(e.target.value)} placeholder="0" aria-invalid={submitted && balanceError ? true : undefined} />
                  <Input type="date" value={balanceAsOf} max={todayIsoUtc()} onChange={(e) => setBalanceAsOf(e.target.value)} aria-label={t('form.openingBalanceAsOf')} />
                </div>
                {submitted && balanceError ? (
                  <p id="balance-error" role="alert" className="flex items-center gap-[5px] text-micro font-600 text-loss">{balanceError}</p>
                ) : (
                  <p className="text-micro font-500 text-ink-4">{t('form.openingBalanceHint')}</p>
                )}
              </div>
            )}
```

(`fmt` = `useFormat()`; add the import. The currency field next to it stays.)

- [ ] **Step 4: Gates and commit**

```bash
npm run lint && npm run format && npx vitest run
git add src/components/bank-accounts/AddTransactionModal.tsx src/components/bank-accounts/BankAccountFormDialog.tsx src/i18n/locales/en/bank_accounts.json src/i18n/locales/cs/bank_accounts.json
git commit -m "feat(bank): opening balance with a date on create; balance leaves the edit form

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 15: Rebuild `BankAccountDetail.tsx`

**Files:**
- Modify: `src/pages/BankAccountDetail.tsx`
- Modify: both `bank_accounts.json` locale files (head/menu/status/empty keys; remove dead keys)
- Modify: `src/i18n/locales/en/common.json`, `src/i18n/locales/cs/common.json` (remove `validation.balanceAnchorOlder`)

**Interfaces:**
- Consumes: everything from Tasks 6–14.

- [ ] **Step 1: Locale keys**

`en/bank_accounts.json` add:

```json
  "actions": { "recordBalance": "Record balance", "importStatement": "Import statement" },
  "menu": { "balanceHistory": "Balance history", "interest": "Interest" },
  "status": { "balanceAsOf": "Balance as of {{date}}", "noData": "No data yet" },
  "empty": { "noDataTitle": "No data on this account yet", "noDataDescription": "Record the balance from your statement or import a bank statement." }
```

(`status` and `empty` already exist: add these keys to the existing objects, keep `status.lastMovement` and `status.lastImport`.)

`cs/bank_accounts.json`:

```json
  "actions": { "recordBalance": "Zaznamenat zůstatek", "importStatement": "Importovat výpis" },
  "menu": { "balanceHistory": "Historie zůstatku", "interest": "Úročení" },
  "status": { "balanceAsOf": "Zůstatek k {{date}}", "noData": "Zatím bez dat" },
  "empty": { "noDataTitle": "Účet zatím nemá žádná data", "noDataDescription": "Zaznamenejte zůstatek z výpisu nebo naimportujte výpis z banky." }
```

(Same merge into the existing `status` and `empty` objects of the Czech file.)

- [ ] **Step 2: Remove the old page parts**

In `src/pages/BankAccountDetail.tsx` delete:
- the imports of `TimePeriodSelector`/`Period`, `chartPeriodStart`, `CHART_PERIODS`, `reconstructBalance`, `significantMovements`, `movementThreshold`, `movementFloor`, `MovementEvent`, `ChartEvent`/`EventCluster`, `MoonyLineChart`, `ChartLegend`, `HeroCard`/`HeroValue`, `Stat`/`Stats`, `NativeBalance`, `calculateZonedInterest`, `History`, `Undo2`, `ListFilter` only if no longer used after the rewrite (run the linter at the end);
- the states `importHistoryOpen`, `hotEventId`, `chartPeriod`;
- the queries `thisMonthResult`, `lastMonthResult` and the `thisMonth`/`lastMonth` ranges; rename the `chart` query to `history` (`queryKey: ['bank-transactions', accountId, 'history']`, `queryFn: () => getCompleteBankHistory(accountId)`);
- `monthNowRows`, `monthPrevRows`, `monthNow`, `monthPrev`; `chartTxs`, `chartPoints`, `floorInAccount`, `threshold`, `events`, `eventTip`, `scrollToTx`, `todayCount`, `yearlyInterest`/`effectiveRate`, `monthLabel`, `prevMonthLabel`, `topCategory`; the `BalanceEvent` type;
- in the JSX: the `<HeroCard …>` block, the `readiness.balanceEstimate` paragraph, the `<Stats columns={3}>` block, the zones `<Card variant="flat">` block, the `importHistoryOpen && (<Card variant="table">…)` block, the `noTransactionsAtAll ? <EmptyState …> :` branch (keep the `<Card variant="table">` ledger JSX that was its `else`).

The renamed history query reads:

```ts
  // Every transaction of the account: the flows window, the section rules and the FX notes
  const {
    data: history,
    isLoading: historyLoading,
    isError: historyError,
    refetch: retryHistory,
  } = useQuery({
    queryKey: ['bank-transactions', accountId, 'history'],
    queryFn: () => (accountId ? getCompleteBankHistory(accountId) : null),
    enabled: !!accountId,
  });
```

`firstForeignDay` now reads only `history?.transactions`; `historyRows` is `bookedInAccountCurrency(history?.transactions ?? [], accountCurrency, toAccountCurrency)` (the old `chartRows`); `foreignNote` reads only `historyRows`. Its two paragraphs become one element used by the sections:

```tsx
  const foreignNotes = (
    <>
      {foreignNote.converted.length > 0 && (
        <p className="mt-2 text-micro text-ink-4">
          {t('readiness.foreignConverted', { currencies: foreignNote.converted.join(', '), currency: accountCurrency })}
        </p>
      )}
      {foreignNote.missing.length > 0 && (
        <p className="mt-2 text-micro text-loss">
          {t('readiness.foreignMissing', { currencies: foreignNote.missing.join(', ') })}
        </p>
      )}
    </>
  );
```

- [ ] **Step 3: New state, data and sections**

Add imports:

```ts
import { Wallet } from 'lucide-react';
import { useBalanceRecords } from '@/hooks/use-balance-records';
import { BalanceSection } from '@/components/bank-accounts/BalanceSection';
import { FlowsCard } from '@/components/bank-accounts/FlowsCard';
import { BalanceRecordDialog } from '@/components/bank-accounts/BalanceRecordDialog';
import { BalanceHistoryDialog } from '@/components/bank-accounts/BalanceHistoryDialog';
import { ImportHistoryDialog } from '@/components/bank-accounts/ImportHistoryDialog';
import { InterestDialog } from '@/components/bank-accounts/InterestDialog';
import { detailSections, hasRecentRows, ledgerPresetForMonth, monthEnd, monthOf, monthStart, newestMonthWithRows } from '@/utils/bank-activity';
```

State:

```ts
  const [recordOpen, setRecordOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [importsOpen, setImportsOpen] = useState(false);
  const [interestOpen, setInterestOpen] = useState(false);
  const [addTxOpen, setAddTxOpen] = useState(false);
  const [windowEnd, setWindowEnd] = useState<number | null>(null);
  const [selectedMonth, setSelectedMonth] = useState<number | null>(null);
  // The ledger opens on the newest month with movements; set once from the data
  const [ledgerReady, setLedgerReady] = useState<boolean>(!!linkedCategory);
```

Data (after `historyRows`):

```ts
  const { records } = useBalanceRecords(accountId);
  const today = utcDayFloor(Date.now() / 1000);
  const currentMonth = monthOf(today);
  const balance = parseFloat(account?.balance || '0');
  const hasAnyTransaction = (anyTransactionResult?.total ?? 0) > 0;
  const sections = detailSections({
    balance,
    hasAnyTransaction,
    hasRecentRows: hasRecentRows(historyRows.rows, currentMonth),
  });
  const effectiveWindowEnd = windowEnd ?? currentMonth;
```

Ledger initial period (replace the initial `thisMonth` logic): keep the state declarations but initialize `dateFrom`/`dateTo` to `''` and `datePreset` to `linkedCategory ? 'allTime' : 'thisMonth'`; then:

```ts
  // Newest booked transaction → ledger preset, once
  const newestMonth = useMemo(() => newestMonthWithRows(historyRows.rows), [historyRows]);
  useEffect(() => {
    if (ledgerReady || historyLoading) return;
    if (newestMonth === null) {
      applyDatePreset('thisMonth');
    } else {
      const preset = ledgerPresetForMonth(newestMonth, currentMonth);
      if (preset === 'custom') {
        setDateFrom(isoDateFromUtcTimestamp(monthStart(newestMonth)));
        setDateTo(isoDateFromUtcTimestamp(monthEnd(newestMonth)));
        setDatePreset('custom');
      } else {
        applyDatePreset(preset);
      }
    }
    setLedgerReady(true);
    // applyDatePreset is stable enough for a one-shot initialisation
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ledgerReady, historyLoading, newestMonth, currentMonth]);
```

and gate the ledger query with `enabled: !!accountId && ledgerReady` (`historyLoading` is the `isLoading` of the history query). The deep link path (`linkedCategory`) starts ready with `allTime`, as today.

Shell status:

```ts
  useShellPage({
    crumb: account?.name,
    status:
      lastImport !== null
        ? { text: t('status.lastImport', { date: fmt.day(lastImport) }), tone: 'neutral' }
        : latestTx
          ? { text: t('status.lastMovement', { date: fmt.day(latestTx.bookingDate) }), tone: 'neutral' }
          : account?.balanceAsOf
            ? { text: t('status.balanceAsOf', { date: fmt.day(account.balanceAsOf) }), tone: 'neutral' }
            : { text: t('status.noData'), tone: 'neutral' },
  });
```

- [ ] **Step 4: Head, menu and the page body**

Head (`description={accountNumber || undefined}`; delete the interest/today parts):

```tsx
        actions={
          <>
            <Button onClick={() => setRecordOpen(true)}>
              <Wallet />
              {t('actions.recordBalance')}
            </Button>
            <Button onClick={() => setCsvImportOpen(true)}>
              <Upload />
              {t('actions.importStatement')}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label={tc('labels.moreActions')}>
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setEditDialogOpen(true)}><Pencil />{t('editAccount')}</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setAddTxOpen(true)}><Plus />{t('transaction.add')}</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setHistoryOpen(true)}><History />{t('menu.balanceHistory')}</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setImportsOpen(true)}><Receipt />{t('importHistory.title')}</DropdownMenuItem>
                {(account.hasZoneDesignation || parseFloat(account.interestRate ?? '0') > 0) && (
                  <DropdownMenuItem onSelect={() => setInterestOpen(true)}><Percent />{t('menu.interest')}</DropdownMenuItem>
                )}
                <DropdownMenuItem onSelect={() => setLocation('/settings/categorization-rules')}><ListFilter />{tc('nav.categorizationRules')}</DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="danger" onSelect={() => setDeleteAccountOpen(true)}><Trash2 />{t('deleteAccount')}</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
```

(import `History`, `Percent`, `Receipt`, `Wallet` from `lucide-react`; two dark buttons are an owner decision, see the spec.)

Body, in place of the removed hero/stats/cards, before the ledger card:

```tsx
      {historyError && <ReportError onRetry={() => void retryHistory()} />}
      {sections.balance && (
        <BalanceSection
          account={account}
          records={records}
          zones={zones ?? []}
          today={today}
          currency={accountCurrency}
          formatValue={formatAccountAmount}
        />
      )}
      {sections.flows && (
        <div className={sections.balance ? 'mt-[17px]' : undefined}>
          <FlowsCard
            transactions={historyRows.rows}
            currency={accountCurrency}
            currentMonth={currentMonth}
            windowEnd={effectiveWindowEnd}
            onWindowEndChange={setWindowEnd}
            selectedOverride={selectedMonth}
            onSelect={setSelectedMonth}
            hero={!sections.balance}
            notes={foreignNotes}
          />
        </div>
      )}
      {!sections.balance && !sections.flows && !sections.ledger && !historyLoading && (
        <EmptyState icon={<Wallet />} title={t('empty.noDataTitle')} description={t('empty.noDataDescription')} />
      )}
      {sections.ledger && (
        <div className={sections.balance || sections.flows ? 'mt-[17px]' : undefined}>
          {/* the existing <Card variant="table"> ledger JSX, unchanged except the head below */}
        </div>
      )}
```

where `foreignNotes` is the existing pair of `readiness.foreignConverted` / `readiness.foreignMissing` paragraphs (render them inside `FlowsCard` through `notes`, or under the balance section when there is no flows card).

Ledger card head: add the small button right before the kind `Segmented`:

```tsx
              <Button variant="outline" size="sm" onClick={() => setAddTxOpen(true)}>
                <Plus />
                {t('transaction.add')}
              </Button>
```

and remove the old `AddTransactionModal` triggers from the head and the empty state. Keep the period `Select` with its eight presets and the custom date inputs exactly as they are.

Dialogs at the end of the component:

```tsx
      <AddTransactionModal accountId={accountId || ''} accountCurrency={currency} open={addTxOpen} onOpenChange={setAddTxOpen} />
      <BalanceRecordDialog open={recordOpen} onOpenChange={setRecordOpen} account={account} records={records} />
      <BalanceHistoryDialog open={historyOpen} onOpenChange={setHistoryOpen} accountId={account.id} currency={accountCurrency} records={records} />
      <ImportHistoryDialog open={importsOpen} onOpenChange={setImportsOpen} batches={importBatches ?? []} onUndo={(batch) => { setImportsOpen(false); setPendingUndo(batch); }} />
      <InterestDialog open={interestOpen} onOpenChange={setInterestOpen} account={account} zones={zones ?? []} balance={balance} currency={accountCurrency} />
```

and pass `onRecordBalance={() => setRecordOpen(true)}` to `<BankAccountFormDialog>`. `handleEditSubmit` must not send `balance` (the form no longer does); `handleImported` keeps selecting the imported range.

- [ ] **Step 5: Delete the reconstruction helpers and their tests**

In `src/utils/bank-activity.ts` delete everything from `export interface BalancePoint` up to (not including) the `const WINDOW_MONTHS = 12;` line added in Task 8 (`reconstructBalance`, `MovementEvent`, `MOVEMENT_FLOOR_EUR`, `movementFloor`, `movementThreshold`, `significantMovements`, the old `const DAY`) and the now-unused `import { convert, type FxQuotes } from '@shared/fx';`; update the module doc comment to "Pure helpers for the bank-account pages: flow summaries of a set of transactions, the monthly flows window of the detail page and its section rules. Amounts are in the account's own currency; the caller converts." In `src/utils/bank-activity.test.ts` delete the `describe` blocks for `reconstructBalance`, `significantMovements` and `movementThreshold`/`movementFloor` and their imports. Confirm with `grep -rn "reconstructBalance\|significantMovements\|movementThreshold\|movementFloor" src` that nothing references them.

- [ ] **Step 6: Remove dead locale keys**

Delete from both `bank_accounts.json` files the keys no longer referenced (verify each with `grep -rn "<key>" src`): `hero.thisMonth`, `hero.expensesExceed`, `hero.incomeExceeds`, `hero.balanced`, `hero.noMovementsThisMonth`, `stats.incomeIn`, `stats.expenseIn`, `stats.monthBalance`, `stats.incomeCount*`, `stats.txCount*`, `stats.mostIn`, `stats.none`, `stats.prevMonth`, `stats.noPrevMonth`, `chart.largeIncome`, `chart.largeExpense`, `chart.thresholdNote`, `chart.reconstructed`, `chart.clusterTitle*`, `readiness.balanceEstimate`, `detail.interestNote`, `detail.todayCount*`, `empty.noTransactionsTitle`, `empty.noTransactionsDescription`, `empty.importCsv`, `empty.addManually`, `zones.effectiveNote`, `zones.title`, `csvImport.importCsv` (if the CSV dialog does not use it), `importHistory.button`. Delete `validation.balanceAnchorOlder` from both `common.json` files. Keep any key that is still referenced.

- [ ] **Step 7: Full gates**

```bash
npm run typecheck && npm run lint && npm run format && npm test && (cd src-tauri && cargo test)
```

Expected: all pass. Fix every type error in the page (the rewrite touches many identifiers).

- [ ] **Step 8: Commit**

```bash
git add src/pages/BankAccountDetail.tsx src/utils/bank-activity.ts src/utils/bank-activity.test.ts src/i18n/locales
git commit -m "feat(bank): account detail sections by data: balance records, flows window, unchanged ledger

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 16: Design-system doc, changelog and manual verification

**Files:**
- Modify: `docs/design-system/README.md` (§6 Button row, §7 Detail row, §9)
- Modify: `CHANGELOG.md` if the repository has one (`ls CHANGELOG.md`)

- [ ] **Step 1: Design system amendments**

§6, Button row: append to the "One primary per page." sentence: " Exception (owner decision 2026-10-10): the bank account detail head has two, Zaznamenat zůstatek and Importovat výpis."

§7, Detail row: replace "Stock detail, bank account detail | Back link, eyebrow, H1, action row, hero with time trace, 3–4 stats, ledger table + aside cards, modals." with two rows:

```markdown
| Detail | Stock detail | Back link, eyebrow, H1, action row, hero with time trace, 3–4 stats, ledger table + aside cards, modals. |
| Detail (bank account) | Bank account detail | Back link, eyebrow, H1, two actions + ··· menu; sections by data: balance hero (line through dated balance records of the last year, axis to Dnes, line ending at the newest record) with 3 balance stats, a flows card (12-month bar window shifted by arrows, the newest month with movements selected), the full-width ledger with its own period filters; balance history, import history and interest tiers are modals from the ··· menu. |
```

§9: replace "Bank accounts mark only movements above a threshold (an absolute floor of about 400 EUR, or 10 % of balance)." with "Bank account charts draw no movement events; dated balance records are drawn as marks (○) on the balance line."

- [ ] **Step 2: Changelog**

If `CHANGELOG.md` exists, add under the unreleased section:

```markdown
- Bank account detail: balance history as dated records (recorded by hand or confirmed from a statement; the account's balance mirrors the newest record), a balance line through the records, a 12-month flows chart, and the ledger opening on the newest month with movements. Balance history, import history and interest tiers moved to the ··· menu. The balance field left the edit form; new accounts take an optional opening balance with a date.
```

- [ ] **Step 3: Run the app on the demo profile and verify every section combination**

Use the Browser pane: `preview_start {name: "moony-demo"}` (demo profile, password `demo1234`, see the launch config; never the `moony-dev` real profile). Then, in the app:

1. Open a savings account, record a balance twice (two days) through "Zaznamenat zůstatek" → the balance section appears with the line and the three stats; the top-bar status and the account list balance follow the newest record.
2. Open "···" → Historie zůstatku: both records listed; delete the newest → the hero shows the previous one.
3. Open a checking account with imported transactions and balance 0 → the flows card is the hero, the ledger opens on the newest month with movements, the arrows shift the window and stop at the current month.
4. Record a balance on that checking account → the balance section appears above the flows card, which becomes compact.
5. Import a CSV (any sample in `demo-home` or `shared/fixtures`) and confirm the statement balance in the import's next steps → a `z výpisu` record appears in the history.
6. Create a new account with an opening balance and date → its first record exists; create one without → the empty state shows.
7. Edit an account → no balance field; "Zaznamenat nový zůstatek" opens the record dialog.

Take a screenshot of each state with `computer {action: "screenshot"}` and check `read_console_messages` for errors. Fix anything that is off before continuing.

- [ ] **Step 4: Commit**

```bash
git add docs/design-system/README.md CHANGELOG.md
git commit -m "docs: bank account detail archetype, no movement events on bank charts, changelog

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 17: Finish the branch

- [ ] **Step 1: Final gates on the branch**

```bash
npm run typecheck && npm run lint && npm run format:check && npm test && (cd src-tauri && cargo test && cargo clippy)
```

- [ ] **Step 2: Merge into main through a temporary worktree (never in the shared checkout)**

```bash
cd ..
git -C Moony-tauri worktree add /tmp/moony-main-merge main
cd /tmp/moony-main-merge && git merge --no-ff feat/bank-account-detail-redesign -m "Merge branch 'feat/bank-account-detail-redesign'" && cd -
git -C Moony-tauri worktree remove /tmp/moony-main-merge
git -C Moony-tauri merge-base --is-ancestor feat/bank-account-detail-redesign main && echo "merged"
```

If `main` moved meanwhile and the merge conflicts, resolve in the temporary worktree, run the gates there, then commit the merge. Do not push. Do not delete the feature worktree or branch until the `merge-base` check prints `merged`.

- [ ] **Step 3: Report**

Tell the owner: the merge commit, what was verified in Step 3 of Task 16, and that nothing was pushed.
