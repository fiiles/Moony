# Mobile Settings: Kategorie, Kategorizace, Co sledovat — design

Date: 2026-11-06. Source: the mobile UX audit (`docs/audits/2026-11-06-mobile-ux-audit.md`,
findings B1 and A5) and the approved mobile spec (`docs/specs/2026-10-16-mobile-app-design.md`
§7 and §8.9), which already promise these pages for v1.

## 1. Why

The phone teaches the categorization engine ("Zapamatovat pro tento účet", "Zařadit i dalších N
plateb", a silently enabled country pack at first run) but gives no way to see or undo what it
learned. A phone-only user with one wrong rule gets every following import mis-filed and must go
to a desktop to fix it. Categories can be created from the picker, never renamed or deleted. And
the first run says "Změnit to můžete kdykoli v Nastavení" about the tracked areas, which is not
true today.

Success: everything the phone can create about categorization, it can also review, switch off and
delete; the first-run promise holds.

## 2. Goals, non-goals, anti-goals

Goals
- **Kategorie** page: every category with its usage, add, edit, delete with reassignment.
- **Kategorizace** page: rule packs on/off (and single pack rules on/off), own rules on/off and
  delete, learned payees with "Zapomenout".
- **Co sledovat** page: the class visibility flags and "exclude personal real estate from the
  overview".

Non-goals
- A rule editor (pattern, match type, priority) — spec §7: "a regex editor does not belong on a
  phone". Rules keep coming from payments.
- Changing a learned payee's category from Settings, the history preview/apply flow
  ("Zařadit i staré platby"). The payment picker already retargets a rule.
- Bulk forget, rule hit charts, the categorization overview stats of the desktop page.
- Any backend, schema or migration change. Every call already exists in `tauri-api.ts`.

Anti-goals
- A delete that drops payments into "Nezařazené" without the user choosing where they go (the
  desktop requires a reassign target for a category in use; so does the phone).
- A page that looks like the desktop table squeezed to 390 pt.

## 3. Where they live

Nastavení root, new rows (each a pushed section page, `SETTINGS_SECTIONS` gains
`categories`, `categorization`, `tracking`):

```
Profil · Jazyk · Hlavní měna · Vzhled · Co sledovat · Zabezpečení · Záloha a přenos ·
Oznámení · Připomínky · Kategorie · Kategorizace · Import · Zamknout · Smazat všechna data
```

Row summaries (the grey second line, as the other rows have):
- Co sledovat — the visible classes, e.g. "Akcie, Krypto, Nemovitosti +2" or "Vše".
- Kategorie — "24 kategorií".
- Kategorizace — "2 balíčky · 5 vlastních pravidel · 12 příjemců".

## 4. Kategorie

- Grouped by kind in the desktop's order (Výdaje, Příjmy, then the remaining kinds), one row per
  category: the category icon chip, name, a usage line from `CategoryUsage`
  ("128 transakcí · 2 pravidla", nothing when unused), a "Systémová" badge for system ones.
- **Add:** `+` in the nav bar → the desktop `CategoryFormDialog` (renders as a phone sheet; it has
  no money or date inputs, so the reused-dialog concern from the audit does not apply).
- **Edit:** tap the row → the same dialog in edit mode. System categories are editable, as on the
  desktop.
- **Delete:** swipe left "Smazat" and long press → menu "Upravit / Smazat" (spec principle 2).
  Not offered for system categories (the menu shows "Systémovou kategorii nelze smazat" as a
  disabled hint). The confirm is a `ConfirmSheet`:
  - unused category: the desktop's "unused" description, one danger button;
  - category in use: the desktop's in-use description with the usage breakdown, a "Přesunout do"
    pick row that opens a category list (same kind first, the deleted one excluded); the danger
    button stays disabled until a target is picked.
- Mutations through the existing `useCategoryMutations` (it already invalidates what the desktop
  invalidates).

## 5. Kategorizace

One scrolling page, three sections:

1. **Balíčky pravidel** — one row per pack (enabled first, then by the localized country name,
   "Globální" first, as `RulePacksCard`), hint "N pravidel", a switch. Tapping the row (not the
   switch) opens a full-height sheet "Pravidla balíčku {{country}}": a search field and every
   rule of the pack ("name · pattern → category") with its own switch (`setPackRuleDisabled`).
2. **Vlastní pravidla** — `getCustomRules`, one row per rule: the rule name (or pattern) as the
   title, "→ category" and the hit count over the desktop's window as the second line, a switch
   for `isActive`. System rules: switch disabled and no delete, a "Systémové" badge (desktop
   behaviour). Delete by swipe left and long press → `ConfirmSheet` with the desktop's delete
   copy. Tap = the long-press menu (Vypnout/Zapnout, Smazat).
3. **Naučení příjemci** — `getLearnedPayees`, one row per entry: the payee (original, else
   normalized, else the IBAN; "jen IBAN" marker as on the desktop), the category, and the scope
   line the desktop shows (account + direction, or the legacy wording). "Zapomenout" by swipe
   left and long press → `ConfirmSheet` with the desktop's `learnedTab.confirmForget` copy. A
   search field above the list when there are more than 15 entries.

Each section has an empty state in one line ("Zatím žádná vlastní pravidla — vznikají, když u
platby zvolíte Zapamatovat." and similar). Loading: skeleton rows; a failed query: the shared
error-with-retry row, never an empty state.

After any change, invalidate the desktop's keys: `rulePacks`, `packRules`, `customRules`,
`learnedPayees`, `ruleHits`, `categorizationOverview`, plus the categorize-queue and ledger keys
the phone reads for rule hints (the builder finds them in `use-categorize-queue` /
`use-categorize`).

## 6. Co sledovat

- **The same three switches as the first-run "Co sledovat" step**, with its labels and the same
  mapping onto `menuPreferences` (`use-first-run.ts` `saveAreas`): Investice → `investments`,
  `bonds`, `crypto`; Nemovitosti → `realEstate`; Úvěry a pojištění → `loans`, `insurance`;
  `otherAssets` stays as it is. A switch shows on when any of its flags is on; turning it on or off
  sets all of its flags. The mapping becomes one pure helper shared by first run and this page.
  Not the desktop's seven separate switches: the phone groups these classes everywhere.
- **Hide only, never delete** (desktop semantics). A hidden class keeps its data and reappears
  when switched back on. The hint under the switches says so in one line.
- **The phone starts honouring `loans` and `insurance`.** Today nothing on the phone reads them,
  so the first-run "Úvěry a pojištění" switch has no effect. Finance hides the Úvěry / Pojištění
  sections, their `+` menu items and their rows in "Nejbližší platby" when the flag is off; the
  Finance subtitle counts only shown sections. Details stay reachable by link (a milestone that
  points at a hidden loan still opens it), as on the desktop.
- One more switch: "Nepočítat vlastní bydlení do přehledu" (`excludePersonalRealEstate`, the
  desktop `DashboardPreferencesCard` copy).
- The first-run "Co sledovat" step keeps its sentence; it is now true.

## 7. Copy

Reuse the desktop strings wherever the concept is the same (`settings:categoryManagement.*`,
`settings:rulePacks.*`, `categorization:*`, the menu/dashboard preference keys). New keys only for
phone-only text (row summaries, empty states, section titles if the desktop has none), in the
`mobile` namespace, in all ten locales.

## 8. Calls I made (for the owner to check)

1. Three separate pages, with "Co sledovat" included in this piece because the audit paired it
   with categorization as the first work item.
2. Single pack rules can be switched off (a sheet per pack) — beyond the spec's "rule packs per
   country", but it is the only phone-side fix when a pack rule mis-files a payee.
3. Learned payees: forget only, no category change or history rewrite from Settings.
4. Order of the new rows in Nastavení as in §3; "exclude personal real estate" goes to Co sledovat,
   not to Připomínky as spec §8.9 had it.
5. Co sledovat uses the first run's three groups, not the desktop's seven switches, and Finance
   starts hiding loans and insurance when their group is off (§6).

## 8a. Settled during the builder check

- `SETTINGS_SECTIONS` here means the phone's (`src/mobile/shell/routes.ts`); the desktop's
  `settings-sections.ts` stays untouched.
- The pack-rules list is a phone `Sheet` with `fill full` (95 % height, scrolling body).
- The learned-payee scope line needs account names: the page reads the bank accounts (the query
  Finance already uses). The desktop `categorization:learnedTab.*` / `scopedLearning` wording is
  reused verbatim.
- Toggling a custom rule rebuilds the full `CustomRuleInput` from the stored rule, exactly as the
  desktop's `toggleCustomRule` does (name, type, pattern, category, priority, `stopProcessing`,
  `ibanPattern`). Hit counts use the desktop's `HIT_DAYS` window (shared constant, not a copy).

## 9. Left to the builder

Component split, query keys for the new reads, the exact grouping order of category kinds (copy
the desktop), and which existing phone components to reuse (`SectionPage`, `SettingsRow`,
`SwitchRow`, `EntityRow` swipe, `RowMenu`, `ConfirmSheet`, `SearchField`, `Sheet`).

Verification: gates green, locale parity, and a run of the phone shell on the demo profile that
toggles a pack, deletes a custom rule, forgets a payee, deletes a category in use with
reassignment, and flips a class off and back on.
