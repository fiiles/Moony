# More UI languages — implementation plan

> **For agentic workers:** execute task-by-task; steps use `- [ ]` checkboxes. Spec:
> `docs/specs/2026-10-10-more-languages-design.md`.

**Goal:** ten UI languages (en, cs + de, es, it, nb, pl, sv, ja, zh) with their own formats,
a dropdown language picker, lazy-loaded locale bundles and an all-locale parity gate.

**Architecture:** one language registry (`src/i18n/languages.ts`) feeds i18n init, the
pickers and the format locale. English is bundled; every other locale is a lazy Vite chunk
read by a custom i18next backend. A Vitest parity test keeps every locale in step with
English.

**Tech stack:** i18next 26, react-i18next 17, Radix dropdown menu, Vite `import.meta.glob`,
Vitest.

## Global constraints

- Work in the worktree `../Moony-tauri-languages`, branch `feat/more-languages`.
- No new npm dependency. No Rust change, no migration.
- Pickers list English first, then the rest alphabetically by native name.
- Every new user-facing string lands in all ten locales.
- `{{variables}}` keep their names; German never uses the `, lower` formatter.

---

### Task 1: Language registry, detection and lazy loading

**Files:**
- Create: `src/i18n/languages.ts`, `src/i18n/languages.test.ts`
- Modify: `src/i18n/index.ts`, `src/i18n/I18nProvider.tsx`, `src/main.tsx`,
  `src/pages/auth-page.tsx`, `src/hooks/use-settings-mutations.ts`

**Interfaces (produced):**
- `LANGUAGES: readonly LanguageInfo[]` — `{ code, native, english, locale }`, picker order.
- `SUPPORTED_LANGUAGES: readonly SupportedLanguage[]` (codes of `LANGUAGES`).
- `type SupportedLanguage = 'en' | 'cs' | 'de' | 'es' | 'it' | 'nb' | 'pl' | 'sv' | 'ja' | 'zh'`.
- `isSupportedLanguage(value: unknown): value is SupportedLanguage`.
- `matchLanguage(tag: string | null | undefined): SupportedLanguage | null` — BCP 47 →
  supported code (`no`/`nn`/`nb-*` → `nb`, `zh-*` → `zh`, primary subtag otherwise).
- `getLocaleForLanguage(lang): string` (re-exported from `index.ts` as today).
- `i18nReady: Promise<unknown>` exported from `src/i18n/index.ts`; `main.tsx` awaits it.
- `changeAppLanguage(lang): Promise<void>` in `index.ts` — loads the bundle, switches,
  persists `moony-language`.

- [ ] Write `languages.test.ts`: order (en first, rest sorted by `native` with
  `localeCompare(…, 'en')`), `matchLanguage` cases (`de-AT`→de, `nn-NO`→nb, `no`→nb,
  `zh-Hant-TW`→zh, `pt-BR`→null, `''`→null), locales map (`nb`→`nb-NO`, `zh`→`zh-CN`).
- [ ] Run `npx vitest run src/i18n/languages.test.ts` → FAIL (module missing).
- [ ] Implement `languages.ts`.
- [ ] In `index.ts`: bundle only English; lazy backend

  ```ts
  const lazyBundles = import.meta.glob<{ default: Record<string, unknown> }>(
    ['./locales/*/*.json', '!./locales/en/*.json']
  );
  const lazyBackend = {
    type: 'backend' as const,
    init() {},
    read(lng: string, ns: string, cb: (e: unknown, d?: unknown) => void) {
      const load = lazyBundles[`./locales/${lng}/${ns}.json`];
      if (!load) return cb(null, {});
      load().then((m) => cb(null, m.default), (e) => cb(e));
    },
  };
  ```

  init with `partialBundledLanguages: true`, `supportedLngs: SUPPORTED_LANGUAGES`,
  `load: 'currentOnly'`, detector `convertDetectedLanguage: (l) => matchLanguage(l) ?? 'en'`;
  export `i18nReady` (the init promise) and `changeAppLanguage`. Set
  `document.documentElement.lang` on `languageChanged` and after init.
