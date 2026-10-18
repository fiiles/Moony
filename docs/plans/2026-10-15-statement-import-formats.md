# Statement import formats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Import CAMT.053, MT940, OFX/QFX and QIF bank statements into a Moony bank account,
with the same preview, deduplication, categorization and undo as the CSV import.

**Architecture:** One parser per format produces a common `Statement` model
(`src-tauri/src/services/statement_import/`). `to_rows` turns it into the CSV import's
`ParsedRow`s, which then go through the CSV import's own duplicate check, categorization,
batch and write path. Those are refactored to take rows, not CSV text. The import dialog
detects the format from the content and skips the column mapping step for statements.

**Tech Stack:** Rust (Tauri 2, rusqlite/SQLCipher, `quick-xml` 0.42, `regex`, `encoding_rs`,
`chrono`), React 19 + TypeScript, TanStack Query, i18next, Vitest.

**Spec:** `docs/specs/2026-10-15-statement-import-formats-design.md`. Section ids (§4.2 …) and
decisions (D1–D16) below refer to it. The owner approved the spec in chat on 2026-10-15.

## Ground rules

Work happens in a sibling worktree `../Moony-tauri-statements` on the branch
`feat/statement-import`. Other sessions switch branches in the shared checkout,
so never commit there. Check
`git branch --show-current` before every commit. Demo runs from the worktree use port 1421. The
page must greet Alex before you type the demo password (`demo1234`). Never type the real
profile's password.

Read `AGENTS.md` (the ten rules), `docs/standards/rust-backend.md`,
`docs/standards/type-contract.md`, `docs/standards/i18n.md`, `docs/standards/testing.md` and
`docs/DEPRECATED.md` before coding. Use TDD: write the failing test, then the code. Make one
Conventional Commit per task, ending with
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. New user-facing strings land in all
ten locales in the same commit.

Gate per task, before committing:

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
npm run lint && npm run typecheck && npm test
```

After a change to a type registered in `bindings.rs`, also regenerate the drift detector:

```bash
cd src-tauri && cargo test generate_bindings -- --ignored
```

At the end of each phase (tasks 10, 12, 13 and 15), merge `feat/statement-import` into local
`main` with `git merge --no-ff`. Never push. The owner releases.

## Refinements of the spec found while planning

These come from reading the code. They are binding for the implementer:

- **R1 (§10):** `translateApiError` and `api-error.ts` only translate `validation.*` and
  `auth.*` keys from `common.json`. File-level errors are therefore `validation.statement*`
  keys in `common`, and those with values use `AppError::validation_with(key, json!({…}))`.
  Row messages and warnings stay `statementImport.*` in `bank_accounts`
  (`translateRowMessage` sends every non-`validation.` key there).
- **R2 (§5):** Skipped entries (CAMT `INFO`/`FUTR`, OFX corrections) stay in
  `Statement.entries` with `skip_reason: Some(key)` instead of a separate `skipped` list. That
  way the ordinals (D5) follow file order.
- **R3 (§5):** `DeclaredBalance` and `SourceCategoryMapping` are defined in
  `csv_import::types`, because the shared core uses them and `csv_import` must not depend on
  `statement_import`. `statement_import` re-exports them.
- **R4 (§6.2):** The MT940 `:25:` value is passed whole as the identifier, because Czech
  domestic numbers are `account/bank`. A German `BLZ/Konto` (8 digits / up to 10 digits) is
  first converted to a `DE` IBAN. The `?20`–`?29` subfields are joined without a separator
  when the previous one is exactly 27 characters long (a full line, possibly cut mid-word),
  otherwise with a space. `?00` is prepended with ` | `.
- **R5 (§7.5):** `accountMatch = none` is only possible when the file's identifier
  canonicalizes to a valid IBAN. A domestic number that matches no account gives `unknown`, so
  unusual domestic notations never force the confirmation checkbox.
- **R6 (§9):** All three statement commands take `country: Option<String>` (the profile
  country). Inspection, preview and import must then decode a legacy-encoded file the same
  way.
- **R7:** `inspect_statement_file` reads only the first 4 KB for detection. A file that is not
  a statement returns `null` regardless of its size, so the 50 MB cap (D16) never blocks a large
  CSV.
- **R8:** Until a format's phase lands, the service answers a detected but not yet implemented
  format with `validation.statementFormatNotYetSupported` (`{ "format": "MT940" }`). It never
  falls back to the CSV path.

## Global Constraints

- The only new Rust dependency is `quick-xml = "0.42"`, the version already in `Cargo.lock`.
- Statement files above 50 MB are refused. XML with a `<!DOCTYPE` is refused.
- No migration. `data_source` stays `csv_import`.
- Money is TEXT. Statement amounts are canonical decimal text (`dedup::canonical_amount`). The
  balance check uses exact decimal arithmetic, never `f64`.
- Dates are the calendar day as written in the file, stored as UTC midnight, never shifted by
  a time zone (ADR 0008).
- `invoke()` only in `src/lib/tauri-api.ts`. `shared/schema.ts` changes follow the 7-step
  procedure. Rust wire types use per-field `#[serde(rename = "camelCase")]` and are
  registered in `bindings.rs`.
- Commands are thin; logic lives in `services/` and takes `&Connection`. Errors are
  `AppError`. Options are validated before DB work. No `.unwrap()` outside tests.
- Every user-facing string exists in all ten locales (`en cs de es it nb pl sv ja zh`) with
  that language's plural forms.
- The statement import mutation refreshes exactly what the CSV import refreshes
  (`refreshMoneyFlows` + removing the preview queries).
- CSV wire types change only additively: `CsvImportPreview.overlap` and
  `CsvPreviewRow.sourceCategory`.
- `src-tauri/src/services/csv_import/tests.rs` passes unchanged after the core refactor.
- Fixture files are synthetic. Real bank exports never enter the repository.

## Review Focus

Each line below is pinned by a test in the task named in brackets:

1. **Windows line endings and trailing spaces** (CRLF, `:61:` lines padded with spaces, QIF
   lines ending in `\r`) parse exactly like LF files. [Tasks 11, 13, 14]
2. **The same account written differently**: a file IBAN with spaces or in lowercase, or a
   Moony account that stores the Czech domestic number `19-2000145399/0800`, still gives
   `accountMatch = selected` for the IBAN `CZ6508000000192000145399`. [Task 6]
3. **Escaped characters in XML and SGML** (`&amp;`, `&lt;`, `&#228;`) come out as `&`, `<`, `ä`
   in counterparty names and descriptions. [Tasks 7, 13]
4. **A statement with balances but no entries** (a quiet month) inspects and previews without
   an error: 0 rows, `balanceCheck = ok` when opening = closing, and nothing to import.
   [Task 8]
5. **Legacy encodings with diacritics** (a windows-1250 MT940 or QIF with `Žluťoučký kůň`)
   decode to the right characters, not mojibake. [Tasks 4, 11]

---

## Status

Executed on 2026-10-15/16 with subagent-driven development (one implementer and one reviewer per
task, a phase-1 review, a final whole-branch review). The branch was merged into local `main`
once, after phase 4 and the final review, instead of after every phase: the owner prefers one
local merge at the end, and `main` then never offered formats it could not read yet.

| Phase | Task | Status | Commits |
|---|---|---|---|
| 1 | 1. Shared core takes rows | done | 767a556d |
| 1 | 2. Overlap hint for every import (D12) | done | 63fbeca9 |
| 1 | 3. Statement model, exact decimals, `to_rows` with the ID guard | done | 9c55ce1e |
| 1 | 4. Format detection, size and DOCTYPE guards, decoding | done | d6986b14 |
| 1 | 5. Merging statements and the balance check | done | 1089e9a2, 3709e358 |
| 1 | 6. Account match | done | faf39af9 |
| 1 | 7. CAMT.053 parser | done | 3ca9621c, 38e839ce |
| 1 | 8. Statement service, wire types, commands | done | fcf241f7 |
| 1 | 9. Import dialog for statements | done — live check on a demo copy (CAMT import, balance, duplicates, account switch, CSV mapping) | ff5a65fc, 8e8f33d6 |
| 1 | 10. Phase 1 docs, ADR 0019 | done; phase-1 review fixes (CAMT hardening) | 671e2408, b3bfd101 |
| 2 | 11. MT940 structure | done | a178dfaf, 905957af |
| 2 | 12. MT940 `:86:` dialects, enable MT940, docs | done — live check (MT940 preview, overlap hint against the CAMT import) | 7f55bb91, c970f770 |
| 3 | 13. OFX/QFX parser, enable OFX, docs | done — live check (OFX preview, currency and account warnings) | e0dfc62d, d96a4361 |
| 4 | 14. QIF parser and date order | done | dfdf97ce |
| 4 | 15. QIF in the dialog (date order, category panel), docs | done — live check (Quicken and windows-1250 QIF, category prefill) | 4d2a13b3, 7893947f |
| — | 16. Whole-branch review and release checks | done — final review fixes; `npm run tauri build -- --debug --no-bundle` builds; the debug binary was not driven (native file dialogs cannot be driven) | 1bc82700 |

Release checklist for the owner (not run by the implementers):
- Dry run on a copy of the real profile with your own CAMT.053, MT940 and OFX exports; compare
  the closing balances with the bank.
- Re-import one file and confirm 0 new rows.
- Check that your banks' bank IDs (CAMT `AcctSvcrRef`/`NtryRef`, MT940 `//` reference) are not
  numbered per statement: such IDs would mark a real payment of a later statement as a duplicate.
- Open the import once in a built app (not `tauri dev`) through the native file dialog.

---|---|---|---|
| 1 | 1. Shared core takes rows (`ParsedImport`, `ImportOptions`, source category map, declared balance) | pending | |
| 1 | 2. Overlap hint for every import (D12) | pending | |
| 1 | 3. Statement model, exact decimals, `to_rows` with the ID guard | pending | |
| 1 | 4. Format detection, size and DOCTYPE guards, decoding | pending | |
| 1 | 5. Merging statements and the balance check | pending | |
| 1 | 6. Account match | pending | |
| 1 | 7. CAMT.053 parser | pending | |
| 1 | 8. Statement service, wire types, commands | pending | |
| 1 | 9. Import dialog for statements | pending | |
| 1 | 10. Phase 1 docs, ADR 0019, manual check, merge | pending | |
| 2 | 11. MT940 structure | pending | |
| 2 | 12. MT940 `:86:` dialects, enable MT940, docs, merge | pending | |
| 3 | 13. OFX/QFX parser, enable OFX, docs, merge | pending | |
| 4 | 14. QIF parser and date order | pending | |
| 4 | 15. QIF in the dialog (date order, category panel), docs, merge | pending | |
| — | 16. Whole-branch review and release checks | pending | |

---

## Phase 1 — shared core and CAMT.053

### Task 1: Shared core takes rows

**Files:**
- Modify: `src-tauri/src/services/csv_import/rows.rs` (`ParsedCsv` → `ParsedImport`,
  `ParsedRow.source_category`)
- Modify: `src-tauri/src/services/csv_import/types.rs` (`ImportOptions`, `DeclaredBalance`,
  `SourceCategoryMapping`, `CsvPreviewRow.source_category`, `CsvImportConfig::import_options`)
- Modify: `src-tauri/src/services/csv_import/mod.rs` (`preview_rows`, `import_rows`, category
  resolution order)
- Create: `src-tauri/src/services/csv_import/core_tests.rs`
- Modify: `src-tauri/src/bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts`

**Interfaces:**
- Produces:
  ```rust
  // csv_import::rows
  pub struct ParsedImport {
      pub rows: Vec<ParsedRow>,
      pub total_rows: usize,
      /// Declared statement period; `None` = the range of the rows (CSV).
      pub period: Option<CsvDateRange>,
      /// Declared closing balance; `None` = derived from the row balances (CSV).
      pub declared_balance: Option<DeclaredBalance>,
  }
  // ParsedRow gains: pub source_category: Option<String>
  pub fn parse_rows(content: &str, account_id: &str, config: &CsvImportConfig) -> Result<ParsedImport>;

  // csv_import::types
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
  pub struct DeclaredBalance { pub amount: String, pub currency: String, #[serde(rename = "asOf")] pub as_of: i64 }
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
  pub struct SourceCategoryMapping { pub source: String, #[serde(rename = "categoryId")] pub category_id: String }
  #[derive(Debug, Clone)]
  pub struct ImportOptions {
      pub skip_duplicates: bool,              // default true
      pub import_anyway_lines: Vec<usize>,
      pub category_overrides: Vec<CsvCategoryOverride>,
      pub source_category_map: Vec<SourceCategoryMapping>,
      pub preview_offset: usize,
      pub preview_limit: usize,               // default 200
  }
  impl Default for ImportOptions { /* skip_duplicates: true, preview_limit: 200, rest empty/0 */ }
  impl ImportOptions { pub fn validate(&self) -> Result<()>; } // validation.csvPreviewRangeInvalid, same bounds as CsvImportConfig
  impl CsvImportConfig { pub fn import_options(&self) -> ImportOptions; }
  // CsvPreviewRow gains: #[serde(rename = "sourceCategory")] pub source_category: Option<String>

  // csv_import (mod.rs)
  pub fn preview_rows(conn: &Connection, account_id: &str, parsed: &ParsedImport,
      options: &ImportOptions, engine: Option<&CategorizationEngine>) -> Result<CsvImportPreview>;
  pub fn import_rows(conn: &mut Connection, account_id: &str, file_name: &str, parsed: &ParsedImport,
      options: &ImportOptions, engine: Option<&CategorizationEngine>) -> Result<CsvImportResult>;
  ```
  `preview_import` and `import_transactions` keep their signatures: `ensure_account_exists`,
  `config.validate()`, `parse_rows`, then the new function with `config.import_options()`.

- [ ] **Step 1: Write the failing tests** in `csv_import/core_tests.rs` (declare it in `mod.rs`
  next to `mod tests;` as `#[cfg(test)] mod core_tests;`):

```rust
use super::test_support::setup_import_db;
use super::*;
use crate::models::InsertBankTransaction;

const DAY: i64 = 86_400;

fn db() -> rusqlite::Connection {
    let conn = setup_import_db();
    conn.execute_batch(
        "INSERT INTO bank_accounts (id, name, currency) VALUES ('acc', 'Main', 'CZK');
         INSERT INTO transaction_categories (id, name) VALUES ('cat_food', 'Groceries'), ('cat_fun', 'Fun');",
    )
    .unwrap();
    conn
}

fn candidate(line: usize, day: i64, amount: &str, credit: bool, source_category: Option<&str>) -> ParsedRow {
    let insert = InsertBankTransaction {
        bank_account_id: "acc".into(),
        transaction_id: None,
        tx_type: if credit { "credit" } else { "debit" }.into(),
        flow_kind: None,
        amount: amount.into(),
        currency: Some("CZK".into()),
        description: Some(format!("row {line}")),
        counterparty_name: None,
        counterparty_iban: None,
        counterparty_bank_code: None,
        counterparty_country: None,
        booking_date: day * DAY,
        value_date: None,
        category_id: None,
        status: Some("booked".into()),
    };
    ParsedRow {
        line,
        booking_date: Some(day * DAY),
        signed_amount: Some(if credit { amount.to_string() } else { format!("-{amount}") }),
        currency: Some("CZK".into()),
        description: insert.description.clone(),
        counterparty: None,
        balance: None,
        source_category: source_category.map(Into::into),
        state: RowState::Candidate(Box::new(insert)),
    }
}

fn parsed(rows: Vec<ParsedRow>) -> ParsedImport {
    ParsedImport { total_rows: rows.len(), rows, period: None, declared_balance: None }
}

fn category_of(conn: &rusqlite::Connection, line: i64) -> (Option<String>, Option<String>) {
    conn.query_row(
        "SELECT category_id, categorization_source FROM bank_transactions WHERE import_line = ?1",
        [line],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn source_category_map_categorizes_as_manual() {
    let mut conn = db();
    let rows = parsed(vec![candidate(1, 20_000, "12.5", false, Some("Food:Groceries"))]);
    let options = ImportOptions {
        source_category_map: vec![SourceCategoryMapping { source: "Food:Groceries".into(), category_id: "cat_food".into() }],
        ..ImportOptions::default()
    };
    import_rows(&mut conn, "acc", "x.qif", &rows, &options, None).unwrap();
    assert_eq!(category_of(&conn, 1), (Some("cat_food".into()), Some("manual".into())));
}

#[test]
fn line_override_beats_source_category_map() {
    let mut conn = db();
    let rows = parsed(vec![candidate(1, 20_000, "12.5", false, Some("Food:Groceries"))]);
    let options = ImportOptions {
        source_category_map: vec![SourceCategoryMapping { source: "Food:Groceries".into(), category_id: "cat_food".into() }],
        category_overrides: vec![CsvCategoryOverride { line: 1, category_id: "cat_fun".into() }],
        ..ImportOptions::default()
    };
    import_rows(&mut conn, "acc", "x.qif", &rows, &options, None).unwrap();
    assert_eq!(category_of(&conn, 1).0, Some("cat_fun".into()));
}

#[test]
fn mapping_to_a_deleted_category_falls_through() {
    let mut conn = db();
    let rows = parsed(vec![candidate(1, 20_000, "12.5", false, Some("Food"))]);
    let options = ImportOptions {
        source_category_map: vec![SourceCategoryMapping { source: "Food".into(), category_id: "gone".into() }],
        ..ImportOptions::default()
    };
    import_rows(&mut conn, "acc", "x.qif", &rows, &options, None).unwrap();
    assert_eq!(category_of(&conn, 1), (None, None));
}

#[test]
fn declared_balance_and_period_win_over_rows() {
    let mut conn = db();
    let mut rows = parsed(vec![candidate(1, 20_005, "500.5", true, None), candidate(2, 20_020, "250", false, None)]);
    rows.period = Some(CsvDateRange { from: 20_001 * DAY, to: 20_030 * DAY });
    rows.declared_balance = Some(DeclaredBalance { amount: "1250.5".into(), currency: "CZK".into(), as_of: 20_030 * DAY });
    let result = import_rows(&mut conn, "acc", "s.xml", &rows, &ImportOptions::default(), None).unwrap();
    let balance = result.statement_balance.expect("declared balance");
    assert_eq!((balance.amount.as_str(), balance.as_of, balance.line), ("1250.5", 20_030 * DAY, 2));
    assert_eq!(result.last_balance.as_deref(), Some("1250.5"));
    let (from, to): (i64, i64) = conn
        .query_row("SELECT date_from, date_to FROM csv_import_batches", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((from, to), (20_001 * DAY, 20_030 * DAY));
}

#[test]
fn preview_rows_shows_the_source_category() {
    let conn = db();
    let rows = parsed(vec![candidate(1, 20_000, "1", false, Some("Food"))]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    assert_eq!(preview.rows[0].source_category.as_deref(), Some("Food"));
}
```

- [ ] **Step 2: Run them and see them fail to compile** (`ParsedImport`, `ImportOptions`,
  `import_rows` do not exist yet).

