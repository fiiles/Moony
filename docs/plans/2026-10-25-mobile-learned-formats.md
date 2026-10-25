# Mobile — learned statement formats, Nastavení → Import, change password, main currency — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A CSV the person once confirmed ("Takhle tomu rozumím") is recognised by its layout next month and goes straight to the preview; Nastavení → Import lists, renames and deletes those learned formats next to the bank export guides; Zabezpečení changes the password with a new recovery key while Face ID keeps unlocking; the main currency can be changed on the phone as on the desktop.

**Architecture:** Learned formats are the stock import's saved-formats pattern applied to bank CSVs: a JSON array in `app_config` (`learned_csv_formats`, at most 20), a Rust service in `services/csv_import/learned.rs` keyed by a layout signature built from header cells only, six thin commands, and the phone's import flow consulting it between the preset match (rung 2) and the content inference (rung 3). Learning happens when an import that used a confirmed or corrected mapping succeeds. Nastavení → Import becomes a pushed section page. The password change reuses the desktop's two commands unchanged; the master key does not change, so the copy biometric unlock keeps in the keychain stays valid. The main currency reuses the desktop's profile update and hints.

**Tech Stack:** Rust (Tauri 2, rusqlite/SQLCipher, serde/specta, uuid, chrono), React 19 + TypeScript, wouter, TanStack Query, react-hook-form + zod, react-i18next (`mobile` namespace, ten locales), Tailwind on the design tokens.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` §8.6 (rung 4 learned formats, rung 5 contribute format), §8.9 (Zabezpečení: change password with the new recovery key box; Jazyk a měna; Import), §11 (learned formats live in the database and travel with the profile). Phase 2b plan `docs/plans/2026-10-18-mobile-phase-2b-import.md` (rungs 1–3 as built, its rulings 1–32 stand). Phase 5 plan `docs/plans/2026-10-17-mobile-phase-5-settings.md` (rulings 3 and 4 — main currency read-only, no password change — are superseded here). Phone contract: `docs/design-system/README.md` §13, `mobile.css`, `mobile.html`.

## Global Constraints

Process:

- Worktree `.claude/worktrees/fervent-zhukovsky-5d31fb`, branch `claude/mobile-learned-formats-5d31fb`, cut from `feat/mobile` bc6d9d4a. `git branch --show-current` before every commit. Never push; never touch `main`, `feat/mobile` or another worktree; the parent mobile session merges this branch.
- `Cargo.lock` stays as on `feat/mobile`; no new crate; no blanket `cargo update`.
- One plain shell command per call; stage explicit paths only; never `git stash`.
- Reuse only, never edit: `src/components/**` (desktop), `src/hooks/**`, `src/utils/**`, the desktop CSV import dialog. Desktop behaviour does not change.
- Rust per `docs/standards/rust-backend.md`: thin commands, services take `&Connection`, `AppError` never `Result<_, String>`, the input struct's `validate()` runs in the command before DB work, no `.unwrap()` outside tests. Types per the 7-step `docs/standards/type-contract.md`: per-field `#[serde(rename = "camelCase")]`, registered in `bindings.rs`, `cd src-tauri && cargo test generate_bindings -- --ignored`, hand mirror in `shared/schema.ts`, wrapper in `src/lib/tauri-api.ts`. Service tests use `Connection::open_in_memory()` with a hand-written `app_config` table.
- No migration (ruling 1). Timestamps are unix epoch seconds (`i64`).
- Key material (master key, passwords, recovery keys) never reaches a log; the master key never reaches the webview. Nothing new goes to the network.
- Every user-facing string in all ten locales (en, cs, de, es, it, nb, pl, sv, ja, zh), phone strings in the `mobile` namespace, plurals per `docs/standards/i18n.md` §2; desktop strings are reused, not copied (`settings:password.*`, `settings:currency.*`, `auth:onboarding.recovery.*`, `auth:recoveryKey.*`, `bank_accounts:*`, `common:*`). A new `validation.*` key the Rust side returns lands in `common.json` of every locale. `src/i18n/locales.test.ts` passes.
- `invoke()` only in `src/lib/tauri-api.ts`.
- Phone contract (README §13): tokens only (`src/mobile/no-colour-literals.test.ts`), `text-m-*` roles (`type-roles.test.ts`), 44 pt targets, `Sheet` + `useSheetHistory`, `EntityRow` rules; a new component gets its `mobile.css` block, a `mobile.html` demo (no horizontal overflow at 360, light and dark) and a README §13 row in the same commit. Hide amounts: no money string outside `useMoneyText()` / `<Money>`; `grep -rn "fmt.money(" src/mobile --include='*.ts' --include='*.tsx'` lists only `use-money-text.ts`.
- Gates before every commit: `npm run format`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`; a task that changes Rust also `cd src-tauri && cargo fmt`, `cd src-tauri && cargo clippy --all-targets -- -D warnings`, `cd src-tauri && cargo test`, `cd src-tauri && cargo test generate_bindings -- --ignored` (no drift left), `cd src-tauri && cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`. Conventional Commit; trailer `Co-Authored-By: <the model that wrote the code> <noreply@anthropic.com>` (Claude Sonnet 5.5 / Claude Opus 5.5 / Claude Haiku 5.5).
- Subagents always get an explicit `model`, never Fable: implementers `sonnet` (Task 4: `opus`), reviews `sonnet` for frontend tasks and `opus` for Task 1 (Rust, type contract, stored data) and Task 4 (password, keys, biometrics); final whole-branch review `opus`. Ledger `.superpowers/sdd/2026-10-25-mobile-learned-formats/progress.md`.
- Simulator (controller only): iPhone 17 Pro Max, UDID `CD3961FF-A79E-4DFA-9A44-B591B7E694E6` (never `booted`), Vite on port 1450; demo profile copied from `.claude/worktrees/feat+mobile/demo-data/mobile-demo-profile` (Alex Novák, `demo1234`) and restored after the password change. Screenshots `docs/screenshots/mobile/learned-NN-*.png`.

From the spec:

- Import ladder (§8.6, D8): structured → preset → learned format → guided recognition; no mapping grid ever. A known file: ≤ 4 taps from the share sheet to "Hotovo".
- Rung 4 (§8.6): a confirmed rung-3 result is saved as the person's own format keyed by the header signature, named after the account's institution, stored in `app_config` (max 20), listed in Nastavení → Import → Naučené formáty with rename and delete; next month the same bank goes straight to the preview. The desktop can read and write the same store.
- §11: learned formats live in the database, so they travel with the profile (backup, restore, desktop transfer).
- Rung 5 is outbound: design note only (section "Rung 5 — design note").
- §8.9 Zabezpečení: change password with the new recovery key box. Jazyk a měna: language and main currency (the 36 codes with a rate source, as on the desktop).

### API inventory (existing)

| Need | Where |
|---|---|
| Stock import's saved formats (the pattern to copy) | `src-tauri/src/services/stock_import/formats.rs`, commands in `commands/stock_import.rs` |
| CSV inspection / inference | `bankAccountsApi.parseCsvFile`, `bankAccountsApi.inferCsvColumns`; `CsvPreviewResult`, `InferredColumns` |
| Phone import flow | `src/mobile/hooks/use-import-flow.ts` (`read`, `continueFromRecognition`, `runImport`), `src/mobile/hooks/use-recognition.ts` (`begin`, `mapping`), `src/mobile/lib/recognition.ts`, `src/mobile/lib/import-flow.ts` (`presetReadable`, `chooseAccount`, `isOpenAccount`) |
| Mapping ↔ config | `MappingState`, `EMPTY_MAPPING`, `AUTO_VALUE`, `buildImportConfig`, `isMappingComplete`, `usesDebitCredit` (`src/components/bank-accounts/csv-import/import-config.ts`) |
| Preview notes / export guides | `ImportNote`, `PreviewBody`, `BankGuides`, `guidesList(presets)` (`src/mobile/lib/export-guide.ts`) |
| Settings shell | `SETTINGS_SECTIONS`, `settingsPath`, `SettingsSection.tsx`, `SectionPage`, `SettingsRow`, `EntityRows`, `KeyValueRow`, `ConfirmSheet`, `Sheet` |
| Password change (desktop, unchanged) | `authApi.prepareChangePassword({ currentPassword })` → `{ recoveryKey }`, `authApi.confirmChangePassword({ currentPassword, newPassword, recoveryKey })`; `changePasswordSchema`, `MIN_PASSWORD_LENGTH` (`@shared/schema`); Rust `services/auth.rs` `prepare_change_password` / `confirm_change_password` |
| Recovery key UI (phone first run) | `RecoveryKeyBox` (`touch`), `pickVerificationGroups`, `recoveryKeyGroups`, `verifyGroups`, `recoveryKeyFileContents`, `useExportTextFile`, `RECOVERY_KEY_FILE`, `exportsOnThisDevice` (see `src/mobile/pages/first-run/RecoveryStep.tsx`), `PasswordInput` (`touch`) and the strength hint in `AccountStep.tsx` |
| Biometrics | `biometricApi.status`, `BIOMETRIC_STATUS_KEY`, `biometricCopy`; Rust `services/biometric_unlock.rs` (stores the session's master key) |
| Main currency (desktop) | `src/components/settings/LanguageCard.tsx`: `updateProfile({ currency })`, `FX_SUPPORTED_CODES`, `isFxSupported`, `currencyName` (`@shared/currencies`), strings `settings:currency.{main,mainHint,example,nativeHint,unsupportedMain}`; phone: `useSettingsMutations().updateProfile`, `CurrencyPickerSheet` |

## Review Focus

Inputs the spec implies and no task's feature tests exercise; each line's test is pinned to the owning task.

1. **Next month's file of the same bank** — same header row and delimiter, other rows, possibly another detected encoding (an ASCII-only month of a Windows-1250 export reads as UTF-8) or more preamble lines: it must match the learned format and go straight to the preview. Task 1: `the_same_layout_matches_across_months_encodings_and_preambles` (the September fixture and the new October fixture; the October file re-encoded to Windows-1250 and with one more preamble line has the same signature).
2. **A file whose "header row" is data** (no header line, the first line is a transaction): nothing is learned and no row value is ever stored. Task 1: `a_data_row_is_never_learned` (a date cell, an amount cell, an account number cell each refused with `validation.learnedFormatHeaderless`, store unchanged); Task 2: the flow's learning step treats that refusal as "not learned" and shows no error.
3. **A learned format that no longer fits the file** (a stored column missing from the file after the bank renamed a header in a way the signature still matched, or a damaged entry): the phone never imports with a partial mapping; it falls back to the recognition. Task 2: `mappingForFile` returns `null` when the date or the amount (or one side of the pair) cannot be resolved, and drops an optional column it cannot resolve.
4. **Password changed while Face ID is on, then the app locks**: Face ID still unlocks (same master key), the new password unlocks, the old one is refused, the old recovery key no longer opens the profile. Task 4: Rust `change_password_keeps_the_master_key_biometric_unlock_holds` + simulator evidence in Task 6.
5. **Stored formats that do not read** (a value written by another version, damaged JSON, one entry of an older shape): the import and Nastavení → Import still open; the damaged entry is skipped and the next save repairs the key. Task 1: `unreadable_stored_formats_never_break_the_list`.

---

## Task 1: Learned formats in Rust — store, signature, commands, wire types

**Files:**
- Create: `src-tauri/src/services/csv_import/learned.rs` (+ `#[cfg(test)] mod tests` in the file), `src-tauri/tests/fixtures/csv/unknown-unicredit-like-october.csv` (same header as `unknown-unicredit-like.csv`, 12 October rows, the balance continuing from the September file's newest row)
- Modify: `src-tauri/src/services/csv_import/mod.rs` (module), `src-tauri/src/services/csv_import/types.rs` (wire types), `src-tauri/src/commands/bank_accounts.rs` (six commands), `src-tauri/src/lib.rs` (handlers, not `#[cfg(mobile)]`), `src-tauri/src/bindings.rs`, `shared/generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts`, `src/i18n/locales/*/common.json` (new validation keys)

**Interfaces:**
- Produces (wire, `types.rs`; TS mirrors in `shared/schema.ts` with the same names):
  - `LearnedCsvMapping { dateColumn: string; dateFormat: string /* chrono or "auto" */; amountColumn: string | null; debitColumn: string | null; creditColumn: string | null; balanceColumn: string | null; transactionIdColumn: string | null; descriptionColumns: string[]; counterpartyColumn: string | null; counterpartyIbanColumn: string | null; counterpartyBankCodeColumn: string | null; counterpartyCountryColumn: string | null; currencyColumn: string | null; decimalSeparator: string /* "auto" | "," | "." */ }`
  - `LearnedCsvFormat { id: string /* "learned:<uuid>" */; name: string; signature: string; headers: string[]; delimiter: string; encoding: string; headerRow: number; mapping: LearnedCsvMapping; accountId: string | null; createdAt: number; lastUsedAt: number; shadowedByPresetId: string | null }`
  - `InsertLearnedCsvFormat { name: string; headers: string[]; delimiter: string; encoding: string; headerRow: number; mapping: LearnedCsvMapping; accountId: string | null }` with `validate()`.
- Produces (service `learned.rs`): `pub const CONFIG_KEY: &str = "learned_csv_formats"`; `pub const MAX_FORMATS: usize = 20`; `pub fn layout_signature(headers: &[String], delimiter: &str) -> String`; `pub fn header_looks_like_data(headers: &[String]) -> bool`; `pub fn list_formats(conn: &Connection) -> Result<Vec<LearnedCsvFormat>>`; `pub fn find_format(conn: &Connection, headers: &[String], delimiter: &str) -> Result<Option<LearnedCsvFormat>>`; `pub fn save_format(conn: &Connection, input: &InsertLearnedCsvFormat) -> Result<LearnedCsvFormat>`; `pub fn rename_format(conn: &Connection, id: &str, name: &str) -> Result<LearnedCsvFormat>`; `pub fn delete_format(conn: &Connection, id: &str) -> Result<()>`; `pub fn mark_used(conn: &Connection, id: &str) -> Result<()>`.
- Produces (commands, `bank_accounts.rs`): `list_learned_csv_formats()`, `find_learned_csv_format(headers: Vec<String>, delimiter: String)`, `save_learned_csv_format(format: InsertLearnedCsvFormat)` (calls `format.validate()` first), `rename_learned_csv_format(id: String, name: String)`, `delete_learned_csv_format(id: String)`, `mark_learned_csv_format_used(id: String)`.
- Produces (TS, `tauri-api.ts`): `learnedFormatsApi = { list(), find(headers, delimiter), save(format), rename(id, name), remove(id), markUsed(id) }`; query key constant `LEARNED_FORMATS_KEY = ['learned-csv-formats'] as const` exported from `src/mobile/lib/learned-format.ts` (Task 2 creates the file; Task 1 may create it with just the key).

Rules:

- **Signature**: `"v1"`, the delimiter, the column count and the normalised header cells, joined with `\u{1e}` between the parts and `\u{1f}` between cells. A cell is normalised by trimming, dropping a leading BOM, collapsing whitespace runs to one space and lowercasing; empty cells stay (a trailing delimiter is part of the layout). Encoding and header-row position are stored but are not in the key (ruling 2).
- **Data, not a header**: a cell that `date_parser` reads as a date, that reads as an amount (`clean_and_parse_amount` and at least one digit), or that holds a run of ≥ 6 digits makes `header_looks_like_data` true; `InsertLearnedCsvFormat::validate()` then fails with `validation.learnedFormatHeaderless`.
- **Validation** (`validate()`): name trimmed non-empty (`validation.nameRequired`) and ≤ 60 characters (`validation.nameTooLong`); at least two non-empty headers (`validation.csvEmptyFile`); the header is no data row (above); the delimiter is one character; the date column and either the amount column or both pair columns are set; every named column resolves in `headers` through `columns::find_column` (`validation.learnedFormatColumns`); `dateFormat` and `decimalSeparator` pass the same checks as `CsvImportConfig::validate` / `parse_decimal_separator`.
- **Save** is an upsert by signature: an existing format of that layout keeps its `id` and `createdAt` and gets the new name, mapping, account, encoding and header row; `lastUsedAt` = now. A 21st format drops the one used least recently (`lastUsedAt`). The list is newest-used first.
- **Read**: like `stock_import::formats::list_formats`, unreadable data is skipped with a `log::warn!` that names no header or column; `shadowedByPresetId` is filled on every read from `csv_presets::detect_preset(&headers)` and written as `null`.
- `rename_format` validates the name like `validate()`; `rename_format`, `delete_format` and `mark_used` return `AppError::NotFound` for an unknown id.

- [ ] **Step 1: Write the failing tests** in `learned.rs`: `nothing_is_learned_at_first`; `the_signature_ignores_case_spaces_and_a_bom` (`["\u{feff}Datum ", "ČÁSTKA"]` = `["datum", "částka"]`, `;` vs `,` differ, 3 vs 4 columns differ); `the_same_layout_matches_across_months_encodings_and_preambles` (Review Focus 1: decode `unknown-unicredit-like.csv` and the October fixture, the October bytes re-encoded to Windows-1250 and with one extra preamble line, through `decode_for_import_in` + `detect_csv_delimiter` + `detect_header_row` + `read_headers`: equal signatures; `find_format` returns the format saved from September); `a_data_row_is_never_learned` (Review Focus 2: header rows `["29.09.2026", "Spotify", "-199,00"]`, `["Datum", "1 525,77"]`, `["1234567890/2700", "Datum", "Částka"]` refused with `validation.learnedFormatHeaderless`; `list_formats` empty); `saving_a_layout_again_replaces_it_and_keeps_its_id_and_creation`; `formats_are_listed_newest_used_first`; `at_most_twenty_formats_are_kept_and_the_least_used_goes` (save 20, `mark_used` the first, save one more: the second-oldest is gone, the first stays); `mark_used_moves_a_format_to_the_top`; `rename_trims_and_validates` (`"  Moje banka  "` → `"Moje banka"`; empty and 61 characters refused; unknown id NotFound); `deleting_removes_only_that_one`; `a_mapping_naming_a_missing_column_is_refused` (`validation.learnedFormatColumns`); `a_mapping_without_date_or_amount_is_refused`; `a_preset_layout_is_reported_as_shadowed` (headers of the Fio preset: `shadowedByPresetId == Some("fio")`; the UniCredit-like headers: `None`); `unreadable_stored_formats_never_break_the_list` (Review Focus 5: `'not json'`; one entry `{ "id": "learned:old", "name": 5 }` beside a good one); `formats_are_stored_as_the_wire_json_in_app_config` (`"headers"`, `"lastUsedAt"`, `"dateColumn"` present, `"shadowedByPresetId":null`).
- [ ] **Step 2: Run** `cd src-tauri && cargo test learned` — FAIL (module missing).
- [ ] **Step 3: Implement** the October fixture, the types (7-step procedure), the service, the commands, the TS mirror and wrapper, the `validation.learnedFormatHeaderless` / `validation.learnedFormatColumns` strings in all ten `common.json` (cs: „Tento řádek vypadá jako data, ne jako záhlaví sloupců.“, „Formát odkazuje na sloupec, který soubor nemá.“).
- [ ] **Step 4: Gates** (npm + Rust, bindings regenerated) — PASS.
- [ ] **Step 5: Commit** `feat(import): remember a confirmed CSV layout as a learned format`.

## Task 2: The import sheet learns and reuses formats

**Files:**
- Create/extend: `src/mobile/lib/learned-format.ts` + `src/mobile/lib/learned-format.test.ts`
- Modify: `src/mobile/hooks/use-import-flow.ts`, `src/mobile/hooks/use-recognition.ts` (`begin` takes an optional seed mapping), `src/mobile/components/import/PreviewBody.tsx` (the note), `src/mobile/components/import/DoneStep.tsx` (the "remembered" line), `src/i18n/locales/*/mobile.json`

**Interfaces:**
- Consumes: Task 1's `learnedFormatsApi`, `LearnedCsvFormat`, `InsertLearnedCsvFormat`, `LEARNED_FORMATS_KEY`.
- Produces (`learned-format.ts`, pure):
  - `normalizeHeader(cell: string): string` — the Rust cell normalisation (trim, BOM, whitespace runs, lowercase).
  - `learnedMappingFrom(mapping: MappingState): LearnedCsvMapping` — `''` → `null`, `rowFilterEnabled` dropped.
  - `mappingForFile(format: LearnedCsvFormat, inspection: CsvPreviewResult): MappingState | null` — every stored column rewritten to the file's own header cell with the same `normalizeHeader`; `null` when the date, or the amount and not a whole pair, cannot be resolved; an optional column that cannot be resolved becomes `''` (Review Focus 3).
  - `defaultFormatName(account: { name: string; institution?: { name: string } | null } | undefined, fileName: string): string` — institution name, else account name, else the file name without extension; trimmed to 60 characters.
  - `learnedPreselect(input: { format: LearnedCsvFormat | null; requested?: string; accounts: readonly ImportAccount[]; todayDay: number }): string | undefined` — the entry point's account wins; else the format's account when it exists and `isOpenAccount`; else `undefined`.
  - `type CsvSource = 'preset' | 'learned' | 'recognized'`; `learningAfterImport(input: { source: CsvSource; learned: LearnedCsvFormat | null; insert: InsertLearnedCsvFormat | null }): { kind: 'save'; insert: InsertLearnedCsvFormat } | { kind: 'markUsed'; id: string } | { kind: 'none' }` — `recognized` saves (also after "Změnit" from a learned format), `learned` marks used, `preset` nothing.
- Produces (flow): `ImportFlow.learned: LearnedCsvFormat | null` (the format the preview uses), `ImportFlow.changeLearned(): Promise<void>` ("Změnit"), `ImportFlow.remembered: LearnedCsvFormat | null` (set when this import saved a new or replaced format; the done step shows it).

Flow (`read` in `use-import-flow.ts`): after `parseCsvFile` and `csvRecognizable`, when `!presetReadable(parsed)`: `learnedFormatsApi.find(parsed.headers, parsed.delimiter)`; a hit whose `mappingForFile` is not `null` sets the mapping (the recognition is not run), `source = 'learned'`, and the account through `chooseAccount` with `preselected = learnedPreselect(…)` → **preview**; a miss (or a `null` mapping, or a failed `find`) runs the inference exactly as today with `source = 'recognized'`. **"Změnit"** runs `inferCsvColumns` and opens the recognition with `recognition.begin(file, seed)` where `seed` is the learned mapping (overview stage, not the start questions); `source` becomes `recognized`, `learned` stays for the name and account. After `runImport` succeeds, `learningAfterImport` decides; a save uses `defaultFormatName(target account, fileName)` for a new layout and the existing name for a replaced one, `accountId` = the account written to, `headers` / `delimiter` / `encoding` / `headerRow` from `csv`; save and markUsed are best effort (a refusal or error never reaches the person, no toast), then `LEARNED_FORMATS_KEY` is invalidated.

UI: in the preview of a learned CSV, one quiet `ImportNote` first: „Rozpoznáno podle uloženého formátu {{name}}“ with a text button „Změnit“ (≥ 44 pt) that calls `changeLearned`. On the done step, when `remembered` is set: one quiet line „Formát jsem si zapamatoval: příště tento soubor přečtu bez otázek.“ (no amounts in either). Strings `import.learned.note`, `import.learned.change`, `import.learned.remembered` in all ten locales.

- [ ] **Step 1: Write the failing tests** (`learned-format.test.ts`): `normalizeHeader` equals the Rust normalisation on the Task 1 cases; `learnedMappingFrom` (empties → null, description order kept); `mappingForFile` resolves `"ČÁSTKA "` to the file's `"Částka"`, returns `null` without a date, `null` with only the debit side of a pair, drops an unresolvable balance; `defaultFormatName` (institution > account > file stem; 61-character institution cut to 60); `learnedPreselect` (requested wins; deleted or closed account → `undefined`; open account → its id); `learningAfterImport` for the three sources (recognized with `insert` null → none).
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/learned-format.test.ts` — FAIL.
- [ ] **Step 3: Implement** the lib, the flow wiring, `begin`'s seed, the note, the done line, the strings.
- [ ] **Step 4: Gates** (npm) — PASS.
- [ ] **Step 5: Commit** `feat(mobile): import a known CSV layout straight to the preview`.

## Task 3: Nastavení → Import page — learned formats and export guides

**Files:**
- Create: `src/mobile/pages/settings/ImportSection.tsx`, `src/mobile/sheets/LearnedFormatSheet.tsx`, `src/mobile/sheets/ExportGuidesSheet.tsx`, `src/mobile/hooks/use-learned-formats.ts`
- Modify: `src/mobile/shell/routes.ts` (`'import'` in `SETTINGS_SECTIONS`, after `'backup'`), `src/mobile/pages/settings/SettingsSection.tsx`, `src/mobile/pages/Settings.tsx` (the Import row pushes the page), `src/mobile/lib/learned-format.ts` (+ test), `src/i18n/locales/*/mobile.json`, design-system files only if a new component class appears

**Interfaces:**
- Consumes: Task 1's `learnedFormatsApi`, `LEARNED_FORMATS_KEY`; Phase 2b's `BankGuides`, `guidesList`; `useImportSheet().openImport({ source: 'settings' })`; `useBankAccounts()`.
- Produces: `useLearnedFormats(): { formats: LearnedCsvFormat[]; isPending; rename: UseMutationResult<…, { id: string; name: string }>; remove: UseMutationResult<…, string> }` (mutations invalidate `LEARNED_FORMATS_KEY`); `learnedFormatSub(format, accounts): { account: string | null; created: number; lastUsed: number }` (pure; the account's name with its institution, `null` when the account is gone).

Page (`SectionPage`, title `settings.rows.import`, lead `settings.import.lead` „Výpisy z banky: formáty, které jsem se naučil, a návody k exportu.“):
1. Primary 48 pt „Importovat výpis“ → `openImport({ source: 'settings' })`.
2. Section „Naučené formáty“: `EntityRows` of formats (title = name; sub = „{{account}} · naučeno {{created}} · naposledy {{lastUsed}}“ with dates through `useFormat().date`, account part left out when the account is gone; a shadowed one adds „banka má vestavěný formát“). Empty: one caption „Když potvrdíte, jak čtu neznámý soubor, zapamatuji si jeho formát a příště ho přečtu bez otázek.“
3. Section „Návody k exportu“: one row „Jak získat výpis z banky“ → `ExportGuidesSheet` (the `BankGuides` list, full sheet).

`LearnedFormatSheet` (tap on a format): a name field (48 pt, max 60, „Uložit“ disabled while unchanged or empty), facts as `KeyValueRow`s (Účet, Naučeno, Naposledy použito, Sloupce: „Datum: X · Částka: Y“ or „Výdaje: A · Příjmy: B“), and „Smazat formát“ (danger) → `ConfirmSheet` „Smazat formát {{name}}? Příští soubor v tomto formátu znovu projde rozpoznáním.“ Toasts on rename/delete reuse `common:status.*` where they fit.

- [ ] **Step 1: Write the failing test**: `learnedFormatSub` (account with institution „Běžný účet · UniCredit“; gone account → `null`; dates passed through).
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** the hook, the page, the two sheets, the route, the strings.
- [ ] **Step 4: Gates** (npm) — PASS.
- [ ] **Step 5: Commit** `feat(mobile): Nastavení → Import with learned formats and export guides`.

## Task 4: Zabezpečení → Změnit heslo (implementer `opus`, reviewer `opus`)

**Files:**
- Create: `src/mobile/sheets/ChangePasswordSheet.tsx`, `src/mobile/hooks/use-change-password.ts`, `src/mobile/lib/change-password.ts` + `.test.ts`
- Modify: `src/mobile/pages/settings/SecuritySection.tsx` (a row „Změnit heslo“ under the card, sub „Dostanete nový obnovovací klíč“), `src-tauri/src/services/auth.rs` (one test), `src/i18n/locales/*/mobile.json`

**Interfaces:**
- Consumes: `authApi.prepareChangePassword`, `authApi.confirmChangePassword`, `changePasswordSchema`, `MIN_PASSWORD_LENGTH`, `RecoveryKeyBox` (`touch`), `pickVerificationGroups`, `recoveryKeyGroups`, `verifyGroups`, `recoveryKeyFileContents`, `useExportTextFile`, `RECOVERY_KEY_FILE`, `exportsOnThisDevice`, `PasswordInput` (`touch`), `biometricApi.status` / `BIOMETRIC_STATUS_KEY`, `biometricCopy`.
- Produces (`change-password.ts`, pure): `type ChangePasswordStep = 'form' | 'key' | 'done'`; `passwordStrength(password: string): 'weak' | 'fair' | 'strong' | null` (the first run's hint, moved here only if it is not exported; otherwise imported); `canConfirmChange(input: { verified: boolean; confirming: boolean }): boolean`.
- Produces (hook): `useChangePassword(): { step; recoveryKey: string | null; prepare(values: { currentPassword; newPassword; confirmPassword }): void; confirm(): void; reset(): void; pending: { prepare: boolean; confirm: boolean } }` — the passwords stay in the hook's state between the phases and are cleared on `reset`, on success and on unmount; never logged.

Sheet (full `Sheet`, closes with `useSheetHistory`): **form** — current password, new password with the strength sentence, confirm (all `PasswordInput touch`, `autoComplete` `current-password` / `new-password`), primary „Pokračovat“ → `prepare`; a wrong current password shows the translated `auth.wrongPassword` inline, nothing else happens. **key** — title `settings:password.recoveryKeyTitle`, the `loss` box `settings:password.oldKeyInvalid`, `RecoveryKeyBox touch`, „Uložit do Souborů“ where `exportsOnThisDevice`, the two-group retype exactly as the first run's `RecoveryStep`, the sentence `settings:password.confirmHint` (phone wording in `mobile` if the desktop one says "click"), and when biometric unlock is on one quiet line „Odemykání {{method}} zůstane zapnuté.“; primary „Změnit heslo“ enabled only when the groups match → `confirm` → toast `settings:toast.passwordChanged`, sheet closes. Closing the sheet or a lock before „Změnit heslo“ changes nothing (ruling 15).

Biometrics: nothing is written to the keychain and the marker is untouched (ruling 14).

- [ ] **Step 1: Write the failing tests**: Rust `change_password_keeps_the_master_key_biometric_unlock_holds` in `services/auth.rs` tests (set up a profile, read the master key with the old password, `prepare` + `confirm`, read it with the new password: equal; the old password → `auth.wrongPassword`; the old recovery key → `auth.invalidRecoveryKey`; the new one → the same master key). TS `change-password.test.ts`: `canConfirmChange` (unverified, confirming); `passwordStrength` boundaries if it lives here.
- [ ] **Step 2: Run** `cd src-tauri && cargo test change_password_keeps` and the vitest file — FAIL.
- [ ] **Step 3: Implement** the lib, the hook, the sheet, the Security row, the strings.
- [ ] **Step 4: Gates** (npm + Rust) — PASS.
- [ ] **Step 5: Commit** `feat(mobile): change the password with a new recovery key`.

## Task 5: Hlavní měna on the phone

**Files:**
- Modify: `src/mobile/pages/Settings.tsx` (a row „Hlavní měna“ after Jazyk, sub `CODE · name`, opens the picker), `src/mobile/pages/settings/ProfileSection.tsx` (the read-only currency row goes), `src/mobile/sheets/CurrencyPickerSheet.tsx` (optional `lead?: ReactNode` above the search), `src/i18n/locales/*/mobile.json` (`settings.profile.currencyReadOnly` removed from all ten)

**Interfaces:**
- Consumes: `useSettingsMutations().updateProfile`, `useCurrency().currencyCode`, `FX_SUPPORTED_CODES`, `isFxSupported`, `currencyName`, `useFormat()`; desktop strings `settings:currency.*`.
- Produces: `CurrencyPickerSheet` gains `lead?: ReactNode` (first-run callers unchanged).

The sheet's lead mirrors the desktop hint: `settings:currency.mainHint` + name (code) + `settings:currency.example` with the example amount through `useMoneyText()` (hide amounts), `settings:currency.nativeHint`, and `settings:currency.unsupportedMain` in `loss` when the current code has no rate source. Picking a code calls `updateProfile.mutate({ currency })` (its toast as on the desktop) and closes. Every phone screen in the main currency follows (query keys carry `currencyCode`); the implementer lists in the report any `src/mobile` query that reads the main currency without keying on it and invalidates it after the change.

- [ ] **Step 1:** No pure logic to test; the implementer checks `npm test` still passes the parity test after the string removal.
- [ ] **Step 2: Implement**.
- [ ] **Step 3: Gates** (npm) — PASS.
- [ ] **Step 4: Commit** `feat(mobile): change the main currency in Nastavení`.

## Task 6: Simulator evidence (controller)

- `npm install`; build in the background: `npm run tauri -- ios dev "iPhone 17 Pro Max" --config '{"build":{"devUrl":"http://localhost:1450","beforeDevCommand":"npm run dev -- --port 1450 --strictPort"}}'`. Demo profile into the app container (`xcrun simctl get_app_container CD3961FF-A79E-4DFA-9A44-B591B7E694E6 com.moonyfinance.mobile data` → `Library/Application Support/com.moonyfinance.mobile`), app stopped while copying.
- Learned format: September file (`unknown-unicredit-like.csv`) shared in → „Takhle tomu rozumím“ → „Vypadá to správně“ → account → import → done line „zapamatoval“; October file → straight to the preview with the note → import (≤ 4 taps). Nastavení → Import: the list, rename, the format sheet, the guides sheet; „Změnit“ on a third read; delete.
- Password: Face ID enrolled (Features → Face ID → Enrolled), biometric unlock on, change `demo1234` → a new password on the copy, lock, Face ID unlocks (`xcrun simctl spawn CD3961FF-A79E-4DFA-9A44-B591B7E694E6 notifyutil -p com.apple.BiometricKit_Sim.pearl.match`), lock, the old password refused, the new one unlocks. Afterwards the demo copy is restored.
- Main currency: CZK → EUR, Přehled in EUR, back to CZK.
- Screenshots `docs/screenshots/mobile/learned-NN-*.png`; afterwards `pkill -f "vite --port 1450"` and `pkill -f "ios dev iPhone 17 Pro Max --config"`.

## Task 7: Final review, fix round, hand-over (controller)

Whole-branch review against `feat/mobile` on `opus` (spec §8.6/§8.9/§11, this plan's rulings, Review Focus, README §13, the ten rules), one fix round, scoped re-review; the „As built / rulings“ section completed; Czech hand-over `.superpowers/sdd/2026-10-25-mobile-learned-formats/handover.md` (also copied to the shared notes folder) with the rung-5 note.

---

## Rung 5 — design note (not implemented; outbound, needs the owner)

**When it is offered.** Once per learned format: on the done step of the first successful import that used it from the store (the second month — the layout has proven itself), a quiet secondary row „Poslat formát autorům Moony“. Never on the first import (the person just taught it), never again after it was sent or dismissed (a `contributedAt` / `contributeDismissedAt` on the format).

**What would leave the phone, shown in full before anything is sent:** the format's header row (the header cells only, as read), the delimiter, the encoding, the date format, the decimal separator, which header is which field (date, amount or debit/credit, balance, description columns, counterparty, account, currency, id), the bank's name as the person's institution list has it, the app version and the profile country. Never: any transaction row, balance, amount, account number, the file's name, the account's name, the person's name or e-mail, the format's own name if the person edited it.

**Consent wording (cs):** title „Pomozte ostatním se stejnou bankou“; body „Moony z vašeho souboru odešle jen tohle: názvy sloupců a to, co v nich je. Žádné platby, zůstatky ani čísla účtů. Text si přečtete celý dřív, než ho odešlete, a odesíláte ho vy — ze svého účtu na GitHubu.“; buttons „Zobrazit a poslat“ / „Teď ne“ / „Už se neptat“.

**Where it would go:** a prefilled new GitHub Discussion in the project's „Formáty výpisů“ category, opened in the system browser through the opener (title „Formát: <banka>“, body = the text above as a fenced JSON block plus one sentence in the app language). The person posts it themselves; nothing is sent by the app, no endpoint of ours exists. Fallbacks: „Zkopírovat text“ (clipboard) for a person without a GitHub account, and the share sheet (Mail) with the same text. URL length: the body fits the GitHub prefill limit for ≤ 40 columns; beyond that, only copy/share are offered.

**Owner decisions needed:** whether to offer it at all; GitHub Discussions vs. an e-mail address; whether the profile country and app version go along; the PRIVACY.md sentence; whether the desktop offers the same.

---

## As built / rulings

Rulings taken while planning (each with what it costs if wrong):

1. **Store in `app_config`, no migration**: `learned_csv_formats` holds the JSON array, at most 20 — the stock import's saved-formats pattern the spec names ("stored like the stock import's saved formats (`app_config`, max 20)"). It is in the database, so it travels with backups, restores and the desktop transfer (§11). Costs a migration into a table if the sync service wants rows.
2. **The signature is the header cells, the delimiter and the column count** (`v1`, normalised cells). The encoding and the header row's position are stored but neither keys nor forces the import: each month's file is decoded and its header found by the file's own detection. A Windows-1250 bank's month without diacritics in its rows reads as UTF-8, and a bank may add a preamble line; a key with either would ask again. The decoded header text already carries the encoding. This narrows the brief's "normalised header row, delimiter, encoding, column count". Costs: two exports with identical headers but another date format share one format (the preview shows row errors and „Změnit“).
3. **A header row that looks like data is never learned** (a date, an amount, ≥ 6 digits in a cell): nothing with row values is stored. Costs: a bank whose column names carry such numbers is recognised every month.
4. **Learning happens when the import succeeds**, not on „Vypadá to správně“: what is stored is a mapping whose preview the person accepted. Costs: confirming and then abandoning the preview teaches nothing.
5. **Ladder order: a readable preset, then a learned format, then the inference.** A learned format is consulted only when no preset can read the file, so a preset always wins; the list reports a format whose layout a shipped preset now matches (`shadowedByPresetId`, Phase 1 review focus 2). Costs nothing.
6. **„Změnit“ opens „Takhle tomu rozumím“ seeded with the learned mapping** (the inference still runs for the options and the sign check); the next successful import replaces the format (same id, same creation date). Costs nothing.
7. **The format remembers its account** and preselects it on reuse when it is open and the entry point named none; the default name is the account's institution, else the account name, else the file name. Costs a tap when one layout serves two accounts.
8. **Eviction drops the least recently used format**, not the oldest. Costs nothing.
9. **„Naposledy použito“ is the last successful import with the format** (the learning import included).
10. **The desktop CSV dialog does not use learned formats**; the commands are shared (not `#[cfg(mobile)]`), so the desktop could read and write the same store. **Owner decision**: whether the desktop import should consult and teach them too.
11. **Nastavení → Import is a pushed page** (`/settings/import`) with „Importovat výpis“, Naučené formáty and the export guides in a sheet. The catalog-refresh switch and the CoinGecko key named in §8.9 are not in this branch (the refresh is an open owner decision from Phase 2b; the key was not in the brief). Costs a later row each.
12. **Rung 5 is a design note only** (above).
13. **The password change mirrors the desktop's commands and adds the phone's own key step**: the first run's recovery key box with „Uložit do Souborů“ and the two-group retype, plus the desktop's warning that the old key stops working. Costs two fields more than the desktop dialog.
14. **Face ID needs no re-enrolment**: `confirm_change_password` re-wraps the same master key (`services/auth.rs`), and biometric unlock keeps a copy of the master key (`services/biometric_unlock.rs`), so the keychain item stays valid; the change writes nothing to the keychain or the marker. Pinned by a Rust test and the simulator. Costs a re-enrolment step if the key derivation ever changes.
15. **A lock in the middle of the change changes nothing**: the two-phase commands persist only on „Změnit heslo“; the sheet closes with the lock and the next attempt shows a new key. The background lock is not paused. Costs a restart for a person who left longer than their lock delay.
16. **The main currency is changeable on the phone** because the desktop offers it (`LanguageCard`); the row sits next to Jazyk (§8.9 „Jazyk a měna“) and Profil's read-only row goes. Costs one row move.

