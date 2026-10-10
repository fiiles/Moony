# More UI languages — design

Status: approved by the owner 2026-10-10.

## Goal

Ship Moony in ten UI languages instead of two: English, Czech and eight new ones —
German, Spanish, Italian, Norwegian (bokmål), Polish, Swedish, Japanese and Simplified
Chinese. Each language brings its own number/date/money format. Arabic was considered and
dropped (RTL layout is out of scope).

## Decisions

| Topic | Decision |
|---|---|
| Codes and formats | `en` en-US, `cs` cs-CZ, `de` de-DE, `es` es-ES, `it` it-IT, `nb` nb-NO, `pl` pl-PL, `sv` sv-SE, `ja` ja-JP, `zh` zh-CN. Format follows the language (one "Language and formats" choice). |
| Order in pickers | English first, then the rest alphabetically by native name. |
| Registry | One table `src/i18n/languages.ts` (code, native name, English name, Intl locale) replaces `SUPPORTED_LANGUAGES` / `LANGUAGE_NAMES` / the locale map. |
| Detection | Stored choice (`moony-language`), else the system language: primary subtag match, `no`/`nn` → `nb`, `zh-*` → `zh`; else English. |
| Loading | English stays bundled as the fallback; every other language is a lazy Vite chunk (`import.meta.glob`) read by a tiny custom i18next backend — no new dependency. `main.tsx` awaits the initial language before the first render; switching awaits the bundle before `changeLanguage` resolves. |
| Picker | New `LanguageSelect` (dropdown menu: globe icon + native name; items show the native name and the money sample in that format). Replaces the `LanguageSegment` segment on the lock screen and the first run, and the tile grid in Settings → Language and formats. |
| Typography | `<html lang>` follows the UI language (CJK glyph variants, hyphenation). `:lang(ja)` / `:lang(zh)` reset the negative heading tracking. |
| Rule packs | No new language → pack mappings: a language is not a country, and countries are picked in the first run. The existing `cs` → `cz` default stays untouched. |
| Backend | `user_profile.language` is free TEXT; no migration, no validation change. |
| Translations | Machine (AI) translations from English, Czech as a tone reference, a shared glossary; brand and broker names untranslated; informal register where the language has one (du / tú / tu / ty / du / du). |
| Gate | A Vitest parity test over all locales: same keys as English (plural-aware), exactly the plural categories `Intl.PluralRules` gives the language (+ `_zero` where English has it; Czech keeps its existing `one/few/other`), the same `{{variables}}`, no empty strings. |
| Rule 8 | "Every user-facing string lands in every locale" — enforced by the parity test, so future features ship all ten. |

## Out of scope

Arabic / RTL; separating the format region from the UI language; translating rule-pack
data, broker help pages or the sample CSV (non-Czech languages get the English variant).

## Verification

Gates (lint, typecheck, Vitest incl. the parity test, cargo test unchanged), plus a live
pass of the demo app in German (longest copy), Japanese and Polish across the lock screen,
first run, sidebar, overview, settings and the main pages; overflow fixes where found.
