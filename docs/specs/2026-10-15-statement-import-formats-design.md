# Statement import formats (CAMT.053, MT940, OFX/QFX, QIF) — design

Date: 2026-10-15. Status: **draft for owner review**. The owner chose these in chat on
2026-10-15: variant 1 (own parsers feeding the shared import pipeline), QIF categories
mapped by name (D13), account mismatch warns and offers to switch (D9), one spec delivered in
four phases (D15), ABO/GPC later (D1). Every other decision below is the author's proposal,
pending the owner's review of this document. The plan will be
`docs/plans/2026-10-15-statement-import-formats.md`. This spec is also the hand-off brief: an
implementer should not need the chat.

## 1. Problem

Bank import accepts CSV only. The file dialog filters to `.csv` (`FileSelectStep.tsx`),
`SUPPORTED_EXTENSIONS` is `['csv', 'txt']` (`csv-import/import-config.ts`) and
`src-tauri/src/services/csv_import/` only reads delimited text. Many banks also export, or only
export, standard formats:

- **CAMT.053** (ISO 20022 XML account statement): the eurozone standard, offered by Czech, German,
  Dutch and Nordic banks.
- **MT940** (SWIFT text statement): older but still exported widely, especially in DE and CZ.
- **OFX/QFX**: US banks and credit unions, and the export of most personal finance apps.
- **QIF**: the legacy exchange format of Quicken, GnuCash, Money Manager Ex and others.

The 2026-10-15 competitive review (`../Moony-launch/audit/competitive-landscape-2026-10-15.md`)
ranks "CAMT.053 + MT940" first and "OFX/QIF" second among missing features. Every verified
competitor has at least two of the four.

## 2. Goals and non-goals

Goals:

- Import CAMT.053, MT940, OFX/QFX and QIF bank statements into a Moony bank account. The
  guarantees match the CSV import: preview, duplicate detection, rules-first categorization, a
  batch that can be undone, and the statement balance offered as a balance record.
- Use what structured formats carry and CSV does not: the account identity, declared opening and
  closing balances, booked vs pending, and statements of several accounts in one file.
- Let users move from another app: QIF categories map to Moony categories in the preview.

Non-goals:

- ABO/GPC (the Czech fixed-width format). Planned later; the module layout leaves room for it.
- camt.052 and camt.054, MT942, MT950. They are detected and refused with a specific message.
- Investment statements: OFX `INVSTMTRS` and QIF `!Type:Invst`. Refused or skipped with a
  message; the broker CSV import covers investments.
- Creating accounts from a statement, or filling an empty IBAN on the target account.
- A balance record from the declared opening balance (only the closing balance is offered).
- MCP. MCP agents already import parsed statements through `bank_transactions_create`. No new tool.
- Applying OFX corrections (`CORRECTACTION`). Such entries are skipped with a message.

## 3. Decisions

| # | Decision |
|---|---|
| D1 | Formats: CAMT.053 (`camt.053.001.02` to `.13`), MT940, OFX 1.x (SGML) and 2.x (XML) including QFX, QIF. Not ABO/GPC (owner: later), not camt.052/054, MT942, investment content. |
| D2 | Each format has a parser that produces a common statement model. `to_rows` turns that into the CSV import's `ParsedRow`s. Duplicate classification, categorization, the batch record and the write path (`create_transaction_with_origin`, ADR 0007) are shared with CSV and not duplicated. |
| D3 | Own parsers. The only new dependency is `quick-xml` (CAMT, OFX 2.x). Existing OFX/MT940 crates are thinly maintained and strict, and bank exports deviate from the standards. |
| D4 | The format is detected from content, never from the extension. A file that is none of the four goes to the CSV path unchanged. |
| D5 | For structured formats, `line` (preview rows, `importAnywayLines`, `categoryOverrides`, `import_line`) is the 1-based ordinal of the entry in the selected statement, counted after batch entries are split. Minified XML has one line, so file lines would collide. |
| D6 | Each format names one field as the bank-side ID (section 6). An ID that occurs more than once in one import is dropped from all entries that carry it, and those entries are matched by the composite key only. |
| D7 | Dates are the calendar day as written in the file, never shifted by a time zone (ADR 0008). |
| D8 | Statements of the same account and currency in one file are merged into one importable statement, with a check that the balance chain is continuous. Statements of different accounts or currencies appear in a picker. |
| D9 | Account identity: when the file's account belongs to another Moony account, the preview warns and offers to import there instead. When it contradicts the selected account and matches no other account, import needs an explicit confirmation. (Owner choice.) |
| D10 | A statement currency that differs from the account currency is a warning, not a block. Rows keep their native currency (ADR 0012), and `NextSteps` already refuses to offer a balance in another currency. |
| D11 | The declared closing balance becomes the import's `statementBalance`, so the existing `NextSteps` flow offers it. When both balances are declared, the preview checks opening + Σ entries = closing in exact decimal arithmetic and shows the result. |
| D12 | Overlap hint for **all** formats, CSV included: new rows dated inside the range of an earlier batch of the same account are counted and the preview names that batch. This is the only mitigation for cross-format duplicates (section 8.3). |
| D13 | QIF categories (`L`) map to Moony categories by name in an editable panel of the preview. Mapped rows are written with the preview override source (`manual`), unmapped rows go through the rules. (Owner choice.) |
| D14 | No migration. `data_source` stays `csv_import`, whose meaning widens to "imported from a file"; the batch's `file_name` shows what was imported. |
| D15 | One spec, four phases, each releasable on its own: (1) shared core + CAMT.053, (2) MT940, (3) OFX/QFX, (4) QIF. (Owner choice.) |
| D16 | Statement files above 50 MB are refused. XML with a `<!DOCTYPE` is refused. `quick-xml` does not expand entities anyway. |