- [ ] `main.tsx`: `await i18nReady` before `createRoot(...).render`.
- [ ] `I18nProvider` / `auth-page` / `use-settings-mutations`: use `isSupportedLanguage` and
  `changeAppLanguage`; the language toast fires after the bundle is loaded
  (`await i18n.loadLanguages(lang)` in `mutationFn` before resolving).
- [ ] Vitest + typecheck pass; commit `feat(i18n): language registry, detection and lazy locale bundles`.

### Task 2: All-locale parity gate

**Files:** Create `src/i18n/locales.test.ts`.

- [ ] Test, for every `SUPPORTED_LANGUAGES` code except `en`, every namespace file of
  `locales/en` (read with `import.meta.glob(..., { eager: true })`):
  1. the file exists;
  2. same base keys as English (plural suffix stripped);
  3. each English plural base has exactly `Intl.PluralRules(lang).pluralCategories`
     (+ `zero` when English has `_zero`); Czech is checked against `one, few, other`
     (+ optional `many`, `zero` as English);
  4. union of `{{name}}` / `{{name, fmt}}` variable names per base equals English's;
  5. no empty strings; no non-string leaves where English has strings.
- [ ] Run → FAIL for the eight missing languages (expected until Task 4).
- [ ] Commit `test(i18n): parity gate over every locale`.

### Task 3: LanguageSelect picker + typography

**Files:**
- Create: `src/components/common/LanguageSelect.tsx`
- Modify: `src/pages/auth-page.tsx` (both `LanguageSegment` uses),
  `src/components/settings/LanguageCard.tsx`, `src/index.css`, locale files (`settings`
  `language.interface` in en+cs now; the others get it in Task 4)
- Delete: `src/components/auth/LanguageSegment.tsx`; note it in `docs/DEPRECATED.md`.

- [ ] `LanguageSelect({ value, onChange, variant: 'quiet' | 'field', className })`:
  `DropdownMenu` + `DropdownMenuRadioGroup`; trigger = `Button` (`variant="ghost" size="sm"`
  for quiet, outline field look for Settings) with `Globe` icon, native name and a chevron;
  items = native name + right-aligned money sample `getFormatters(locale, currency).money(1234.56, currency)`
  (currency: main currency when known, else EUR) with `lang` attribute per item.
- [ ] Lock screen + first run: quiet variant, same absolute position.
- [ ] Settings: replace the tile grid with a `SettingsRow` (label `language.interface`,
  hint `language.description`) holding the field variant.
- [ ] `index.css`: `:lang(ja), :lang(zh)` reset the `--ls-*` heading tracking tokens to
  `normal`/`0` and `letter-spacing: normal` on body.
- [ ] Lint, typecheck; commit `feat(i18n): dropdown language picker on the lock screen, first run and settings`.

### Task 4: Translations

- [ ] Write the glossary `docs/standards/i18n-glossary.md` (untranslated names, key finance
  terms per language, register, punctuation, the `lower` rule).
- [ ] Eight Sonnet subagents, one per language, each writes all 18 namespace files of its
  language from English (Czech as tone reference) and runs the parity test for its language.
- [ ] Register nothing (the glob picks the files up); run the parity test → PASS.
- [ ] Commit per language `feat(i18n): <language> translation`.

### Task 5: Docs and rules

- [ ] `AGENTS.md` rule 8 → "every locale"; `docs/standards/i18n.md` (ten languages, lazy
  loading, the registry, the parity test replaces the manual check, plurals per language,
  glossary link).
- [ ] Commit `docs(i18n): ten-locale rule and parity gate`.

### Task 6: Live UX pass

- [ ] Demo app on port 1421 (a separate dev-server port, because another dev server may already run), switch to German, Japanese
  and Polish; check lock screen, first run, sidebar, overview, settings, bank accounts,
  stocks, reports, a dialog; fix overflow (truncation, `min-w-0`, wrapping) found.
- [ ] Commit fixes `fix(ui): …`.

### Task 7: Gates and merge

- [ ] `npm run lint && npm run typecheck && npm test && npm run build`; `cargo test` not
  needed (no Rust change).
- [ ] Merge `--no-ff` into local `main` through a temporary worktree; verify ancestry; no push.
