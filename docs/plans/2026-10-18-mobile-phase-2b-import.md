# Mobile Phase 2b — statement import on the phone and the categorize queue — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A person imports a bank statement on the phone without ever mapping columns — from the share sheet, the Files picker, an account, Údržba or Nastavení — and then categorizes what stayed uncategorized in a one-tap queue.

**Architecture:** One import sheet, hosted once in the phone shell (`ImportSheetProvider`), runs the recognition ladder on the desktop's unchanged parsers and import commands: structured formats (`inspect_statement_file` …) and preset CSV (`parse_csv_file` …) go straight to the preview; an unknown CSV goes through a new Rust content inference (`infer_csv_columns`) shown as the sentence "Takhle tomu rozumím" with per-field correction, never a grid. Share-in is new Rust plumbing: iOS document types in `Info.ios.plist`, `RunEvent::Opened` kept in managed state and announced by an event, a drain command, and deletion of the plaintext copies iOS leaves in the app's Inbox folders. The queue is a new Rust read (uncategorized rows with a suggestion from the categorization engine or MCP) plus a 30-day snooze column (migration 023), and a phone page that reuses Finance's category picker and learning. Presets gain a per-locale `mobileExport` guide (spec §8.6 rung 2), shown when a file cannot be had or read. A PDF spike runs in a scratch crate and ends as a written recommendation.

**Tech Stack:** Rust (Tauri 2.12, rusqlite/SQLCipher, serde/specta), React 19 + TypeScript, wouter, TanStack Query, Radix (Dialog as the sheet, DropdownMenu as menus), `@tauri-apps/plugin-dialog` `open()`, `@tauri-apps/api/event` `listen()`, react-i18next (`mobile` namespace, ten locales), Tailwind on the design tokens.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` §8.5 (the queue) and §8.6 (import on a phone, the recognition ladder) are binding; §4, §6, §7 (Import výpisu row), §10.3–10.5 for components. Prototype `docs/specs/2026-10-16-mobile-app-prototype.html` frames 05–08 (notes 2 and 10) show the layout; the spec wins (Phase 1 ruling R7). Phone component contract: `docs/design-system/mobile.css` + `mobile.html` + `docs/design-system/README.md` §13 (including its "Hide amounts" paragraph). Bank export facts: `docs/plans/2026-10-16-mobile-app-bank-exports.md` (T1). Parent plan: `docs/plans/2026-10-16-mobile-app.md` (Phase 2 outline, review focus 1 and 4). Out of scope: rung 4 (learned formats), rung 5 (contribute format), the catalog refresh download (a proposal for the owner in the hand-over), manual "Přidat transakci", app shortcuts, Android share-in (`content://`), shipping a PDF reader unless the spike works end to end.

## Global Constraints