## 4. Architecture

```
file bytes ──► statement_import::detect ──► None ─────────────► CSV path (unchanged)
                       │ Some(format)
                       ▼
             decode (per-format encoding rules)
                       ▼
     camt.rs / mt940.rs / ofx.rs / qif.rs  ──►  Vec<Statement>
                       ▼
             merge (D8) + account match (D9) + balance check (D11)
                       ▼
             to_rows(statement, options)  ──►  ParsedImport { rows: Vec<ParsedRow>, … }
                       ▼
  csv_import core (shared): preview_rows / import_rows
     dedup::BankOccurrenceChecker → category (override → source map → rules)
     → insert_batch → create_transaction_with_origin → finish_batch
```

### 4.1 Backend module

New `src-tauri/src/services/statement_import/`:

| File | Responsibility |
|---|---|
| `mod.rs` | `inspect`, `preview`, `import`: orchestration, taking `&Connection` (rule 4) |
| `detect.rs` | `detect(bytes) -> Option<StatementFormat>` and the refusals for unsupported variants |
| `model.rs` | `Statement`, `Entry`, `DeclaredBalance`, `AccountRef` (section 5) |
| `decimal.rs` | Canonical decimal text ↔ scaled `i128` for the balance check; no `f64` |
| `camt.rs` | CAMT.053 → `Vec<Statement>` (streaming `quick-xml` reader) |
| `mt940.rs` | MT940 → `Vec<Statement>`, including the `:86:` dialects |
| `ofx.rs` | OFX 1.x/2.x → `Vec<Statement>` (one tolerant tag scanner for both) |
| `qif.rs` | QIF → `Vec<Statement>` |
| `merge.rs` | D8 merging and chain check, D11 balance check |
| `account_match.rs` | D9: the Moony account a statement belongs to |
| `to_rows.rs` | `Entry` → `ParsedRow` with an `InsertBankTransaction` candidate |
| `types.rs` | Wire types (section 9) |
| `tests/` | Parser and service tests (section 11) |

Parsers are pure functions: `fn parse(text: &str) -> Result<Vec<Statement>>`. They do no DB work
and no I/O. Errors are `AppError::Validation` with an i18n key (section 10). No `.unwrap()`
outside tests (rule 5).

### 4.2 Refactor of the shared core (`csv_import/mod.rs`)

Today `preview_import` and `import_transactions` take the file text and a `CsvImportConfig` and
call `parse_rows` themselves. Split them:

- `ParsedCsv` becomes `ParsedImport { rows, total_rows, period: Option<CsvDateRange>,
  declared_balance: Option<DeclaredBalance> }`. CSV fills `period` and `declared_balance` with
  `None`, so behaviour stays as it is today: the period and balance come from the rows.
- `ParsedRow` gains `source_category: Option<String>` (QIF only; `None` elsewhere).
- New `ImportOptions { skip_duplicates, import_anyway_lines, category_overrides,
  source_category_map, preview_offset, preview_limit }`. `CsvImportConfig::import_options()`
  derives it, so the CSV wire type does not change.
- New `preview_rows(conn, account_id, &ParsedImport, &ImportOptions, engine)` and
  `import_rows(conn, account_id, file_name, &ParsedImport, &ImportOptions, engine)` contain the
  current bodies. `preview_import` and `import_transactions` become `parse_rows` + these.