```bash
cd src-tauri && cargo test csv_import::core_tests
```

- [ ] **Step 3: Implement.**
  - Rename `ParsedCsv` → `ParsedImport` in `rows.rs` and `mod.rs`, add the two fields
    (`parse_rows` sets them to `None`), and add `source_category: None` wherever `rows.rs`
    builds a `ParsedRow`.
  - Add the types above to `types.rs`. `CsvPreviewRow` gets `source_category`, and
    `preview_rows` fills it from `row.source_category`.
  - Move the body of `preview_import` into `preview_rows`, starting with
    `ensure_account_exists(conn, account_id)?; options.validate()?;`. Move the body of
    `import_transactions` into `import_rows` the same way. Every `config.` read in the moved
    bodies becomes `options.`.
  - Replace the override lookup in both functions with one helper used by both:
    ```rust
    /// The category the user chose for a row: its own pick in the preview, else the mapping of
    /// its source category (QIF). Both count as manual; a since-deleted category is ignored.
    fn explicit_category(conn: &Connection, options: &ImportOptions, row: &ParsedRow) -> Result<Option<String>> {
        let picked = options.category_overrides.iter().find(|o| o.line == row.line).map(|o| &o.category_id);
        let mapped = row.source_category.as_ref().and_then(|source| {
            options.source_category_map.iter().find(|m| &m.source == source).map(|m| &m.category_id)
        });
        for id in [picked, mapped].into_iter().flatten() {
            if super::bank_accounts::category_exists(conn, id)? {
                return Ok(Some(id.clone()));
            }
        }
        Ok(None)
    }
    ```
    `resolve_row_category` becomes `explicit_category` (source `SOURCE_MANUAL`), then the rules
    as today. In `import_rows`, the explicit category sets `insert.category_id` and
    `categorization_source = Some(SOURCE_MANUAL)` before `enrich_with_rules`.
  - Batch period in `import_rows`:
    `parsed.period.map(|p| (p.from, p.to)).or(statement_range.map(|r| (r.from, r.to)))`.
  - `statement_balance` in the result: when `parsed.declared_balance` is set, build
    `StatementBalance { amount, currency, as_of, line: parsed.rows.last().map_or(0, |r| r.line), batch_id }`.
    Otherwise use today's row-derived function. `last_balance` is
    `parsed.declared_balance.as_ref().map(|b| b.amount.clone()).or_else(|| last_balance(&parsed.rows))`.
  - Re-export `ImportOptions`, `DeclaredBalance`, `SourceCategoryMapping` and `ParsedImport`
    from `csv_import` (`pub use` in `mod.rs`), so `statement_import` and the tests reach them
    as `csv_import::…`.
  - Register `DeclaredBalance` and `SourceCategoryMapping` in `bindings.rs`. Mirror them, and
    `CsvPreviewRow.sourceCategory: string | null`, in `shared/schema.ts`. Regenerate
    `generated-types.ts`.

- [ ] **Step 4: Run the new tests and the whole CSV suite.** The CSV suite must pass without
  edits.

```bash
cd src-tauri && cargo test csv_import
```

- [ ] **Step 5: Gate and commit** — `refactor(import): shared core takes parsed rows and import options`.

### Task 2: Overlap hint for every import (D12)

**Files:**
- Modify: `src-tauri/src/services/csv_import/mod.rs`, `types.rs`, `core_tests.rs`
- Modify: `src-tauri/src/bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts`
- Modify: `src/components/bank-accounts/csv-import/PreviewStep.tsx`
- Modify: `src/i18n/locales/*/bank_accounts.json` (10 files)

