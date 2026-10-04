# Contributing to Moony

Thank you for your interest in Moony! This document describes how the project is
actually developed and how you can contribute.

## How This Project Is Developed

Moony is maintained by a **solo maintainer working with AI agents**, using
**trunk-based development on `main`** — small, frequent commits, no long-lived
branches. There is no formal review queue; quality is enforced by a tiered set
of automated gates and a documented workflow.

The authoritative process lives in
[`docs/standards/workflow.md`](./docs/standards/workflow.md). In short:

- **Tiered workflow** — features (new command, table, page, or domain, or
  changes over ~150 lines) go through design → spec (`docs/specs/`) →
  plan (`docs/plans/`) → TDD → code review → verification. Small fixes (bug
  fix, copy change, config tweak) need TDD + gates + verification only.
- **Gates:**
  - **pre-commit** (fast, <10s): lint-staged (staged-file ESLint/Prettier),
    `npm run typecheck`, `cargo fmt --check`
  - **pre-push** (heavy): `npm test`, `cargo clippy -- -D warnings`, `cargo test`
  - **CI** (authoritative): everything above plus generated-types drift check
    and a blocking `cargo audit --deny warnings` (the few known, non-actionable
    advisories are listed with reasons in `.cargo/audit.toml`)
- **Coding standards** live in [`docs/standards/`](./docs/standards/)
  (Rust backend, TypeScript frontend, type contract, testing, i18n).
- **Architecture decisions** are recorded as ADRs in
  [`docs/architecture/decisions/`](./docs/architecture/decisions/).
- **Instructions for AI coding agents** live in [`AGENTS.md`](./AGENTS.md)
  (`CLAUDE.md` imports it); they point at the same standards.

## How to Contribute

External contributions are welcome, with the caveat that this is primarily a
personal project:

- 🐛 **Bug reports** — the most valuable contribution (see below)
- ✨ **Feature ideas** — open an issue for discussion before writing code
- 📝 **Documentation fixes**
- 🌍 **Translations and i18n improvements**

If you want to submit code, open an issue first to align on the approach, then
open a pull request against `main`. The pull-request template asks what changed,
why and how you tested it, and has a checklist that mirrors the gates above
(lint/typecheck/tests/clippy, English **and** Czech strings, migration rules,
regenerated types). Add an entry under *Unreleased* in
[`CHANGELOG.md`](./CHANGELOG.md) for user-visible changes, and update
[`PRIVACY.md`](./PRIVACY.md) if a change adds a network request or stores new
personal data. Your change must pass the same gates listed above (CI runs them
on every PR).

Please follow the [Code of Conduct](./CODE_OF_CONDUCT.md). Security problems go
through [`SECURITY.md`](./SECURITY.md), not a public issue.

## Licensing of Contributions

Moony is licensed under the **GNU Affero General Public License v3.0** (see
[LICENSE](./LICENSE)). By submitting a contribution you agree that:

1. Your contribution is licensed under the AGPL-3.0, the same terms as the rest
   of the project.
2. You additionally grant Filip Král a perpetual, worldwide, non-exclusive,
   royalty-free, irrevocable license to use, modify, and redistribute your
   contribution **under other license terms as well**, including proprietary or
   commercial licenses.

Point 2 keeps dual licensing possible — without it, every past contributor would
have to be asked for permission before Moony could ever be offered under terms
other than the AGPL.

## Development Setup

### Prerequisites