Process (from the parent task and the parent session's message of 2026-10-19):

- Worktree `.claude/worktrees/mobile-import-f98eb2`, branch `claude/mobile-import-f98eb2`, fast-forwarded to `feat/mobile` 09777337 (Phase 5 included). `git branch --show-current` before every commit. Never push; never merge into or touch `main`, `feat/mobile` or another worktree; never merge `main` into this branch. The parent session merges this branch into `feat/mobile`.
- `Cargo.lock` stays as on `feat/mobile` (`libc = 0.2.189`); no blanket `cargo update`. A new crate only inside the PDF spike's scratch crate (Task 9), never in `src-tauri/Cargo.toml`.
- One plain shell command per call (the sandbox refuses `&&` / `;` chains except one `cd src-tauri && cargo …`, `git -C`, `HOME=` prefixes, scripts); stage explicit paths only; never `git stash`.
- Reuse only, never edit: `src/components/**` (desktop), `src/hooks/**`, `src/lib/money-flow-refresh.ts`, `src/utils/**`. `src/mobile/shell/routes.ts` and `MobileShell.tsx` are touched minimally (one route swap, the provider and the received-file host).
- Rust follows `docs/standards/rust-backend.md` (thin commands, services take `&Connection`, `AppError` never `Result<_, String>`, inputs validated before DB work, no `.unwrap()` outside tests) and the 7-step `docs/standards/type-contract.md` (per-field `#[serde(rename = "camelCase")]`, `bindings.rs` + `cd src-tauri && cargo test generate_bindings -- --ignored`, hand mirror in `shared/schema.ts`, wrapper in `src/lib/tauri-api.ts`). Service tests use `Connection::open_in_memory()` with a hand-written minimal schema. Migration 023 is appended (`023_categorize_snooze`), nothing else in `migrations.rs` changes.
- Phone-only Rust (`RunEvent::Opened`, the received-files commands) is `#[cfg(mobile)]`, as Phase 5's export commands are: the desktop app's behaviour does not change.
- Every user-facing string in the `mobile` namespace in all ten locales (en, cs, de, es, it, nb, pl, sv, ja, zh), plural forms per `docs/standards/i18n.md` §2 (en/de/nb/sv `_one _other`; es/it `_one _many _other`; cs `_one _few _other`; pl `_one _few _many _other`; ja/zh `_other`). Desktop strings are reused, not copied: `bank_accounts:csvImport.*`, `statementImport.*`, `importHistory.*`, `categorization.*`, `readiness.*`, `common:*`. `src/i18n/locales.test.ts` must pass.
- `invoke()` only in `src/lib/tauri-api.ts`.
- Gate per task before committing: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check` (run `npm run format` first). A task that changes Rust also runs `cd src-tauri && cargo fmt`, `cd src-tauri && cargo clippy --all-targets -- -D warnings`, `cd src-tauri && cargo test`, `cd src-tauri && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`. Conventional Commit, trailer `Co-Authored-By: <the model that wrote the code> <noreply@anthropic.com>` (Claude Sonnet 5.5 / Claude Opus 5.5 / Claude Haiku 5.5).
- Subagents: implementers `sonnet` (Task 7: `opus`), translations and data entry `haiku`, task reviews `sonnet`, reviews of Rust / security / capability tasks (1, 4, 6, 7) `opus`, final whole-branch review `opus`; always an explicit `model`, never Fable. Ledger `.superpowers/sdd/2026-10-18-mobile-phase-2b-import/progress.md`.
- Simulator (controller only, never an implementer): iPhone 17, UDID `9B31123B-089C-451D-858B-587ACE9BEB2D`, Vite on port 1440, addressed by UDID, never `booted`. The demo profile (Alex Novák, `demo1234`) comes from the scratchpad copy named in the brief, or `cd src-tauri && cargo run --bin seed_demo -- --out <scratch path>`.

From the spec:

- **No mapping grid, ever** (§8.6): every rung answers without asking when it can. Targets: ≤ 4 taps from the share sheet to "Hotovo" for a known file, ≤ 8 taps including one correction for an unknown CSV. The app never guesses the date or the amount silently: when either is not inferred, the recognition opens on that field's correction.
- **Inputs** (§8.6): the share sheet / "Otevřít v" (Moony registered for the statement document types), the Files picker inside the sheet, an account's "Importovat výpis", the Údržba card's statement row, Nastavení → Import. A file shared while locked: unlock first, then the import sheet opens with the file, once.
- **Preview** keeps the desktop's guarantees: the counts ("Naimportuje se 48 transakcí · 3 duplicity přeskočeny"), the period, the balance check sentence, the account-match sentence with "Importovat do účtu X" when the file belongs elsewhere, the rows with status and the category each would get; a row's category can be fixed in the preview.
- **Done** has one primary button, **Zařadit N plateb** (the queue), and the desktop's secondary rows (Zobrazit importované, Zapnout balíček pravidel, Nastavit zůstatek podle výpisu). Undo stays in Historie importů.
- **Queue** (§8.5): "N plateb bez kategorie", newest first; row = chip, counterparty, amount, date and a suggested category as a pill (its source as the pill's sub-label); tap the pill = accept (haptic); "Jiná…" = the picker of §8.4; after an accept the row may expand once with "Zařadit i další N platby od X" (switch, default on); a row swiped away as "Bez kategorie, nechat" leaves the queue for 30 days; empty = "Všechno zařazeno" with "Zobrazit cashflow". Reachable from Údržba, the Finance row and the import's done step.
- **Hide amounts** (D6, README §13): every money string in the sheet, the preview, the done step, the recognition rows and the queue goes through `useMoneyText()` / `<Money>` / `signedAmountText`; counts, dates and percents stay. Acceptance: `grep -rn "fmt.money(" src/mobile --include='*.ts' --include='*.tsx'` lists only `src/mobile/hooks/use-money-text.ts`.
- Phone contract: `Sheet` + `useSheetHistory` (stacked sheets; Android back closes the newest), `EntityRow` rules (chevron only without a trailing value), 44 pt targets, `text-m-*` roles, tokens only, light only until Phase 6; a new component gets its `mobile.css` block, a `mobile.html` demo (no horizontal overflow at 360) and a README §13 row in the same commit.
- Money is TEXT, amounts stay in their own currency, never converted 1:1 (ADR 0012).
- Privacy (§4, §9): this phase adds no network request. A bank's web-banking link opens only on a tap, in the system browser (opener). Statement files that iOS copies into the app's Inbox folders are deleted once imported or abandoned, and swept at startup.

### API inventory (existing, `src/lib/tauri-api.ts` unless noted)

| Need | Wrapper / hook | Command |
|---|---|---|
| Inspect a statement (CAMT.053, MT940, OFX/QFX, QIF) | `bankAccountsApi.inspectStatementFile(accountId, path, country)` → `StatementInspection \| null` (`null` = not a statement) | `inspect_statement_file` |
| Preview / import a statement | `bankAccountsApi.previewStatementImport(…)`, `useStatementImportMutation()` (`src/components/bank-accounts/csv-import/use-csv-import-mutations.ts`) | `preview_statement_import`, `import_statement_file` |
| Inspect a CSV (preset, header mapping, date format, decimal) | `bankAccountsApi.parseCsvFile(path, delimiter?, skipRows?, headerRow?, presetId?, encoding?, country?)` → `CsvPreviewResult` | `parse_csv_file` |
| Preview / import a CSV | `bankAccountsApi.previewCsvImport(accountId, path, config)`, `useCsvImportMutation(accountId)` | `preview_csv_import`, `import_csv_transactions` |
| Mapping and config helpers (pure, desktop) | `mappingFromInspection`, `buildImportConfig`, `isMappingComplete`, `summarizePreview`, `nextStepsFor`, `statementDateOrder`, `statementImportBlocked`, `fileNameFromPath`, `formatBalance`, `regionName` (`src/components/bank-accounts/csv-import/import-config.ts`) | — |
| Done-step writes | `useRecordBalance(accountId)`, `useEnableRulePack()` (same mutations file) | — |
| Presets | `bankAccountsApi.getCsvPresets()` | `get_csv_presets` |
| Accounts | `useBankAccounts()` → `BankAccountWithInstitution[]` (`institutionId`, `institution`) | `get_all_bank_accounts` |
| Add an account | desktop `BankAccountFormDialog` as Finance's `+` uses it | — |
| Profile country (legacy encodings) | `useAuth().user?.country` | — |
| Pick a file | `open({ multiple: false, directory: false })` from `@tauri-apps/plugin-dialog`, then `dialogPathToFsPath()` (`src/mobile/lib/export-file.ts`, Phase 5: iOS returns percent-encoded `file://` URLs) | — |
| Categorize with learning | `useCategorize(tx, categories)` (`src/mobile/hooks/use-categorize.ts`), `CategoryPickerSheet`, `useLedgerMutations()` (accept/decline an MCP suggestion) | `update_transaction_category`, `learn_categorization`, … |
| Transaction details | `TransactionSheet` (`src/mobile/sheets/TransactionSheet.tsx`) | — |

## Review Focus

Inputs the spec implies and no task's own feature tests exercise; each line's test is pinned to the owning task.

1. **A statement shared into a locked or not-yet-running app** (cold start from the share sheet): the path survives until unlock and the import sheet opens with it exactly once — never twice (a second unlock, a remount, the event and the startup drain racing) and never lost. Task 1: `ReceivedFiles` drain test (push, take, take again is empty; non-`file` URLs dropped; duplicates once). Task 3: `received-queue.ts` test (startup drain and the event in either order deliver each path once; a path delivered while a sheet is open waits).
2. **The discard command given a path outside the Inbox folders** (a compromised webview, `../` segments, a symlink out): refused with `validation.receivedFileOutside`, nothing deleted. Task 1: `is_received_copy` tests with `..`, a sibling folder, a nested folder, a symlink and the two real Inbox folders.
3. **Hide amounts on** across the preview rows, the done step's balance sentence, the recognition's three sample rows and the queue rows: every amount masked, the sign kept. Tasks 2, 5, 8: the task reviewer runs the README §13 grep and rejects any `fmt.money(`; the controller takes one screenshot with the eye on.
4. **An unknown Czech CSV read while the UI is Japanese** (decimal comma, NBSP thousands "−1 249,00", `dd.mm.yyyy`): the inference finds date and amount from content alone and the preview formats in the app language. Task 7: `infers_czech_comma_amounts_with_nbsp_thousands` (no header synonyms used: headers renamed to `Sloupec 1…n`).
5. **A file that is no statement** (a PDF from a bank app, a photo, an empty CSV, a CSV without dates): the sheet says plainly what it can read, offers the bank's export guide and the supported formats, and never shows a raw error. Task 2: `fileKind` test (pdf, heic/jpg, unknown extension tried as text); Task 7: `no_date_column_yields_no_date` and an empty file; Task 8: `recognitionStart` opens on the date field when the date is missing and ends unreadable when date or amount is still missing after correction.

---

## Task 1: Received files — iOS document types, `RunEvent::Opened`, drain and discard (Rust)

**Files:**
- Create: `src-tauri/src/services/received_files.rs`, `src-tauri/src/commands/received_files.rs`
- Modify: `src-tauri/src/services/mod.rs`, `src-tauri/src/commands/mod.rs` (`#[cfg(mobile)] pub mod received_files;`), `src-tauri/src/lib.rs` (managed state, startup sweep, handlers, the `Opened` arm in `.run`), `src-tauri/Info.ios.plist`, `src/lib/tauri-api.ts`

**Interfaces:**
- Produces (Rust, `services::received_files`):
  - `pub struct ReceivedFiles` (a `Mutex<Vec<PathBuf>>`, `Default`), `pub fn push_urls(&self, urls: &[url::Url]) -> usize` (keeps `file` URLs only, `Url::to_file_path`, skips a path already queued; returns how many were added), `pub fn take(&self) -> Vec<PathBuf>` (drains, oldest first).
  - `pub fn inbox_dirs(document_dir: &Path, temp_dir: &Path, identifier: &str) -> [PathBuf; 2]` → `<Documents>/Inbox` (where iOS copies a file opened in Moony) and `<tmp>/<identifier>-Inbox` (where the document picker's copy lands).
  - `pub fn is_received_copy(path: &Path, inbox_dirs: &[PathBuf]) -> bool`: the canonicalized path is a regular file directly inside one canonicalized inbox dir (no `..`, no symlink leaving it, no nested folder).
  - `pub fn discard(path: &Path, inbox_dirs: &[PathBuf]) -> Result<()>`: `AppError::Validation("validation.receivedFileOutside")` when not a received copy; a file already gone is `Ok(())`.
  - `pub fn sweep(inbox_dirs: &[PathBuf]) -> Result<usize>`: removes every regular file directly inside the dirs (missing dirs are fine).
- Produces (commands, `#[cfg(mobile)]`): `take_received_files(state) -> Result<Vec<String>>`, `discard_received_file(app, path: String) -> Result<()>`; event `received-files` (no payload) emitted after `push_urls` added something.
- Produces (TS): `receivedFilesApi.take(): Promise<string[]>`, `receivedFilesApi.discard(path: string): Promise<void>`, `export const RECEIVED_FILES_EVENT = 'received-files'` — all three documented "phones only" like `dataApi.stageBackupExport`.

- [ ] **Step 1: Write the failing service tests** in `received_files.rs` (`tempfile` dirs): `push_keeps_file_urls_and_drops_others` (`https://…`, `moony://…` dropped), `push_queues_a_path_once`, `take_drains_in_order_and_twice_is_empty`, `is_received_copy_accepts_a_file_in_either_inbox`, `is_received_copy_refuses_parent_segments_siblings_nested_and_symlinks`, `discard_refuses_outside_with_the_validation_key`, `discard_of_a_missing_file_is_ok`, `sweep_removes_files_and_keeps_folders`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test received_files` — expected FAIL (module missing).
- [ ] **Step 3: Implement the service**, then the two thin commands (the inbox dirs from `app.path().document_dir()`, `app.path().temp_dir()` and `app.config().identifier`).
- [ ] **Step 4: Wire `lib.rs`:** `.manage(ReceivedFiles::default())` under `#[cfg(mobile)]`; in `setup` a `#[cfg(mobile)]` startup sweep off the main thread (as `spawn_staged_export_sweep`, best effort, logged); register both commands under `#[cfg(mobile)]`; in `.run(|app, event| …)` a `#[cfg(mobile)] tauri::RunEvent::Opened { urls }` arm that pushes and, when something was added, emits `received-files` to the app (log the count, never the paths).
- [ ] **Step 5: Declare the document types in `src-tauri/Info.ios.plist`** (iOS only — `bundle.fileAssociations` in `tauri.conf.json` would register the desktop app for CSV files too): `CFBundleDocumentTypes` with one entry "Bank statement" (`LSHandlerRank` `Alternate`, `LSItemContentTypes`: `public.comma-separated-values-text`, `public.plain-text`, `public.xml`, `com.moonyfinance.ofx`, `com.moonyfinance.qif`, `com.moonyfinance.mt940`), `UTImportedTypeDeclarations` for the three own types (extensions `ofx`/`qfx`, `qif`, `sta`/`940`/`mt940`; conforming to `public.data` and `public.text`), and `LSSupportsOpeningDocumentsInPlace` = `false` (iOS copies the file into `Documents/Inbox`, which Task 1 cleans). Leave `src-tauri/gen/apple/**` alone: the controller regenerates and commits it after the iOS build.
- [ ] **Step 6: TS wrappers** in `tauri-api.ts` (a `receivedFilesApi` block next to `dataApi`).
- [ ] **Step 7: Gates** (all four npm gates and the four Rust gates) — PASS.
- [ ] **Step 8: Commit** `feat(mobile): received statement files — iOS document types, opened-file queue and inbox cleanup`.

## Task 2: The import sheet — structured formats and preset CSV (frontend)

**Files:**
- Create: `src/mobile/lib/import-flow.ts` + `import-flow.test.ts`, `src/mobile/hooks/use-import-flow.ts`, `src/mobile/hooks/use-import-sheet.tsx` (provider + hook), `src/mobile/sheets/ImportSheet.tsx`, `src/mobile/components/import/FileBar.tsx`, `src/mobile/components/import/PreviewRows.tsx`, `src/mobile/components/import/DoneStep.tsx`, `src/mobile/sheets/CategoryChoiceSheet.tsx`
- Modify: `src/mobile/pages/finance/AccountDetail.tsx` (the "Importovat výpis" button opens the import sheet instead of the placeholder), `src/mobile/shell/MobileShell.tsx` (wrap the routes in `ImportSheetProvider`), `src/i18n/locales/*/mobile.json` (new `import.*` block, ten locales), `docs/design-system/mobile.css`, `mobile.html`, `README.md` §13

**Interfaces:**
- Consumes: the API inventory; `dialogPathToFsPath` (Phase 5); `receivedFilesApi.discard` (Task 1).
- Produces:
  - `export interface ImportRequest { accountId?: string; path?: string; source: 'account' | 'upkeep' | 'settings' | 'shared' }` and `useImportSheet(): { openImport(request: ImportRequest): void; open: boolean }` (`use-import-sheet.tsx`); `ImportSheetProvider` renders the one `ImportSheet`.
  - `import-flow.ts` (pure): `fileKind(name: string): 'readable' | 'pdf' | 'image'` (pdf; heic/heif/jpg/jpeg/png/gif/webp; anything else — no extension too — is tried as text); `chooseAccount(input: { preselected?: string; statement?: { match: AccountMatch; matchedAccountId: string | null }; presetInstitutionId?: string | null; accounts: readonly { id: string; institutionId: string | null; terminationDate: number | null }[]; todayDay: number }): { accountId: string | null; reason: 'preselected' | 'statement' | 'institution' | 'only' | 'ask' }`; `importFacts(result: CsvImportResult): { imported: number; duplicates: number; errors: number; uncategorized: number; categorized: number }`; `export type ImportStage = 'pick' | 'reading' | 'account' | 'statement' | 'recognize' | 'correct' | 'preview' | 'importing' | 'done' | 'unreadable'` (`recognize` / `correct` are used from Task 8 on).
  - `CategoryChoiceSheet` props `{ open; onOpenChange; categories; value: string | null; onChoose(categoryId: string | null): void }` — a pure choice (no save, no learning) for the preview's overrides.

Flow (`use-import-flow.ts`, one state machine over `ImportStage`):

- **pick**: no file yet. A "Vybrat soubor" primary (Files picker, no type filter — the person may hold a `.STA` or an extensionless export) and, from Task 6 on, the export guide of the account's bank.
- **reading**: `fileKind` first (`pdf` / `image` → **unreadable** with the PDF sentence); a profile without any open account goes to **account** first (only "Nový účet…"); then `inspectStatementFile(accountId ?? firstOpenAccount, path, country)` (the account only anchors the account-match check) — a statement goes to **statement** (pick one of several statements; the QIF date-order segment when `dateOrderAmbiguous`), then **account** if `chooseAccount` says `ask`, then **preview**; `null` → `parseCsvFile(path, …, country)`: a detected preset or a complete header mapping (`isMappingComplete(mappingFromInspection(inspection))`) → **account** / **preview**; otherwise **unreadable** in this task (Task 8 inserts **recognize** here). A thrown error → **unreadable** with the translated error as its sub-line, never a raw message.
- **account**: radio rows of the open accounts (`PickRow`, chip, name, bank · currency) and "Nový účet…" (the desktop `BankAccountFormDialog`, then the new account is chosen). The statement's `accountMatch === 'other'` in the preview shows `bank_accounts:statementImport.match` text with an "Importovat do účtu X" text button that switches the target and re-previews.
- **preview**: the sheet's title "Import výpisu", sub "Do účtu <name> · <bank>" (+ "Ze sdílení" for `shared`); the counts line "Naimportuje se N transakcí · M duplicit přeskočeno" (`summarizePreview`), the period (`fmt.day` range), the statement's balance check sentence, then `PreviewRows`: the first 50 rows as entity rows (initials chip, counterparty, `signedAmountText` value in the row's currency, sub "date · category or Bez kategorie", a status badge for duplicate / error / skipped like `LedgerRow`'s `RowBadge`), "a N dalších" under them; a tap on an `ok` row opens `CategoryChoiceSheet` and sets a `categoryOverrides` entry. Footer: "Zrušit" ghost, "Importovat N transakcí" primary (`bank_accounts:csvImport.importButton_*`). Duplicates are skipped (no "import anyway" on the phone, ruling 3).
- **importing** → **done**: three flat stats Importováno / Duplicity / Chyby (`StatRow`), the balance sentence (statement or `lastBalance`), "N pohybů zařadila pravidla, M zůstává bez kategorie", primary "Zařadit M plateb" (hidden when M = 0) → closes the sheet and pushes `/finance/uncategorized?account=<id>` (`leaveAfterSheet` pattern), then the secondary rows from `nextStepsFor` (Nastavit zůstatek → `useRecordBalance`, Zapnout balíček pravidel → `useEnableRulePack`, Zobrazit importované → the account detail). Footer: "Hotovo" ghost alone.
- **unreadable**: the reason (PDF: "Výpis v PDF zatím přečíst neumím — stáhněte si z banky CSV, CAMT, MT940, OFX nebo QIF."; otherwise "Tento soubor neobsahuje výpis, který bych uměl přečíst"), the supported formats line (`bank_accounts:statementImport.supportedFormats`), "Vybrat jiný soubor" primary.
- Whenever the sheet closes (any stage) and the path came from a share or the picker's copy, `receivedFilesApi.discard(path)` runs best-effort on iOS (`TAURI_ENV_PLATFORM === 'ios'`); a refusal (a desktop dev path) is ignored silently.
- Every write refreshes through the desktop mutations, which already call `refreshMoneyFlows` (AGENTS rule 7).

- [ ] **Step 1: Write the failing tests** `import-flow.test.ts`: `fileKind` (`vypis.PDF` → pdf, `scan.heic` → image, `export` → readable, `x.sta` → readable); `chooseAccount` (preselected wins; statement `selected`/`other` → the matched account with reason `statement` when not preselected; the one open account of the preset's institution; a closed account never chosen; one open account → `only`; two candidates → `ask`); `importFacts` (categorized = imported − uncategorized, never negative).
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/import-flow.test.ts` — FAIL.
- [ ] **Step 3: Implement** `import-flow.ts`, then the hook, the provider, the sheet and its three components, the `CategoryChoiceSheet` (search + `PickGroup`/`PickRow` like the picker, "Bez kategorie" first), the AccountDetail wiring, the strings (ten locales; `haiku` may translate from the `en`/`cs` the implementer writes) and the contract rows (`.m-filebar`, `.m-imp-*` for the preview rows and the done block).
- [ ] **Step 4: Run** the four npm gates — PASS. Reviewer: hide-amounts grep; 44 pt targets; one primary per footer.
- [ ] **Step 5: Commit** `feat(mobile): import sheet for statements and preset CSV with preview, overrides and the done step`.

## Task 3: Entry points and share-in host

**Files:**
- Create: `src/mobile/lib/received-queue.ts` + `received-queue.test.ts`, `src/mobile/components/import/ReceivedFileHost.tsx`
- Modify: `src/mobile/shell/MobileShell.tsx` (mount `ReceivedFileHost` while unlocked, inside the provider), `src/mobile/pages/Settings.tsx` (the Import row opens the sheet with no account; sub-line "CSV, CAMT.053, MT940, OFX, QIF"; drop its `PlaceholderSheet`), `src/mobile/components/UpkeepCard.tsx` (the `statement_due` verb opens the sheet for `row.accountIds[0]`, source `upkeep`), `src/i18n/locales/*/mobile.json`

**Interfaces:**
- Consumes: `receivedFilesApi`, `RECEIVED_FILES_EVENT` (Task 1); `useImportSheet` (Task 2).
- Produces: `createReceivedQueue(deliver: (path: string) => void): { offer(paths: readonly string[]): void; sheetClosed(): void }` — pure: offers dedupe by path for the queue's lifetime, deliver one path at a time, the next only after `sheetClosed()`.

- [ ] **Step 1: Write the failing test** `received-queue.test.ts`: `delivers_each_path_once_when_drain_and_event_overlap`, `waits_for_the_sheet_before_the_next_path`, `a_path_offered_again_later_is_not_redelivered`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** the queue; `ReceivedFileHost` (only mounted while `appStatus === 'unlocked'`, so a file shared while locked waits in Rust until the unlock): on mount `receivedFilesApi.take()` → `offer`, `listen(RECEIVED_FILES_EVENT)` → `take()` → `offer`; deliver = `openImport({ path, source: 'shared' })`; outside iOS the host does nothing (`TAURI_ENV_PLATFORM`). Wire Settings and Údržba.
- [ ] **Step 4: Run** the npm gates — PASS.
- [ ] **Step 5: Commit** `feat(mobile): open shared statements after unlock; import from Nastavení and Údržba`.

## Task 4: Categorize queue backend — snooze column, queue read with suggestions (Rust)

**Files:**
- Create: `src-tauri/src/services/categorize_queue.rs`
- Modify: `src-tauri/src/db/migrations.rs` (append `MIGRATION_023` + `("023_categorize_snooze", MIGRATION_023)`), `src-tauri/src/services/mod.rs`, `src-tauri/src/models/categorization.rs`, `src-tauri/src/commands/categorization.rs`, `src-tauri/src/lib.rs` (two handlers), `src-tauri/src/bindings.rs`, `shared/generated-types.ts` (regenerated), `shared/schema.ts`, `src/lib/tauri-api.ts`; `src-tauri/src/services/bank_history.rs` only to make its column list and row mapper `pub(crate)` if the queue reuses them

**Interfaces:**
- Migration 023: `ALTER TABLE bank_transactions ADD COLUMN categorize_snoozed_until INTEGER;` (unix seconds, nullable; spec §11's one new column).
- Models (wire, camelCase): `SuggestionSource` = `"mcp" | "learned" | "rule" | "pack"`; `CategorySuggestion { categoryId: string; source: SuggestionSource }`; `CategorizeQueueRow { transaction: BankTransaction; suggestion: CategorySuggestion | null }`; `CategorizeQueue { rows: CategorizeQueueRow[]; total: number; snoozed: number }`; `SnoozeCategorization { transactionIds: string[]; days: number }` with `validate()` (1–1000 ids, each non-empty; `days` 1–365).
- Service: `pub fn list(conn: &Connection, engine: &CategorizationEngine, account_id: Option<&str>, now: i64) -> Result<CategorizeQueue>` — booked rows (`status = 'booked'`) with `category_id IS NULL` and no snooze in force (`categorize_snoozed_until IS NULL OR <= now`), newest first (`booking_date DESC, created_at DESC`), at most 500 rows (`total` counts all); `snoozed` = rows held back by a snooze. Suggestion: a stored `suggested_category_id` (MCP) first; else `engine.explain(&input)` — `Layer::Custom` / `OwnAccount` → `rule`, `Learned` → `learned`, `Pack` → `pack`, no match → none; a suggested id whose category no longer exists → none. `pub fn snooze(conn, ids: &[String], until: i64) -> Result<usize>` touches only uncategorized rows.
- Commands: `get_categorize_queue(db, categorization, account_id: Option<String>) -> Result<CategorizeQueue>`, `snooze_categorization(db, input: SnoozeCategorization) -> Result<usize>` (validate, then `until = now + days·86400`). Not `cfg(mobile)`: harmless on the desktop, which keeps its own counts.
- TS: `categorizationApi.getCategorizeQueue(accountId?: string): Promise<CategorizeQueue>`, `categorizationApi.snoozeCategorization(transactionIds: string[], days: number): Promise<number>`.

- [ ] **Step 1: Write the failing service tests** (in-memory schema with `bank_transactions` incl. the new column, `transaction_categories`): `lists_booked_uncategorized_newest_first`, `excludes_pending_categorized_and_snoozed_until_it_expires`, `filters_by_account`, `mcp_suggestion_wins_over_the_engine`, `engine_layers_map_to_sources` (a learned payee → `learned`, a custom rule → `rule`), `drops_a_suggestion_for_a_deleted_category`, `caps_rows_but_counts_total`, `snooze_skips_categorized_rows`; plus a migration test that 023 applies on top of 022 (follow `migration_012`'s chain pattern).
- [ ] **Step 2: Run** `cd src-tauri && cargo test categorize_queue` — FAIL.
- [ ] **Step 3: Implement** migration, models (+ `validate()`), service, commands, handlers, bindings, regenerate (`cd src-tauri && cargo test generate_bindings -- --ignored`), `schema.ts` mirror, wrappers.
- [ ] **Step 4: Gates** (npm + Rust) — PASS.
- [ ] **Step 5: Commit** `feat(categorization): categorize queue with suggestions and a 30-day snooze (migration 023)`.

## Task 5: The categorize queue page (frontend)

**Files:**
- Create: `src/mobile/lib/categorize-queue.ts` + `.test.ts`, `src/mobile/hooks/use-categorize-queue.ts`, `src/mobile/pages/finance/CategorizeQueue.tsx`, `src/mobile/components/QueueRow.tsx`
- Modify: `src/mobile/hooks/use-categorize.ts` (additive `PickOptions.onApplied?: (changed: ChangedRow[]) => void` and `quiet?: boolean`, which skips the history toast), `src/mobile/shell/MobileShell.tsx` (the `uncategorized` route renders `CategorizeQueue`), `src/mobile/pages/finance/FinancePlaceholder.tsx` (drop the `queue` kind), `src/mobile/pages/Finance.tsx` (the Nezařazené row always opens the queue, `?account=<id>` when one account holds them), `src/mobile/hooks/use-upkeep.ts` + `use-finance.ts` + `use-account-upkeep.ts` (the count and the accounts from the queue: snoozed rows no longer count; delete `useUncategorizedAll` if nothing else uses it), `src/mobile/components/UpkeepCard.tsx` (the `uncategorized` verb opens the queue), `src/i18n/locales/*/mobile.json`, design-system files

**Interfaces:**
- Consumes: Task 4's wrappers and types; `useCategorize`, `CategoryPickerSheet`, `TransactionSheet`, `useLedgerMutations().acceptSuggestion`, `ProgressBar` (cashflow), `EntityRow` swipe props.
- Produces: `useCategorizeQueue(accountId?: string)` — key `['bank-transactions', 'categorize-queue', accountId ?? 'all']` (under the `bank-transactions` prefix, so every `refreshMoneyFlows` refreshes it); `categorize-queue.ts`: `queueProgress(snapshotIds: readonly string[], openIds: ReadonlySet<string>): { handled: number; total: number }`, `rowState(id, openIds, local: ReadonlyMap<string, LocalOutcome>): 'open' | 'done' | 'left'` with `LocalOutcome = { kind: 'categorized'; categoryId: string; batch: ChangedRow[] } | { kind: 'left' }`, `suggestionLabelKey(source: SuggestionSource): string`.

Page (`/finance/uncategorized`, optional `?account=`): back button "Finance"; the body opens with the large count "N plateb bez kategorie" (open rows, plural) and the sub-line (one account: "<account> · z výpisu <newest import day>"; several: "<n> účty"), "· K už zařazeno" when K > 0; the progress row (`ProgressBar` + "3 z 12"). The rows are a **snapshot** taken when the page opens (rows never jump while you work); a row that left the open set shows done (category name, muted) or "nechána na 30 dní". `QueueRow` (block entity row, prototype frame 05): title + signed amount (`signedAmountText`), sub "<den> · <detail>", then a pill row — with a suggestion: a dark pill (check, category name, small source label "návrh z MCP" / "naučené" / "podle pravidla" / "balíček pravidel") and a ghost "Jiná…"; without: "Vybrat kategorii…" and a ghost "Nechat". Tap the dark pill = accept: an MCP suggestion through `acceptSuggestion`, an engine suggestion through `useCategorize.pick(id, { remember: true, applyHistory: true, quiet: true, onApplied })`; then `selectionTick()` (haptic). "Jiná…" / "Vybrat…" / a tap on the row = `CategoryPickerSheet` (its own switches; `onApplied` given too). After an accept whose `changed` is not empty the row expands once: "Zařazeny i další N platby od <protistrana>" with a switch, on; switching it off restores those rows (`restoreTransactionCategories`) and disables the switch (ruling 4). Swipe left = "Nechat" (`snoozeCategorization([id], 30)`), swipe right = Podrobnosti (`TransactionSheet`), long press = menu (Podrobnosti, Kategorie, Nechat 30 dní). Under the list the caption from frame 05. Empty (no open rows): "Všechno zařazeno" + one primary "Zobrazit cashflow" (`routes.cashflow`).

- [ ] **Step 1: Write the failing tests** (`queueProgress` counts rows gone from the open set; `rowState` prefers the local outcome, `left` beats `done`; a batch-categorized row not touched locally is `done`; `suggestionLabelKey` covers all four sources).
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** lib, hook, the `useCategorize` options, `QueueRow`, the page, the wiring (Finance, Údržba, upkeep counts), strings, contract (`.m-queue`, `.m-pillrow`, `.m-pill--dark|--ghost|--done`, the expansion).
- [ ] **Step 4: Run** the npm gates — PASS. Reviewer: hide-amounts grep; swipe actions also reachable from the menu.
- [ ] **Step 5: Commit** `feat(mobile): categorize queue with suggestion pills, batch learning, snooze and progress`.

## Task 6: Presets carry a mobile export guide; guides in the sheet (Rust data + frontend)

**Files:**
- Modify: `src-tauri/src/services/csv_presets.rs` (field + validation + test), the fifteen bank presets in `src-tauri/resources/csv-presets/*.json` (not `uk-debit-credit.json`, which is a layout, not a bank), `src-tauri/src/bindings.rs` / `shared/generated-types.ts` (regenerated if the type is registered), `shared/schema.ts`
- Create: `src/mobile/lib/export-guide.ts` + `.test.ts`, `src/mobile/components/import/ExportGuide.tsx`
- Modify: `src/mobile/sheets/ImportSheet.tsx` / `use-import-flow.ts` (the guide on **pick** and **unreadable**; "Návody bank" list on **unreadable**), `src/i18n/locales/*/mobile.json`, design-system files

**Interfaces:**
- Rust (`CsvPreset`): `#[serde(rename = "mobileExport", default)] pub mobile_export: Option<MobileExportGuide>`; `MobileExportGuide { route: ExportRoute ("app" | "web"), format: String, steps: BTreeMap<String, Vec<String>> (locale → steps), #[serde(rename = "appUrlScheme")] app_url_scheme: Option<String>, #[serde(rename = "webUrl")] web_url: Option<String> }`, `deny_unknown_fields`. `validate_preset` adds: all ten locales present, 2–4 non-empty steps each, a `web` route has an `https://` `webUrl`, `appUrlScheme` (when set) is a bare scheme without `://`.
- Data: from T1 per bank — the route the person can actually take on a phone (`app` where the bank's own app exports CSV per T1: mBank, Wise, Revolut "Excel"; `web` everywhere else: web banking in Safari → export CSV → save to Files → "Sdílet → Moony"), the format to pick, three steps per locale, `webUrl` = the bank's public internet-banking entry; `appUrlScheme` stays `null` (T1 documents none; never invent one). Steps never claim an app cannot export (T1 caveat).
- TS: `CsvPreset.mobileExport: MobileExportGuide | null` mirror; `export-guide.ts`: `guideSteps(guide, language: string): string[]` (the language, else `en`), `guideForAccount(presets, account): CsvPreset | null` (by `institutionId`), `guidesList(presets): CsvPreset[]` (banks with a guide, sorted by country then name).
- The guide view: "Jak získat výpis z <banka>" section head, numbered steps (13 / 450 `ink-2`), the format as a caption, a secondary "Otevřít internetové bankovnictví" (opener `openUrl(webUrl)`) for a web route. On **pick** it shows the account's bank guide under "Vybrat soubor"; on **unreadable** the list of banks opens a guide in place.
- Catalog refresh: **not implemented** (no outbound request). The hand-over describes the opt-in design for the owner.

- [ ] **Step 1: Write the failing tests**: Rust `a_mobile_export_guide_needs_all_ten_locales`, `a_web_route_needs_an_https_url`, `a_url_scheme_is_bare` (unit tests on `validate_preset` with small JSON), and the existing `all_presets_are_valid` (now covering the guides); TS `guideSteps` fallback to `en`, `guideForAccount` by institution, `guidesList` order.
- [ ] **Step 2: Run** both — FAIL.
- [ ] **Step 3: Implement** the Rust field and validation, the 15 guides (the implementer writes `en` and `cs` from T1; `haiku` fills the other eight locales in the JSON), the TS mirror, the lib and the view.
- [ ] **Step 4: Gates** (npm + Rust) — PASS.
- [ ] **Step 5: Commit** `feat(import): per-bank mobile export guides in the presets and in the import sheet`.

## Task 7: Content-based column inference for an unknown CSV (Rust) — implementer `opus`

**Files:**
- Create: `src-tauri/src/services/csv_import/infer.rs` (+ tests in the same file or `infer_tests.rs`)
- Modify: `src-tauri/src/services/csv_import/mod.rs`, `src-tauri/src/services/csv_import/types.rs` (wire types), `src-tauri/src/commands/bank_accounts.rs` (`infer_csv_columns`), `src-tauri/src/lib.rs` (handler), `src-tauri/src/bindings.rs`, `shared/generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts`

**Interfaces:**
- Wire types: `InferredField { column: string; confidence: number }`; `SignCheck = "verified" | "inverted" | "unchecked"`; `InferredColumns { date: InferredField | null; dateFormat: string | null; dateAmbiguous: boolean; amount: InferredField | null; debit: InferredField | null; credit: InferredField | null; balance: InferredField | null; signCheck: SignCheck; currency: InferredField | null; counterpartyIban: InferredField | null; counterparty: InferredField | null; transactionId: InferredField | null; description: string[]; decimalSeparator: string }`.
- Service: `pub fn infer_columns(headers: &[String], rows: &[Vec<String>]) -> InferredColumns` (pure; decimal separator detected from the numeric-looking cells with `detect_decimal_separator`).
- Command: `infer_csv_columns(file_path: String, delimiter: Option<String>, header_row: Option<usize>, encoding: Option<String>, country: Option<String>) -> Result<InferredColumns>` — the same decode (`decode_for_import_in`), delimiter and header row as `parse_csv_file` reported (the frontend passes them back), the first `SAMPLE_ROWS` (200) non-empty data rows. The desktop `CsvPreviewResult` does not change (ruling 8).
- TS: `bankAccountsApi.inferCsvColumns(filePath, { delimiter, headerRow, encoding, country })`.

Rules (spec §8.6 rung 3; content decides, header synonyms only break ties via `suggest_column_mappings`):

- *date*: ≥ 90 % of non-empty cells parse with one format (`date_parser::detect_date_format`); several date columns → the header tie-break (booking date over value date), else the leftmost. `dateAmbiguous` when dmy/mdy cannot be told.
- *amount*: a column whose non-empty cells ≥ 90 % parse as amounts (`clean_and_parse_amount`) with both signs present, or a *debit + credit pair*: two numeric columns where exactly one is filled (non-zero) on ≥ 90 % of rows; a one-signed column next to a direction word column is not handled (left `None` → correction).
- *balance*: numeric and consistent with the amounts: `balance[i] − balance[i−1] = amount[i]` on ≥ 80 % of consecutive pairs in file order or reversed (newest-first files) → `signCheck: verified`; `= −amount[i]` → `inverted`; otherwise no balance and `unchecked`.
- *currency*: three uppercase letters on ≥ 90 % of cells, ≤ 5 distinct values; *counterpartyIban*: ISO 13616 shape with a valid mod-97 on ≥ 90 % of non-empty cells, or Czech domestic `[prefix-]number/bank` on ≥ 90 %; *transactionId*: unique non-empty on every row, neither date nor amount, not a 1…n row counter; *counterparty*: text with 2 ≤ distinct ≤ 90 % of rows and a mean length 3–40; *description*: the remaining text columns, longest mean length first, at most three.
- `confidence`: the share of cells that matched, lowered by 0.1 when only the content (no header synonym) decided.

- [ ] **Step 1: Write the failing tests**: for each preset fixture in `src-tauri/tests/fixtures/csv/` that a preset reads (`fio`, `ceska-sporitelna`, `revolut`, `wise`, `n26`, `de-sparkasse`, `us-chase`, `uk-debit-credit`, `generic-iso`) `infers_the_preset_columns_<bank>` — date and amount (or debit + credit) equal the preset's resolved columns; synthetic: `infers_czech_comma_amounts_with_nbsp_thousands` (Review Focus 4, headers `Sloupec 1…n`), `balance_proves_the_sign_newest_first`, `balance_shows_inverted_signs`, `infers_a_debit_credit_pair`, `flags_ambiguous_day_month`, `no_date_column_yields_no_date`, `a_row_counter_is_not_a_transaction_id`, `an_empty_file_infers_nothing`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test infer` — FAIL.
- [ ] **Step 3: Implement** the service, the command, the types (7-step procedure), the wrapper.
- [ ] **Step 4: Gates** (npm + Rust) — PASS.
- [ ] **Step 5: Commit** `feat(import): infer the columns of an unknown CSV from its content`.

## Task 8: "Takhle tomu rozumím" and the per-field correction (frontend)

**Files:**
- Create: `src/mobile/lib/recognition.ts` + `.test.ts`, `src/mobile/components/import/RecognizeStep.tsx`, `src/mobile/components/import/CorrectStep.tsx`
- Modify: `src/mobile/hooks/use-import-flow.ts` (no preset → `inferCsvColumns` → **recognize**; corrections; the sample rows), `src/mobile/sheets/ImportSheet.tsx`, `src/i18n/locales/*/mobile.json`, design-system files

**Interfaces:**
- Consumes: `InferredColumns` (Task 7), `CsvPreviewResult` and `MappingState` / `buildImportConfig` (desktop `import-config.ts`).
- Produces (`recognition.ts`, pure): `type RecognitionField = 'date' | 'amount' | 'description' | 'counterparty' | 'currency' | 'balance' | 'transactionId'`; `mappingFromInferred(inferred: InferredColumns, inspection: CsvPreviewResult): MappingState`; `recognizedFields(mapping: MappingState): Array<{ field: RecognitionField; columns: string[] }>` (in the order above; `columns` empty = not used); `columnOptions(field: RecognitionField, inspection: CsvPreviewResult, inferred: InferredColumns): Array<{ id: string; label: string; samples: string[]; apply: Partial<MappingState> }>` (each column with its header and two sample values from `sampleRows`; for `amount` also the inferred debit + credit pair; for `date` when `dateAmbiguous` the two day/month orders; a "nepoužít" option for optional fields); `recognitionStart(mapping): { stage: 'recognize' } | { stage: 'correct'; field: 'date' | 'amount' }`; `canImport(mapping): boolean` (date and amount or the pair).

The **recognize** stage (frame 06): the file bar ("<file> · N řádků · oddělovač ; · UTF-8 · formát zatím neznám"), section head "Takhle tomu rozumím", caption "Tři pohyby ze souboru tak, jak je načtu:", three rows from `previewCsvImport(…, { previewLimit: 3 })` drawn as `PreviewRows`, then the recognized list (check / minus icon, the field name, the column names it uses; the amount line adds "znaménko ověřeno podle zůstatku" for `verified` and a `loss` warning "výdaje v souboru vypadají kladně" for `inverted`; an unused optional field greyed with its consequence, e.g. ID pohybu "duplicity poznám podle data a částky"). Footer: "Opravit…" ghost, "Vypadá to správně" primary (→ **account** / **preview** with this mapping). **correct** (frame 07): "Opravit…" first lists the fields (rows; tap one), a field opens its title ("Co je částka?" …), the one sentence, `columnOptions` as radio rows with the samples as their sub-line, "Ukázka po výběru:" and one re-previewed sample row, footer "Zpět" ghost / "Použít" primary. Date or amount still missing after "Použít" from the start screen → **unreadable** with the guide list (Task 6). Taps: share → recognize → "Vypadá to správně" → (account) → "Importovat" → "Hotovo" stays within the spec's ≤ 8 with one correction.

- [ ] **Step 1: Write the failing tests**: `mappingFromInferred` (amount vs. pair; description columns kept in order), `recognizedFields` order and empties, `columnOptions` (two samples, the pair option for amount, the two orders for an ambiguous date, "nepoužít" only for optional fields), `recognitionStart` (missing date → correct date first; missing amount → correct amount), `canImport`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** lib, the two steps, the flow wiring, strings, contract (`.m-recog`, `.m-colopt`).
- [ ] **Step 4: Run** the npm gates — PASS.
- [ ] **Step 5: Commit** `feat(mobile): guided recognition of an unknown CSV with per-field correction`.

## Task 9: PDF spike (time-boxed ≤ 2 h, no repo code)

**Files:** a scratch crate under the session scratchpad (`…/scratchpad/pdf-spike/`), never inside `src-tauri`; the result `.superpowers/sdd/2026-10-18-mobile-phase-2b-import/pdf-spike.md`.

Question: can text-layer PDF statements of the three most common Czech banks (Česká spořitelna, Fio, KB) be parsed deterministically in Rust? The agent synthesizes three text-layer PDFs per bank that mimic each bank's public statement layout as described in public sources (header block, a table of booking date, counterparty / account, description, amount with sign, running balance; Czech diacritics; a second page), with any tool available offline in the scratch crate (`printpdf` or `lopdf`'s writer), extracts them with `pdf-extract` and with `lopdf` text extraction, and reports per crate: licence, the added release binary size for `aarch64-apple-ios` (an empty binary vs. one that links the crate: `cargo build --release --target aarch64-apple-ios` in the scratch crate), what the text order looks like (columns interleaved? diacritics intact? amounts split?), whether a per-bank line parser of < 150 lines gets 100 % of the synthesized rows, and the honest caveat that synthesized PDFs are not the banks' real files. Ends with a go / no-go recommendation for the owner (format, crate, licence, size, accuracy, what a real-file test needs). Runs in the background from the start; touches no repo file except the result note.

## Task 10: Simulator verification and screenshots (controller)

- Build from this worktree in the background: `npm run tauri -- ios dev "iPhone 17" --config '{"build":{"devUrl":"http://localhost:1440","beforeDevCommand":"npm run dev -- --port 1440 --strictPort"}}'` (≥ 25 GB free first); after the first build, check the generated `src-tauri/gen/apple/moony_iOS/Info.plist` holds the document types and commit it.
- Demo profile into the app container; sample files from `src-tauri/tests/fixtures/` (`csv/fio.csv`, `statements/camt053/basic.xml`, `statements/mt940/cz-free-text.sta`, an unknown CSV made from `fio.csv` with renamed headers) copied into the container's `Documents` and shared in with `xcrun simctl openurl 9B31123B-089C-451D-858B-587ACE9BEB2D file://…`; the picker through the Files app.
- Evidence: a CSV imported end to end from share-in while locked (unlock → sheet once), a CAMT statement via the picker, an unknown CSV through "Takhle tomu rozumím" with one correction, the queue used (accept a pill, picker, "Nechat", the batch switch, empty state), hide amounts on the preview and the queue, the Inbox copy gone afterwards (`ls` of the container). Screenshots `docs/screenshots/mobile/import-NN.png` committed. Afterwards `pkill -f "tauri ios dev"`, `pkill -f "vite --port 1440"`.

---

## As built / rulings (2026-10-19)

Rulings taken while planning (each says what was decided and what it costs if wrong):

1. **The brief's Phase 0 assumption did not hold**: `feat/mobile` has no `bundle.fileAssociations` and no `RunEvent::Opened` handler. Task 1 builds both. Costs nothing.
2. **Document types are declared in `Info.ios.plist`, not in `tauri.conf.json`'s `bundle.fileAssociations`**: the latter would also register the desktop app as a CSV/XML handler. Android share-in (`content://`) stays a follow-up. Costs an Android pass later.
3. **The phone skips two desktop preview options**: "import anyway" for possible duplicates (duplicates are skipped) and the QIF source-category mapping (rows get categories from the rules). Both stay on the desktop. Costs one more step in the preview if the owner wants them.
4. **The queue's batch learning is applied on accept, the switch undoes it**: an accept learns the pick and categorizes the other matching payments at once (the picker's defaults); the row then expands once with "Zařazeny i další N platby" and the switch on; switching it off restores those rows and the switch stays off. Costs a pre-commit switch if the owner wants the choice before the write.
5. **Údržba and Finance count the queue** (snoozed rows excluded), no longer the desktop's uncategorized stat query; the desktop keeps its own count. Costs one query swap back.
6. **The Files picker has no type filter**: bank exports carry `.STA`, `.940` or no extension; an unreadable file gets a plain sentence and the guide. Costs a filter list.
7. **Received copies are deleted**: the sheet discards a shared or picked copy when it closes, Rust accepts only files directly inside the two Inbox folders, and startup sweeps them. Costs nothing; a person who wants the file keeps it in Files.
8. **The inference is its own command** (`infer_csv_columns`); the desktop's `CsvPreviewResult` does not change. Costs a second read of a small file.
9. **The export guides live in the preset JSON** (spec §8.6: "the guides are data") with all ten locales enforced by the preset validation test, not in `mobile.json`. Costs a move if the owner wants them translated with the app strings.
10. **No catalog refresh, no learned formats, no "contribute format"** (owner's scope for this phase); the hand-over carries the catalog-refresh proposal.
