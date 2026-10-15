# 0019. Statement files share the bank import pipeline

Date: 2026-10-15

## Status

Accepted

## Context

Until now the bank import read CSV only: `services/csv_import/` detects the delimiter, the header
row and the columns of delimited text, and the file dialog offered `.csv` and `.txt`. Many banks
also export, or only export, ISO 20022 CAMT.053, SWIFT MT940, OFX/QFX or QIF. Unlike CSV, these
formats can carry the account identity, the declared opening and closing balance, booked against
pending payments, and several accounts in one file.

The CSV import already owns what must not exist twice: duplicate classification, rules-first
categorization, the recorded batch with its undo and the write path
(`create_transaction_with_origin`, ADR 0007). The question is how the new formats reach it.

Alternatives considered: a second import pipeline per format (every later fix to duplicates or
categorization would be made twice, and the two would drift); an OFX or MT940 crate (the
existing ones are thinly maintained and strict, and bank exports deviate from the standards);
choosing the format by file extension (statement files often carry a generic extension such as
`.txt` or `.xml`, and a CSV can be named anything); a new `data_source` value per format (a
migration for a label nothing branches on).

## Decision

- **One pipeline, a parser in front of it.** Each format has a parser in
  `services/statement_import/` that turns the file into a neutral statement model.
  `to_rows` turns that model into the `ParsedRow`s the CSV import already works on. The shared
  core in `csv_import` (`preview_rows`, `import_rows`) takes parsed rows and `ImportOptions` and
  does everything else: duplicate check, categorization, the batch record and the writes.
  `preview_import` and `import_transactions` are `parse_rows` plus that core, and the CSV tests
  pass unchanged.
- **Own parsers.** The only new dependency is `quick-xml` (CAMT, later OFX 2.x). The parsers are
  pure functions from text to statements: no database access and no I/O.
- **The format comes from the content.** `detect` reads the first 4 KB and decides on what is
  there, never on the extension. A file that is none of the four formats goes to the CSV path
  unchanged. Variants Moony recognizes and does not read are refused with their own message
  instead of being parsed as CSV: camt.052 and camt.054 (by the namespace of the XML), MT942
  (SWIFT text with lines starting `:20:` and `:25:` and a line starting `:34F:`, whether or not
  it has `:61:` movements) and MT950 (text that reads as an MT940, that is `:20:` and `:25:`
  plus a `:61:`, `:60F:` or `:60M:` line, whose header block 2 is `{2:I950` or `{2:O950`).
  Anything else that is SWIFT-like but lacks `:20:` or `:25:` is not recognized and goes to CSV.
- **Bank IDs are unique or absent.** Each format names one field as the bank-side ID (for CAMT
  the account servicer reference). An ID that occurs more than once in one import is dropped from
  every entry that carries it, and those entries are matched by the composite key only. The
  duplicate check treats a repeated ID inside one batch as a certain duplicate that cannot be
  forced, which would silently drop a real payment. IDs are not namespaced by format.
- **No migration.** `data_source` stays `csv_import`, which now means "imported from a file". The
  batch's `file_name` shows what was imported, and `csv_import_batches` keeps its name.

Phase 1 ships the pipeline and CAMT.053. MT940, OFX/QFX and QIF follow in later phases on the
same pipeline.

## Consequences

- There is one write path for every format. A fix to duplicate classification, categorization or
  undo applies to all of them, and a statement import is as reversible as a CSV import.
- A new format adds a parser, its detection rule and a synthetic fixture with a test. It never
  adds a second pipeline.
- A payment imported from a bank's CSV and again from its statement of another format is not
  recognized as a duplicate when its description differs. The preview only hints at it: new rows
  that fall inside the period of an earlier import of the same account are counted and the
  batch is named. This applies to CSV too. Fuzzy matching is out of scope.
- For structured formats `line` (preview rows, "import anyway", category picks, row messages) is
  the 1-based ordinal of the entry in the statement, not a line of the file. Minified XML has one
  line, so file lines would collide.
- The CSV wire types keep their names (`CsvImportPreview`, `CsvPreviewRow`, `CsvImportResult`)
  and gain optional fields only. The code and the table keep "csv" in their names for a source
  that is no longer only CSV.
- A file's account and declared balances are used where the format has them: the preview checks
  that the payments add up to the closing balance, the closing balance is offered as a balance
  record, and a statement that belongs to another Moony account is flagged.
- Statement files above 50 MB and XML with a `<!DOCTYPE` are refused.
