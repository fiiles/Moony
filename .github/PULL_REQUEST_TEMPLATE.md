<!--
Thanks for contributing! For anything larger than a small fix, please open an issue first
so we can agree on the approach (see CONTRIBUTING.md). The checklist mirrors the gates in
docs/standards/workflow.md.
-->

## What and why

<!-- What changed, and what problem does it solve? Link the issue: "Fixes #123". -->

## How it was tested

<!-- Tests added, commands run, manual steps (and on which OS). Do not include real financial data. -->

## Checklist

- [ ] **Gates are green locally**: `npm run lint && npm run typecheck && npm test`, and in `src-tauri/`: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- [ ] **Tests** cover the change (regression test for a bug fix; pure functions in `src/utils/` and `shared/`; Rust services in `src-tauri/src/services/`) — see `docs/standards/testing.md`
- [ ] **Both locales**: every new or changed user-facing string is in `en/` **and** `cs/` (`docs/standards/i18n.md`)
- [ ] **Types**: if `shared/schema.ts` or a Rust wire type changed, `shared/generated-types.ts` is regenerated (`cd src-tauri && cargo test generate_bindings -- --ignored`)
- [ ] **Migrations** (if the database changed): appended as the next number in `src-tauri/src/db/migrations.rs` (never edit the baseline or an applied migration), `docs/architecture/database.md` updated — see `docs/playbooks/add-db-migration.md`
- [ ] **Docs** updated when behavior changed (README for features, an ADR for architecture decisions); `CHANGELOG.md` has an entry under *Unreleased* for user-visible changes
- [ ] **Privacy**: no new network request or stored personal data — or `PRIVACY.md` and the in-app copy are updated to say so
- [ ] Commits follow [Conventional Commits](https://www.conventionalcommits.org/)

## Licensing

By submitting this pull request I agree to the terms in "Licensing of Contributions" in
`CONTRIBUTING.md`: my contribution is licensed under the AGPL-3.0, and I additionally grant the
maintainer the right to relicense it under other terms (dual licensing).

- [ ] I agree