Rulings taken while executing (in order; each with what it costs if wrong):

17. **`passwordStrength` moved to `src/mobile/lib/password-strength.ts`** (+ test), imported by the first run's `AccountStep` and the change-password sheet. Costs nothing.
18. **The SDD workspace is kept** (the hand-over lives in it). Costs one git-ignored folder.
19. **No keep-awake**: the auto-mode classifier refused it at the start. Costs a possible sleep during long waits.
20. **The simulator profile is seeded with `seed_demo`** (Alex Novák, `demo1234`): the brief's `feat+mobile/demo-data/mobile-demo-profile` was empty, and the parent asked for a re-seed after migration 023 → 024 anyway. Costs nothing unless the parent's copy holds extra test data.
21. **`feat/mobile` merged twice** (3fe2ff7f, then ab92825b at the parent's ask); the only conflict was README §13's Import-sheet row, resolved keeping both edits. This branch adds no migration, so the parent's "025" note does not apply. Costs nothing.
22. **Flow decisions given to the Task 2 implementer**: a learned format whose mapping does not fit the file still names the save (`learned?.name ?? defaultFormatName(…)`, so a person's own name is never overwritten); "Změnit" opens the recognition on its overview with the day/month order counted as confirmed; the done step's "remembered" line shows only when the save resolved. Costs nothing.
23. **The learned-format list row says what fits one line**: the account's own name and „naposledy {{date}}“, a shadowed format leading with „banka má vestavěný formát“; the creation date, the institution and the full shadow sentence live in the format sheet. The planned one-line composition could not be seen at 360 pt. Costs: the list no longer shows when a format was learned.
24. **The key step says old backups keep the old password** („Zálohy pořízené před změnou se dál otevírají starým heslem.“): backups carry `key.enc` and `recovery.enc` (`services/backup.rs`). Costs one sentence.
25. **The change flow saves its key as `moony-recovery-key-YYYY-MM-DD.txt`** with a dated title and a note that it is valid only once the change was confirmed; the Rust staged-export allow-list (`is_text_export_file_name`) grew by exactly that shape. Every attempt makes a new key and abandoned attempts are normal (ruling 15), so the first run's undated name could let iOS "Replace" overwrite the only valid key file. Costs: two attempts on the same day share a name (an `-HHmm` suffix is an owner decision).
26. **The header data check was narrowed after the final review** (ruling 3 re-sized): a cell counts as an amount only when, beside the number, at most one run of ≤ 3 letters remains (an ISO code or "Kč"); dates and runs of ≥ 6 digits still count. The plan's rule refused layouts with column names such as "Popis 1", "Address Line 1" or "E2E Reference" for ever; a transaction row still trips the date check. Costs: a header made only of a short code and a number ("EUR 2") still counts as data.
27. **The delimiter of a learned format is comma, semicolon or tab only** (final review), matching `validation.csvDelimiterInvalid` and what the detector returns. Costs nothing.
