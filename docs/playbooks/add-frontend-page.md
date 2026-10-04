# Playbook: Add a Frontend Page

Checklist for a new routed page. Conventions and rationale are owned by
`docs/standards/typescript-frontend.md` and `docs/standards/i18n.md`.

## Checklist

1. **Page file.** `src/pages/<PageName>.tsx` — PascalCase filename, **default
   export** (`docs/standards/typescript-frontend.md` rule 2).
2. **Route.** In `src/App.tsx`, import the page and add
   `<ProtectedRoute path="/<route>" component={<PageName>} />` inside the wouter
   `Switch`. Only `/auth` is public — every other route goes under
   `<ProtectedRoute>`.
3. **Sidebar entry.** Add an item to the right group in
   `src/components/common/nav-config.ts` (`id`, `labelKey`, `url`, `icon`, optional
   `prefKey` when the user may hide it — copy an existing entry); the sidebar and the
   Settings → Menu visibility toggles both render from that file. A new `prefKey` also
   needs the field in `MenuPreferences` (`shared/schema.ts`) and the Rust profile. The
   `labelKey` (`nav.<key>`) must be added to BOTH `src/i18n/locales/en/common.json` AND
   `cs/common.json`. A page inside Settings is a section instead: add it to
   `src/components/settings/settings-sections.ts`, then give it an icon in
   `SettingsLayout.tsx` and a component in `src/pages/Settings.tsx` (both typed over the
   section ids, so the compiler lists what is missing).
4. **Hooks.** Data access per the standards: reads in `src/hooks/use-<domain>.ts`
   (`useQuery`, kebab-case array keys), writes in
   `src/hooks/use-<domain>-mutations.ts` with the full mutation contract
   (`docs/standards/typescript-frontend.md` rules 2–3). Never call `invoke()`
   outside `src/lib/tauri-api.ts`.
5. **i18n namespace.** Page strings go in a domain namespace: create BOTH
   `src/i18n/locales/en/<ns>.json` AND `cs/<ns>.json`, register the namespace in
   `src/i18n/index.ts` (`NAMESPACES` list + `resources` map), and use
   `useTranslation('<ns>')` + `useTranslation('common')` per
   `docs/standards/i18n.md`.

## Verify

```bash
npm run lint && npm run typecheck && npm test
```

Plus a visual check: `npm run tauri dev` → open the page from the sidebar, in both
English and Czech (Czech must not fall back to English — see
`docs/standards/i18n.md` rule 1).