**Interfaces:**
- Consumes: `preview_rows` (Task 1).
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
  pub struct ImportOverlapBatch { #[serde(rename = "fileName")] pub file_name: String, #[serde(rename = "dateRange")] pub date_range: CsvDateRange }
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
  pub struct ImportOverlap { #[serde(rename = "newRowsInCoveredRange")] pub new_rows_in_covered_range: usize, pub batches: Vec<ImportOverlapBatch> }
  // CsvImportPreview gains: pub overlap: Option<ImportOverlap>
  ```

- [ ] **Step 1: Failing tests** in `core_tests.rs`:

```rust
fn earlier_batch(conn: &rusqlite::Connection, from_day: i64, to_day: i64) {
    conn.execute(
        "INSERT INTO csv_import_batches (id, bank_account_id, file_name, date_from, date_to) VALUES ('b1', 'acc', 'june.csv', ?1, ?2)",
        [from_day * DAY, to_day * DAY],
    )
    .unwrap();
}

#[test]
fn overlap_counts_new_rows_inside_an_earlier_batch_excluding_its_last_day() {
    let conn = db();
    earlier_batch(&conn, 20_010, 20_020);
    let rows = parsed(vec![
        candidate(1, 20_012, "1", false, None), // inside
        candidate(2, 20_020, "2", false, None), // the batch's last day: not counted
        candidate(3, 20_025, "3", false, None), // after
    ]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    let overlap = preview.overlap.expect("overlap");
    assert_eq!(overlap.new_rows_in_covered_range, 1);
    assert_eq!(overlap.batches[0].file_name, "june.csv");
}

#[test]
fn no_overlap_without_new_rows_in_range() {
    let conn = db();
    earlier_batch(&conn, 20_010, 20_020);
    let rows = parsed(vec![candidate(1, 20_030, "1", false, None)]);
    let preview = preview_rows(&conn, "acc", &rows, &ImportOptions::default(), None).unwrap();
    assert_eq!(preview.overlap, None);
}

#[test]
fn csv_preview_reports_overlap_too() {
    let conn = db();
    // 2026-06-12 = day 20_616
    earlier_batch(&conn, 20_610, 20_620);
    let config = CsvImportConfig {
        date_column: "Date".into(),
        date_format: "%Y-%m-%d".into(),
        amount_column: Some("Amount".into()),
        description_columns: Some(vec!["Description".into()]),
        ..CsvImportConfig::default()
    };
    let preview = preview_import(&conn, "acc", "Date,Amount,Description\n2026-06-12,-10,Coffee\n", &config, None).unwrap();
    assert_eq!(preview.overlap.map(|o| o.new_rows_in_covered_range), Some(1));
}
```

- [ ] **Step 2: Run and see them fail.** `cargo test csv_import::core_tests::overlap`

- [ ] **Step 3: Implement** in `mod.rs`:

```rust
/// D12: `ok` rows dated inside an earlier batch of the account, last day excluded (rows
/// booked later on the day of the previous export are new, not overlap). Up to three
/// batches, newest first.
fn import_overlap(conn: &Connection, account_id: &str, ok_days: &[i64]) -> Result<Option<ImportOverlap>> {
    if ok_days.is_empty() {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT file_name, date_from, date_to FROM csv_import_batches
         WHERE bank_account_id = ?1 AND date_from IS NOT NULL AND date_to IS NOT NULL
         ORDER BY imported_at DESC, id",
    )?;
    let batches: Vec<(String, i64, i64)> = stmt
        .query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let covers = |day: i64, from: i64, to: i64| day >= from && day < to;
    let count = ok_days.iter().filter(|&&d| batches.iter().any(|&(_, f, t)| covers(d, f, t))).count();
    if count == 0 {
        return Ok(None);
    }
    let hit: Vec<ImportOverlapBatch> = batches
        .into_iter()
        .filter(|&(_, f, t)| ok_days.iter().any(|&d| covers(d, f, t)))
        .take(3)
        .map(|(file_name, from, to)| ImportOverlapBatch { file_name, date_range: CsvDateRange { from, to } })
        .collect();
    Ok(Some(ImportOverlap { new_rows_in_covered_range: count, batches: hit }))
}
```

  In `preview_rows`, push `insert.booking_date` to `ok_days` for every row that gets
  `CsvRowStatus::Ok` (the whole file, not only the page), and set
  `overlap: import_overlap(conn, account_id, &ok_days)?`. Register the two types, mirror them
  in `schema.ts` (`overlap: ImportOverlap | null` on `CsvImportPreview`) and regenerate.

- [ ] **Step 4: Show the hint.** In `PreviewStep.tsx`, above the table and below the summary
  bar, render a notice when `preview.overlap` is set. Use the same notice styling as the
  existing preview notes in that file.

```tsx
{preview?.overlap && (
  <p className="mb-3 rounded-r3 border border-line bg-paper-2 px-3.5 py-2.5 text-micro text-ink-3">
    {t('csvImport.overlapHint', {
      count: preview.overlap.newRowsInCoveredRange,
      file: preview.overlap.batches[0]?.fileName ?? '',
    })}
  </p>
)}
```

  Keys (`bank_accounts`, plural by `count` per `docs/standards/i18n.md` §2: `_one`/`_other` in
  English, `_one`/`_few`/`_many`/`_other` in cs and pl, `_other` only in ja and zh):
  - en `csvImport.overlapHint_one`: "1 new payment falls in a period that {{file}} already covers. If you imported the same payments in another format, they may not be recognized as duplicates."
  - en `csvImport.overlapHint_other`: "{{count}} new payments fall in a period that {{file}} already covers. If you imported the same payments in another format, they may not be recognized as duplicates."
  - cs `_one`: "1 nová platba spadá do období, které už pokrývá {{file}}. Pokud jste tytéž platby importovali v jiném formátu, nemusí je Moony poznat jako duplicity."
  - cs `_few`: "{{count}} nové platby spadají do období, které už pokrývá {{file}}. Pokud jste tytéž platby importovali v jiném formátu, nemusí je Moony poznat jako duplicity."
  - cs `_many`/`_other`: "{{count}} nových plateb spadá do období, které už pokrývá {{file}}. Pokud jste tytéž platby importovali v jiném formátu, nemusí je Moony poznat jako duplicity."
  - de, es, it, nb, pl, sv, ja, zh: translate the English text with the same `{{count}}` and
    `{{file}}` variables.

- [ ] **Step 5: Run** `cargo test csv_import` and `npm test -- src/i18n/locales.test.ts`, then
  the gate.
- [ ] **Step 6: Commit** — `feat(import): warn when new rows fall inside an earlier import's period`.

### Task 3: Statement model, exact decimals, `to_rows` with the ID guard

**Files:**
- Create: `src-tauri/src/services/statement_import/mod.rs` (module declarations and
  re-exports for now)
- Create: `src-tauri/src/services/statement_import/model.rs`, `decimal.rs`, `to_rows.rs`
- Modify: `src-tauri/src/services/mod.rs` (`pub mod statement_import;`)

**Interfaces:**
- Consumes: `ParsedImport`, `ParsedRow`, `RowState`, `DeclaredBalance` (Task 1),
  `dedup::canonical_amount`, `models::currency::is_iso_4217`, `InsertBankTransaction::validate`.
- Produces:
  ```rust
  // model.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
  pub enum StatementFormat { #[serde(rename = "camt053")] Camt053, #[serde(rename = "mt940")] Mt940, #[serde(rename = "ofx")] Ofx, #[serde(rename = "qif")] Qif }
  impl StatementFormat { pub fn label(self) -> &'static str; } // "CAMT.053", "MT940", "OFX", "QIF"
  #[derive(Debug, Clone, Default, PartialEq)]
  pub struct AccountRef { pub identifier: Option<String>, pub bank_code: Option<String>, pub name: Option<String> }
  #[derive(Debug, Clone, Default, PartialEq)]
  pub struct Statement {
      pub account: AccountRef, pub currency: Option<String>, pub period: Option<(i64, i64)>,
      pub opening: Option<DeclaredBalance>, pub closing: Option<DeclaredBalance>,
      pub sequence: Option<String>, pub entries: Vec<Entry>,
  }
  #[derive(Debug, Clone, Default, PartialEq)]
  pub struct Entry {
      pub booking_date: i64, pub value_date: Option<i64>,
      pub amount: String, pub is_credit: bool, pub currency: Option<String>,
      pub bank_id: Option<String>, pub description: Option<String>,
      pub counterparty_name: Option<String>, pub counterparty_account: Option<String>,
      pub counterparty_bank_code: Option<String>, pub counterparty_country: Option<String>,
      pub pending: bool, pub source_category: Option<String>,
      /// R2: row message key of an entry that is shown but never imported.
      pub skip_reason: Option<&'static str>,
  }
  pub fn utc_day(date: chrono::NaiveDate) -> i64; // midnight UTC seconds
  /// Joins the non-empty parts with " | " (the CSV multi-column separator); None when all are empty.
  pub fn join_description<'a>(parts: impl IntoIterator<Item = &'a str>) -> Option<String>;

  // decimal.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Dec { mantissa: i128, scale: u32 }
  impl Dec {
      pub fn parse(text: &str) -> Option<Dec>;
      pub fn zero() -> Dec;
      pub fn add(self, other: Dec) -> Option<Dec>;
      pub fn neg(self) -> Dec;
      pub fn is_zero(self) -> bool;
      pub fn to_text(self) -> String; // canonical: "-12.5", "1250.5", "0"
  }

  // to_rows.rs
  pub fn to_rows(statement: &Statement, account_id: &str) -> ParsedImport;
  ```

- [ ] **Step 1: Failing tests.** In `decimal.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::Dec;

    fn d(s: &str) -> Dec { Dec::parse(s).expect(s) }

    #[test]
    fn adds_exactly() {
        assert_eq!(d("0.1").add(d("0.2")).unwrap(), d("0.3"));
        assert_eq!(d("1000.00").add(d("500.50")).unwrap().add(d("-250")).unwrap().to_text(), "1250.5");
    }

    #[test]
    fn canonical_text() {
        assert_eq!(d("-012.50").to_text(), "-12.5");
        assert_eq!(d("0.000").to_text(), "0");
        assert_eq!(d("+7").to_text(), "7");
    }

    #[test]
    fn rejects_non_decimals() {
        for bad in ["", "1,5", "1.2.3", "abc", "1e5", "-"] {
            assert!(Dec::parse(bad).is_none(), "{bad}");
        }
    }
}
```

  In `to_rows.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::csv_import::RowState;

    fn entry(amount: &str, credit: bool, id: Option<&str>) -> Entry {
        Entry { booking_date: 20_000 * 86_400, amount: amount.into(), is_credit: credit, bank_id: id.map(Into::into), ..Entry::default() }
    }

    fn statement(entries: Vec<Entry>) -> Statement {
        Statement { currency: Some("CZK".into()), entries, ..Statement::default() }
    }

    fn insert(row: &crate::services::csv_import::ParsedRow) -> &crate::models::InsertBankTransaction {
        match &row.state { RowState::Candidate(i) => i, other => panic!("not a candidate: {other:?}") }
    }

    #[test]
    fn entries_become_candidates_with_ordinals() {
        let parsed = to_rows(&statement(vec![entry("500.5", true, Some("A")), entry("250", false, Some("B"))]), "acc");
        assert_eq!(parsed.rows.iter().map(|r| r.line).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(parsed.rows[1].signed_amount.as_deref(), Some("-250"));
        let second = insert(&parsed.rows[1]);
        assert_eq!((second.tx_type.as_str(), second.amount.as_str()), ("debit", "250"));
        assert_eq!(second.currency.as_deref(), Some("CZK"));
        assert_eq!(second.status.as_deref(), Some("booked"));
    }

    #[test]
    fn repeated_ids_are_dropped_unique_ids_trimmed() {
        let parsed = to_rows(&statement(vec![entry("1", true, Some("X")), entry("2", true, Some(" X ")), entry("3", true, Some(" Y "))]), "acc");
        assert_eq!(insert(&parsed.rows[0]).transaction_id, None);
        assert_eq!(insert(&parsed.rows[1]).transaction_id, None);
        assert_eq!(insert(&parsed.rows[2]).transaction_id.as_deref(), Some("Y"));
    }

    #[test]
    fn pending_zero_skipped_and_bad_currency() {
        let mut pending = entry("1", true, None);
        pending.pending = true;
        let mut skipped = entry("2", true, None);
        skipped.skip_reason = Some("statementImport.notBooked");
        let mut bad = entry("3", true, None);
        bad.currency = Some("XYZ".into());
        let parsed = to_rows(&statement(vec![pending, entry("0.00", true, None), skipped, bad]), "acc");
        assert_eq!(insert(&parsed.rows[0]).status.as_deref(), Some("pending"));
        assert!(matches!(&parsed.rows[1].state, RowState::SkippedZero(m) if m.key == "csvImport.zeroAmount"));
        assert!(matches!(&parsed.rows[2].state, RowState::SkippedFiltered(m) if m.key == "statementImport.notBooked"));
        assert!(matches!(&parsed.rows[3].state, RowState::Error(m) if m.key == "validation.currencyUnknown"));
    }

    #[test]
    fn closing_balance_and_period_are_declared() {
        let mut s = statement(vec![]);
        s.period = Some((1, 2));
        s.closing = Some(DeclaredBalance { amount: "5".into(), currency: "CZK".into(), as_of: 2 });
        let parsed = to_rows(&s, "acc");
        assert_eq!(parsed.period.map(|p| (p.from, p.to)), Some((1, 2)));
        assert_eq!(parsed.declared_balance.map(|b| b.amount), Some("5".into()));
        assert_eq!(parsed.total_rows, 0);
    }
}
```

- [ ] **Step 2: Run and see them fail.** `cargo test statement_import`

- [ ] **Step 3: Implement.**
  - `decimal.rs`: `parse` accepts an optional sign, digits, an optional `.` and up to 18
    fraction digits, nothing else. `add` rescales both sides to the larger scale with
    `checked_mul(10i128.pow(..))` and `checked_add`. `to_text` prints the integer part, `.`
    and the fraction without trailing zeros, with no sign for zero.
  - `to_rows.rs`:
    ```rust
    pub fn to_rows(statement: &Statement, account_id: &str) -> ParsedImport {
        let mut entries = statement.entries.clone();
        drop_repeated_ids(&mut entries);
        let rows: Vec<ParsedRow> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| entry_row(i + 1, e, statement, account_id))
            .collect();
        ParsedImport {
            total_rows: rows.len(),
            rows,
            period: statement.period.map(|(from, to)| CsvDateRange { from, to }),
            declared_balance: statement.closing.clone(),
        }
    }

    /// D6: an ID that is not unique in the import would make the duplicate check treat the
    /// second entry as a certain duplicate and drop a real payment.
    fn drop_repeated_ids(entries: &mut [Entry]) {
        let clean = |id: &Option<String>| id.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let mut counts: std::collections::HashMap<String, usize> = Default::default();
        for e in entries.iter() {
            if let Some(id) = clean(&e.bank_id) { *counts.entry(id).or_default() += 1; }
        }
        for e in entries.iter_mut() {
            e.bank_id = clean(&e.bank_id).filter(|id| counts.get(id) == Some(&1));
        }
    }
    ```
    `entry_row` builds the `ParsedRow`:
    - The currency is `entry.currency`, else `statement.currency`, uppercased.
    - `skip_reason` → `RowState::SkippedFiltered(CsvRowMessage { line, key, detail: None })`.
    - `canonical_amount(&amount) == "0"` → `SkippedZero` with `csvImport.zeroAmount`.
    - A currency that is not `is_iso_4217` → `Error` `validation.currencyUnknown` with the
      currency in `detail`.
    - Otherwise build the `InsertBankTransaction`: `amount = canonical_amount(&e.amount)`,
      `counterparty_iban = e.counterparty_account`,
      `status = Some(if e.pending { "pending" } else { "booked" })`, `flow_kind: None`,
      `category_id: None`. Then call `validate()`: `Err(AppError::Validation(key))` → `Error`
      with that key; any other error → `Error` `csvImport.rowCannotParse` with the display
      text in `detail`.
    - `signed_amount` is the canonical amount with `-` for debits. `counterparty` =
      `counterparty_name`, `balance = None`.
  - `model.rs`: `utc_day(d) = d.and_time(chrono::NaiveTime::MIN).and_utc().timestamp()`.
    `join_description` trims parts, drops empty ones and joins with ` | `.

- [ ] **Step 4: Run** `cargo test statement_import` — PASS.
- [ ] **Step 5: Gate and commit** — `feat(statement-import): statement model, exact decimals and rows`.

### Task 4: Format detection, size and DOCTYPE guards, decoding

**Files:**
- Create: `src-tauri/src/services/statement_import/detect.rs`
- Create: `src-tauri/src/services/statement_import/decode.rs`
- Modify: `src/i18n/locales/*/common.json` (10 files)

**Interfaces:**
- Consumes: `csv_import::decode_csv_content_for_country`, `DecodedCsv`.
- Produces:
  ```rust
  pub const DETECT_BYTES: usize = 4096;
  pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
  /// Ok(None) = not a statement (CSV path). Err = a statement variant Moony refuses.
  pub fn detect(head: &[u8]) -> Result<Option<StatementFormat>>;
  pub fn check_size(len: u64) -> Result<()>;            // validation.statementTooLarge
  pub fn reject_doctype(text: &str) -> Result<()>;      // validation.statementDoctype
  // decode.rs
  pub fn decode(bytes: &[u8], format: StatementFormat, country: Option<&str>) -> DecodedCsv;
  ```

- [ ] **Step 1: Failing tests** (`detect.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn det(s: &str) -> Option<StatementFormat> { detect(s.as_bytes()).unwrap() }
    fn refused(s: &str) -> String { detect(s.as_bytes()).unwrap_err().to_string() }

    #[test]
    fn detects_each_format() {
        assert_eq!(det("OFXHEADER:100\nDATA:OFXSGML\n\n<OFX>"), Some(StatementFormat::Ofx));
        assert_eq!(det("<?xml version=\"1.0\"?><?OFX OFXHEADER=\"200\"?><OFX>"), Some(StatementFormat::Ofx));
        assert_eq!(det("\u{feff}  <?xml version=\"1.0\"?>\n<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.08\">"), Some(StatementFormat::Camt053));
        assert_eq!(det("!Type:Bank\nD1/2'26\n^"), Some(StatementFormat::Qif));
        assert_eq!(det("!account\nNChecking\n^"), Some(StatementFormat::Qif));
        assert_eq!(det(":20:X\r\n:25:CZ6508000000192000145399\r\n:28C:1\r\n:60F:C260831CZK1,00\r\n"), Some(StatementFormat::Mt940));
        assert_eq!(det("{1:F01BANKXXXX}{2:O940}{4:\n:20:X\n:25:Y\n:61:2609050905C1,NTRFNONREF\n-}"), Some(StatementFormat::Mt940));
    }

    #[test]
    fn csv_and_other_text_are_not_statements() {
        assert_eq!(det("Date;Amount;Note:20:x\n2026-01-01;5;a"), None);
        assert_eq!(det("<html><body>"), None);
        assert_eq!(det(""), None);
    }

    #[test]
    fn refuses_other_variants() {
        assert!(refused("<?xml?><Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.052.001.02\">").contains("validation.statementUnsupportedVariant"));
        assert!(refused(":20:X\n:25:Y\n:34F:CZK0,\n:61:2609050905C1,NTRFX\n").contains("validation.statementUnsupportedVariant"));
    }

    #[test]
    fn guards() {
        assert!(check_size(MAX_FILE_BYTES).is_ok());
        assert!(check_size(MAX_FILE_BYTES + 1).unwrap_err().to_string().contains("validation.statementTooLarge"));
        assert!(reject_doctype("<?xml?><!DOCTYPE x [<!ENTITY a \"b\">]><x/>").is_err());
    }
}
```

  And in `decode.rs` (Review Focus 5):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt940_in_windows_1250_with_cz_country() {
        let (bytes, _, _) = encoding_rs::WINDOWS_1250.encode(":86:Žluťoučký kůň\n");
        assert_eq!(decode(&bytes, StatementFormat::Mt940, Some("CZ")).text, ":86:Žluťoučký kůň\n");
    }

    #[test]
    fn ofx_charset_header_wins() {
        let (body, _, _) = encoding_rs::WINDOWS_1252.encode("<NAME>Café");
        let mut bytes = b"OFXHEADER:100\nCHARSET:1252\n\n".to_vec();
        bytes.extend_from_slice(&body);
        assert!(decode(&bytes, StatementFormat::Ofx, Some("CZ")).text.ends_with("<NAME>Café"));
    }

    #[test]
    fn camt_uses_the_xml_declaration() {
        let (body, _, _) = encoding_rs::WINDOWS_1250.encode("<?xml version=\"1.0\" encoding=\"windows-1250\"?><Nm>Kůň</Nm>");
        assert!(decode(&body, StatementFormat::Camt053, None).text.contains("<Nm>Kůň</Nm>"));
    }
}
```

- [ ] **Step 2: Run and see them fail.**

- [ ] **Step 3: Implement.**
  - `detect`: decode `head` lossily as UTF-8, strip a BOM and leading whitespace, then apply
    §7.1 in order, case-insensitively:
    1. `OFXHEADER:` anywhere in the head, or `<OFX>` → `Ofx`.
    2. Starts with `<?xml` or `<`: find the first `xmlns="…"` value. `camt.053.` → `Camt053`;
       `camt.052.`/`camt.054.` →
       `AppError::validation_with("validation.statementUnsupportedVariant", json!({"variant": "camt.052"}))`
       (use the variant that was found). Any other XML → `None`.
    3. First non-empty line starts with `!type:`, `!account` or `!option:` → `Qif`.
    4. Lines (after an optional `{4:`) where some line starts with `:20:`, some with `:25:`,
       and some with `:61:`, `:60f:` or `:60m:`. If a line starts with `:34f:` →
       unsupported variant `MT942`. If the head contains `{2:I950` or `{2:O950` →
       unsupported `MT950`. Otherwise → `Mt940`.
    5. Otherwise `None`.
  - `check_size`: `len > MAX_FILE_BYTES` →
    `validation_with("validation.statementTooLarge", json!({"limitMb": 50}))`.
  - `reject_doctype`: case-insensitive `<!doctype` anywhere → `AppError::Validation("validation.statementDoctype".into())`.
  - `decode`:
    - `Camt053`, and `Ofx` whose head starts with `<?xml`: read
      `encoding="…"` from the first 200 bytes with
      `regex::Regex::new(r#"encoding\s*=\s*["']([A-Za-z0-9._:-]+)["']"#)`, resolve it with
      `encoding_rs::Encoding::for_label`, default UTF-8, and decode with `Encoding::decode`,
      which honours a BOM.
    - OFX 1.x: scan the header lines before the first `<` for `CHARSET:` and `ENCODING:`.
      `ENCODING:UTF-8` → UTF-8. `CHARSET:<digits>` → label `windows-<digits>`. Any other
      `CHARSET` value → that label. `NONE` or nothing → the MT940/QIF rule.
    - `Mt940`, `Qif`: `csv_import::decode_csv_content_for_country(bytes, None, country)`.
  - Add the English keys to `common.json` under `validation`, and translate them in the other
    nine locales:
    - `statementTooLarge`: "This statement file is larger than {{limitMb}} MB."
    - `statementDoctype`: "This XML file contains a document type declaration, which Moony does not read for safety."
    - `statementUnsupportedVariant`: "Moony cannot import {{variant}} files. Export a CAMT.053, MT940, OFX or QIF statement instead."
    - Czech: "Soubor výpisu je větší než {{limitMb}} MB." / "Tento XML soubor obsahuje deklaraci typu dokumentu, kterou Moony z bezpečnostních důvodů nečte." / "Soubory {{variant}} Moony importovat neumí. Exportujte výpis ve formátu CAMT.053, MT940, OFX nebo QIF."

- [ ] **Step 4: Run** `cargo test statement_import` and `npm test -- src/i18n/locales.test.ts`.
- [ ] **Step 5: Gate and commit** — `feat(statement-import): detect statement formats and decode them`.

### Task 5: Merging statements and the balance check

**Files:**
- Create: `src-tauri/src/services/statement_import/merge.rs`

**Interfaces:**
- Consumes: `Statement`, `Dec` (Task 3),
  `financial_semantics::canonical_account_identifier`.
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
  pub enum BalanceCheck { #[serde(rename = "ok")] AddsUp, #[serde(rename = "mismatch")] Mismatch, #[serde(rename = "unavailable")] Unavailable }
  #[derive(Debug, Clone, PartialEq)]
  pub struct MergedStatement { pub statement: Statement, pub balance_check: BalanceCheck, pub balance_difference: Option<String> }
  pub fn merge(statements: Vec<Statement>) -> Vec<MergedStatement>;
  ```

- [ ] **Step 1: Failing tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::csv_import::types::DeclaredBalance;

    fn bal(amount: &str, day: i64) -> Option<DeclaredBalance> {
        Some(DeclaredBalance { amount: amount.into(), currency: "CZK".into(), as_of: day * 86_400 })
    }
    fn e(amount: &str, credit: bool, day: i64) -> Entry {
        Entry { amount: amount.into(), is_credit: credit, booking_date: day * 86_400, ..Entry::default() }
    }
    fn st(iban: &str, seq: &str, opening: &str, closing: &str, entries: Vec<Entry>) -> Statement {
        Statement {
            account: AccountRef { identifier: Some(iban.into()), ..AccountRef::default() },
            currency: Some("CZK".into()),
            sequence: Some(seq.into()),
            opening: bal(opening, 1),
            closing: bal(closing, 2),
            entries,
            ..Statement::default()
        }
    }
    const A: &str = "CZ6508000000192000145399";

    #[test]
    fn same_account_pages_merge_in_sequence_order() {
        let merged = merge(vec![
            st(A, "00002", "1100", "1000", vec![e("100", false, 2)]),
            st(&A.to_lowercase(), "00001", "1000", "1100", vec![e("100", true, 1)]),
        ]);
        assert_eq!(merged.len(), 1);
        let m = &merged[0];
        assert_eq!(m.statement.entries.len(), 2);
        assert!(m.statement.entries[0].is_credit);
        assert_eq!(m.statement.opening.as_ref().unwrap().amount, "1000");
        assert_eq!(m.statement.closing.as_ref().unwrap().amount, "1000");
        assert_eq!(m.balance_check, BalanceCheck::AddsUp);
    }

    #[test]
    fn chain_break_is_a_mismatch_with_its_difference() {
        let merged = merge(vec![
            st(A, "1", "1000", "1100", vec![e("100", true, 1)]),
            st(A, "2", "1150", "1150", vec![]),
        ]);
        assert_eq!((merged[0].balance_check, merged[0].balance_difference.as_deref()), (BalanceCheck::Mismatch, Some("50")));
    }

    #[test]
    fn sum_mismatch_pending_and_skipped_excluded() {
        let mut pending = e("999", false, 1);
        pending.pending = true;
        let mut skipped = e("5", true, 1);
        skipped.skip_reason = Some("statementImport.notBooked");
        let ok = merge(vec![st(A, "1", "0.1", "0.3", vec![e("0.2", true, 1), pending, skipped])]);
        assert_eq!(ok[0].balance_check, BalanceCheck::AddsUp);
        let bad = merge(vec![st(A, "1", "10", "30", vec![e("15", true, 1)])]);
        assert_eq!((bad[0].balance_check, bad[0].balance_difference.as_deref()), (BalanceCheck::Mismatch, Some("5")));
    }

    #[test]
    fn different_accounts_or_currencies_stay_apart_and_missing_balance_is_unavailable() {
        let mut eur = st(A, "1", "0", "0", vec![]);
        eur.currency = Some("EUR".into());
        let mut open_only = st("CZ5508000000001234567899", "1", "0", "0", vec![]);
        open_only.closing = None;
        let merged = merge(vec![st(A, "1", "0", "0", vec![]), eur, open_only]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[2].balance_check, BalanceCheck::Unavailable);
    }
}
```

- [ ] **Step 2: Run and see them fail.**

- [ ] **Step 3: Implement `merge`.**
  - Group key: `(account key, currency uppercased)`. The account key is
    `canonical_account_identifier(id, bank_code)` when there is an identifier, else
    `name:<account name>`, else `""`. Groups keep the order of their first statement in the
    file.
  - Within a group, sort by `sequence` (compare as numbers when both parse as `u64` after
    removing everything from the first `/`, as in MT940 `00009/001`, else as strings), then by
    `period` start. The sort must be stable.
  - Concatenate the entries. `opening` = the first statement's, `closing` = the last's,
    `period` = (min start, max end) over statements that have one, `account` and `currency`
    from the first.
  - Check: walk consecutive pieces. When `prev.closing` and `next.opening` are both present
    and differ → `Mismatch` with `next.opening − prev.closing`; stop there. Compare `Dec`
    values by subtracting (`a.add(b.neg())?.is_zero()`), never with `==`: the derived
    equality also compares the scale, so `0.30` and `0.3` would differ.
    Otherwise, if the merged opening and closing are both present: `sum` = Σ over entries
    with `skip_reason == None && !pending` of `±amount`, and
    `diff = closing − (opening + sum)`. Zero → `AddsUp`, else `Mismatch` with `diff.to_text()`.
    Missing balances → `Unavailable`. A balance or amount that `Dec::parse` rejects →
    `Unavailable`. An overflow (`None` from `add`) → `Unavailable`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Gate and commit** — `feat(statement-import): merge statements and check balances`.

### Task 6: Account match

**Files:**
- Modify: `src-tauri/src/services/financial_semantics.rs` (`find_account_by_identifier` + tests)
- Create: `src-tauri/src/services/statement_import/account_match.rs`
- Create: `src-tauri/src/services/statement_import/test_support.rs`

**Interfaces:**
- Consumes: `canonical_structured_identifier`, `valid_iban`, `account_number_key`,
  `AccountRef`.
- Produces:
  ```rust
  // financial_semantics.rs
  pub enum IdentifierMatch { Iban(Vec<String>), Number(Vec<String>), Unrecognized }
  /// Bank account ids whose stored iban/bban names the same account as `identifier`.
  pub fn accounts_matching_identifier(conn: &Connection, identifier: &str, bank_code: Option<&str>) -> Result<IdentifierMatch>;
  // account_match.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
  pub enum AccountMatch { #[serde(rename = "selected")] Selected, #[serde(rename = "other")] Other, #[serde(rename = "none")] NoMatch, #[serde(rename = "unknown")] Unknown }
  pub fn match_account(conn: &Connection, selected_id: &str, account: &AccountRef) -> Result<(AccountMatch, Option<String>)>;
  // test_support.rs
  pub fn setup_statement_db() -> rusqlite::Connection; // csv_import::test_support::setup_import_db + iban, bban, institution_id columns + institutions(id, country)
  ```

- [ ] **Step 1: Failing tests** in `account_match.rs` (Review Focus 2):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::statement_import::test_support::setup_statement_db;

    const IBAN: &str = "CZ6508000000192000145399";

    fn db() -> rusqlite::Connection {
        let conn = setup_statement_db();
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, currency, iban, bban) VALUES
               ('main', 'Main', 'CZK', 'cz65 0800 0000 1920 0014 5399', NULL),
               ('domestic', 'Old', 'CZK', NULL, '19-2000145399/0800'),
               ('savings', 'Savings', 'CZK', 'CZ5508000000001234567899', NULL),
               ('bare', 'Cash', 'CZK', NULL, NULL);",
        )
        .unwrap();
        conn
    }
    fn file(id: &str) -> AccountRef { AccountRef { identifier: Some(id.into()), ..AccountRef::default() } }

    #[test]
    fn formatting_differences_still_match() {
        let conn = db();
        conn.execute("DELETE FROM bank_accounts WHERE id = 'domestic'", []).unwrap();
        assert_eq!(match_account(&conn, "main", &file("CZ65 0800 0000 1920 0014 5399")).unwrap(), (AccountMatch::Selected, Some("main".into())));
    }

    #[test]
    fn czech_domestic_bban_matches_the_iban() {
        let conn = db();
        conn.execute("DELETE FROM bank_accounts WHERE id = 'main'", []).unwrap();
        assert_eq!(match_account(&conn, "domestic", &file(IBAN)).unwrap(), (AccountMatch::Selected, Some("domestic".into())));
    }

    #[test]
    fn other_none_and_unknown() {
        let conn = db();
        conn.execute("DELETE FROM bank_accounts WHERE id = 'domestic'", []).unwrap();
        assert_eq!(match_account(&conn, "savings", &file(IBAN)).unwrap(), (AccountMatch::Other, Some("main".into())));
        assert_eq!(match_account(&conn, "savings", &file("DE89370400440532013000")).unwrap(), (AccountMatch::NoMatch, None));
        assert_eq!(match_account(&conn, "bare", &file("DE89370400440532013000")).unwrap(), (AccountMatch::Unknown, None));
        assert_eq!(match_account(&conn, "savings", &AccountRef::default()).unwrap(), (AccountMatch::Unknown, None));
        // R5: an unrecognized domestic notation never forces the confirmation
        assert_eq!(match_account(&conn, "savings", &file("ACC-77")).unwrap(), (AccountMatch::Unknown, None));
    }
}
```

- [ ] **Step 2: Run and see them fail.**

- [ ] **Step 3: Implement.**
  - `accounts_matching_identifier`:
    - Compute `wanted = canonical_structured_identifier(identifier, bank_code, None)`.
    - If `valid_iban(&wanted)`: for each row of `SELECT ba.id, ba.iban, ba.bban, i.country
      FROM bank_accounts ba LEFT JOIN institutions i ON i.id = ba.institution_id ORDER BY
      ba.id`, the row matches when `canonical_structured_identifier(raw, None,
      country.as_deref())` equals `wanted` for its `iban` or its `bban`. Return
      `Iban(ids)`.
    - Else build the keys `account_number_key(identifier)` and, with a bank code,
      `account_number_key(&format!("{bank_code}{identifier}"))`. A US routing number plus a
      9-digit account only becomes a key in the second form. If neither exists →
      `Unrecognized`. Otherwise match rows whose `account_number_key(iban)` or
      `account_number_key(bban)` equals one of the keys, and return `Number(ids)`.
    - Else `Unrecognized`.
  - `match_account`:
    - No identifier → `Unknown`.
    - `Iban(ids)`: the selected id is in `ids` → `Selected`. Exactly one other id →
      `Other(id)`. No ids and the selected account has a non-empty `iban` or `bban` →
      `NoMatch`. Otherwise `Unknown`.
    - `Number(ids)`: same, except that an empty match is always `Unknown` (R5).
    - `Unrecognized` → `Unknown`.
    - Several other matches → `Unknown`, so the preview never guesses between two accounts.
  - Unit tests for `accounts_matching_identifier` in `financial_semantics.rs`: an IBAN against
    a stored Czech bban; a US-style `021000021` + `123456789` against a stored bban
    `021000021123456789`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Gate and commit** — `feat(statement-import): recognise the account a statement belongs to`.

### Task 7: CAMT.053 parser

**Files:**
- Modify: `src-tauri/Cargo.toml` (`quick-xml = "0.42"`)
- Create: `src-tauri/src/services/statement_import/xml_tree.rs`
- Create: `src-tauri/src/services/statement_import/camt.rs`
- Create: `src-tauri/tests/fixtures/statements/camt053/basic.xml` (the fixture below)
- Create: `src-tauri/tests/fixtures/statements/camt053/batch-and-pending.xml`

**Interfaces:**
- Consumes: `Statement`, `Entry`, `AccountRef`, `utc_day`, `join_description` (Task 3),
  `reject_doctype` (Task 4), `canonical_amount`.
- Produces:
  ```rust
  // xml_tree.rs — a small DOM over quick-xml events
  #[derive(Debug, Clone, Default)]
  pub struct Node { pub name: String /* local name */, pub attrs: Vec<(String, String)>, pub text: String, pub children: Vec<Node> }
  impl Node {
      pub fn child(&self, name: &str) -> Option<&Node>;
      pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node>;
      pub fn at(&self, path: &[&str]) -> Option<&Node>;        // first match along the path
      pub fn text_at(&self, path: &[&str]) -> Option<&str>;    // trimmed, None when empty
      pub fn attr(&self, name: &str) -> Option<&str>;
  }
  pub fn parse_xml(text: &str) -> Result<Node>; // validation.statementMalformed on any XML error
  // camt.rs
  pub fn parse(text: &str) -> Result<Vec<Statement>>;
  ```

- [ ] **Step 1: Write the fixture** `camt053/basic.xml`. It balances: 1000.00 + 500.50 − 250.00 =
  1250.50.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
 <BkToCstmrStmt>
  <GrpHdr><MsgId>M1</MsgId><CreDtTm>2026-10-01T06:00:00</CreDtTm></GrpHdr>
  <Stmt>
   <Id>S1</Id><ElctrncSeqNb>9</ElctrncSeqNb>
   <FrToDt><FrDtTm>2026-09-01T00:00:00</FrDtTm><ToDtTm>2026-09-30T23:59:59</ToDtTm></FrToDt>
   <Acct><Id><IBAN>CZ6508000000192000145399</IBAN></Id><Ccy>CZK</Ccy></Acct>
   <Bal><Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp><Amt Ccy="CZK">1000.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-08-31</Dt></Dt></Bal>
   <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="CZK">1250.50</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-09-30</Dt></Dt></Bal>
   <Ntry>
    <NtryRef>N1</NtryRef><Amt Ccy="CZK">500.50</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts>
    <BookgDt><Dt>2026-09-05</Dt></BookgDt><ValDt><Dt>2026-09-05</Dt></ValDt><AcctSvcrRef>REF-1</AcctSvcrRef>
    <NtryDtls><TxDtls>
     <RltdPties><Dbtr><Nm>Acme &amp; Söhne s.r.o.</Nm></Dbtr><DbtrAcct><Id><IBAN>CZ5508000000001234567899</IBAN></Id></DbtrAcct></RltdPties>
     <RltdAgts><DbtrAgt><FinInstnId><BIC>GIBACZPX</BIC></FinInstnId></DbtrAgt></RltdAgts>
     <RmtInf><Ustrd>Faktura 2026-17</Ustrd></RmtInf>
    </TxDtls></NtryDtls>
   </Ntry>
   <Ntry>
    <Amt Ccy="CZK">250.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>BOOK</Sts>
    <BookgDt><DtTm>2026-09-20T23:30:00+02:00</DtTm></BookgDt><AcctSvcrRef>REF-2</AcctSvcrRef>
    <NtryDtls><TxDtls><RltdPties><Cdtr><Nm>Lidl &#268;R</Nm></Cdtr></RltdPties><AddtlTxInf>Platba kartou</AddtlTxInf></TxDtls></NtryDtls>
   </Ntry>
  </Stmt>
 </BkToCstmrStmt>
</Document>
```

  `batch-and-pending.xml`, a camt.053.001.08 document (status as `Sts/Cd`, names in `Pty`).
  Booked check: 0 + 100 − 30 = 70; the pending and the `INFO` entries are excluded.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
 <BkToCstmrStmt>
  <GrpHdr><MsgId>M2</MsgId><CreDtTm>2026-10-01T06:00:00</CreDtTm></GrpHdr>
  <Stmt>
   <Id>S2</Id>
   <Acct><Id><IBAN>CZ6508000000192000145399</IBAN></Id><Ccy>CZK</Ccy></Acct>
   <Bal><Tp><CdOrPrtry><Cd>PRCD</Cd></CdOrPrtry></Tp><Amt Ccy="CZK">0.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-08-31</Dt></Dt></Bal>
   <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="CZK">70.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-09-30</Dt></Dt></Bal>
   <Ntry>
    <Amt Ccy="CZK">100.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>
    <BookgDt><Dt>2026-09-10</Dt></BookgDt><AcctSvcrRef>B-1</AcctSvcrRef>
    <NtryDtls>
     <TxDtls><Refs><AcctSvcrRef>B-1-a</AcctSvcrRef></Refs><AmtDtls><TxAmt><Amt Ccy="CZK">60.00</Amt></TxAmt></AmtDtls>
      <RltdPties><Dbtr><Pty><Nm>Jan Novák</Nm></Pty></Dbtr></RltdPties></TxDtls>
     <TxDtls><AmtDtls><TxAmt><Amt Ccy="CZK">40.00</Amt></TxAmt></AmtDtls>
      <RltdPties><Dbtr><Pty><Nm>Eva Malá</Nm></Pty></Dbtr></RltdPties></TxDtls>
    </NtryDtls>
   </Ntry>
   <Ntry>
    <Amt Ccy="CZK">30.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>PDNG</Cd></Sts>
    <BookgDt><Dt>2026-09-28</Dt></BookgDt><AcctSvcrRef>P-1</AcctSvcrRef>
   </Ntry>
   <Ntry>
    <Amt Ccy="CZK">5.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>INFO</Cd></Sts>
    <BookgDt><Dt>2026-09-29</Dt></BookgDt>
   </Ntry>
   <Ntry>
    <NtryRef>N-4</NtryRef><Amt Ccy="CZK">30.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>
    <BookgDt><Dt>2026-09-29</Dt></BookgDt><AddtlNtryInf>Card 1234</AddtlNtryInf>
    <NtryDtls><TxDtls><RltdPties><Cdtr><Pty><Nm>Shop</Nm></Pty></Cdtr></RltdPties></TxDtls></NtryDtls>
   </Ntry>
  </Stmt>
 </BkToCstmrStmt>
</Document>
```

- [ ] **Step 2: Failing tests** in `camt.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::statement_import::model::utc_day;
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> i64 { utc_day(NaiveDate::from_ymd_opt(y, m, d).unwrap()) }
    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/statements/camt053/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn basic_statement() {
        let s = &parse(&fixture("basic.xml")).unwrap()[0];
        assert_eq!(s.account.identifier.as_deref(), Some("CZ6508000000192000145399"));
        assert_eq!(s.currency.as_deref(), Some("CZK"));
        assert_eq!(s.period, Some((day(2026, 9, 1), day(2026, 9, 30))));
        assert_eq!(s.opening.as_ref().map(|b| (b.amount.as_str(), b.as_of)), Some(("1000", day(2026, 8, 31))));
        assert_eq!(s.closing.as_ref().map(|b| b.amount.as_str()), Some("1250.5"));
        assert_eq!(s.sequence.as_deref(), Some("9"));
        let first = &s.entries[0];
        assert_eq!((first.amount.as_str(), first.is_credit, first.bank_id.as_deref()), ("500.5", true, Some("REF-1")));
        assert_eq!(first.counterparty_name.as_deref(), Some("Acme & Söhne s.r.o."));
        assert_eq!(first.counterparty_account.as_deref(), Some("CZ5508000000001234567899"));
        assert_eq!(first.counterparty_bank_code.as_deref(), Some("GIBACZPX"));
        assert_eq!(first.counterparty_country.as_deref(), Some("CZ"));
        assert_eq!(first.description.as_deref(), Some("Faktura 2026-17"));
        assert_eq!(first.value_date, Some(day(2026, 9, 5)));
        let second = &s.entries[1];
        assert_eq!(second.booking_date, day(2026, 9, 20)); // DtTm with +02:00, never shifted
        assert_eq!(second.counterparty_name.as_deref(), Some("Lidl ČR"));
        assert_eq!(second.description.as_deref(), Some("Platba kartou"));
    }

    #[test]
    fn batch_split_pending_info_and_newer_version() {
        let s = &parse(&fixture("batch-and-pending.xml")).unwrap()[0];
        assert_eq!(s.opening.as_ref().map(|b| b.amount.as_str()), Some("0"));
        let ids: Vec<_> = s.entries.iter().map(|e| e.bank_id.as_deref()).collect();
        assert_eq!(ids, vec![Some("B-1-a"), Some("B-1/2"), Some("P-1"), None, Some("N-4")]);
        assert_eq!(s.entries[0].amount, "60");
        assert_eq!(s.entries[0].counterparty_name.as_deref(), Some("Jan Novák"));
        assert!(s.entries[2].pending);
        assert_eq!(s.entries[3].skip_reason, Some("statementImport.notBooked"));
        assert_eq!(s.entries[4].counterparty_name.as_deref(), Some("Shop"));
        assert_eq!(s.entries[4].description.as_deref(), Some("Card 1234"));
    }

    #[test]
    fn batch_that_does_not_add_up_stays_one_entry() {
        let text = fixture("batch-and-pending.xml").replace("<Amt Ccy=\"CZK\">40.00</Amt>", "<Amt Ccy=\"CZK\">41.00</Amt>");
        let s = &parse(&text).unwrap()[0];
        assert_eq!(s.entries[0].amount, "100");
        assert_eq!(s.entries[0].bank_id.as_deref(), Some("B-1"));
        assert_eq!(s.entries[0].counterparty_name.as_deref(), Some("Jan Novák"));
    }

    #[test]
    fn debit_balance_is_negative_and_doctype_refused() {
        let text = fixture("basic.xml").replacen("<CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-08-31", "<CdtDbtInd>DBIT</CdtDbtInd><Dt><Dt>2026-08-31", 1);
        assert_eq!(parse(&text).unwrap()[0].opening.as_ref().unwrap().amount, "-1000");
        let doctype = fixture("basic.xml").replacen("?>", "?><!DOCTYPE Document>", 1);
        assert!(parse(&doctype).unwrap_err().to_string().contains("validation.statementDoctype"));
    }

    #[test]
    fn malformed_xml_is_a_validation_error() {
        assert!(parse("<Document><Stmt>").unwrap_err().to_string().contains("validation.statementMalformed"));
    }
}
```

- [ ] **Step 3: Run and see them fail.**

- [ ] **Step 4: Implement.**
  - `Cargo.toml`: `quick-xml = "0.42"` with a comment `# CAMT.053 statements (statement_import)`.
  - `xml_tree::parse_xml`: `quick_xml::Reader::from_str(text)`. Keep a stack of `Node`s:
    - `Event::Start` pushes a node (local name via `e.local_name()`, attributes decoded and
      unescaped).
    - `Event::Empty` appends a childless node to the top.
    - `Event::Text` appends the decoded text to the top's `text`.
    - `Event::CData` appends raw.
    - `Event::GeneralRef` (quick-xml ≥ 0.38 reports entity references as their own event):
      append the character from `resolve_char_ref()` or from the predefined XML entities
      `amp lt gt quot apos`. Anything else → `validation.statementMalformed` with
      `{"detail": "entity <name>"}`.
    - `Event::End` pops into the parent.
    - `Event::DocType` → `validation.statementDoctype`.
    Any reader error, or an unclosed element at EOF →
    `validation_with("validation.statementMalformed", json!({"detail": <element name or "xml">}))`.
    Check the 0.42 docs for the exact method names before writing this; the events listed
    are the contract.
  - `camt::parse`:
    - `reject_doctype(text)?`, `parse_xml(text)?`, then
      `root.at(&["BkToCstmrStmt"]).children("Stmt")`. No `Stmt` →
      `validation.statementNoStatements`.
    - Implement the mapping table of spec §6.1 literally. Helpers:
      - `fn iso_day(text: &str) -> Option<i64>` takes the first 10 characters, parses
        `%Y-%m-%d` and applies `utc_day`.
      - `fn balance(stmt: &Node, codes: &[&str]) -> Option<DeclaredBalance>` finds the first
        `Bal` whose `Tp/CdOrPrtry/Cd` is in `codes` in priority order; the amount is
        `canonical_amount` with `-` for `DBIT`, the currency comes from `Amt@Ccy` and the day
        from `Dt/Dt` or `Dt/DtTm`.
      - `fn status(ntry: &Node) -> &str` returns `Sts/Cd` text, else `Sts` text,
        uppercased.
      - `fn party(tx: &Node, credit: bool) -> (name, account, bank_code, country)`: for a
        credit use `Dbtr`/`DbtrAcct`/`DbtrAgt`, for a debit `Cdtr`/`CdtrAcct`/`CdtrAgt`. The
        name is `RltdPties/<P>/Nm`, else `RltdPties/<P>/Pty/Nm`. The account is
        `…Acct/Id/IBAN`, else `…Acct/Id/Othr/Id`. The bank code is
        `RltdAgts/<P>Agt/FinInstnId/BICFI`, else `/BIC`, else `/ClrSysMmbId/MmbId`. The
        country is the first two letters of a valid IBAN, else `<P>/PstlAdr/Ctry` (or
        `<P>/Pty/PstlAdr/Ctry`).
    - Entry details: when `NtryDtls/TxDtls` is absent, the party and remittance lookups run
      on the `Ntry` itself; that covers banks that place `RltdPties` and `RmtInf` on the
      entry.
    - Description: `join_description` over all `RmtInf/Ustrd`, `RmtInf/Strd/CdtrRefInf/Ref`,
      `AddtlTxInf`. Fall back to `Ntry/AddtlNtryInf`.
    - Batch split: let `amounts` be the `AmtDtls/TxAmt/Amt` (else `Amt`) of each `TxDtls`.
      When there are ≥ 2 `TxDtls`, each has an amount, and `Dec` sums them to the `Ntry`
      amount, emit one entry per `TxDtls`. The ID is its own `Refs/AcctSvcrRef`, else
      `<entry id>/<n>`, where the entry id is `Ntry/AcctSvcrRef` or `NtryRef`. Otherwise emit
      one entry using the first `TxDtls` for details.
    - `INFO`, `FUTR` or any status other than `BOOK`/`PDNG` → `skip_reason =
      Some("statementImport.notBooked")`.
    - A `Ntry` without a parseable amount or booking date →
      `validation.statementMalformed` with `{"detail": "Ntry <n>"}`, where n is the 1-based
      `Ntry` index in its statement.
- [ ] **Step 5: Run** `cargo test statement_import::camt` — PASS.
- [ ] **Step 6: Gate and commit** — `feat(statement-import): CAMT.053 parser`.

### Task 8: Statement service, wire types, commands

**Files:**
- Create: `src-tauri/src/services/statement_import/types.rs`
- Modify: `src-tauri/src/services/statement_import/mod.rs` (`load`, `inspect`, `preview`, `import`)
- Create: `src-tauri/src/services/statement_import/service_tests.rs`
- Create: `src-tauri/src/commands/statement_import.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`generate_handler!`),
  `src-tauri/src/bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts`,
  `src/lib/tauri-api.ts`
- Modify: `src/i18n/locales/*/common.json` (10 files)

**Interfaces:**
- Consumes: Tasks 1–7.
- Produces (Rust; TS mirrors per spec §9 with these exact names):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
  pub enum DateOrder { #[serde(rename = "dmy")] Dmy, #[serde(rename = "mdy")] Mdy, #[serde(rename = "ymd")] Ymd }
  pub struct SourceCategoryCount { pub name: String, pub count: usize }
  pub struct StatementWarning { pub key: String, pub detail: Option<String> }
  pub struct StatementSummary {
      pub index: usize, #[serde(rename = "accountIdentifier")] pub account_identifier: Option<String>,
      #[serde(rename = "accountName")] pub account_name: Option<String>, pub currency: Option<String>,
      pub period: Option<CsvDateRange>, #[serde(rename = "openingBalance")] pub opening_balance: Option<DeclaredBalance>,
      #[serde(rename = "closingBalance")] pub closing_balance: Option<DeclaredBalance>,
      #[serde(rename = "entryCount")] pub entry_count: usize, #[serde(rename = "skippedCount")] pub skipped_count: usize,
      #[serde(rename = "balanceCheck")] pub balance_check: BalanceCheck, #[serde(rename = "balanceDifference")] pub balance_difference: Option<String>,
      #[serde(rename = "accountMatch")] pub account_match: AccountMatch, #[serde(rename = "matchedAccountId")] pub matched_account_id: Option<String>,
      #[serde(rename = "sourceCategories")] pub source_categories: Vec<SourceCategoryCount>,
  }
  pub struct StatementInspection {
      pub format: StatementFormat, pub encoding: String, pub statements: Vec<StatementSummary>,
      #[serde(rename = "defaultStatementIndex")] pub default_statement_index: usize,
      #[serde(rename = "dateOrderAmbiguous")] pub date_order_ambiguous: bool,
      #[serde(rename = "detectedDateOrder")] pub detected_date_order: Option<DateOrder>,
      pub warnings: Vec<StatementWarning>,
  }
  pub struct StatementImportOptions { // TS StatementImportOptionsInput
      #[serde(rename = "statementIndex")] pub statement_index: usize,
      #[serde(rename = "dateOrder", default)] pub date_order: Option<DateOrder>,
      #[serde(rename = "skipDuplicates", default = "default_true")] pub skip_duplicates: bool,
      #[serde(rename = "importAnywayLines", default)] pub import_anyway_lines: Vec<usize>,
      #[serde(rename = "categoryOverrides", default)] pub category_overrides: Vec<CsvCategoryOverride>,
      #[serde(rename = "sourceCategoryMap", default)] pub source_category_map: Vec<SourceCategoryMapping>,
      #[serde(rename = "previewOffset", default)] pub preview_offset: usize,
      #[serde(rename = "previewLimit", default = "default_preview_limit")] pub preview_limit: usize,
  }
  impl StatementImportOptions {
      pub fn import_options(&self) -> ImportOptions;
      /// validation.statementIndexInvalid, validation.statementDateOrderInvalid (dateOrder on a non-QIF file)
      pub fn validate(&self, format: StatementFormat, statement_count: usize) -> Result<()>;
  }

  // mod.rs
  pub struct Loaded { pub format: StatementFormat, pub encoding: &'static str, pub statements: Vec<MergedStatement>,
      pub warnings: Vec<StatementWarning>, pub date_order_ambiguous: bool, pub detected_date_order: Option<DateOrder> }
  pub fn load(bytes: &[u8], country: Option<&str>, date_order: Option<DateOrder>) -> Result<Option<Loaded>>;
  pub fn inspect(conn: &Connection, account_id: &str, bytes: &[u8], country: Option<&str>) -> Result<Option<StatementInspection>>;
  pub fn preview(conn: &Connection, account_id: &str, bytes: &[u8], country: Option<&str>,
      options: &StatementImportOptions, engine: Option<&CategorizationEngine>) -> Result<CsvImportPreview>;
  pub fn import(conn: &mut Connection, account_id: &str, file_name: &str, bytes: &[u8], country: Option<&str>,
      options: &StatementImportOptions, engine: Option<&CategorizationEngine>) -> Result<CsvImportResult>;

  // commands/statement_import.rs
  #[tauri::command] pub async fn inspect_statement_file(db, account_id: String, file_path: String, country: Option<String>) -> Result<Option<StatementInspection>>;
  #[tauri::command] pub async fn preview_statement_import(db, categorization, account_id: String, file_path: String, country: Option<String>, options: StatementImportOptions) -> Result<CsvImportPreview>;
  #[tauri::command] pub async fn import_statement_file(app, db, categorization, account_id: String, file_path: String, country: Option<String>, options: StatementImportOptions) -> Result<CsvImportResult>;
  ```
  ```ts
  // src/lib/tauri-api.ts, in bankAccountsApi
  inspectStatementFile: (accountId: string, filePath: string, country?: string) =>
    tauriInvoke<StatementInspection | null>('inspect_statement_file', { accountId, filePath, country }),
  previewStatementImport: (accountId: string, filePath: string, options: StatementImportOptionsInput, country?: string) =>
    tauriInvoke<CsvImportPreview>('preview_statement_import', { accountId, filePath, country, options }),
  importStatementFile: (accountId: string, filePath: string, options: StatementImportOptionsInput, country?: string) =>
    tauriInvoke<CsvImportResult>('import_statement_file', { accountId, filePath, country, options }),
  ```

- [ ] **Step 1: Failing service tests** in `service_tests.rs`. They use
  `setup_statement_db`, the CAMT fixtures, and an account `acc` with IBAN
  `CZ6508000000192000145399`:

```rust
use super::*;
use crate::services::statement_import::test_support::setup_statement_db;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/statements/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn db() -> rusqlite::Connection {
    let conn = setup_statement_db();
    conn.execute_batch(
        "INSERT INTO bank_accounts (id, name, currency, iban) VALUES
           ('acc', 'Main', 'CZK', 'CZ6508000000192000145399'),
           ('other', 'Savings', 'CZK', 'CZ5508000000001234567899');",
    ).unwrap();
    conn
}
fn options() -> StatementImportOptions {
    StatementImportOptions { statement_index: 0, date_order: None, skip_duplicates: true, import_anyway_lines: vec![],
        category_overrides: vec![], source_category_map: vec![], preview_offset: 0, preview_limit: 200 }
}

#[test]
fn csv_is_not_a_statement() {
    let conn = db();
    assert!(inspect(&conn, "acc", b"Date,Amount\n2026-01-01,5\n", None).unwrap().is_none());
}

#[test]
fn inspect_camt_summary() {
    let conn = db();
    let insp = inspect(&conn, "acc", &fixture("camt053/basic.xml"), None).unwrap().unwrap();
    assert_eq!(insp.format, StatementFormat::Camt053);
    let s = &insp.statements[0];
    assert_eq!((s.entry_count, s.balance_check, s.account_match), (2, BalanceCheck::AddsUp, AccountMatch::Selected));
    let other = inspect(&conn, "other", &fixture("camt053/basic.xml"), None).unwrap().unwrap();
    assert_eq!((other.statements[0].account_match, other.statements[0].matched_account_id.as_deref()), (AccountMatch::Other, Some("acc")));
}

#[test]
fn import_then_reimport_is_idempotent() {
    let mut conn = db();
    let first = import(&mut conn, "acc", "basic.xml", &fixture("camt053/basic.xml"), None, &options(), None).unwrap();
    assert_eq!(first.imported_count, 2);
    let balance = first.statement_balance.unwrap();
    assert_eq!((balance.amount.as_str(), balance.line), ("1250.5", 2));
    let again = import(&mut conn, "acc", "basic.xml", &fixture("camt053/basic.xml"), None, &options(), None).unwrap();
    assert_eq!((again.imported_count, again.skipped_duplicates.len()), (0, 2));
}

#[test]
fn pending_is_promoted_by_its_booked_successor() {
    let mut conn = db();
    let text = String::from_utf8(fixture("camt053/batch-and-pending.xml")).unwrap();
    import(&mut conn, "acc", "a.xml", text.as_bytes(), None, &options(), None).unwrap();
    let booked = text.replacen("<Cd>PDNG</Cd>", "<Cd>BOOK</Cd>", 1);
    let result = import(&mut conn, "acc", "b.xml", booked.as_bytes(), None, &options(), None).unwrap();
    assert_eq!(result.promoted_count, 1);
}

#[test]
fn empty_statement_previews_without_error() {
    // Review Focus 4
    let conn = db();
    let text = String::from_utf8(fixture("camt053/basic.xml")).unwrap();
    let start = text.find("<Ntry>").unwrap();
    let end = text.rfind("</Ntry>").unwrap() + "</Ntry>".len();
    let quiet = format!("{}{}", &text[..start], &text[end..]).replace("1250.50", "1000.00");
    let insp = inspect(&conn, "acc", quiet.as_bytes(), None).unwrap().unwrap();
    assert_eq!((insp.statements[0].entry_count, insp.statements[0].balance_check), (0, BalanceCheck::AddsUp));
    let preview = preview(&conn, "acc", quiet.as_bytes(), None, &options(), None).unwrap();
    assert_eq!((preview.total_rows, preview.ok_count), (0, 0));
}

#[test]
fn options_are_validated() {
    let conn = db();
    let bad = StatementImportOptions { statement_index: 3, ..options() };
    assert!(preview(&conn, "acc", &fixture("camt053/basic.xml"), None, &bad, None).unwrap_err().to_string().contains("validation.statementIndexInvalid"));
    let order = StatementImportOptions { date_order: Some(DateOrder::Dmy), ..options() };
    assert!(preview(&conn, "acc", &fixture("camt053/basic.xml"), None, &order, None).unwrap_err().to_string().contains("validation.statementDateOrderInvalid"));
}

#[test]
fn not_yet_supported_formats_say_so() {
    let conn = db();
    let err = inspect(&conn, "acc", b"!Type:Bank\nD1/2/26\nT5\n^\n", None).unwrap_err().to_string();
    assert!(err.contains("validation.statementFormatNotYetSupported"));
}
```

  Delete `not_yet_supported_formats_say_so` in Task 14, when the last format lands.

- [ ] **Step 2: Run and see them fail.**

- [ ] **Step 3: Implement the service.**
  - `load`: `detect(&bytes[..min(len, DETECT_BYTES)])?`. `None` → `Ok(None)`. Then
    `check_size(bytes.len() as u64)?`, `decode`, and dispatch: `Camt053 → camt::parse`. Every
    other format →
    `validation_with("validation.statementFormatNotYetSupported", json!({"format": format.label()}))`
    (R8; each later phase replaces its arm). An empty parse →
    `validation.statementNoStatements`. Then `merge`. The warnings and date order fields come
    from the parser; they are empty or false for CAMT.
  - `inspect`: `load(...)?` → for each merged statement build a `StatementSummary`:
    - `entry_count` = entries without `skip_reason`, `skipped_count` = those with one.
    - `source_categories`: count by name, sorted by count descending, then name.
    - Account match via `match_account`.
    `default_statement_index` is the first `Selected`, else 0.
  - `preview` / `import`: `load(bytes, country, options.date_order)?` — `None` →
    `AppError::Validation("validation.statementNotRecognized".into())`. Then
    `options.validate(loaded.format, loaded.statements.len())?`,
    `to_rows(&loaded.statements[i].statement, account_id)`, and
    `csv_import::preview_rows` / `import_rows` with `options.import_options()`.
  - `validate`: `statement_index >= count` → `validation.statementIndexInvalid`.
    `date_order.is_some() && format != Qif` → `validation.statementDateOrderInvalid`. Also
    run `self.import_options().validate()`.
- [ ] **Step 4: Implement the commands** in `commands/statement_import.rs`, mirroring
  `commands/bank_accounts.rs`:

```rust
/// R7: detection needs only the head; a non-statement is answered without reading the rest,
/// so the statement size cap never applies to a CSV.
fn read_statement_file(path: &str) -> Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| AppError::Internal(format!("Cannot open file: {e}")))?;
    let mut head = vec![0u8; statement_import::DETECT_BYTES];
    let n = file.read(&mut head).map_err(|e| AppError::Internal(format!("Cannot read file: {e}")))?;
    head.truncate(n);
    if statement_import::detect(&head)?.is_none() {
        return Ok(None);
    }
    let len = file.metadata().map_err(|e| AppError::Internal(format!("Cannot read file: {e}")))?.len();
    statement_import::check_size(len)?;
    let mut bytes = head;
    file.read_to_end(&mut bytes).map_err(|e| AppError::Internal(format!("Cannot read file: {e}")))?;
    Ok(Some(bytes))
}
```

  - `inspect_statement_file`: `match read_statement_file(&file_path)? { None => Ok(None),
    Some(bytes) => db.with_conn(|conn| statement_import::inspect(conn, &account_id, &bytes,
    country.as_deref())) }`.
  - `preview_statement_import` and `import_statement_file`: `None` →
    `validation.statementNotRecognized`. The import takes the file name the way
    `import_csv_transactions` does (fallback `"statement"`), and calls
    `portfolio::spawn_fx_backfill` when `imported_count > 0`. Both use
    `categorization.0.clone()` as the engine.
  - Register the three commands in `lib.rs` and the module in `commands/mod.rs`. Register
    every new type in `bindings.rs` and mirror them in `shared/schema.ts` (spec §9 shapes,
    `StatementImportOptionsInput` with optional fields). Add the three wrappers to
    `bankAccountsApi` (above) and regenerate `generated-types.ts`.
  - `common.json` `validation` keys, in all locales:
    - `statementNotRecognized`: "This file is not a statement Moony can read."
    - `statementNoStatements`: "The file contains no account statement."
    - `statementMalformed`: "The statement file is damaged or incomplete ({{detail}})."
    - `statementIndexInvalid`: "Choose one of the statements in the file."
    - `statementDateOrderInvalid`: "A date order can only be chosen for QIF files."
    - `statementFormatNotYetSupported`: "Moony cannot import {{format}} statements yet."
    - Czech: "Tento soubor není výpis, který Moony umí přečíst." / "Soubor neobsahuje žádný výpis z účtu." / "Soubor výpisu je poškozený nebo neúplný ({{detail}})." / "Vyberte jeden z výpisů v souboru." / "Pořadí data lze zvolit jen u souborů QIF." / "Výpisy {{format}} zatím Moony importovat neumí."
- [ ] **Step 5: Run** `cargo test statement_import`, the bindings regeneration and
  `npm run typecheck`.
- [ ] **Step 6: Gate and commit** — `feat(statement-import): inspect, preview and import statement files`.

### Task 9: Import dialog for statements

**Files:**
- Rename: `src/components/bank-accounts/CsvImportDialog.tsx` → `StatementImportDialog.tsx`
  (`git mv`; component `StatementImportDialog`). Update the imports in
  `src/pages/BankAccountDetail.tsx` and `src/components/auth/OnboardingWizard.tsx`.
- Modify: `src/components/bank-accounts/csv-import/import-config.ts` (+ `import-config.test.ts`)
- Create: `src/components/bank-accounts/csv-import/StatementSummaryCard.tsx`
- Create: `src/components/bank-accounts/csv-import/use-statement-preview.ts`
- Modify: `src/components/bank-accounts/csv-import/use-csv-import-mutations.ts`
- Modify: `src/components/bank-accounts/csv-import/PreviewStep.tsx`,
  `ImportResultStep.tsx`, `FileSelectStep.tsx`
- Modify: `src/utils/csv-import-messages.ts` (+ `csv-import-messages.test.ts`)
- Modify: `src/i18n/locales/*/bank_accounts.json` (10 files)

**Interfaces:**
- Consumes: the three `bankAccountsApi` wrappers and the schema types (Task 8).
- Produces:
  ```ts
  // import-config.ts
  export const SUPPORTED_EXTENSIONS = ['csv', 'txt', 'xml', 'sta', 'mt940', '940', 'ofx', 'qfx', 'qif'] as const;
  export function isSupportedImportPath(path: string): boolean;     // replaces isSupportedCsvPath
  export type ImportKind = 'csv' | 'statement';
  export type ImportStep = 'file' | 'mapping' | 'preview' | 'done';
  export function importSteps(kind: ImportKind): ImportStep[];       // csv: all four; statement: no 'mapping'
  export function statementImportBlocked(summary: StatementSummary | undefined, confirmed: boolean, targetIsSelected: boolean): boolean;
  // use-csv-import-mutations.ts
  export function useStatementImportMutation(): UseMutationResult<CsvImportResult, Error,
    { accountId: string; filePath: string; options: StatementImportOptionsInput; country?: string }>;
  // csv-import-messages.ts
  export function formatRowMessage(message: CsvRowMessage, t: TFunction, tc: TFunction, kind?: ImportKind): string;
  ```

- [ ] **Step 1: Failing tests** in `import-config.test.ts`:

```ts
describe('statement import helpers', () => {
  it('accepts statement extensions', () => {
    for (const path of ['/a/s.xml', 'C:\\x\\S.STA', '/a/s.940', '/a/s.mt940', '/a/s.ofx', '/a/s.QFX', '/a/s.qif', '/a/s.csv']) {
      expect(isSupportedImportPath(path)).toBe(true);
    }
    expect(isSupportedImportPath('/a/s.pdf')).toBe(false);
  });

  it('statements skip the mapping step', () => {
    expect(importSteps('csv')).toEqual(['file', 'mapping', 'preview', 'done']);
    expect(importSteps('statement')).toEqual(['file', 'preview', 'done']);
  });

  it('blocks import until a contradicting account is confirmed', () => {
    const base = { accountMatch: 'selected' } as StatementSummary;
    expect(statementImportBlocked(base, false, true)).toBe(false);
    expect(statementImportBlocked({ ...base, accountMatch: 'none' }, false, true)).toBe(true);
    expect(statementImportBlocked({ ...base, accountMatch: 'none' }, true, true)).toBe(false);
    // 'other' and the user stays on the selected account: needs the confirmation too
    expect(statementImportBlocked({ ...base, accountMatch: 'other' }, false, true)).toBe(true);
    // 'other' and the user switched to the matching account: fine
    expect(statementImportBlocked({ ...base, accountMatch: 'other' }, false, false)).toBe(false);
    expect(statementImportBlocked(undefined, false, true)).toBe(true);
  });
});
```

  The existing `isSupportedCsvPath` tests move to `isSupportedImportPath` unchanged. In
  `csv-import-messages.test.ts`: `formatRowMessage({ line: 3, key: 'statementImport.notBooked', detail: null }, t, tc, 'statement')`
  uses `statementImport.rowMessage`, and without `kind` it uses `csvImport.rowMessage`.

- [ ] **Step 2: Run and see them fail.** `npm test -- src/components/bank-accounts src/utils/csv-import-messages.test.ts`

- [ ] **Step 3: Implement the helpers.** `statementImportBlocked` returns true when the summary
  is missing. Otherwise, when `accountMatch` is `'none'`, or is `'other'` while the target is
  still the selected account, it returns `!confirmed`. Else false. `formatRowMessage` picks
  `statementImport.rowMessage`/`rowMessageDetail` for `kind === 'statement'`.

- [ ] **Step 4: Dialog routing.** In `StatementImportDialog.tsx`:
  - Add state: `kind: ImportKind | null`, `statementInspection: StatementInspection | null`,
    `statementIndex`, `targetAccountId` (initially the `accountId` prop; reset in `reset()`),
    `accountConfirmed: boolean`. `useBankAccount` and the mutation use `targetAccountId`.
  - `handleFilePicked(path)`: first
    `bankAccountsApi.inspectStatementFile(targetAccountId, path, user?.country ?? undefined)`.
    - `null` → set `kind = 'csv'` and run today's `inspectFile` path unchanged.
    - Otherwise set `kind = 'statement'`, store the inspection, set
      `statementIndex = defaultStatementIndex`, clear the overrides and import-anyway lines
      and `accountConfirmed`, then `setStep('preview')`.
    - On error, show the same toast as today. Use the same `inspectRequest` guard against
      stale responses.
  - The file dialog filter becomes
    `[{ name: t('statementImport.fileFilter'), extensions: [...SUPPORTED_EXTENSIONS] }]`.
    `useFileDrop` uses `isSupportedImportPath`; it is wired in `use-file-drop.ts` through
    the finder at `import-config.ts:412`, so rename that finder's predicate.
  - Steps: `importSteps(kind ?? 'csv')` mapped to labels as today.
  - Statement preview: `useStatementPreview({ accountId: targetAccountId, filePath, options,
    country, enabled })`. It is a copy of `useCsvPreview` with the query key
    `['statement-import-preview', accountId, filePath, debounced, country]` and
    `bankAccountsApi.previewStatementImport`. The options are built from `statementIndex`,
    `importAnywayLines`, `overrides`, `previewOffset: previewPage * 100` and
    `previewLimit: 100`.
  - The `useStatementImportMutation` `onSuccess` is identical to `useCsvImportMutation`'s, and
    also removes `['statement-import-preview']`.
  - The preview footer's back button for statements reads `statementImport.back` ("Choose
    another file") and returns to `'file'`.
  - Import is disabled when
    `statementImportBlocked(summary, accountConfirmed, targetAccountId === accountId)` is true,
    in addition to today's `canImport` conditions.
  - Switching the account (D9): `setTargetAccountId(summary.matchedAccountId)`,
    `setAccountConfirmed(false)`, and re-run `inspectStatementFile` with the new account so
    `accountMatch` reads `selected`.
- [ ] **Step 5: `StatementSummaryCard`** renders, above `PreviewStep`:
  - A picker (radio group, labelled by account identifier or name and currency) when
    `statements.length > 1`.
  - A format badge (`statementImport.format.<format>`), the account identifier or name, the
    period (`formatDate` with `locale`), opening and closing balances
    (`NativeBalance`/`formatCurrency` as `PreviewStep` formats amounts), and the entry and
    skipped counts.
  - The check line: `statementImport.check.ok` (success tone), `.mismatch` with
    `{{difference}}` and the currency (warning tone), `.unavailable` (muted).
  - The match block:
    - `selected`: a muted line.
    - `other`: a warning naming the matched account (look it up with `useBankAccounts`)
      with a button `statementImport.match.switch` and the confirmation checkbox
      `statementImport.match.confirm` for staying.
    - `none`: a warning with the checkbox.
    - `unknown`: an info line with the identifier, if there is one.
  - A currency warning `statementImport.currencyDiffers` when the summary currency differs
    from the account currency.
  - The `inspection.warnings`, through `t(key, { detail })`.
  Use the design system tokens (`docs/design-system/README.md`) and the components
  `PreviewStep` already uses (`Badge`, `Checkbox`, `Button`).
- [ ] **Step 6: `PreviewStep`** gets a prop `kind: ImportKind`. For `'statement'` the first
  column header is `t('statementImport.entry')` (`#`) instead of `t('csvImport.line')`, and
  row messages pass `kind` to `formatRowMessage`. `ImportResultStep` gets
  `kind` and `importedAccountId`. When `importedAccountId !== accountId` it shows a button
  `statementImport.openAccount` that calls `setLocation(`/bank-accounts/${importedAccountId}`)`
  (wouter `useLocation`, as `src/pages/BankAccounts.tsx:268` does) and closes the dialog,
  instead of "Show imported transactions". The dialog calls `onImported` only when the target is
  the page's account.
- [ ] **Step 7: Strings** (`bank_accounts`, all ten locales; English and Czech given, the
  rest translated per `docs/standards/i18n.md`):

  | Key | en | cs |
  |---|---|---|
  | `statementImport.button` (replaces the account page's "Import CSV" key; remove the old key from all locales if nothing else uses it) | Import statement | Importovat výpis |
  | `csvImport.title` (new value) | Import statement | Import výpisu |
  | `csvImport.titleFor` (new value) | Import statement into {{account}} | Import výpisu do účtu {{account}} |
  | `statementImport.fileFilter` | Bank statements | Bankovní výpisy |
  | `statementImport.supportedFormats` | CSV, CAMT.053 (XML), MT940, OFX/QFX and QIF | CSV, CAMT.053 (XML), MT940, OFX/QFX a QIF |
  | `statementImport.format.camt053` / `mt940` / `ofx` / `qif` | CAMT.053 / MT940 / OFX / QIF | same |
  | `statementImport.pickStatement` | The file contains several statements. Choose one: | Soubor obsahuje více výpisů. Vyberte jeden: |
  | `statementImport.period` | {{from}} – {{to}} | {{from}} – {{to}} |
  | `statementImport.opening` / `closing` | Opening balance / Closing balance | Počáteční zůstatek / Konečný zůstatek |
  | `statementImport.entries_one` / `_other` (cs `_few`, `_many`) | {{count}} payment / {{count}} payments | {{count}} pohyb / {{count}} pohyby / {{count}} pohybů |
  | `statementImport.skipped_one` / `_other` (cs `_few`, `_many`) | {{count}} not booked | {{count}} nezaúčtovaný / nezaúčtované / nezaúčtovaných |
  | `statementImport.check.ok` | The statement adds up: opening balance plus payments equals the closing balance. | Výpis sedí: počáteční zůstatek plus pohyby dává konečný zůstatek. |
  | `statementImport.check.mismatch` | The payments do not add up to the closing balance (difference {{difference}}). The file may be incomplete. | Pohyby nedávají konečný zůstatek (rozdíl {{difference}}). Soubor může být neúplný. |
  | `statementImport.check.unavailable` | The file does not state both balances, so it cannot be checked for completeness. | Soubor neuvádí oba zůstatky, úplnost proto nejde ověřit. |
  | `statementImport.match.selected` | The statement belongs to this account. | Výpis patří k tomuto účtu. |
  | `statementImport.match.other` | This statement belongs to {{account}}. | Tento výpis patří k účtu {{account}}. |
  | `statementImport.match.switch` | Import into {{account}} | Importovat do účtu {{account}} |
  | `statementImport.match.none` | This statement belongs to {{identifier}}, which is not this account. | Tento výpis patří k účtu {{identifier}}, ne k tomuto. |
  | `statementImport.match.unknown` | Account in the file: {{identifier}} | Účet v souboru: {{identifier}} |
  | `statementImport.match.confirm` | Import into this account anyway | Přesto importovat do tohoto účtu |
  | `statementImport.currencyDiffers` | The statement is in {{statement}}, the account in {{account}}. Payments keep their currency; the balance from the statement cannot be taken over. | Výpis je v {{statement}}, účet v {{account}}. Pohyby si ponechají svou měnu; zůstatek z výpisu nepůjde převzít. |
  | `statementImport.entry` | # | # |
  | `statementImport.rowMessage` | Payment {{line}}: {{message}} | Pohyb {{line}}: {{message}} |
  | `statementImport.rowMessageDetail` | Payment {{line}}: {{message}} ({{detail}}) | Pohyb {{line}}: {{message}} ({{detail}}) |
  | `statementImport.notBooked` | Not booked yet (information only) | Zatím nezaúčtováno (jen informativně) |
  | `statementImport.back` | Choose another file | Vybrat jiný soubor |
  | `statementImport.openAccount` | Open {{account}} | Otevřít účet {{account}} |

  `FileSelectStep` shows `statementImport.supportedFormats` under the drop zone.
- [ ] **Step 8: Run** `npm run lint && npm run typecheck && npm test`.
- [ ] **Step 9: Manual check in the dev app.** Start `npm run tauri dev` from the worktree on
  port 1421 and sign in to the demo profile. Import `src-tauri/tests/fixtures/statements/camt053/basic.xml`
  into a demo account whose IBAN you set to `CZ6508000000192000145399`. Check:
  - the summary card shows "adds up" and "belongs to this account";
  - after the import, `NextSteps` offers the balance 1 250,50 CZK as of 30. 9. 2026;
  - importing the same file again shows two duplicates and nothing to import;
  - on another account, the switch button appears and works;
  - a CSV from `src-tauri/tests/fixtures/csv/` still goes through the mapping step.
  Use the dev bridge path key `moony-dev-csv-path` to bypass the native dialog. Take
  screenshots of the summary card for the review.
- [ ] **Step 10: Gate and commit** — `feat(import): statement import dialog with summary, account check and picker`.

### Task 10: Phase 1 docs, ADR 0019, merge

**Files:**
- Create: `docs/architecture/decisions/0019-statement-files-share-the-import-pipeline.md`
  (+ index line in `docs/architecture/decisions/README.md`)
- Modify: `docs/architecture/overview.md`, `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md`
- Modify: `src-tauri/src/models/bank_accounts.rs` (doc comment of `DataSource::CsvImport`),
  `src-tauri/src/services/csv_import/types.rs` (doc comment of `CsvRowMessage.line`)

- [ ] **Step 1: ADR 0019** in the format of ADR 0018, status Accepted (the owner approved the
  spec). Context: CSV-only import (spec §1). Decision: D2, D3, D4, D6, D14 in prose.
  Consequences: one write path for every format; cross-format duplicates only hinted (D12);
  new formats add a parser and a fixture, never a second pipeline.
- [ ] **Step 2: Living docs.**
  - `overview.md`: in the import data flow, add the detection fork and the
    `statement_import` module.
  - `README.md`: the feature line "Bank accounts with CSV import (16 bank presets…)" lists
    "CSV (16 bank presets…) and CAMT.053 statements". Change the sentence at line 33 ("you
    import CSV statements") to "you import bank statements".
  - `CONTRIBUTING.md`: add a section "Adding a statement fixture" after "Adding a bank preset":
    synthetic data only, location `src-tauri/tests/fixtures/statements/<format>/`, one test
    per fixture asserting the parsed entries.
  - `CHANGELOG.md` Unreleased → Added: "**CAMT.053 statements.** The bank import reads ISO
    20022 CAMT.053 XML statements: it recognizes the account, checks that the payments add up
    to the closing balance, offers that balance, imports pending payments as pending, and
    warns when new payments fall in a period an earlier import already covers (CSV too)."
  - The two doc comments per spec §13.
- [ ] **Step 3: Gate, commit** — `docs: statement import phase 1 (ADR 0019, README, overview, changelog)`.
- [ ] **Step 4: Phase review and merge.** Run a code review of the phase diff
  (`git diff main...feat/statement-import`) and fix its findings with tests. `main` is
  checked out in the shared checkout, so the merge runs there, and only when
  `git -C ../Moony-tauri branch --show-current` prints
  `main` and `git -C ../Moony-tauri status --porcelain`
  prints nothing:
  `git -C ../Moony-tauri merge --no-ff feat/statement-import`.
  Otherwise stop and tell the owner. Never push. Update the Status table.

## Phase 2 — MT940

### Task 11: MT940 structure

**Files:**
- Create: `src-tauri/src/services/statement_import/mt940.rs`
- Create: `src-tauri/tests/fixtures/statements/mt940/basic.sta` (below, CRLF line endings)

**Interfaces:**
- Consumes: Task 3 model, `utc_day`, `canonical_amount`, `valid_iban`.
- Produces:
  ```rust
  pub fn parse(text: &str) -> Result<Vec<Statement>>;
  /// R4: a German BLZ/Konto as an IBAN; None for anything else.
  pub(crate) fn german_iban(account: &str) -> Option<String>;
  /// Filled in Task 12; this task stores the raw `:86:` text as the description.
  fn apply_details(entry: &mut Entry, details: &str);
  ```

- [ ] **Step 1: Fixture** `mt940/basic.sta`. Write it with CRLF line endings
  (`printf` or an editor setting), with trailing spaces after the second `:61:` line, and
  with two statements of the same account (pages `00009/001` and `00009/002`):

```
:20:STMT20260930
:25:CZ6508000000192000145399
:28C:00009/001
:60F:C260831CZK1000,00
:61:2609050905C500,50NTRFNONREF//BANKREF001
:86:Acme s.r.o. Faktura 2026-17
:62M:C260905CZK1500,50
-
:20:STMT20260930
:25:CZ6508000000192000145399
:28C:00009/002
:60M:C260905CZK1500,50
:61:2609200920D250,NMSCNONREF//BANKREF002   
Card 1234
:86:Platba kartou Lidl
:61:2612311231RD10,NTRFNONREF//NONREF
:86:Vrácení poplatku
:62F:C261231CZK1260,50
-
```

- [ ] **Step 2: Failing tests** in `mt940.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::statement_import::{merge::{merge, BalanceCheck}, model::utc_day};
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> i64 { utc_day(NaiveDate::from_ymd_opt(y, m, d).unwrap()) }
    fn fixture() -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/statements/mt940/basic.sta", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn crlf_pages_entries_and_balances() {
        let text = fixture();
        assert!(text.contains("\r\n"), "fixture must keep CRLF");
        let statements = parse(&text).unwrap();
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].sequence.as_deref(), Some("00009/001"));
        let merged = merge(statements);
        assert_eq!(merged[0].balance_check, BalanceCheck::AddsUp);
        let e = &merged[0].statement.entries;
        assert_eq!((e[0].amount.as_str(), e[0].is_credit, e[0].bank_id.as_deref()), ("500.5", true, Some("BANKREF001")));
        assert_eq!(e[0].booking_date, day(2026, 9, 5));
        assert_eq!((e[1].amount.as_str(), e[1].is_credit), ("250", false));
        assert_eq!(e[1].description.as_deref(), Some("Platba kartou Lidl"));
        assert!(e[2].is_credit, "RD is a reversal of a debit: a credit");
        assert_eq!(e[2].bank_id, None, "NONREF is never an ID");
    }

    #[test]
    fn supplementary_details_when_86_is_missing() {
        let text = fixture().replace(":86:Platba kartou Lidl\r\n", "");
        let e = &parse(&text).unwrap()[1].entries[0];
        assert_eq!(e.description.as_deref(), Some("Card 1234"));
    }

    #[test]
    fn entry_date_across_new_year() {
        let s = ":20:X\n:25:CZ6508000000192000145399\n:60F:C251231CZK0,\n:61:2601021231D5,NMSCNONREF\n:62F:D260102CZK5,\n";
        let e = &parse(s).unwrap()[0].entries[0];
        assert_eq!((e.booking_date, e.value_date), (day(2025, 12, 31), Some(day(2026, 1, 2))));
        let s2 = s.replace("2601021231", "2512310102");
        let e2 = &parse(&s2).unwrap()[0].entries[0];
        assert_eq!(e2.booking_date, day(2026, 1, 2));
    }

    #[test]
    fn swift_envelope_and_german_account() {
        let s = "{1:F01DEUTDEFFAXXX0000000000}{2:O9401200261001DEUTDEFFAXXX00000000002610011200N}{4:\n:20:X\n:25:37040044/532013000\n:60F:C260901EUR0,\n:61:2609010901C1,NTRFNONREF//R1\n:86:x\n:62F:C260901EUR1,\n-}";
        let st = &parse(s).unwrap()[0];
        assert_eq!(st.account.identifier.as_deref(), Some("DE89370400440532013000"));
        assert_eq!(st.currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn czech_domestic_account_is_kept_whole() {
        let s = ":20:X\n:25:19-2000145399/0800\n:60F:C260901CZK0,\n:62F:C260901CZK0,\n";
        assert_eq!(parse(s).unwrap()[0].account.identifier.as_deref(), Some("19-2000145399/0800"));
    }

    #[test]
    fn decoded_windows_1250_keeps_diacritics() {
        // Review Focus 5, end to end with Task 4's decoder
        let (bytes, _, _) = encoding_rs::WINDOWS_1250.encode(":20:X\n:25:CZ6508000000192000145399\n:60F:C260901CZK0,\n:61:2609010901C1,NTRFNONREF\n:86:Žluťoučký kůň\n:62F:C260901CZK1,\n");
        let text = crate::services::statement_import::decode::decode(&bytes, StatementFormat::Mt940, Some("CZ")).text;
        assert_eq!(parse(&text).unwrap()[0].entries[0].description.as_deref(), Some("Žluťoučký kůň"));
    }

    #[test]
    fn broken_61_is_malformed() {
        let s = ":20:X\n:25:Y\n:60F:C260901CZK0,\n:61:garbage\n";
        assert!(parse(s).unwrap_err().to_string().contains("validation.statementMalformed"));
    }
}
```

- [ ] **Step 3: Run and see them fail.**

- [ ] **Step 4: Implement.**
  - Normalize: replace `\r\n` and `\r` with `\n`. Strip the SWIFT blocks: remove
    `{1:…}`, `{2:…}`, `{3:…}`, `{5:…}` and the `{4:` opener and `-}` closer. Each `{4:`
    block is then a message.
  - Split into fields: a line matching `^:(\d{2}[A-Z]?):` starts a field. Other lines
    (trimmed at the end) continue the previous field with `\n`. A line `-` ends the
    statement.
  - `:20:` starts a new `Statement` when the current one already has a `:20:`.
  - Balance fields use the regex `^([CD])(\d{6})([A-Z]{3})([\d,]+)$` →
    `DeclaredBalance` (amount `,`→`.` then `canonical_amount`, `-` for `D`, day from
    `YYMMDD` via `yymmdd()`). `:60F:`/`:60M:` → `opening`, `:62F:`/`:62M:` → `closing`,
    currency from the balance.
  - `:61:` uses the regex on the first line
    `^(\d{6})(\d{4})?(RC|RD|C|D)([A-Z])?(\d[\d,]*)([NSF][A-Z0-9]{3})(.*?)(?://(.*))?$`
    (non-greedy customer reference). A non-match → `validation.statementMalformed` with
    `{"detail": ":61: <n>"}`. The second line (if any) is the supplementary details.
    - Direction: `C`, `RD` → credit; `D`, `RC` → debit.
    - Booking date: per spec §6.2 with `booking_from(value, Some((mm, dd)))`.
    - Bank ID: group 8 trimmed, unless empty or `NONREF` (case-insensitive).
  - `:86:` → `apply_details(last_entry, text)`. In this task, `apply_details` sets
    `description` to the text with newlines turned into spaces. Task 12 replaces it.
  - When no `:86:` followed an entry, the supplementary details become the description.
  - `yymmdd`: `YY < 80` → 20YY, else 19YY.
  - `german_iban(account)`: when `account` is `\d{8}/\d{1,10}`, return
    `DE{check:02}{blz}{konto:0>10}` with `check = 98 − mod97(blz + konto + "131400")`, and
    only if `valid_iban` accepts it. Do the arithmetic in a small local `mod97` over the
    digit string, as `financial_semantics` does. Apply it to `:25:` before storing; otherwise
    store the `:25:` text trimmed.
  - Wire `Mt940 → mt940::parse` into `load` (Task 8) only in Task 12.
- [ ] **Step 5: Run** — PASS.
- [ ] **Step 6: Gate and commit** — `feat(statement-import): MT940 statements, lines and balances`.

### Task 12: MT940 `:86:` dialects, enable MT940, docs, merge

**Files:**
- Modify: `src-tauri/src/services/statement_import/mt940.rs`
- Create: `src-tauri/tests/fixtures/statements/mt940/de-subfields.sta`,
  `nl-slash.sta`, `cz-free-text.sta`
- Modify: `src-tauri/src/services/statement_import/mod.rs` (dispatch),
  `service_tests.rs` (add an MT940 import test)
- Modify: `src/i18n/locales/*/bank_accounts.json` only if a string changes (none expected),
  `README.md`, `CHANGELOG.md`

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn german_subfields() {
    let mut e = Entry::default();
    // ?20 and ?32 are full 27-character lines cut mid-word
    apply_details(&mut e, "166?00GUTSCHRIFT?100599?20Rechnung 2026-17 Vielen Dan?21k fuer die Zahlung?30COBADEFFXXX?31DE89370400440532013000?32Maximilian Mustermann-Schmi?33dt");
    assert_eq!(e.description.as_deref(), Some("GUTSCHRIFT | Rechnung 2026-17 Vielen Dank fuer die Zahlung"));
    assert_eq!(e.counterparty_name.as_deref(), Some("Maximilian Mustermann-Schmidt"));
    assert_eq!(e.counterparty_account.as_deref(), Some("DE89370400440532013000"));
    assert_eq!(e.counterparty_bank_code.as_deref(), Some("COBADEFFXXX"));
}

#[test]
fn german_subfields_short_segments_get_a_space() {
    let mut e = Entry::default();
    apply_details(&mut e, "166?20Miete?21Oktober");
    assert_eq!(e.description.as_deref(), Some("Miete Oktober"));
}

#[test]
fn subfield_separator_other_than_question_mark() {
    let mut e = Entry::default();
    apply_details(&mut e, "166>20Miete>32Vermieter GmbH");
    assert_eq!((e.description.as_deref(), e.counterparty_name.as_deref()), (Some("Miete"), Some("Vermieter GmbH")));
}

#[test]
fn dutch_slash_tags() {
    let mut e = Entry::default();
    apply_details(&mut e, "/TRTP/SEPA OVERBOEKING/IBAN/NL91ABNA0417164300/BIC/ABNANL2A/NAME/J. Jansen/REMI/USTD//Huur oktober/EREF/NOTPROVIDED");
    assert_eq!(e.counterparty_name.as_deref(), Some("J. Jansen"));
    assert_eq!(e.counterparty_account.as_deref(), Some("NL91ABNA0417164300"));
    assert_eq!(e.counterparty_bank_code.as_deref(), Some("ABNANL2A"));
    assert_eq!(e.description.as_deref(), Some("Huur oktober"));
}

#[test]
fn dutch_cntp() {
    let mut e = Entry::default();
    apply_details(&mut e, "/CNTP/NL91ABNA0417164300/ABNANL2A/J. Jansen/Amsterdam/REMI/USTD//Huur/");
    assert_eq!((e.counterparty_account.as_deref(), e.counterparty_bank_code.as_deref(), e.counterparty_name.as_deref()),
        (Some("NL91ABNA0417164300"), Some("ABNANL2A"), Some("J. Jansen")));
}

#[test]
fn free_text_takes_a_single_valid_iban() {
    let mut e = Entry::default();
    apply_details(&mut e, "PLATBA NA UCET CZ55 0800 0000 0012 3456 7899\nNAJEM RIJEN");
    assert_eq!(e.description.as_deref(), Some("PLATBA NA UCET CZ55 0800 0000 0012 3456 7899 NAJEM RIJEN"));
    assert_eq!(e.counterparty_account.as_deref(), Some("CZ5508000000001234567899"));
    let mut two = Entry::default();
    apply_details(&mut two, "CZ5508000000001234567899 a CZ6508000000192000145399");
    assert_eq!(two.counterparty_account, None);
}
```

  Plus one fixture test per dialect file that runs `parse` over the file and asserts the first
  entry's name, account and description. Plus a `service_tests.rs` test: importing
  `mt940/basic.sta` into `acc` writes 3 rows and offers the closing balance `1260.5`.

- [ ] **Step 2: Run and see them fail.**
- [ ] **Step 3: Implement `apply_details`** per spec §6.2 and R4:
  - **Dialect choice.**
    - Subfield: `^\d{3}(\S)\d{2}`, where the captured separator is not alphanumeric. Split on
      `sep` followed by two digits.
    - Slash: the text starts with `/`, and one of the known tags `EREF ORDP BENM NAME IBAN
      BIC ACCW REMI CNTP TRTP MARF CSID PURP RTRN ULTD ULTC ADDR ISDT` appears as `/TAG/`.
      Values run until the next `/KNOWN/` or the end, with a trailing `/` trimmed. `REMI`
      drops a leading `USTD//` or `STRD/CUR/`. `CNTP` splits into account, BIC, name and
      city on `/`.
    - Free text: everything else.
  - **Subfield description.** Join `?20…?29` then `?60…?63` in order. Each segment has its
    line breaks removed, and is appended without a space when the previous segment is
    exactly 27 characters, else with one space. Prepend `?00` with ` | `. The name `?32` +
    `?33` is joined by the same 27-character rule.
  - **Free text.** Newlines become spaces and whitespace is collapsed. For the IBAN, split
    the text into whitespace tokens. For every token that starts with two letters and two
    digits, concatenate it with 0–8 following tokens, and keep each concatenation that
    `valid_iban` accepts. A single greedy regex would swallow the words after the IBAN. Use
    the distinct IBANs found; exactly one → `counterparty_account`.
  - **Wiring.** Replace the MT940 arm in `load` with `mt940::parse`.
    `not_yet_supported_formats_say_so` keeps its QIF sample until Task 14.
- [ ] **Step 4: Docs.** README: the format list gains MT940. CHANGELOG Unreleased → Added:
  "**MT940 statements**, including the common German, Dutch and free-text payment details."
  CONTRIBUTING: "a new `:86:` layout needs a fixture and a test, never a bank-specific branch".
- [ ] **Step 5: Gate, commit** — `feat(statement-import): MT940 payment details and import`.
- [ ] **Step 6: Manual check** of `mt940/basic.sta` in the dev app, then the phase review
  and merge as in Task 10, Step 4.

## Phase 3 — OFX/QFX

### Task 13: OFX/QFX parser, enable OFX, docs, merge

**Files:**
- Create: `src-tauri/src/services/statement_import/ofx.rs`
- Create: `src-tauri/tests/fixtures/statements/ofx/checking-sgml.ofx` (below, CRLF),
  `ofx/card-xml.qfx`, `ofx/investment-only.ofx`
- Modify: `src-tauri/src/services/statement_import/mod.rs` (dispatch, warnings),
  `service_tests.rs`, `README.md`, `CHANGELOG.md`
- Modify: `src/i18n/locales/*/bank_accounts.json`, `src/i18n/locales/*/common.json` (10 + 10)

**Interfaces:**
- Produces:
  ```rust
  pub struct OfxParse { pub statements: Vec<Statement>, pub warnings: Vec<StatementWarning> }
  pub fn parse(text: &str) -> Result<OfxParse>;
  ```

- [ ] **Step 1: Fixture** `ofx/checking-sgml.ofx` (CRLF):

```
OFXHEADER:100
DATA:OFXSGML
VERSION:102
SECURITY:NONE
ENCODING:USASCII
CHARSET:1252
COMPRESSION:NONE
OLDFILEUID:NONE
NEWFILEUID:NONE

<OFX>
<SIGNONMSGSRSV1><SONRS><STATUS><CODE>0<SEVERITY>INFO</STATUS><DTSERVER>20261001120000<LANGUAGE>ENG</SONRS></SIGNONMSGSRSV1>
<BANKMSGSRSV1><STMTTRNRS><TRNUID>1<STATUS><CODE>0<SEVERITY>INFO</STATUS>
<STMTRS><CURDEF>USD
<BANKACCTFROM><BANKID>021000021<ACCTID>123456789<ACCTTYPE>CHECKING</BANKACCTFROM>
<BANKTRANLIST><DTSTART>20260901<DTEND>20260930
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20260905120000.000[-5:EST]<TRNAMT>500.50<FITID>F1<NAME>ACME &amp; SONS &lt;US&gt;<MEMO>Invoice 17</STMTTRN>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260920<TRNAMT>-250,00<FITID>F2<NAME>GROCERY   </STMTTRN>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260921<TRNAMT>-1.00<FITID>F3<CORRECTFITID>F2<CORRECTACTION>REPLACE<NAME>FIX</STMTTRN>
</BANKTRANLIST>
<LEDGERBAL><BALAMT>1250.50<DTASOF>20260930</LEDGERBAL>
</STMTRS></STMTTRNRS></BANKMSGSRSV1>
</OFX>
```

  `ofx/card-xml.qfx`:

```xml
<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<?OFX OFXHEADER="200" VERSION="220" SECURITY="NONE" OLDFILEUID="NONE" NEWFILEUID="NONE"?>
<OFX>
 <SIGNONMSGSRSV1><SONRS><STATUS><CODE>0</CODE><SEVERITY>INFO</SEVERITY></STATUS><DTSERVER>20261001</DTSERVER><LANGUAGE>ENG</LANGUAGE><INTU.BID>1234</INTU.BID></SONRS></SIGNONMSGSRSV1>
 <CREDITCARDMSGSRSV1><CCSTMTTRNRS><TRNUID>1</TRNUID><STATUS><CODE>0</CODE><SEVERITY>INFO</SEVERITY></STATUS>
  <CCSTMTRS><CURDEF>EUR</CURDEF><CCACCTFROM><ACCTID>DE89370400440532013000</ACCTID></CCACCTFROM>
   <BANKTRANLIST><DTSTART>20260901</DTSTART><DTEND>20260930</DTEND>
    <STMTTRN><TRNTYPE>DEBIT</TRNTYPE><DTPOSTED>20260912</DTPOSTED><TRNAMT>-12.30</TRNAMT><FITID>C1</FITID><NAME>Caf&#233; M&#252;ller</NAME></STMTTRN>
   </BANKTRANLIST>
   <LEDGERBAL><BALAMT>-12.30</BALAMT><DTASOF>20260930</DTASOF></LEDGERBAL>
  </CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1>
</OFX>
```

  `ofx/investment-only.ofx`:

```
OFXHEADER:100
DATA:OFXSGML
VERSION:102
SECURITY:NONE
ENCODING:USASCII
CHARSET:1252
COMPRESSION:NONE
OLDFILEUID:NONE
NEWFILEUID:NONE

<OFX>
<INVSTMTMSGSRSV1><INVSTMTTRNRS><TRNUID>1<STATUS><CODE>0<SEVERITY>INFO</STATUS>
<INVSTMTRS><DTASOF>20260930<CURDEF>USD<INVACCTFROM><BROKERID>broker.example<ACCTID>X1</INVACCTFROM></INVSTMTRS>
</INVSTMTTRNRS></INVSTMTMSGSRSV1>
</OFX>
```

- [ ] **Step 2: Failing tests** (`ofx.rs`, Review Focus 1 and 3):

```rust
#[test]
fn sgml_checking_statement() {
    let parsed = parse(&fixture("checking-sgml.ofx")).unwrap();
    let s = &parsed.statements[0];
    assert_eq!((s.account.identifier.as_deref(), s.account.bank_code.as_deref()), (Some("123456789"), Some("021000021")));
    assert_eq!(s.currency.as_deref(), Some("USD"));
    assert_eq!(s.period, Some((day(2026, 9, 1), day(2026, 9, 30))));
    assert_eq!(s.opening, None);
    assert_eq!(s.closing.as_ref().map(|b| (b.amount.as_str(), b.as_of)), Some(("1250.5", day(2026, 9, 30))));
    let e = &s.entries;
    assert_eq!((e[0].amount.as_str(), e[0].is_credit, e[0].bank_id.as_deref()), ("500.5", true, Some("F1")));
    assert_eq!(e[0].booking_date, day(2026, 9, 5));
    assert_eq!(e[0].counterparty_name.as_deref(), Some("ACME & SONS <US>"));
    assert_eq!(e[0].description.as_deref(), Some("Invoice 17"));
    assert_eq!((e[1].amount.as_str(), e[1].is_credit), ("250", false), "decimal comma, trailing spaces");
    assert_eq!(e[1].counterparty_name.as_deref(), Some("GROCERY"));
    assert_eq!(e[1].description.as_deref(), Some("GROCERY"));
    assert_eq!(e[2].skip_reason, Some("statementImport.correctionSkipped"));
}

#[test]
fn xml_credit_card_with_char_refs() {
    let s = &parse(&fixture("card-xml.qfx")).unwrap().statements[0];
    assert_eq!(s.account.identifier.as_deref(), Some("DE89370400440532013000"));
    assert_eq!(s.entries[0].counterparty_name.as_deref(), Some("Café Müller"));
    assert!(!s.entries[0].is_credit);
}

#[test]
fn investment_only_is_refused() {
    assert!(parse(&fixture("investment-only.ofx")).unwrap_err().to_string().contains("validation.statementInvestmentOnly"));
}

#[test]
fn mixed_file_warns_about_investments() {
    let text = fixture("checking-sgml.ofx").replace("</OFX>", "<INVSTMTMSGSRSV1><INVSTMTTRNRS><INVSTMTRS></INVSTMTRS></INVSTMTTRNRS></INVSTMTMSGSRSV1></OFX>");
    let parsed = parse(&text).unwrap();
    assert_eq!(parsed.warnings[0].key, "statementImport.investmentIgnored");
}
```

  Plus a service test: importing `checking-sgml.ofx` into an account `us` with bban
  `021000021123456789` gives `accountMatch = selected`, 2 imported rows, 1 skipped (the
  correction), and a reimport imports 0.

- [ ] **Step 3: Run and see them fail.**
- [ ] **Step 4: Implement the tag scanner.**
  - Drop everything before the first `<OFX` (the header, or the XML prolog with the `<?OFX?>`
    processing instruction). `reject_doctype` first.
  - Tokenize: `<NAME>` → open, `</NAME>` → close, and the text between tags → text (trimmed,
    entity-decoded: `&amp; &lt; &gt; &quot; &apos; &nbsp;` and `&#NNN;`/`&#xHH;`; an unknown
    entity stays literal). Element names are uppercased.
  - Build nodes with a stack. An open after which non-empty text follows before the next
    `<` is a leaf with that value. A leaf is closed implicitly by the next open or close, and
    a following `</SAME>` for that leaf is skipped. A close pops until the matching name; a
    close with no matching open is ignored.
  - Reuse `xml_tree::Node` for the result, so the lookups (`at`, `text_at`) are the same
    as in CAMT.
- [ ] **Step 5: Implement the mapping** per spec §6.3:
  - Statements are each `STMTRS` and `CCSTMTRS` found anywhere.
  - `DTPOSTED` and the others: the first 8 characters, `%Y%m%d`.
  - `TRNAMT`: when it contains exactly one `,` and no `.`, replace `,` with `.`. Then
    `canonical_amount`; a negative value → debit with the absolute amount.
  - Name: `NAME`, else `PAYEE/NAME`. Memo: `MEMO`, else the name.
  - `CORRECTACTION` present → `skip_reason = Some("statementImport.correctionSkipped")`.
  - Files with `INVSTMTRS` and no bank/card statement →
    `validation.statementInvestmentOnly`. Mixed → warning
    `StatementWarning { key: "statementImport.investmentIgnored", detail: None }`.
  - Wire `Ofx → ofx::parse` into `load` and carry the warnings into `Loaded`.
- [ ] **Step 6: Strings.**
  - `bank_accounts`:
    - `statementImport.correctionSkipped`: en "A correction of an earlier payment; not applied", cs "Oprava dřívějšího pohybu; neprovádí se".
    - `statementImport.investmentIgnored`: en "The file also contains investment transactions. They are not imported; use the broker import for those.", cs "Soubor obsahuje i investiční transakce. Ty se neimportují; použijte pro ně import od brokera."
  - `common` `validation.statementInvestmentOnly`: en "This file contains only investment transactions. Use the broker import for those.", cs "Tento soubor obsahuje jen investiční transakce. Použijte pro ně import od brokera."
  - All ten locales.
- [ ] **Step 7: Docs.** README format list gains OFX/QFX. CHANGELOG Added: "**OFX and QFX
  statements** (bank and credit card), from banks and from other finance apps."
- [ ] **Step 8: Gate, commit** — `feat(statement-import): OFX and QFX statements`.
- [ ] **Step 9: Manual check** of both OFX fixtures in the dev app, then the phase review
  and merge as in Task 10, Step 4.

## Phase 4 — QIF

### Task 14: QIF parser and date order

**Files:**
- Create: `src-tauri/src/services/statement_import/qif.rs`
- Create: `src-tauri/tests/fixtures/statements/qif/quicken-us.qif` (below, CRLF),
  `qif/european.qif`, `qif/two-accounts.qif`
- Modify: `src-tauri/src/services/statement_import/mod.rs` (dispatch, date order),
  `service_tests.rs` (delete `not_yet_supported_formats_say_so`; add QIF tests)

**Interfaces:**
- Consumes: `date_parser::detect_date_format`, `csv_import::{detect_decimal_separator,
  clean_and_parse_amount, amount_to_text}`, `DateOrder` (Task 8).
- Produces:
  ```rust
  pub struct QifParse { pub statements: Vec<Statement>, pub warnings: Vec<StatementWarning>,
      pub date_order_ambiguous: bool, pub detected_date_order: Option<DateOrder> }
  /// `date_order` = the user's choice; None = detect.
  pub fn parse(text: &str, date_order: Option<DateOrder>) -> Result<QifParse>;
  ```

- [ ] **Step 1: Fixtures.** `qif/quicken-us.qif` (CRLF):

```
!Account
NChecking
TBank
^
!Type:Bank
D08/31'26
T1,000.00
POpening Balance
L[Checking]
^
D9/5'26
T500.50
PAcme
MInvoice 17
LIncome:Salary/Work
^
D9/20'26
T-250.00
PGrocery
LFood:Groceries
SFood:Groceries
$-200.00
SHousehold
$-50.00
^
D9/21'26
T-30.00
PTransfer to savings
L[Savings]
^
```

  `qif/european.qif`, written as UTF-8 and converted with
  `iconv -f UTF-8 -t WINDOWS-1250 european.utf8.qif > european.qif` (keep only the converted
  file):

```
!Type:Bank
D15.10.2026
T-1.234,56
PŽluťoučký kůň
LJídlo
^
D03.10.2026
T2.000,00
PZaměstnavatel
^
```

  `qif/two-accounts.qif`:

```
!Account
NChecking
TBank
^
!Type:Bank
D09/05/2026
T-10.00
PShop
^
!Account
NSavings
TBank
^
!Type:Bank
D09/15/2026
T100.00
PInterest
^
!Account
NBroker
TInvst
^
!Type:Invst
D09/20/2026
NBuy
YACME
I10.00
Q1
T10.00
^
D09/21/2026
NSell
YACME
I11.00
Q1
T11.00
^
```

- [ ] **Step 2: Failing tests** (`qif.rs`, Review Focus 1 and 5):

```rust
#[test]
fn quicken_us_file() {
    let text = fixture_text("quicken-us.qif");
    assert!(text.contains("\r\n"));
    let p = parse(&text, None).unwrap();
    assert!(!p.date_order_ambiguous);
    assert_eq!(p.detected_date_order, Some(DateOrder::Mdy));
    let s = &p.statements[0];
    assert_eq!(s.account.name.as_deref(), Some("Checking"));
    assert_eq!(s.opening.as_ref().map(|b| (b.amount.as_str(), b.as_of)), Some(("1000", day(2026, 8, 31))));
    assert_eq!(s.closing, None);
    assert_eq!(s.entries.len(), 3, "the opening balance record is not an entry");
    assert_eq!((s.entries[0].amount.as_str(), s.entries[0].is_credit), ("500.5", true));
    assert_eq!(s.entries[0].source_category.as_deref(), Some("Income:Salary"));
    assert_eq!(s.entries[0].description.as_deref(), Some("Invoice 17"));
    assert_eq!(s.entries[1].source_category, None, "splits with different categories");
    assert_eq!(s.entries[1].amount, "250");
    assert_eq!(s.entries[2].source_category, None, "a transfer has no category");
    assert_eq!(s.entries[2].booking_date, day(2026, 9, 21));
}

#[test]
fn european_file_in_windows_1250() {
    let bytes = fixture_bytes("european.qif");
    let text = crate::services::statement_import::decode::decode(&bytes, StatementFormat::Qif, Some("CZ")).text;
    let p = parse(&text, None).unwrap();
    assert_eq!(p.detected_date_order, Some(DateOrder::Dmy));
    let e = &p.statements[0].entries[0];
    assert_eq!((e.amount.as_str(), e.is_credit, e.booking_date), ("1234.56", false, day(2026, 10, 15)));
    assert_eq!(e.counterparty_name.as_deref(), Some("Žluťoučký kůň"));
    assert_eq!(e.source_category.as_deref(), Some("Jídlo"));
}

#[test]
fn ambiguous_dates_need_a_choice() {
    let text = "!Type:Bank\nD03/04/2026\nT5\n^\nD05/06/2026\nT6\n^\n";
    let detected = parse(text, None).unwrap();
    assert!(detected.date_order_ambiguous);
    let chosen = parse(text, Some(DateOrder::Mdy)).unwrap();
    assert_eq!(chosen.statements[0].entries[0].booking_date, day(2026, 3, 4));
    let dmy = parse(text, Some(DateOrder::Dmy)).unwrap();
    assert_eq!(dmy.statements[0].entries[0].booking_date, day(2026, 4, 3));
}

#[test]
fn two_accounts_and_investments() {
    let p = parse(&fixture_text("two-accounts.qif"), None).unwrap();
    assert_eq!(p.statements.iter().map(|s| s.account.name.clone().unwrap()).collect::<Vec<_>>(), vec!["Checking", "Savings"]);
    assert_eq!((p.warnings[0].key.as_str(), p.warnings[0].detail.as_deref()), ("statementImport.investmentIgnored", Some("2")));
}

#[test]
fn two_digit_years() {
    let p = parse("!Type:Bank\nD12/31/99\nT1\n^\nD1/1/27\nT1\n^\n", None).unwrap();
    // 99 > current YY + 1 → 1999; 27 ≤ current YY + 1 (in 2026) → 2027
    assert_eq!(p.statements[0].entries[0].booking_date, day(1999, 12, 31));
    assert_eq!(p.statements[0].entries[1].booking_date, day(2027, 1, 1));
}
```

  Service tests:
  - Importing `quicken-us.qif` into `acc` gives `accountMatch = unknown` and
    `balanceCheck = unavailable`. Its preview rows carry `source_category`. No
    `statementBalance` is offered.
  - `preview(…, StatementImportOptions { date_order: Some(DateOrder::Dmy), .. })` on the
    ambiguous sample is accepted.
  - `inspect` of `two-accounts.qif` has 2 summaries.

- [ ] **Step 3: Run and see them fail.**
- [ ] **Step 4: Implement** per spec §6.4:
  - Lines are `trim_end`-ed (CR and spaces).
  - Section headers are case-insensitive. `!Account` blocks are read until `^` (`N`, `T`) and
    set the current account. A `!Type:` of `Bank|Cash|CCard|Oth A|Oth L` opens or continues
    that account's statement (one unnamed account when there is no `!Account`).
    `!Type:Invst` counts its records into the warning
    (`detail` = the count). Other `!` sections are skipped to the next `!` line.
  - Records collect fields until `^`. Splits: collect the `S` values; the source category is
    the shared `S` category when all splits have the same one, else `None`. A non-split
    record uses `L`.
  - Category text: strip `/class`. `[…]` → `None`.
  - The opening balance record: the first record of an account whose `L` is
    `[<account name>]` (case-insensitive) becomes `opening` (amount `T`, day `D`, currency
    `None`). It is not an entry.
  - Dates: gather all `D` values, normalize (`'` → `/`, remove spaces), and call
    `detect_date_format`. Map the returned format to `DateOrder`: a leading `%d` → `Dmy`,
    `%m` → `Mdy`, `%Y`/`%y` → `Ymd`. When there is no detectable date at all →
    `validation.statementMalformed` (`{"detail": "D"}`). The user's `date_order` wins.
    `parse_qif_date(raw, order)` splits on `/ . - '`, notes whether the separator before the
    year was `'`, and expands two-digit years per the spec, with "current YY" =
    `chrono::Utc::now().year() % 100`.
  - Amounts: `T` (fall back to `U`). Decide the separator once with
    `detect_decimal_separator` over all amount fields, then `clean_and_parse_amount` →
    `amount_to_text(abs)` and the sign → `is_credit`. A record without a readable `D` or `T`
    → `validation.statementMalformed` with `{"detail": "record <n>"}`.
  - Fields: `P` → `counterparty_name`. `M` → `description`, else `P`. No bank IDs.
  - Wire `Qif → qif::parse(text, date_order)` into `load`. `Loaded` gets the warnings and the
    two date order fields. `inspect` passes them on. Delete
    `not_yet_supported_formats_say_so` and R8's fallback arm, which no longer has a format to
    serve.
- [ ] **Step 5: Run** — PASS.
- [ ] **Step 6: Gate, commit** — `feat(statement-import): QIF files with date order detection`.

### Task 15: QIF in the dialog (date order, category panel), docs, merge

**Files:**
- Create: `src/components/bank-accounts/csv-import/source-categories.ts` (+ `source-categories.test.ts`)
- Create: `src/components/bank-accounts/csv-import/SourceCategoryPanel.tsx`
- Modify: `StatementImportDialog.tsx`, `StatementSummaryCard.tsx`, `PreviewStep.tsx`
- Modify: `src/i18n/locales/*/bank_accounts.json` (10 files), `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: `StatementInspection.dateOrderAmbiguous/detectedDateOrder`,
  `StatementSummary.sourceCategories`, `CsvPreviewRow.sourceCategory`, `categoryDisplayName`.
- Produces:
  ```ts
  export interface MatchableCategory { id: string; name: string; parentId: string | null; isSystem: boolean }
  /** Prefill for the category panel (spec §8.2): source path → category id, or null. */
  export function matchSourceCategories(
    sources: string[],
    categories: MatchableCategory[],
    displayName: (category: MatchableCategory) => string
  ): Record<string, string | null>;
  export function foldName(text: string): string; // lowercase, NFD without combining marks, collapsed whitespace
  ```

- [ ] **Step 1: Failing tests** (`source-categories.test.ts`):

```ts
import { describe, expect, it } from 'vitest';
import { matchSourceCategories, type MatchableCategory } from './source-categories';

const cats: MatchableCategory[] = [
  { id: 'cat_groceries', name: 'Groceries', parentId: 'cat_food', isSystem: true },
  { id: 'cat_food', name: 'Food', parentId: null, isSystem: true },
  { id: 'u1', name: 'Kino', parentId: null, isSystem: false },
  { id: 'u2', name: 'Dárky', parentId: null, isSystem: false },
  { id: 'u3', name: 'Dárky', parentId: 'cat_food', isSystem: false },
];
const czech: Record<string, string> = { cat_groceries: 'Potraviny', cat_food: 'Jídlo' };
const display = (c: MatchableCategory) => (c.isSystem ? czech[c.id] ?? c.name : c.name);

describe('matchSourceCategories', () => {
  it('matches a full path, a leaf, the display name and folds diacritics', () => {
    expect(
      matchSourceCategories(['Food:Groceries', 'potraviny', 'KINO', 'jidlo'], cats, display)
    ).toEqual({ 'Food:Groceries': 'cat_groceries', potraviny: 'cat_groceries', KINO: 'u1', jidlo: 'cat_food' });
  });

  it('leaves ambiguous and unknown names unmapped', () => {
    expect(matchSourceCategories(['Dárky', 'Auto'], cats, display)).toEqual({ Dárky: null, Auto: null });
  });

  it('a full path disambiguates a repeated leaf', () => {
    expect(matchSourceCategories(['Jídlo:Dárky'], cats, display)).toEqual({ 'Jídlo:Dárky': 'u3' });
  });
});
```

- [ ] **Step 2: Run and see them fail.**
- [ ] **Step 3: Implement `matchSourceCategories`.**
  - `foldName` = `text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().replace(/\s+/g, ' ').trim()`.
  - A category's names are `{name, displayName(c)}` folded.
  - For `Parent:Child`: find categories whose names contain the folded child and whose parent's
    names contain the folded parent. Exactly one → its id.
  - Otherwise, for the last segment: exactly one category with that name → its id.
  - Else `null`.
- [ ] **Step 4: UI.**
  - **Date order.** `StatementSummaryCard` shows a `Select` (`dmy`/`mdy`/`ymd`, labels
    `statementImport.dateOrder.<order>`) when `inspection.dateOrderAmbiguous`. The default is
    `detectedDateOrder` (the backend already defaults it to the European order). The choice
    goes into the preview and import options as `dateOrder`. Until the user picks, the
    import button stays disabled, the same as the CSV mapping's date confirmation.
  - **Category panel.** `SourceCategoryPanel` (collapsible, open by default) lists
    `summary.sourceCategories` with counts and a `CategoryPick` each. It is prefilled once per
    file from `matchSourceCategories(sources, categories, (c) => categoryDisplayName(c, tc))`;
    `null` shows "Leave to the rules". Its state becomes `options.sourceCategoryMap`
    (entries with a category only).
  - **Source category in rows.** `PreviewStep` shows `row.sourceCategory` in a muted line
    under the description when present.
- [ ] **Step 5: Strings** (all ten locales):

  | Key | en | cs |
  |---|---|---|
  | `statementImport.dateOrder.label` | Dates in this file are written as | Data jsou v souboru zapsaná jako |
  | `statementImport.dateOrder.dmy` / `mdy` / `ymd` | day.month.year / month/day/year / year-month-day | den.měsíc.rok / měsíc/den/rok / rok-měsíc-den |
  | `statementImport.dateOrder.hint` | Every date in the file could be read both ways. Check one payment you know. | Každé datum v souboru jde přečíst oběma způsoby. Ověřte to na platbě, kterou znáte. |
  | `statementImport.categories.title` | Categories from the file | Kategorie ze souboru |
  | `statementImport.categories.hint` | Matched by name. Change any of them; payments without a category go through your rules. | Spárováno podle názvu. Kteroukoli můžete změnit; pohyby bez kategorie projdou vašimi pravidly. |
  | `statementImport.categories.rules` | Leave to the rules | Nechat na pravidlech |
  | `statementImport.categories.count_one` / `_other` (cs `_few`, `_many`) | {{count}} payment / {{count}} payments | {{count}} pohyb / {{count}} pohyby / {{count}} pohybů |

- [ ] **Step 6: Docs.** README format list gains QIF. CHANGELOG Added: "**QIF files** from
  Quicken, GnuCash and other apps: categories are matched to yours by name in the preview,
  and the opening balance record is recognized."
- [ ] **Step 7: Gate, commit** — `feat(import): QIF date order and category mapping in the preview`.
- [ ] **Step 8: Manual check** of the three QIF fixtures in the dev app:
  - `quicken-us.qif`: Income:Salary and Food:Groceries prefilled where the demo categories
    match;
  - `european.qif`: diacritics intact;
  - `two-accounts.qif`: the picker and the investment warning.
  Then the phase review and merge as in Task 10, Step 4.

## Task 16: Whole-branch review and release checks

- [ ] **Step 1:** Run a whole-feature review (`git diff <commit before Task 1>..main`)
  against the spec: every D1–D16, R1–R8 and §13 item is implemented or explicitly deferred.
  Fix the findings with tests.
- [ ] **Step 2:** `npm run tauri build -- --debug --no-bundle` succeeds. Open the debug build
  against a copy of the demo profile and import one fixture of each format through the
  native file dialog. `tauri dev` has no CSP, so this is where a CSP problem would show.
- [ ] **Step 3:** Hand the owner the release checklist below; do not run it yourself:
  - a dry run on a copy of the real profile with the owner's own CAMT/MT940/OFX exports;
  - comparing the closing balances with the bank;
  - reimporting one file to confirm 0 new rows.
- [ ] **Step 4:** Update the Status table and commit
  `docs(plan): statement import formats — review and release checks`.

## Execution rulings

Decisions the controller took while executing this plan (from the execution ledger), each with
its reason and what it costs if wrong.

- **R-1** — T13/T14 tests use helper fns the plan does not spell out; implementers write them like T7/T11 helpers — trivial — none.
- **R-2** — statement_import/mod.rs re-exports detect, check_size, DETECT_BYTES, MAX_FILE_BYTES (T8) — the plan's command calls them as statement_import::… — none.
- **R-3** — `decode` is declared `pub(crate) mod decode;` in statement_import (T4) so T11/T14 tests can call it — none.
- **R-4** — T8 makes csv_import::types::{default_true, default_preview_limit} pub(crate) and reuses them instead of copying — avoids duplicate defaults drifting — none.
- **R-5** — T2 overlap notice uses the existing PreviewStep note styling, not the illustrative classes in the plan — design-system consistency — a slightly different look than the plan snippet.
- **R-6** — models: sonnet for implementers and task reviews by default; opus for T9 (dialog integration, design judgment) and the final review; escalate to opus at fix round 4 — owner asked me to choose by difficulty — cost/turns.
- **R-7** — commit trailer stays 'Co-Authored-By: Claude Opus 5.5' for subagent commits — the session's attribution rule names it and the branch history is uniform — a trailer that names the session model rather than the subagent model.
- **R-8** — keep; the line only identifies the batch's anchor row for NextSteps — a misleading line number in a balance tooltip if ever shown.
- **R-9** — review report ended without the Assessment line; spec ✅ and zero Critical/Important findings, so I take the quality verdict as Approved — the report's content is complete — a missed quality concern the reviewer never wrote down.
- **R-10** — Task 4 commit carries a 'Claude Sonnet 5.5' trailer; history is not rewritten for uniformity and later subagents may name their own model — accuracy over uniformity, no history rewrite — mixed trailers on the branch.
- **R-11** — accept the total sort order Sequence::{Number < Text} (None first) instead of the brief's pairwise rule — the pairwise rule is not a total order and sort_by may panic — mixed numeric/text sequences order numbers before text.
- **R-12** — for an unsplit multi-TxDtls entry the bank ID is the entry's own ref (AcctSvcrRef/NtryRef), not the first TxDtls ref — a part's ref does not identify the whole entry and the brief's test requires B-1 — a bank that reuses the entry ref across statements would dedupe by entry, which is what the spec intends.
- **R-13** — sourceCategories counts only entries without skip_reason, mirroring entryCount — skipped rows are never imported — a QIF category used only by skipped rows would not appear in the panel.
- **R-14** — statementImportBlocked gets `inspectedAccountId === targetAccountId` instead of the brief's `targetAccountId === accountId` — closes a path where a statement of X imports into a switched-to Y without the D9 confirmation — a confirmation asked once more than the brief intended in some switch sequences.
- **R-15** — accept the extra copy changes (unsupportedFile, lead, removed dropNote) and the replacement of csvImport.importCsv/button by statementImport.button — the old copy said CSV-only and became false — wording the owner has not seen.
- **R-16** — Important 2 (use-statement-preview duplicates use-csv-preview, plan said "copy") — extract a shared useImportPreview — the two hooks must stay in lockstep — a slightly larger fix round.
- **R-17** — review Minors 1, 2 and 5 (target can switch during the write; "Choose another file" keeps kind and the switched target; null re-read leaves the target switched) are treated as Important — each can write transactions or a balance to an account the user did not see confirmed — slightly larger fix round.
- **R-18** — merge feat/statement-import into local main once, after phase 4 and the final review, instead of after every phase — the owner's standing preference is one local merge at the end, and main then never advertises formats it cannot read; this makes phase-1 Important 1 moot (Minor) — if the session stops before phase 4, nothing of the feature is on main.
- **R-19** — fix the reviewer's "before any release" CAMT items now in one phase-1 fix wave (UTF-16 declaration without BOM, xml_tree node cap + detail truncation, strict balance CdtDbtInd + currency case, bounded head/full reads, MT942/MT950 refusal per spec, stale "file line" doc comments) — they are cheap and would otherwise pile onto the final review — a slightly larger phase 1.
- **R-20** — `german_iban` must not convert `NNNNNNNN/BBBB` with exactly 4 digits after the slash: that is a Czech account/bank pair (canonical_account_identifier handles it) — Czech bank codes are always 4 digits and German Kontonummern of 4 digits are rare — a real German BLZ/4-digit-Konto stays unconverted and matches as unknown, never as a wrong account.
- **R-21** — accept `:NS:` as a field start (ignored), so non-SWIFT lines are not glued onto :86: — spec §6.2 lists :NS: as ignored — none.
- **R-22** — carry into Task 12: `:86:` continuation lines keep their trailing whitespace (only CR stripped) so the 27-character rule sees the original segments; open entry closes on every tag except :86:/:NS:; no phantom statement from stray fields after `-`; move `malformed` to model.rs for all parsers; pin `*.sta` fixtures with `-text` in .gitattributes — all touch mt940.rs which Task 12 edits anyway; cheap — none.
- **R-23** — the subfield dialect needs a separator that is not one of `. , / - : ; + ' ( ) %` (characters of amounts and references) AND at least two `sep + two digits` marks; the brief's bare `^\d{3}(\S)\d{2}` misreads "150,30 EUR Miete" — spec §6.2 says the dialect is chosen from the field's shape; real subfield fields always carry several marks — a real one-mark subfield field with an unusual separator would be read as free text (text kept, structure lost).
- **R-24** — add an .editorconfig exception for src-tauri/tests/fixtures/statements/mt940/*.sta (end_of_line unset, trim_trailing_whitespace false) alongside the .gitattributes rule — editors would otherwise break the CRLF fixtures — none.
- **R-25** — besides the Important known-leaf fix, fix in the same round: refuse a file whose aggregate elements (elements with children) are still open at EOF (truncated download → validation.statementMalformed), treat `<` not followed by a letter, `/`, `!` or `?` as text, and loosen the test that pins xml_tree's caps — each silently loses or invents data — cheap — a non-conforming exporter that never closes aggregates would be refused instead of read.
- **R-26** — QIF opening DeclaredBalance carries currency "" (type is non-optional); the summary card formats balances with summary.currency (null for QIF), and NextSteps only reads the closing balance (None for QIF), so nothing renders the empty code — none today; a future reader of openingBalance.currency must handle "".
- **R-27** — carry into Task 15: in qif.rs drop the year-first candidate when every date has a two-digit year in the last position (QIF never writes YY/MM/DD), so `15.10.26` is not reported ambiguous; fix the CONTRIBUTING format list — an unneeded date-order select on every European 2-digit-year QIF is poor UX — none.
- **R-28** — categoryDisplayName gets the bank_accounts `t` (categoryNames live in bank_accounts.json), not the brief's `tc` (common) — the plan was wrong about the namespace — none.
- **R-29** — the ambiguous-date select defaults to the detected order (the brief) rather than spec §6.4's profile-locale order; import stays disabled until the user picks or confirms — the user must look at it either way — a US user sees day-first preselected and has to change it.
- **R-30** — keep the QIF date-order pick and the source-category picks when the user switches statements in the same file — the date order is file-level in the backend and picks are keyed by source name — a pick made for one account's "Food" also applies to another account's "Food" in the same file.
- **R-31** — the final fix wave also takes these cheap correctness items: strip a trailing ISO currency from an otherwise-valid IBAN in MT940 :25: (Rabobank), add .mta/.swi to the statement file filter, neutral "skipped" wording for the skipped-count label (OFX corrections are not "not booked") in all locales, count panel mappings in manualCount — each is a few lines and fixes visible wrong behaviour — slightly larger wave.
- **R-32** — for a QIF account without a name, the first record anywhere with payee "Opening Balance" and a bracketed L is the opening balance (not only the first record) — GnuCash or back-dated entries can precede it — a genuine later transfer with that payee, with no earlier opening record, would be shown as the opening balance instead of a payment.
