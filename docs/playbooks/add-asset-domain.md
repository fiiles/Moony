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
   fields, money as `string` (ADR 0001,
   `docs/architecture/decisions/0001-czk-base-currency-and-money-as-text.md`).
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
12. **i18n.** New namespace in BOTH `src/i18n/locales/en/<ns>.json` AND
    `src/i18n/locales/cs/<ns>.json`, registered in `src/i18n/index.ts` (`NAMESPACES`
    list + `resources` map) — `docs/standards/i18n.md`.
13. **Portfolio integration.** Net-worth inclusion does not happen by itself:
    - include the new asset class in the metrics computation in
      `src-tauri/src/commands/portfolio.rs`;
    - extend `portfolio_metrics_history` via a migration: a new CZK-TEXT total
      column plus a `*_by_currency` JSON-TEXT column (conventions: ADR 0001 and
      `docs/architecture/database.md`);
    - extend the history **backfill** so existing snapshot rows gain the new class.
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
it (in both English and Czech).