- **Node.js** 22 or later (CI uses Node 22; Vite 8 and ESLint 10 do not run on older versions)
- **Rust** (latest stable, via [rustup](https://rustup.rs/))
- Platform-specific dependencies (see [README.md](./README.md#prerequisites))

### Commands

```bash
# Install frontend dependencies
npm install

# Run in development mode
npm run tauri dev

# Run frontend only (for UI development)
npm run dev

# Build for production
npm run tauri build

# Quality gates (run before committing)
npm run lint && npm run typecheck && npm test
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test

# After changing dependencies
npm run licenses                                      # regenerate THIRD_PARTY_LICENSES.md
cargo audit --file src-tauri/Cargo.lock --deny warnings   # from the repo root, as CI does it
```

### Browser dev bridge (optional)

While `npm run tauri dev` is running you can also open the same app in a regular
browser at `http://localhost:1420`. A dev-only bridge (`src/dev/browser-bridge.ts`
plus the `moonyBrowserBridge` plugin in `vite.config.ts`) relays every Tauri
command from the browser tab to the real Rust backend inside the Tauri window,
which makes browser dev tools, automation and screenshots available. Notes:

- It is never shipped: `main.tsx` imports it only behind `import.meta.env.DEV`
  and the Vite plugin applies to `serve` only.
- Keep the Tauri window visible. A minimized window has its JavaScript
  suspended by the OS; the bridge then fails requests after a few seconds with
  "Tauri window is not responding" instead of hanging.
- Native dialogs (file open/save) cannot be shown from the browser tab; use the
  Tauri window for those flows.
- `.claude/launch.json` describes the dev server for the Claude Code browser
  panel (`moony-dev`).

### Demo data

`seed_demo` builds a complete, encrypted Moony data folder filled with deterministic,
synthetic finances — bank accounts with a year of transactions, stocks, crypto, bonds,
real estate, loans, insurance, exchange-rate and portfolio history. Use it for
screenshots, demos, UI work and reproducing bugs without touching real data. The unlock
password is `demo1234` (the recovery key is printed when the generator finishes).

```bash
cd src-tauri
cargo run --bin seed_demo                       # writes ../demo-home/ (macOS layout)
cargo run --bin seed_demo -- --out <dir>        # writes the data folder into <dir>
MOONY_SEED_OUT=<dir> cargo run --bin seed_demo  # same, through the environment
```

**An explicit `--out` folder is cleared only when an earlier `seed_demo` run created it** (it
leaves a `.seed_demo` marker); a non-empty folder without that marker is refused, so the tool
cannot overwrite a real profile. Still, never point `--out` at your live data folder.

Moony finds its data through the OS-specific application-data folder, so running the app
against the demo profile means redirecting that folder:

- **macOS** — redirect `HOME` (keep rustup and cargo pointing at the real ones), from the repo root:

  ```bash
  HOME=$(pwd)/demo-home RUSTUP_HOME=~/.rustup CARGO_HOME=~/.cargo npm run tauri dev
  ```

  `.claude/launch.json` has the same thing as the `moony-demo` configuration.
  For an empty profile to test the first-run wizard, use a fresh folder instead
  (`mkdir -p firstrun-home`, then the same command with `HOME=$(pwd)/firstrun-home`; the
  `moony-firstrun` configuration).
- **Linux** — the data folder follows `XDG_DATA_HOME`, so generate into a subfolder named
  after the app identifier and point the variable at its parent:

  ```bash
  (cd src-tauri && cargo run --bin seed_demo -- --out ../demo-data/com.filipkral.moony)
  XDG_DATA_HOME=$(pwd)/demo-data npm run tauri dev
  ```

- **Windows** — the data folder cannot be redirected with an environment variable. Run the
  app under a separate, demo-only identifier so it never touches the real folder, and
  generate the demo data into that identifier's folder:

  ```powershell
  '{"identifier":"com.filipkral.moony.demo"}' | Set-Content -Encoding ascii demo-config.json
  cd src-tauri
  cargo run --bin seed_demo -- --out "$env:APPDATA\com.filipkral.moony.demo"
  cd ..
  npm run tauri dev -- --config demo-config.json
  ```

The Linux and Windows recipes are not tested as regularly as the macOS one; if one does not
work, please open an issue. `demo-home/`, `demo-data/`, `firstrun-home/` and
`demo-config.json` are ignored by git.

### Project Structure

```
Moony/
├── src/                    # React frontend (TypeScript)
│   ├── components/         # UI components
│   ├── hooks/              # Custom React hooks
│   ├── i18n/               # Internationalization
│   ├── lib/                # Utilities and API client
│   ├── pages/              # Page components
│   └── utils/              # Helper functions
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── commands/       # Tauri command handlers
│   │   ├── db/             # Database migrations
│   │   ├── models/         # Data models
│   │   └── services/       # Business logic
│   ├── resources/          # Bank CSV presets and categorization rule packs
│   ├── tests/fixtures/     # Sample bank statements used by the preset tests
│   └── tauri.conf.json     # Tauri configuration
├── shared/                 # Shared types and calculations
├── docs/                   # Standards, architecture, ADRs, playbooks, specs, plans
├── scripts/                # Maintenance scripts (third-party licence inventory)
└── public/                 # Static assets
```

### Adding a bank preset

Bank CSV formats are data, not code: one JSON file per bank in
[`src-tauri/resources/csv-presets/`](./src-tauri/resources/csv-presets/). The import
detects a preset from the file's header row, so a user with that bank's export
needs no manual column mapping.

1. Export a statement from the bank (a few rows, personal data replaced by fake
   values) and save it as `src-tauri/tests/fixtures/csv/<id>.csv`.
   Encoding and line endings must be what the bank really produces.
2. Add `src-tauri/resources/csv-presets/<id>.json` (`<id>` is kebab-case):

   ```json
   {
     "schemaVersion": 1,
     "id": "revolut",
     "bankName": "Revolut",
     "country": "GB",
     "institutionId": "inst_revolut",
     "matchHeaders": ["Completed Date", "Started Date", "State", "Amount"],
     "delimiter": ",",
     "encoding": "UTF-8",
     "dateColumn": "Completed Date",
     "dateFormat": "%Y-%m-%d %H:%M:%S",
     "amountColumn": "Amount",
     "descriptionColumns": ["Description"],
     "currencyColumn": "Currency",
     "balanceColumn": "Balance",
     "rowFilter": { "column": "State", "equals": "COMPLETED" },
     "exportHelpUrl": "https://…",
     "sampleFixture": "revolut.csv"
   }
   ```

   - `matchHeaders` are compared after normalization (lowercase, diacritics and
     punctuation ignored, text in brackets dropped), and **all** must be present
     for the preset to match; when two presets match, the one with more
     `matchHeaders` wins. List every header that makes this export unmistakable —
     and make sure the date and amount (or debit/credit) columns are among them.
     A preset whose headers are a subset of another bank's will be shadowed by
     it; the validity test below catches that.
   - Use `debitColumn` + `creditColumn` instead of `amountColumn` for two-column
     statements (amount = credit − debit).
   - `dateFormat` is a [chrono format](https://docs.rs/chrono/latest/chrono/format/strftime/index.html);
     it is applied strictly, one format per file. `encoding` only matters for
     files that are not valid UTF-8.
   - `rowFilter` keeps only rows whose column equals the value (pending rows).
   - `transactionIdColumn` only for a column that is unique per transaction at
     the bank; it feeds duplicate detection.
   - `institutionId` ties the preset to a row of the seeded `institutions` table;
     omit it for banks that are not seeded.
3. Register the file in `PRESET_SOURCES` in `src-tauri/src/services/csv_presets.rs`.
4. Run `cd src-tauri && cargo test csv` — `all_presets_are_valid` parses every
   preset, checks it is detected from its own headers (and not shadowed), and
   imports its `sampleFixture` (a synthetic file when there is none) with zero
   errors. Presets without a real fixture are best-effort until someone with that
   bank confirms them.

Column names the generic detector does not know yet (a new language) are added to
`src-tauri/src/services/csv_import/columns.rs`, with a test next to them.

### Adding a broker format

Stock exports are read by one adapter per source (XTB, Trading 212, Degiro, Interactive
Brokers and Moony's own table), each a module under
[`src-tauri/src/services/stock_import/adapters/`](./src-tauri/src/services/stock_import/adapters/).
An adapter recognises a file from its headers and produces the configuration the generic
parser reads it with, so a user with that broker's export needs no manual column mapping.
Everything else (preview, duplicates, holdings checks, import, undo) is shared. The design is
in [`docs/specs/2026-10-04-stock-csv-import-wizard-design.md`](./docs/specs/2026-10-04-stock-csv-import-wizard-design.md).

1. Get an export from the broker (a few trades, deposits and dividends included, personal data
   replaced by fake values) and save it as `src-tauri/tests/fixtures/csv/stocks-<id>.csv`.
   Delimiter, encoding, line endings and number and date formats must be what the broker really
   writes. Never commit real data.
2. Add the module `adapters/<id>.rs` with `detect(headers, sample_rows) -> bool` and
   `config(headers, sample_rows) -> StockImportConfig`, and register it in `adapters/mod.rs`.
   - `detect` looks at the headers (with the broker's language variants), and where the headers
     alone are ambiguous at a few sample values. Detection order matters: specific adapters come
     before generic ones, and an adapter whose headers are a subset of another's is shadowed by it,
     so the guard test below fails for it.
   - `config` addresses columns by their 0-based position in the file's own header row (broker
     files have blank and repeated header cells), and sets the date format (a chrono format,
     applied strictly), the decimal separator, how the direction is read (a type column with a
     value table, or the sign of the quantity), where the currency comes from (a column, one
     fixed currency, or the instrument's listing) and the broker's own transaction id when the
     file has one (it feeds duplicate detection).
   - A quirk the generic configuration cannot express (a quantity and price hidden in a comment,
     symbol suffixes that differ from Yahoo Finance's, rows of other asset classes) becomes a
     field of `StockImportTransforms`. That is a change of the wire contract: follow
     `docs/standards/type-contract.md` (the Rust type in `services/stock_import/types.rs`, the
     mirror in `shared/schema.ts`, `cargo test generate_bindings -- --ignored`) and implement it
     in `parse.rs`.
   - Add the source id as a `SOURCE_<ID>` constant in `types.rs` and to `StockImportSourceId` in
     `shared/schema.ts`.
3. Add the fixture to the guard test `all_stock_adapters_detect_their_fixture`. It checks that
   the file is detected as its own source and as no other, that `parse_file` of the detected
   configuration yields no errors, and the number of trades, their directions and the values of
   the first one. Add parse tests for what is special about the format (thousands separators,
   decimal comma, currency signs, `GBX` prices, dates without separators, blank headers).
4. Header words the generic detector does not know yet (a new language) go to
   `services/stock_import/columns.rs`, with a test next to them.
5. Add the source to the wizard:
   - `SOURCE_IDS`, the `BrokerId` union and `SOURCE_HELP_URLS` in
     `src/components/stocks/import/import-config.ts` (the help link opens the broker's own page
     about exporting a history; the test next to it checks every broker has one);
   - `stocks.importWizard.sources.<id>` in **both** `src/i18n/locales/cs/stocks.json` and
     `en/stocks.json`: `name`, `guideTitle`, `guide` (where to click, which report, which period,
     which format, in one or two sentences) and an optional `note` for the broker's caveats (a
     file with deposits and dividends in it, a limit on the period). The note is shown again on
     the mapping step.
6. Run `cd src-tauri && cargo test stock_import` and `npm test`.

Row messages (`importWizard.row.*`) are i18n keys, never English prose: a new kind of skipped
row or error needs its key in both locale files and a case in the parser.

## Code Style

Follow the standards in [`docs/standards/`](./docs/standards/). Highlights:

- **TypeScript/React** — typed components, TanStack Query for data fetching,
  shadcn/ui components, react-hook-form + zod for forms, all user-facing
  strings translated in both locales (`docs/standards/i18n.md`)
- **Rust** — commands in `commands/`, business logic in `services/`, proper
  `Result` error handling, tests per `docs/standards/testing.md`
- **CSS** — Tailwind utilities on the design-system tokens (`docs/design-system/`); light theme only

## Commit Message Guidelines

We follow the [Conventional Commits](https://www.conventionalcommits.org/)
specification:

```
<type>(<scope>): <description>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `build`, `ci`.

Examples:

```
feat(investments): add dividend tracking feature
fix(crypto): resolve price update race condition
docs(readme): update installation instructions
refactor(auth): simplify password validation logic
```

Keep commits small and focused — one logical change per commit.

## Reporting Bugs

Use the **Bug report** form when you open an issue; it asks for the same things:

1. **Moony version** — click the (i) About button in the top bar of the app
2. **Operating system and version** (and Apple Silicon vs. Intel on macOS, the
   distribution on Linux), and whether you installed from a release or built
   from source
3. **Steps to reproduce** — a small sample CSV with fake values helps a lot
4. **Expected behavior** vs **actual behavior**, with the exact error message and
   screenshots if applicable
5. **Logs** — open the About dialog, click **Open logs**, and paste the relevant
   lines from the newest log file (they can contain ticker symbols and file
   paths — read them before posting)

Never paste real financial data; reproduce with made-up values or the
[demo profile](#demo-data). Node.js and Rust versions only matter when you
build from source — then please add them too. For security problems, do **not**
file a public issue; see [`SECURITY.md`](./SECURITY.md).

## Suggesting Features

For new features:

1. **Check existing issues** to avoid duplicates
2. **Open a feature request issue** (there is a form) before implementing
3. **Describe the feature** and its use case
4. **Wait for discussion** before starting work

## Questions?

If you have questions about contributing, feel free to:

- Open a discussion on GitHub
- Check existing issues and discussions

---

Thank you for contributing to Moony! 🌙