- Category resolution order: per-line override, then `source_category_map` for the row's
  `source_category`, then the rules. Both of the first two use source `manual` and both check
  that the category still exists (as `resolve_row_category` does today).
- Batch period: `ParsedImport.period` when set (the statement's declared period), otherwise
  the range of the rows, as today.
- `statementBalance`: `declared_balance` when set (`line` = ordinal of the statement's last
  entry, 0 for an empty statement). Otherwise the current row-derived logic. `lastBalance`
  follows the same rule.
- The overlap hint (D12, section 8.3) is computed in `preview_rows`, so CSV gets it too.

The CSV tests (`csv_import/tests.rs`) must pass unchanged after the refactor. That is the gate
for phase 1's first task.

## 5. Statement model

```rust
pub enum StatementFormat { Camt053, Mt940, Ofx, Qif }

pub struct AccountRef {
    pub identifier: Option<String>, // IBAN or domestic number exactly as in the file
    pub bank_code: Option<String>,  // OFX BANKID, MT940 "BLZ/" prefix, CAMT Othr scheme
    pub name: Option<String>,       // QIF !Account N
}

pub struct DeclaredBalance { pub amount: String, pub currency: String, pub as_of: i64 }

pub struct Statement {
    pub account: AccountRef,
    pub currency: Option<String>,          // ISO 4217
    pub period: Option<(i64, i64)>,        // declared, inclusive days
    pub opening: Option<DeclaredBalance>,
    pub closing: Option<DeclaredBalance>,
    pub sequence: Option<String>,          // MT940 :28C:, CAMT ElctrncSeqNb: merge order
    pub entries: Vec<Entry>,
    pub skipped: Vec<SkippedEntry>,        // counted and shown, never imported
}

pub struct Entry {
    pub booking_date: i64,                 // UTC midnight of the written day (D7)
    pub value_date: Option<i64>,
    pub amount: String,                    // canonical decimal, positive
    pub is_credit: bool,
    pub currency: Option<String>,
    pub bank_id: Option<String>,           // D6, before the uniqueness guard
    pub description: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_account: Option<String>,
    pub counterparty_bank_code: Option<String>,
    pub counterparty_country: Option<String>,
    pub pending: bool,
    pub source_category: Option<String>,   // QIF only
}
```

`to_rows` turns each `Entry` into `RowState::Candidate(InsertBankTransaction)` with
`tx_type = credit|debit`, `status = pending|booked`, `flow_kind = None`, and validates it with
`InsertBankTransaction::validate()`. An invalid entry becomes `RowState::Error` with the
validation key. A zero amount becomes `RowState::SkippedZero`. `SkippedEntry` becomes
`RowState::SkippedFiltered` with its message key. Multi-line descriptions are joined with
` | `, the separator CSV uses for several description columns.

## 6. Format mappings

### 6.1 CAMT.053

Elements are matched by local name, so every namespace version `camt.053.001.02`–`.13` parses
the same way. One `Stmt` is one `Statement`.

| Model | Source |
|---|---|
| account | `Stmt/Acct/Id/IBAN`, else `Acct/Id/Othr/Id` (bank code from `Othr/SchmeNm` or `Acct/Svcr/FinInstnId`) |
| currency | `Stmt/Acct/Ccy`, else the currency of the `CLBD` balance |
| period | `Stmt/FrToDt` (date part of `FrDtTm`/`ToDtTm`) |
| opening | `Bal` with `Tp/CdOrPrtry/Cd` = `OPBD`, else `PRCD` |
| closing | `Bal` with `CLBD` |
| balance sign | `Bal/CdtDbtInd` = `DBIT` → negative |
| sequence | `Stmt/ElctrncSeqNb`, else `LglSeqNb` |
| entry amount | `Ntry/Amt` (text) + `@Ccy`; direction `Ntry/CdtDbtInd`. `RvslInd` needs no handling because the indicator already gives the direction. |
| status | `Ntry/Sts` as text (older versions) or `Ntry/Sts/Cd` (newer): `BOOK` → booked, `PDNG` → pending, anything else (`INFO`, `FUTR`) → skipped with `statementImport.notBooked` |
| booking date | `BookgDt/Dt`, else the date part of `BookgDt/DtTm`, else `ValDt` |
| value date | `ValDt/Dt` or the date part of `ValDt/DtTm` |
| bank ID | `TxDtls/Refs/AcctSvcrRef`, else `Ntry/AcctSvcrRef`, else `Ntry/NtryRef` |
| counterparty | Credit: `RltdPties/Dbtr` (`Nm` or `Pty/Nm`) and `DbtrAcct/Id/IBAN` or `Othr/Id`. Debit: `Cdtr`/`CdtrAcct`. Bank code: `RltdAgts/DbtrAgt` or `CdtrAgt` `FinInstnId/BICFI`/`BIC`, else `ClrSysMmbId/MmbId`. Country: from the IBAN, else `PstlAdr/Ctry`. |
| description | `RmtInf/Ustrd` (all occurrences), then `RmtInf/Strd/CdtrRefInf/Ref`, then `AddtlTxInf`. If all are empty, `Ntry/AddtlNtryInf`. |

**Batch entries.** A `Ntry` with several `TxDtls` that each have their own `AmtDtls/TxAmt/Amt`
(or `Amt`) becomes one entry per `TxDtls`. If a `TxDtls` has no ref of its own, its bank ID is
`<entry ref>/<n>` (n from 1). If the `TxDtls` amounts do not add up to the `Ntry` amount, the
`Ntry` stays one entry, with the first `TxDtls` as its details.

**Amounts in another currency** (`AmtDtls/InstdAmt`, `CcyXchg`) are ignored. The entry amount
in the account currency is the native value.

### 6.2 MT940

The parser accepts a bare tag stream, one or more SWIFT envelopes (`{1:…}{2:…}{4:` … `-}`), and
messages separated by a `-` line. Lines that start with a tag begin a new field. Any other line
continues the previous field (`:86:` spans up to six lines).

| Tag | Model |
|---|---|
| `:20:` | starts a statement |
| `:25:` | account: an IBAN, or `bankcode/account` (the part before `/` becomes `bank_code`) |
| `:28C:` | sequence |
| `:60F:` / `:60M:` | opening (`D`/`C`, `YYMMDD`, currency, amount with decimal comma) |
| `:62F:` / `:62M:` | closing. `:60M:`/`:62M:` mark the pages of one statement split across messages. Each page is parsed as its own statement and D8 chains them. |
| `:61:` | entry: value date `YYMMDD`, optional entry date `MMDD`, mark `C`/`D`/`RC`/`RD`, optional funds code letter, amount, `N`+3-char type, customer reference, optional `//` + bank reference, optional next line of supplementary details |
| `:86:` | details of the preceding `:61:` |
| `:64:`, `:65:`, `:NS:` and unknown tags | ignored |

- Direction: `C` and `RD` are credits, `D` and `RC` are debits.
- Years: `YY` < 80 → 20YY, otherwise 19YY. The booking date is the entry date when present,
  with the year taken from the value date. Entry month 12 with value month 1 → previous year.
  Entry month 1 with value month 12 → next year. Without an entry date the booking date is the
  value date.
- Bank ID: the reference after `//`, when non-empty and not `NONREF` (case-insensitive). The
  customer reference is never an ID.
- MT942 (`:34F:` present) and MT950 are refused with `statementImport.unsupportedVariant`.

`:86:` is read in one of three dialects. The dialect is chosen per field from its shape:

1. **Subfield dialect** (DE/AT and some CZ banks): a 3-digit code, then subfields introduced by
   a separator character (usually `?`, sometimes `>`; the character after the code decides).
   `?20`–`?29` and `?60`–`?63` → description (joined without separators, with the line breaks
   removed), `?32`+`?33` → counterparty name, `?31` → counterparty account, `?30` →
   counterparty bank code, `?00` → prepended to the description.
2. **Slash dialect** (NL/BE and others): `/TAG/value` pairs. `/NAME/` → counterparty name,
   `/IBAN/` or `/ACCW/` → counterparty account, `/BIC/` → counterparty bank code,
   `/REMI/` (with `USTD//` or `STRD/CUR/` stripped) → description. `/CNTP/` (`account/BIC/name/city`)
   fills all three when present. `/EREF/` and `/TRTP/` are ignored.
3. **Free text**: the whole field is the description, with the line breaks turned into spaces.
   A counterparty IBAN is taken from it only when exactly one IBAN-shaped token passes the
   checksum (`financial_semantics::valid_iban`).

Supplementary details from `:61:` go first in the description when `:86:` gives nothing.

### 6.3 OFX / QFX

The same tolerant tag scanner reads OFX 1.x SGML and OFX 2.x XML. The scanner treats a leaf
element's value as running until the next `<`. Closing tags are optional for leaves and
required for aggregates in 1.x, but neither is required here. The header (`OFXHEADER:100 …` or
`<?OFX …?>`) is read for the encoding, then dropped. QFX is OFX with extra Intuit elements,
which are ignored.

| Model | Source |
|---|---|
| statement | each `STMTRS` (bank) and `CCSTMTRS` (credit card) |
| account | `BANKACCTFROM/ACCTID` (+ `BANKID` → bank code), `CCACCTFROM/ACCTID` |
| currency | `CURDEF` |
| period | date part of `BANKTRANLIST/DTSTART` and `DTEND` |
| closing | `LEDGERBAL/BALAMT` at `DTASOF` (no opening balance in OFX, so the D11 check is `unavailable`) |
| entry | `STMTTRN`: `TRNAMT` signed (positive = credit), `DTPOSTED` (first 8 characters, `YYYYMMDD`), `FITID` → bank ID, `NAME` or `PAYEE/NAME` → counterparty name, `MEMO` → description (else `NAME`), `CURRENCY/CURSYM` → currency if present |
| skipped | `STMTTRN` with `CORRECTACTION` → `statementImport.correctionSkipped` |

An amount with one `,` and no `.` uses a decimal comma. A file with `INVSTMTRS` and no bank or
card statement is refused with `statementImport.investmentOnly`. A mixed file imports its bank
and card statements and warns `statementImport.investmentIgnored`.

### 6.4 QIF

Sections start with `!Type:` or `!Account`. Records end with `^`. Field codes are the first
character of a line.

| Section | Handling |
|---|---|
| `!Account` | Sets the current account (`N` name, `T` type) for the `!Type:` sections that follow. Several accounts → several statements (D8 picker). |
| `!Type:Bank`, `Cash`, `CCard`, `Oth A`, `Oth L` | Transactions of the current account (or of one unnamed account) |
| `!Type:Invst` | Skipped, warning `statementImport.investmentIgnored` with the record count |
| `!Type:Cat`, `Class`, `Memorized`, `Prices`, `!Option:*`, `!Clear:*` | Ignored |

| Field | Model |
|---|---|
| `D` | Date. The order is detected over all dates in the file with `date_parser::detect_date_format`, after normalizing `'` to `/` and removing spaces. `'YY` → 20YY. A 2-digit `/YY` → 20YY when ≤ current YY + 1, else 19YY. |
| `T` (or `U`) | Amount, signed. The decimal separator is decided once per file with `csv_import::detect_decimal_separator` and parsed with `clean_and_parse_amount`, so thousands separators are handled. |
| `P` | counterparty name |
| `M` | description (else `P`) |
| `L` | source category. `/class` is removed and `Parent:Child` is kept as the full path. `[Account]` (a transfer) has no source category. |
| `S`/`E`/`$` | Splits. The transaction is imported once with its `T` total. The source category is that of the splits when they all share one, else none. |
| `N`, `C`, `A`, `F`, `%` | ignored |

No bank IDs: QIF rows deduplicate by the composite key only.

**Opening balance record.** Quicken-style exports start an account with a record whose `L` is
`[<the account's own name>]`. That record is not imported. It becomes the statement's
`opening` (amount `T`, day `D`). QIF has no closing balance, so the D11 check is `unavailable`
and no `statementBalance` is offered. A closing balance computed by Moony is never offered,
because only a declared value may become a balance record.

**Ambiguous dates.** If the detection cannot decide the day/month order (every day ≤ 12), the
inspection reports `dateOrderAmbiguous` and the preview shows a select (`dmy`/`mdy`/`ymd`). The
default is the order of the profile's locale. `options.dateOrder` carries the choice.

## 7. Shared rules

### 7.1 Detection (D4)

`detect` looks at the first 4 KB of the file after a BOM, ignoring leading whitespace and
letter case:

1. `OFXHEADER:` or `<OFX>` → OFX.
2. An XML document whose root namespace contains `camt.053.` → CAMT.053. `camt.052.` or
   `camt.054.` → refused (`statementImport.unsupportedVariant`).
3. A first non-empty line starting with `!Type:`, `!Account` or `!Option:` → QIF.
4. A line starting with `:20:` and another with `:25:`, plus a `:61:` or `:60F:`/`:60M:`
   line, optionally inside `{4:` → MT940.
5. Otherwise `None`: the CSV path.

### 7.2 Encoding

| Format | Rule |
|---|---|
| CAMT, OFX 2.x | The XML declaration's `encoding` via `encoding_rs`, default UTF-8, BOM wins |
| OFX 1.x | `CHARSET` header (`1252` → windows-1252, `1250` → windows-1250, `ISO-8859-1`, …). `ENCODING:UTF-8` → UTF-8. `NONE` or absent → the CSV rule below. |
| MT940, QIF | `csv_import::decode_csv_content_for_country`: UTF-8 or BOM, else the legacy fallback for the profile country, the same as CSV |

### 7.3 Bank IDs (D6)

The per-format sources are listed in section 6. After parsing, IDs are trimmed. An ID that
occurs more than once in the selected statement is removed from every entry that carries it.

The guard exists because `BankOccurrenceChecker::check` treats a repeated ID within one batch
as a certain duplicate that cannot be forced, which would silently drop a real payment. IDs are
not namespaced by format, so a bank whose CSV ID equals its CAMT `AcctSvcrRef` dedupes across
the two formats for free.

### 7.4 Merging statements (D8)

Statements are grouped by (canonical account identifier or QIF account name, currency). Within a
group they are ordered by `sequence`, else by period start, then concatenated:

- `opening` = the first statement's, `closing` = the last statement's, `period` = their union.
- Chain check: each statement's opening must equal the previous statement's closing. A break is
  reported as `balanceCheck = mismatch` with the difference at the first break.
- MT940 `:60M:`/`:62M:` continuation pages inside one statement are part of the same chain.

Each group is one `StatementSummary` in the inspection, indexed from 0. `options.statementIndex`
picks one. The default is the group that matches the selected account (D9), else index 0.

### 7.5 Account match (D9)

New `financial_semantics::find_account_by_identifier(conn, identifier, bank_code) ->
Option<String>` reuses `canonical_structured_identifier`, `valid_iban` and `account_number_key`.
These are the rules that already decide own-account identity, so a CZ `prefix-account/bank`
converts to an IBAN in the same way. It returns the id of the bank account whose `iban` or
`bban` matches.

| `accountMatch` | When | Preview |
|---|---|---|
| `selected` | the statement identifies the selected account | "Account matches" line |
| `other` | it identifies another Moony account | Warning with the button "Import into {name}", which switches the dialog's target account and re-runs the preview. Staying requires the confirmation checkbox. |
| `none` | it carries an identity, matches no account, and the selected account has an `iban` or `bban` | Warning. Import is disabled until the confirmation checkbox is ticked. |
| `unknown` | no identity in the file (QIF), or the selected account has no identifiers and no other account matches | Only an info line with the file's account identifier |

### 7.6 Balance check (D11)

`decimal.rs` parses canonical decimal text into an `i128` scaled to the largest fraction length
in play. The check is exact equality, with no tolerance and no `f64`.

`balanceCheck`:

- `ok`: opening + Σ signed booked entries = closing. Pending entries are excluded, because
  booked balances exclude them.
- `mismatch`: the equation fails, or the chain breaks; `balanceDifference` carries the
  difference.
- `unavailable`: opening or closing is missing.

A mismatch is a warning ("The statement may be incomplete"), not a block.

### 7.7 Currency (D10)

Entries without their own currency take the statement currency. If the statement currency
differs from the account currency, the summary warns. `NextSteps` already hides the balance
offer when `statementBalance.currency !== account.currency`.

## 8. Preview and import behaviour

### 8.1 Rows

Preview rows use the existing `CsvPreviewRow` shape. `line` is the entry ordinal (D5), the
table's first column header reads `#` for statements, and `sourceCategory` is shown under the
description for QIF rows. Statuses, the "import anyway" checkbox for possible duplicates,
per-row category picks and pending → booked promotion all behave as for CSV.

### 8.2 QIF category mapping (D13)

When the selected statement has source categories, the preview shows a collapsible panel
"Categories from the file". Each row shows the source path, its row count and a `CategoryPick`.
The pick is prefilled by a pure frontend function `matchSourceCategories(sources, categories,
displayName)`:

1. Compare case-, diacritics- and whitespace-insensitively (folded in TS).
2. A full path `Parent:Child` matches a Moony child category whose parent name also matches.
3. Otherwise the leaf segment matches when exactly one Moony category has that name.
4. Names are compared against the stored `name` and against `categoryDisplayName(category)` in
   the current UI language, so the seeded "Groceries" also matches "Potraviny".
5. Otherwise the pick is unset, and "Leave to the rules" is shown.

The panel's state becomes `options.sourceCategoryMap`. A per-row pick in the table wins over the
map.

### 8.3 Overlap hint (D12, all formats)

`preview_rows` counts the `ok` rows whose booking day falls in `[date_from, date_to)` of an
earlier batch of the same account. The batch's last day is excluded, so the rows booked later on
the day of the previous export do not trigger it. If the count is > 0,
`CsvImportPreview.overlap` lists the count and up to three such batches (file name and range).
The preview then shows: "N new payments fall in a period that 'file' already covers. If you
imported the same payments in another format, they may not be recognized as duplicates."

### 8.4 Import

`import_statement_file` re-reads and re-parses the file, which is deterministic, then calls
`import_rows`. The result is the existing `CsvImportResult`. When the user switched the target
account (D9), the result step shows "Open {account}" instead of "Show imported transactions",
because the page behind the dialog belongs to another account.

## 9. Wire contract

New types follow rule 3 (per-field `#[serde(rename)]`, registered in `bindings.rs`,
`generated-types.ts` regenerated) and the 7-step procedure for `shared/schema.ts` (rule 2):

```ts
export type StatementFormat = 'camt053' | 'mt940' | 'ofx' | 'qif';
export type DateOrder = 'dmy' | 'mdy' | 'ymd';
export type BalanceCheck = 'ok' | 'mismatch' | 'unavailable';
export type AccountMatch = 'selected' | 'other' | 'none' | 'unknown';

export interface DeclaredBalance { amount: string; currency: string; asOf: number }
export interface SourceCategoryCount { name: string; count: number }
export interface StatementWarning { key: string; detail: string | null }

export interface StatementSummary {
  index: number;
  accountIdentifier: string | null;
  accountName: string | null;
  currency: string | null;
  period: CsvDateRange | null;
  openingBalance: DeclaredBalance | null;
  closingBalance: DeclaredBalance | null;
  entryCount: number;
  skippedCount: number;
  balanceCheck: BalanceCheck;
  balanceDifference: string | null;
  accountMatch: AccountMatch;
  matchedAccountId: string | null;
  sourceCategories: SourceCategoryCount[];
}

export interface StatementInspection {
  format: StatementFormat;
  encoding: string;
  statements: StatementSummary[];
  defaultStatementIndex: number;
  dateOrderAmbiguous: boolean;
  detectedDateOrder: DateOrder | null;
  warnings: StatementWarning[];
}

export interface SourceCategoryMapping { source: string; categoryId: string }

export interface StatementImportOptionsInput {
  statementIndex: number;
  dateOrder?: DateOrder | null;
  skipDuplicates?: boolean;            // default true
  importAnywayLines?: number[];
  categoryOverrides?: CsvCategoryOverride[];
  sourceCategoryMap?: SourceCategoryMapping[];
  previewOffset?: number;
  previewLimit?: number;               // default 200
}

export interface ImportOverlap {
  newRowsInCoveredRange: number;
  batches: { fileName: string; dateRange: CsvDateRange }[];
}
// CsvImportPreview gains `overlap: ImportOverlap | null`.
// CsvPreviewRow gains `sourceCategory: string | null`.
```

`StatementImportOptions::validate()` rejects an out-of-range `statementIndex` and a `dateOrder`
on a non-QIF file. Commands call it before any DB work (rule 5).

Commands live in a new `src-tauri/src/commands/statement_import.rs`. They stay thin (rule 4)
and get wrappers in `src/lib/tauri-api.ts` only (rule 1):

| Command | Returns |
|---|---|
| `inspect_statement_file(accountId, filePath)` | `StatementInspection \| null` (`null` = not a statement, use the CSV path) |
| `preview_statement_import(accountId, filePath, options)` | `CsvImportPreview` |
| `import_statement_file(accountId, filePath, options)` | `CsvImportResult` |

The mutation hook invalidates exactly what `use-csv-import-mutations.ts` invalidates, which
follows rule 7.

## 10. Errors and i18n

File-level errors (`AppError::Validation`, namespace `bank_accounts`):

| Key | When |
|---|---|
| `statementImport.tooLarge` | the file is over 50 MB |
| `statementImport.doctype` | the XML has a `<!DOCTYPE` |
| `statementImport.unsupportedVariant` | camt.052/054, MT942, MT950 (`detail` names the variant) |
| `statementImport.investmentOnly` | OFX with only investment statements |
| `statementImport.malformed` | the parser cannot continue (`detail`: the element or tag and the entry ordinal) |
| `statementImport.noStatements` | the format is recognized but the file contains no statement |

Row messages: `statementImport.notBooked`, `statementImport.correctionSkipped`, plus the
existing `csvImport.*` and `validation.*` keys.

Warnings: `statementImport.investmentIgnored`.

UI strings: the dialog title "Import statement" (the button on the account page changes from
"Import CSV" to "Import statement"), the supported-formats line, the summary card, the D9
warnings and checkbox, the D11 results, the D12 hint, the QIF date order select and the category
panel. Every key lands in all ten locales (rule 8).

## 11. Testing

Per `docs/standards/testing.md`:

- **Parsers** (pure, no DB): inline fixtures, one test per rule in section 6. That includes
  year-end MT940 entry dates, `RC`/`RD`, the three `:86:` dialects, CAMT `Sts` and `Sts/Cd`,
  `PRCD` opening, batch split and the batch that does not add up, `BookgDt/DtTm` with an
  offset, OFX SGML without closing tags, OFX 2.x XML, `CHARSET:1252`, a decimal comma in
  `TRNAMT`, `CORRECTACTION`, QIF `'` years, ambiguous date order, splits, the opening balance
  record, several `!Account` sections, and `!Type:Invst`.
- **Detection**: every format, the refusals, CSV with a `:` in a header still going to `None`,
  a BOM and leading whitespace.
- **Merge and balance check**: chain ok, chain break, sum mismatch, pending excluded, exact
  decimal arithmetic on amounts like `0.1 + 0.2`.
- **Service tests** with `Connection::open_in_memory()` and a minimal schema (the
  `budgeting.rs` pattern):
  - for each format, preview and import;
  - idempotence: a second import of the same file imports 0 rows and reports all as duplicates;
  - a CAMT pending entry promoted by its booked successor;
  - the ID uniqueness guard;
  - each `accountMatch` outcome;
  - the overlap hint for a statement and for CSV;
  - `statementBalance` from the declared closing balance;
  - the QIF source category map applied with source `manual`, losing to a per-line override.
- **CSV regression**: `csv_import/tests.rs` passes unchanged after the refactor (section 4.2).
- **Frontend**: pure tests for `matchSourceCategories` and for the step routing helper (CSV
  steps vs statement steps), in the `import-config.test.ts` style.
- **Fixture files**: synthetic and anonymized only, under
  `src-tauri/tests/fixtures/statements/{camt053,mt940,ofx,qif}/`, one per bank dialect the
  docs claim. Real exports never go into the repo.
- **Manual**: the demo profile in a worktree dev run with each fixture. Before release, a dry
  run on a copy of the owner's real profile with the owner's own CAMT/MT940/OFX exports.

## 12. Phases (D15)

Each phase ends with the gates green, docs updated, a CHANGELOG entry and a manual check, and
can ship on its own.

1. **Shared core + CAMT.053.** Core refactor (4.2) with CSV tests unchanged; `statement_import`
   module with `detect`, `model`, `decimal`, `merge`, `account_match`, `to_rows`, `camt`; the
   three commands and wire types; the overlap hint; the dialog generalization (file filter,
   routing, summary card, D9 switch and confirmation, statement picker); i18n; ADR 0019.
2. **MT940.** Parser with the `:86:` dialects and `:60M:`/`:62M:` chains; fixtures for at
   least one DE subfield bank, one NL slash bank and one CZ bank.
3. **OFX/QFX.** Tag scanner for both versions, credit card statements, corrections skipped,
   investment refusal or warning.
4. **QIF.** Parser, the date order select, the opening balance record, splits, the category
   panel and `matchSourceCategories`.

## 13. Documentation to update

- **ADR 0019** (phase 1), "Statement files share the bank import pipeline": D2, D3, D4, D6,
  D14.
- `docs/architecture/overview.md`: the import data flow.
- `README.md`: the feature list ("CSV import (16 bank presets …)" becomes the list of formats),
  and the "you import CSV statements" sentence.
- `CONTRIBUTING.md`: "Adding a statement fixture or an MT940 dialect" next to "Adding a bank
  preset".
- `CHANGELOG.md`: one entry per phase.
- `models/bank_accounts.rs` doc comment of `DataSource::CsvImport`: "imported from a file
  (CSV or a statement format)".
- `CsvRowMessage.line` doc comment: "file line for CSV, entry ordinal for statement formats".

## 14. Risks

- **Cross-format duplicates.** The composite key includes the description, and the description
  of one payment differs between a bank's CSV and its CAMT. The D12 hint makes overlaps
  visible but does not resolve them. Fuzzy matching is deliberately out of scope.
- **Banks that change OFX `FITID`s between downloads** would import doubles. The same D12 hint
  is the only mitigation.
- **MT940 `:86:` variety.** The free-text dialect always yields a usable description, but
  counterparty fields depend on fixtures from real banks. Phase 2 needs at least the three
  dialect fixtures listed above.
- **CAMT placement differences** (counterparty on `Ntry` vs `TxDtls`, `Pty` wrapper in newer
  versions). Matching by local name and accepting both placements covers the known variants.
  New variants need a fixture and a test, never a special case keyed on a bank name.
