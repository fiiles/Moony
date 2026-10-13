# Playbook: Add a New Asset Domain

The full checklist for a new asset class (a new "thing" that contributes to net
worth). Execute top to bottom — the order matters, because each layer imports the one
below it. Rationale lives in the standards; each step links to its owner. Copy the
canonical examples from `docs/architecture/overview.md`, and check
`docs/DEPRECATED.md` before copying any existing domain's pattern.

## Checklist

1. **Migration.** Create the domain's table(s) — follow
   `docs/playbooks/add-db-migration.md` end to end (including the
   `docs/architecture/database.md` catalog update).
2. **Models.** `src-tauri/src/models/<domain>.rs`: structs deriving
   `Serialize, Deserialize, specta::Type` with per-field
   `#[serde(rename = "camelCase")]`; an `Insert*` struct implementing
   `validate() -> Result<(), AppError>` with i18n keys
   (`docs/standards/rust-backend.md` rules 3 + 7). Declare the module in
   `src-tauri/src/models/mod.rs`.
3. **Service.** `src-tauri/src/services/<domain>.rs`: all business logic, functions
   taking `&Connection`, with `#[cfg(test)]` tests at the bottom per
   `docs/standards/testing.md`. Declare it in `src-tauri/src/services/mod.rs`.
4. **Commands.** `src-tauri/src/commands/<domain>.rs`: thin wrappers only —
   validate → delegate to the service → map errors as `AppError`
   (`docs/standards/rust-backend.md` rule 1).
5. **Registration.** `pub mod <domain>;` in `src-tauri/src/commands/mod.rs` + one
   entry per command in `tauri::generate_handler![]` in `src-tauri/src/lib.rs`.
6. **Bindings + regenerate.** Register the new types in `collect_types()` in
   `src-tauri/src/bindings.rs`, then
   `cd src-tauri && cargo test generate_bindings -- --ignored`; commit the
   regenerated `shared/generated-types.ts` (`docs/standards/type-contract.md`).
7. **Wire contract.** Hand-mirror the types into `shared/schema.ts` — camelCase
   fields, money as `string` (decimal TEXT, ADR 0001), and a converted amount as
   `string | null` next to the response's `currency` and `fxQuality`
   (`docs/architecture/decisions/0012-explicit-currencies-and-eur-rate-reference.md`).
8. **API namespace.** Add `<domain>Api` to `src/lib/tauri-api.ts` — the only file
   allowed to call `invoke()` (`docs/standards/typescript-frontend.md` rule 1).
9. **Hooks.** `src/hooks/use-<domain>.ts` (reads, `useQuery`, kebab-case keys) and
   `src/hooks/use-<domain>-mutations.ts` (writes). Every mutation follows the **full
   mutation contract**: invalidate the domain key(s) + `["portfolio-metrics"]` +
   `["cashflow-report"]`, then `await portfolioApi.recordSnapshot()` and invalidate
   `["portfolio-history"]`. Canonical: `src/hooks/use-bank-account-mutations.ts`
   (`docs/standards/typescript-frontend.md` rule 3).
10. **Page + route.** `src/pages/<Domain>.tsx` (PascalCase, default export) + route
    in `src/App.tsx` under `<ProtectedRoute>` + sidebar entry — follow
    `docs/playbooks/add-frontend-page.md`.
11. **Components.** `src/components/<domain>/` — named exports; forms and dialogs
    per `docs/standards/typescript-frontend.md` rule 4 (react-hook-form +
    zodResolver, dialog state lifted to the page).
12. **i18n.** New namespace `src/i18n/locales/<lang>/<ns>.json` in EVERY locale,
    registered in `src/i18n/index.ts` (`NAMESPACES` list + the English import in
    `resources.en`) — `docs/standards/i18n.md`.
13. **Portfolio integration.** Net-worth inclusion does not happen by itself:
    - give the class a native per-currency breakdown: one more field of `ClassBreakdowns`
      (`services/portfolio_history.rs`), filled from the class's own currency columns in
      `net_worth::compute_breakdowns` (`services/net_worth.rs`);
    - extend `portfolio_metrics_history` via a migration with **one** `*_by_currency`
      JSON-TEXT column (`TEXT NOT NULL DEFAULT '{}'`) and no CZK or other converted total
      column — snapshots store native breakdowns only (ADR 0012, conventions in
      `docs/architecture/database.md`);
    - teach the snapshot writer and the history readers the new column, and add the class to
      `net_worth::ClassTotals` so `metrics_report` and `history_report` convert it into the
      report currency (an unconvertible amount stays out of the totals and is named in
      `fxQuality`; never 1:1);
    - extend the history **backfill** so existing snapshot rows gain the new class;
    - if the class holds a currency the user may type freely, make sure `fx_backfill` covers its
      currency columns so historical conversion has rates (`held_currencies`).
14. **README.** Add the new asset class to the feature list in `README.md`
    (`docs/standards/workflow.md`, definition of done).

## Verify

Full gate suite:

```bash
npm run lint && npm run typecheck && npm test
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
cd src-tauri && cargo test generate_bindings -- --ignored
git diff --exit-code shared/generated-types.ts
```

Plus a manual app run: `npm run tauri dev` → create, edit, and delete an entity of
the new class; confirm the dashboard net worth and the portfolio trend chart reflect
it (in English and in at least one other language).
