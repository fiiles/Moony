# Moony mobile (iOS and Android) — design

Date: 2026-10-16. Status: **draft for owner review**. The concept was approved by the owner in
chat on 2026-10-16 with three rulings: (1) iOS **and** Android from the first release, one visual
language on both, no platform-specific chrome; (2) dark mode on mobile, and therefore on the
desktop too, from one shared set of dark tokens (the desktop dark theme is its own project:
`2026-10-16-dark-mode-design.md`); (3) the v1 scope as proposed, except that statement import
without manual column mapping is a v1 requirement, not a v2 item (§8.6). Prototype:
`2026-10-16-mobile-app-prototype.html`. This spec is also the hand-off brief: an implementer
should not need the chat.

## 1. Problem

Moony is a desktop application with a minimum window width of 1080 px, a 264 px sidebar with 17
pages, six-to-nine-column tables, hover-revealed row menus, chart tooltips on hover and file
dialogs with drag-and-drop. Nothing of that works on a phone, and the roadmap lists "a phone app"
under "not before 1.0".

The owner wants a **standalone** phone app first: a person chooses Moony on a computer *or* on a
phone, keeps the same privacy promise (local encrypted data, no cloud, no telemetry) and gets the
same picture of their wealth. A sync service between the person's own devices comes later; the
phone app must not make it harder.

The phone changes the job. On a computer Moony is a workbench; on a phone it is a glance, a
two-minute monthly ritual (statement in, payments categorized, balances recorded) and the place
where a policy number or a loan schedule is looked up when needed. The design below optimizes for
those, keeps the desktop's visual language, and leaves the wide-screen analysis to the desktop
until a later version.

## 2. Goals and non-goals

Goals:

- A complete personal-finance app on iOS and Android with the same data model and encryption as
  the desktop, so a profile can move between the two (backup = transfer) and a later sync service
  works on one schema.
- The five frequent jobs (§3) each take seconds, one-handed, with the phone's own strengths:
  share sheet, camera, biometrics, local notifications.
- Statement import that **never asks a phone user to map columns in a grid** (§8.6).
- One design system for both apps: shared tokens (light and dark), a mobile component layer in
  `docs/design-system/`.
- All ten UI languages at release; Czech is the reference.

Non-goals (v1):

- Sync, household profiles, bank connections, MCP on the phone (§13).
- Stocks analysis (TWR split, drawdown, drag-range), projection, cashflow planning, calculators:
  v2 or never, see §7.
- Tablet layouts. The phone layout stretches to tablets; a two-pane tablet design is later.
- Apple Watch, Wear OS, widgets (v2), Siri/Assistant shortcuts.

## 3. Who it is for and the five jobs

A person who keeps their finances in Moony on a phone only. They import statements from their
bank's app, hold stocks, crypto, bonds, property, loans and insurance, and want one number and
one agenda. Ordered by frequency, with the time the design budgets for each:

| # | Job | How often | Budget |
|---|---|---|---|
| 1 | Glance: how am I doing, is anything due | daily | 10 s from pocket to answer |
| 2 | Keep data fresh: import the statement, categorize new payments, record balances | monthly | 2 min for all accounts |
| 3 | Check the month: where the money went, are the limits held | weekly | 30 s |
| 4 | Record an event: a trade, a revaluation, an extra payment, a new policy, a document photo | occasionally | 1 min |
| 5 | Look something up: policy number, loan schedule, a position's history | occasionally | 3 taps |
| 6 | Analyze: performance by tag, projection, calculators | rarely | desktop |

## 4. Principles

1. **One number per screen.** Inherited from the desktop; stricter on a phone.
2. **Fingers, not a pointer.** Nothing lives behind hover. Every row offers *swipe + long press*
   (the pair every platform knows); a `···` button is never the only way.
3. **Short visits.** Biometric unlock to the overview in 2 s. The monthly ritual in 2 min.
4. **Input where it originates.** Statement from the bank app's share sheet, a contract through
   the camera, a balance from a notification.
5. **The phone is the only copy.** Backup is part of data upkeep, not a settings page.
6. **The same language as the desktop.** Same words, tokens, number formats and category icons.
   A person who later syncs does not relearn anything.
7. **Privacy in public.** Hide amounts with one tap, blur the app-switcher snapshot, lock when
   backgrounded.

## 5. Both platforms, one visual language

Ruling (owner, 2026-10-16): the app looks the same on iOS and Android. No iOS-only large-title
blur, no Material FAB, no platform fonts. Moony's own tab bar, sheets, segmented control and
Inter Variable (bundled, so neither SF nor Roboto ever leaks in). Reference width 390 pt; every
layout is checked at 360 dp (the narrowest common Android) and 430 pt.

What differs is the platform layer under the UI, and nothing above it:

| Concern | iOS | Android |
|---|---|---|
| Statement in | Share sheet and "Open in" (document types for csv, txt, xml, ofx/qfx, qif, sta/mt940) | `ACTION_SEND` / `ACTION_VIEW` intent filters on the same MIME types |
| File picker | Files (document picker) | Storage Access Framework |
| Biometrics | Face ID / Touch ID; DB key in the Keychain behind biometry | BiometricPrompt; DB key in the Keystore |
| Backup target | iCloud Drive or any Files location | any SAF folder (Google Drive, local) |
| Notifications | local `UNUserNotificationCenter` | local, one channel per reminder group, `POST_NOTIFICATIONS` on 13+ |
| App shortcuts | Home Screen quick actions | app shortcuts |
| Back | edge swipe, nav back button | system back gesture and button: closes a sheet, pops a detail, never leaves the app from a sheet |
| Privacy screen | blurred app-switcher snapshot | blurred recents snapshot (`FLAG_SECURE` optional, off by default so screenshots stay possible) |
| Safe areas | Dynamic Island, home indicator | cutouts, gesture bar |
| Distribution | App Store | Play, plus a signed APK on GitHub releases (owner decision pending) |

## 6. Navigation

Four tabs and an avatar. No "More" tab (D2).

```
Přehled · Finance · Cashflow · Majetek       avatar (Přehled, trailing) → Nastavení
```

| Tab | The one question it answers | What it absorbed from the desktop |
|---|---|---|
| **Přehled** | How am I doing and what should I do? | Dashboard, Co vás čeká, data upkeep |
| **Finance** | What do I have with banks and insurers, and what happened on it? | Bank accounts, detail, ledger, import, categorization, **Loans, Insurance** (D10) |
| **Cashflow** | Where does it go and do I hold my limits? | Cashflow **and Budgets** (merged, D3) |
| **Majetek** | What do I own? | Stocks, Crypto, Bonds, Real estate, Other assets, Watchlist |

Why "Finance" (owner ruling, 2026-10-16): a loan and a policy are contracts with an institution,
like a bank account, not wealth; the owner wanted them next to the accounts. "Finanční služby" is
too long for a tab, "Účty" too narrow for a policy, "Smlouvy" wrong for an account, "Peníze"
collides with Cashflow. "Finance" is seven letters, everyday Czech ("moje finance") and reads the
same in every UI language (`Finance`, `Finanzen`, `Finanzas`, `Finanse`, `Ekonomi`, 財務, 财务).
Majetek is then pure assets, which also makes the hero and the rows of that tab add up.

Rules:

- Each tab is one scrolling screen with a collapsing title; details push on top of it; forms are
  sheets (half height for short ones, full height for long ones). The tab bar stays visible on
  tab roots and detail pages, hides under sheets.
- "Add" lives where the object lives: `+` in the nav bar of Finance (a typed menu: účet, úvěr,
  pojistku) and Majetek (investici, kryptoměnu, dluhopis, nemovitost, aktivum) and on every class
  page. Přehled has no `+`; its Údržba card carries the actions that are due (§8.3). Global
  shortcuts come from the platform: app shortcuts (Zaznamenat zůstatek · Importovat výpis ·
  Přidat transakci) and the share sheet.
- Settings open from the avatar on Přehled as a full-height sheet with its own sections (§8.9).
- The hide-amounts eye sits in the Přehled nav bar (leading); its state is global and persists
  (D6).
- Deep links: notifications and shortcuts open the target directly (an account, the categorize
  queue, the import sheet, a policy).

## 7. Feature disposition

| Desktop | On the phone | Why |
|---|---|---|
| Přehled | **Kept**, one column; new **Údržba** card on top, shown only when something is due | job 1 and 2 |
| Bankovní účty + detail | **Kept**; tables become two-line rows; filters move to a sheet; category by swipe | core |
| Rozpočty | **Merged into Cashflow**: a category row shows its limit as a progress bar; limits are managed from `···` | one question, one screen |
| Cashflow | **Kept**; Úspora is the hero, Příjmy/Výdaje under it | one number |
| Plánování cashflow | **v2** — fits a phone as a list; deferred for scope only | |
| Akcie, Krypto + details | **Kept**; 8 columns become name · value · change · gain; Analýza is replaced by one figure, TWR this year, in the stats | |
| Dluhopisy | **Kept**; the maturity ladder becomes a dated list "Nejbližší kupóny a splatnosti" | a list beats yearly bars at 390 pt |
| Nemovitosti + detail | **Kept**; six tabs become stacked sections; photos straight from the camera | |
| Ostatní aktiva | **Kept** + a light detail page (desktop has none; natural on a phone) | |
| Úvěry + detail | **Kept, listed as a section of the Finance tab** (no separate list page); the aggregate debt trajectory is dropped on the phone, each loan keeps its own chart; the extra-payment what-if is a sheet; the schedule a list "Příštích 12" | contracts sit with accounts (D10) |
| Pojištění + detail | **Kept, listed as a section of the Finance tab, detail reordered "facts first"**: policy number with copy, limits, documents, next payment; the premiums chart last; the 12-month calendar becomes the next-payment date on every row | job 5 |
| Sledování akcií | **Kept** as the segment *Pozice · Sledované* on the Akcie page | |
| Projekce | **v2** (one sentence on Majetek plus a page with the band chart) | |
| Analýza akcií | **Dropped**; v2 "lite" (TWR split by one tag group); drag-range and the drawdown panel never | needs width and a pointer |
| Kalkulačky | **v2** | |
| Pravidla kategorizace | **Editor dropped.** Rules come from payments ("Zapamatovat pro tento účet", "Zařadit i dalších N plateb"); Settings lists rules with on/off and delete | a regex editor does not belong on a phone |
| Import výpisu | **Kept and extended**: share sheet and Files; structured formats, presets, **guided recognition** of unknown CSV, learned formats, per-bank export guides (§8.6) | the monthly ritual |
| Import brokera | **v1** for the known sources (XTB, Trading 212, Degiro, IBKR, Moony); custom formats v2 | |
| Co vás čeká / milníky | **Kept** + local notifications | |
| Nastavení | **Kept**; adds biometrics, background lock, hide amounts, notifications, backup to Files/iCloud/SAF, profile transfer, import formats | |
| Export CSV | **Kept** through the share sheet | |
| MCP server | **Never on the phone** (a local HTTP server for desktop AI tools; a future sync service may expose the data instead) | |
| Updater, open folder, drag-and-drop | **Never** (stores, Files, share sheet) | |

## 8. Screens

Shared vocabulary: *nav bar* (title, leading and trailing buttons), *sheet* (bottom card over a
scrim, drag handle, Zrušit left and the primary named like the title right, like the desktop
modal footer), *row* (the entity row of §10.3), *stat* (label, value, note), *hero* (label, one
number, change, chart).

### 8.1 Lock

Biometric prompt on launch; the password field is the fallback and stays the master secret; the
recovery flow is the desktop's three steps. The DB key is stored behind biometry only after the
person opts in (first run or Settings → Zabezpečení). Background lock after 1 minute by default
(Ihned / 1 min / 5 min / 15 min); the app-switcher snapshot is blurred. Language segment on the
lock screen as on the desktop.

### 8.2 First run

The desktop's five steps in one column, without the live preview: Soukromí (main currency,
banking country) → Účet (name, surname, e-mail, password) → Klíč (recovery key; "Uložit do
Souborů", "Zkopírovat", then confirm two groups) → Co sledovat (four area cards) → První účet.
Two new steps in the "explain first, then the system prompt" pattern:

- **Biometrie**: "Odemykat Moony obličejem / otiskem?" — Zapnout / Teď ne.
- **Záloha**: "Telefon je jediná kopie vašich dat. Zapnout šifrovanou zálohu do iCloud Drive /
  vybrané složky?" — Zapnout (recommended) / Teď ne (D7). The backup is the encrypted profile
  (same bytes as the desktop backup zip); Moony never sees it.

### 8.3 Přehled

Nav bar: large title is the greeting ("Dobrý den, Filipe."), subtitle "Stav majetku k 16. 10.",
leading eye (hide amounts), trailing avatar. Sections in order; each is hidden when empty:

1. **Údržba** — an action card, present only when something is due. One row per item, each with
   its verb button:
   - "4 zůstatky starší než měsíc" → **Zaznamenat** (one sheet listing those accounts with the
     last balance and date; type the new ones; date defaults to today; one Uložit).
   - "12 plateb bez kategorie" → **Zařadit** (§8.5).
   - "Je čas na výpis z Fio" (last import of an account older than 35 days) → **Importovat**
     (opens the import sheet with that bank's export guide, §8.6).
   - "Záloha před 41 dny" or "Zatím žádná záloha" → **Zálohovat**.
   - "Ocenění Lipová je rok staré" → **Přecenit**.
   The triggers reuse the milestone kinds `balances_stale`, `backup_stale`, `valuation_stale` and
   add `statement_due` and `uncategorized` (the latter two are not milestones on the desktop; the
   service that computes the card lives next to milestones).
2. **Hero** Čisté jmění: value in display size, "▲ 8,4 % · + 13 913 € za 30 dní", period segment
   (30 dní · 3 měsíce · Letos · Rok · Vše), chart 160 pt. Touch: a finger on the chart draws the
   crosshair, the value and date above rewrite to that day, a light haptic tick on every day
   change, release snaps back. Big-step marks (○) are tappable and open a callout with the dated
   facts (desktop §9).
3. **Three stats** in one row: Aktiva · Závazky · Volné peníze.
4. **Co vás čeká** — three agenda rows (desktop `AgendaRow`), "Zobrazit vše (N)". Swipe right:
   Připomenout za týden; swipe left: Skrýt; long press: the full menu (Skrýt, Za týden, Za
   měsíc, Nepřipomínat …); tap: the source page.
5. **Rozložení majetku**: ring 110 pt on the left, legend with amounts on the right.
6. **Poslední pohyby**: five rows, "Zobrazit vše" → Účty.
7. **Začínáme** (first weeks only, dismissible), as on the desktop.
8. Conversion-quality note when a currency is left out of totals, as on the desktop.

### 8.4 Finance (accounts, loans, policies) and account detail

**Finance**: one scrolling screen with three sections. Total "Celkem na účtech" as a compact
hero (value, change 30 days). "Nezařazené platby (12)" as a row above the accounts when non-zero.
**Účty**: account rows — bank logo chip, name, sub "Fio · CZK · výpis 10. 10." (or "ručně ·
zůstatek 2. 10."), trailing balance and its 30-day change; excluded accounts carry the
"Vyloučeno" badge; sort in `···`. **Úvěry**: the section head carries the outstanding total; loan
rows — name, sub "banka · sazba · fixace do", trailing outstanding balance (no sign: the section
head already says "nesplaceno", and a minus would cost the name its room at 390 pt) and the
monthly payment; tap → loan detail (§8.8). **Pojištění**: the section head
carries the annual premium; policy rows — type icon, name, insurer; trailing premium per period
and the next payment date; tap → policy detail (§8.8); ended policies behind a toggle in `···`.
"Vývoj hotovosti" is a small trend card at the end. Trailing `+`: Přidat účet / úvěr / pojistku.
The desktop's loan and insurance list pages (summary stats, debt trajectory, payment calendar)
have no phone equivalent: the section heads carry the one figure each, the Přehled chart and
each loan's own chart show the trajectory, and the next payment sits on every policy row.

**Account detail**: nav bar title = account name, subtitle "Fio · CZK · 2100…/2010", `···`
(Upravit účet, Přidat transakci, Historie zůstatku, Historie importů, Úročení, Automatická
kategorizace, Smazat účet). Two full-width buttons under the hero: **Zaznamenat zůstatek**
(primary) and **Importovat výpis** (the desktop's two-primary exception, kept). Then:

- Hero "Zůstatek k 10. 10.": the balance-record line with ○ marks, 3 stats under it (Změna
  letos, Průměrně za měsíc, Roční úrok) when they have data.
- **Toky**: a horizontally scrollable 12-month bar strip (in/out per month, newest selected);
  tapping a month filters the ledger to it.
- **Ledger**: search by pull-down, segment Vše · Příchozí · Odchozí, a Filtr button opening a
  sheet (Kategorie incl. Nezařazené, Období: Tento měsíc … Vlastní). Rows grouped by day with
  sticky day heads; row = counterparty chip, name, sub "Kategorie · detail", trailing signed
  amount (credits in `gain` with "+"), badges Čekající / Vratka. Infinite scroll replaces paging.
- **Swipe left on a row = Kategorie** (opens the picker directly); swipe right = Podrobnosti;
  long press = the row menu.
- **Transaction sheet**: amount, counterparty, date, category (the picker below), "Význam
  pohybu" (Automaticky / Příjem / Výdaj / Přesun), note, "Podrobnosti" folded (bank id, value
  date, batch, file, line…), Smazat as a text danger action.
- **Category picker** (half sheet): search field, recent categories first, then all with icon
  chips; under the list the two learning options as switches, worded as on the desktop:
  "Zapamatovat pro tento účet a směr" and "Zařadit i dalších N plateb" (count computed live);
  an MCP-suggested category appears as a pill to accept or decline; "Nová kategorie…" at the end.

### 8.5 Zařazení plateb (the categorize queue)

A full-screen list "12 plateb bez kategorie", newest first, reachable from Údržba, from the
account row, from the import's done step and from the monthly notification. Each row shows the
payment (chip, counterparty, amount, date) and a **suggested category as a pill** when one
exists (rule pack hit without confidence, learned payee, MCP suggestion; the source is the pill's
sub-label). Tap the pill = accept (haptic, row slides out); tap "Jiná…" = the picker of §8.4.
After an accept the row may expand once with "Zařadit i další 3 platby od Lidl" (switch, default
on when the matches are the same counterparty and direction). A row can be swiped away as
"Bez kategorie, nechat" (it leaves the queue for 30 days). When the queue is empty the screen
says "Všechno zařazeno" with one button, "Zobrazit cashflow". Rejected alternative: a card-swipe
deck (D4).

### 8.6 Statement import on a phone

The owner's requirement: a person must be able to import statements on a phone **without manual
column mapping**; the monthly import is what brings them back to the app. The design is a ladder:
every rung answers without asking when it can, and the next rung only appears when the previous
one could not decide. The target is ≤ 4 taps from the share sheet to "Hotovo" for a known file
and ≤ 8 taps, including one correction, for an unknown CSV.

**Inputs.** The import starts from: the bank app's share sheet (Moony is registered for the
statement document types, §5), the Files / SAF picker inside the sheet, an e-mail attachment
shared to Moony, an account's "Importovat výpis", the Údržba card, the monthly notification, the
app shortcut. A file shared into Moony while locked first unlocks (biometrics) and then opens the
import sheet with the file attached.

**Rung 1 — structured formats.** CAMT.053, MT940, OFX/QFX and QIF are self-describing and go
straight to the preview, with the account match, balance check and multi-statement choice the
desktop already has (2026-10-15 spec). Nothing new.

**Rung 2 — a known CSV.** `detect_preset` matches the header signature of the shipped presets
(16 today) and the person's own learned formats (rung 4). Straight to the preview. Two
additions:

- **Catalog refresh**: the preset catalog (the same JSON files as `src-tauri/resources/csv-presets/`)
  is also published as a versioned index on the project's GitHub; the app refreshes it at most
  once a day when online, the same trust model and switch as the desktop update check ("Stahovat
  nové formáty výpisů", on by default; the request sends nothing but the version). New banks
  reach every phone without an app release. The desktop gets the same refresh.
- **Presets carry a mobile export guide**: a `mobileExport` block per locale — the bank app's
  menu path in three steps and the format to pick ("Historie → Export → CSV → Sdílet → Moony"),
  and an `appUrlScheme` when the bank app has one, so "Otevřít aplikaci Fio" works. The guides
  are data, so the catalog refresh updates them too. Before implementation the preset banks'
  phone export paths are verified and written into the presets (T1 in §14); the design does not
  depend on the answer, but the answer decides how often rung 3 is hit.

**Rung 3 — guided recognition of an unknown CSV (new, v1).** The CSV reader already detects
delimiter, encoding and the header row. A new inference step assigns roles to columns from their
**content**, with header names only as a tie-breaker through a multilingual synonym list (Datum /
Date / Buchungstag / Data …; Částka / Amount / Betrag / Objem / Kwota …):

- *date*: ≥ 90 % of rows parse as one date pattern; the dmy/mdy ambiguity reuses the QIF
  date-order question;
- *amount*: a numeric column with a decimal comma or point and a sign, or a *debit + credit pair*
  where exactly one of the two is filled per row;
- *balance*: numeric and consistent with the amounts (balance[i] − balance[i−1] = amount[i] on
  most consecutive rows) — this also proves the sign convention, so "expenses negative" is
  verified, not assumed;
- *currency*: three-letter ISO codes with low cardinality; *IBAN*: the IBAN pattern;
  *transaction id*: unique per row; *counterparty*: text with moderate cardinality;
  *description*: the longest free-text columns, all of them.

The person never sees a grid. The sheet shows **"Takhle tomu rozumím"**: three real rows from the
file rendered as Moony transaction rows (date, counterparty, description, signed amount) and a
line "Rozpoznáno: datum, částka, popis, protistrana" with the unrecognized optional fields
greyed. Two buttons: **Vypadá to správně** (→ preview) and **Opravit**. Correction is per field:
tapping a field name (Datum, Částka, Popis, Protistrana, Měna, Zůstatek) opens a vertical list of
the file's columns, each with its header and two sample values; pick one or "nepoužít". Every
change re-renders the three sample rows. Confidence rules: when the date or the amount cannot be
inferred, the sheet opens directly on that field's correction; the app never guesses either
silently. A file that still has no date or amount after correction ends with "Tento soubor
neobsahuje výpis, který bych uměl přečíst" and the supported-format list.

**Rung 4 — learned formats.** A confirmed rung-3 result is saved as the person's own preset keyed
by the header signature (plus delimiter, encoding, date format, decimal separator), named after
the account's institution ("Formát: Raiffeisenbank"), stored like the stock import's saved formats
(`app_config`, max 20), listed in Settings → Import → Naučené formáty with rename and delete. Next
month the same bank is rung 2. The desktop reads and writes the same store, so a learned format
travels with the profile.

**Rung 5 — contribute the format.** After a successful import with a learned format, the done
step offers once: "Poslat tento formát autorům Moony, aby ho dostali všichni". It opens a
prefilled GitHub discussion in the browser containing only the header row, the mapping and the
bank name — no transactions, no balances, and the person sees the text before sending. The owner
turns it into a catalog preset (rung 2). This is how the catalog grows past the 16 banks without
the owner collecting sample files.

**Preview and done.** The preview keeps the desktop's guarantees in a phone layout: counts
("Naimportuje se 48 transakcí · 3 duplicity přeskočeny"), the period, the balance check sentence,
the account-match sentence with "Importovat do účtu X" when the file belongs elsewhere, the
rows with status chips and the category each would get; a category can be fixed inline through
the picker. The done step has one primary button, **Zařadit 12 plateb** (the queue of §8.5), and
the secondary rows the desktop has (Zobrazit importované, Zapnout balíček pravidel, Nastavit
zůstatek podle výpisu). Undo stays in Historie importů.

**What this does not solve, said plainly.** Some bank apps export only PDF from the phone. A
PDF statement reader is v2 research with per-bank templates for the largest Czech banks (pdf.js
exists on the desktop; the Rust side would need a text extractor); it is not promised. The only
way to remove the export step entirely is a bank connection through a PSD2 aggregator, which the
roadmap keeps "not before 1.0, optional, documented in PRIVACY.md"; it is out of this design's
scope. Until then the per-bank guide, the share sheet, the recognition ladder and the monthly
reminder make the ritual as short as a file-based import can be.

**Manual entry** stays for cash and for banks that export nothing: "Přidat transakci" is a sheet
with the amount keypad first (unit inside the field), direction segment, date defaulting to today,
counterparty and category; a long press on the tab-bar Účty icon on Android and the app shortcut
on both open it directly.

### 8.7 Cashflow (with budgets)

Nav bar title "Cashflow"; segment Měsíc · Čtvrtletí · Rok, under it ‹ Říjen 2026 ›; `···` holds
Účty (N) scope, **Spravovat rozpočty**, Export. Sections:

1. Hero **Úspora** ("+ 3 120 Kč", note "míra úspor 24 %"); two stats Příjmy / Výdaje with the
   monthly average note.
2. Twelve-month bars (income `s1`, expense `s3`, months outside the period muted). Tapping a bar
   switches the period to that month.
3. A one-line budget summary when the period is a month and limits exist: "Rozpočty: 5 v mezích ·
   1 nad limitem · 2 blíží se limitu".
4. **Výdaje podle kategorií**: row = category icon chip, name, count; trailing amount; under the
   row a bar that is the *share of spending* (as on the desktop) or, when the category has a
   limit in that period, the *limit progress* "1 250 / 3 000 Kč" tinted `loss` when over. Tap →
   the category's transactions (a pushed list with the swipe-to-categorize of §8.4; moved rows
   dim).
5. **Příjmy podle kategorií** and **Investice, spoření a převody**, same rows.
6. The report-quality note when FX is missing or a month is partial.

**Spravovat rozpočty** (pushed page): one row per category with a limit (icon, name, limit in
effect or "Naplánováno", validity and upcoming changes in the sub-line), then "Výdaje bez
limitu" rows with a Nastavit limit button, `+` Přidat rozpočet. A limit opens a sheet with
category, monthly limit (hint: the 6-month average), Od / Do months (Do optional, "Bez konce");
the overwrite warning and "Přepsat a uložit" second click as on the desktop. Row long press:
Upravit limit, Smazat interval (when more than one), Smazat celý rozpočet (confirm names that
past months go too).

### 8.8 Majetek, class pages and details

**Majetek**: hero **Aktiva** with a thin 100 % allocation bar (s1–s4 + Ostatní) and the legend;
then groups as rows: Investice (Akcie, Krypto, Dluhopisy, Ostatní aktiva), Nemovitosti, and a
last section Hotovost with one row "Na účtech" that opens the Finance tab, so the rows add up to
the hero. Each row: icon, name, trailing value and period change. Loans and policies are not
here (D10). Classes hidden by the menu preference are not listed; empty classes lead to their
empty state with the add button. Trailing `+`: the typed menu (§6).

**Akcie** (Krypto mirrors it): hero portfolio value with buy/sell marks and the period segment;
three stats: Nerealizovaný výnos, Dividendy 12 m, **Zhodnocení letos (TWR)**; segment *Pozice ·
Sledované*; positions as rows (logo, ticker + name, "ručně" badge; trailing value, under it
▲ 12,3 % · + 1 230 €); sort Podle hodnoty / výnosu / názvu; pull-down search; `···`:
Aktualizovat ceny (also pull-to-refresh), Importovat výpis brokera, Historie importů, Uzavřené
pozice, Export. *Sledované*: search field (Yahoo) on top, rows with native price, day change,
target state badge; tap → watch detail (price chart with the target line, Nastavit cíl sheet,
notes as a plain editor rendered as markdown in view mode, key data).

**Position detail**: title ticker, subtitle name; hero value with the dashed Vloženo line and
events (tap a mark → callout; the matching row flashes); stats 2 × 3 (Hodnota, Cena, Vloženo,
Nerealizovaný, Realizovaný, Dividendy); buttons **Koupit · Prodat**; `···` (Upravit cenu, Odhad
dividendy, Upravit název, Sledovat, Smazat); transactions with the Vše / Nákupy / Prodeje
segment; O společnosti as a key-value card at the end. Buy/Sell sheet: quantity, price
(prefilled with the current price), currency from the quote, date today.

**Dluhopisy**: three stats (Nominál, Roční kupón, Nejbližší splatnost); "Nejbližší kupóny a
splatnosti" as a dated list replacing the ladder chart; bond rows (name, ISIN; nominal × qty,
coupon %, maturity with "do roka" / "splaceno" badges); tap → edit sheet; matured behind a toggle
in `···`.

**Nemovitost**: hero valuation trace (purchase ▲, revaluations ○); stats (Hodnota, Kapitál / LTV,
Hrubý výnos, Čistý cashflow); **Přecenit** primary; stacked sections instead of tabs: Finance
(linked loans and policies as rows with Připojit, Roční bilance as a key-value card), Náklady
(recurring and one-time, with add), Galerie (grid, **Vyfotit** and Vybrat z fotek), Dokumenty
(rows, Vyfotit / Vybrat soubor; the encrypted viewer), Poznámky. `···`: Upravit, Historie
přecenění, Otevřít v kalkulačce (v2), Smazat.

**Ostatní aktivum**: a light detail (value, valuation trace, quantity and average price,
transactions) with Koupit · Prodat and Přecenit; the add sheet keeps the desktop fields with the
optional first purchase folded.

**Úvěr**: hero outstanding balance with the schedule extension and marks; stats 2 × 2 (Zaplacené
úroky, Splaceno z jistiny, Zbývá splátek, Konec fixace) plus Příští splátka in the hero note;
**Mimořádná splátka** primary → a sheet with amount, Zkrátit dobu / Snížit splátku, the live
result and "Zapsat jako provedenou"; "Příštích 12 splátek" as a list with "Po letech" behind a
segment; Události list; `···` (Upravit, Zadat zůstatek k datu, Zapsat změnu sazby, Smazat).
Warnings (no payment, payment below interest) are inline alerts under the hero.

**Pojistka**: facts first — a key-value card with Číslo smlouvy (copy button), Pojistitel, Typ,
Pojistné (amount · frequency), Příští platba, Konec smlouvy / výročí, linked property; then
**Limity krytí** rows; **Dokumenty** (Vyfotit / Vybrat soubor, the encrypted viewer); Poznámky;
the cumulative-premiums chart last. Buttons: Upravit; `···` (Ukončit / Obnovit, Smazat).

**Insurance list**: rows (type icon, policy name, insurer; trailing premium, next payment date),
"Nejbližší platby" as a dated list replacing the 12-month calendar chart, type filter and ended
toggle in `···`.

### 8.9 Nastavení (sheet from the avatar)

Sections, each a pushed page; the danger zone last:

- **Profil** — name, surname, e-mail, country.
- **Jazyk a měna** — language, main currency (36 with a rate source, as on the desktop).
- **Vzhled** — Systém / Světlý / Tmavý (shared with the desktop setting).
- **Zabezpečení** — biometric unlock switch, background lock (Ihned / 1 / 5 / 15 min), change
  password (with the new recovery key box), hide amounts on launch.
- **Záloha a přenos** — automatic encrypted backup (target folder, last backup, Zálohovat nyní),
  Obnovit ze zálohy, **Přenést profil z Moony na počítači** (restore from a desktop backup
  file; same migrations), Exportovat všechna data (JSON) via the share sheet.
- **Oznámení** — the ten milestone groups as switches (desktop mute list), the monthly upkeep
  reminder (day of month), deadline reminders (14 days and 1 day before).
- **Kategorie** — list with usage counts, add / edit / delete with reassignment (desktop dialog).
- **Kategorizace** — rule packs per country, the rules list (on/off, delete; no editor),
  learned payees with "Zapomenout".
- **Import** — learned statement formats, catalog refresh switch, CoinGecko key.
- **Připomínky** — insurance notice days, exclude personal real estate from overviews.
- **Viditelnost tříd** — the menu-preference flags.
- **O aplikaci**, **Smazat všechna data** (typed confirmation).

## 9. What the phone adds

- **Share sheet in and out**: statements and broker exports in; CSV and JSON exports out.
- **Camera** for documents and property photos (encrypted attachments already exist).
- **Biometrics** with the password as the master secret.
- **Local notifications** scheduled from the milestone computation on every unlock: T−14 and
  T−1 for deadlines, the monthly "Je čas naimportovat výpisy" with the bank guide, backup and
  stale balances. No server, no push. Watchlist targets are checked when the app opens; a
  background refresh is v2 and never promised.
- **Hide amounts** (eye), **privacy screen**, **background lock**.
- **App shortcuts** (long press on the icon) in v1; **widgets** in v2 (net worth with the hide
  option, next deadline).
- **Backup to the person's own cloud folder** as an encrypted blob; **profile transfer** between
  desktop and phone through that backup.

## 10. Visual language on a phone

### 10.1 One design system, two layers

The design system stays one: `docs/design-system/` with `tokens.css` (light block and the dark
block defined by the desktop dark-mode project, same variable names) and `moony.css` for the
desktop components. The mobile app adds `mobile.css` next to it — the mobile component contract
(type scale, entity row, tab bar, sheet, stat row, hero at phone size) in plain CSS, and a §13
"Mobile" in the README. Components on both apps use tokens only; a token never gets a
platform-specific name.

### 10.2 Type scale (phone)

Inter Variable, tabular numerals on every number. Dynamic Type / font scale: the text roles
scale with the system setting up to 130 %; display and stat values up to 110 %.

| Role | Size / weight / tracking | Token |
|---|---|---|
| Display (hero number) | 36 / 580 / −0.06em | `m-fs-display` |
| Large title | 30 / 600 / −0.05em | `m-fs-title-lg` |
| Title (nav bar collapsed, sheet title) | 20 / 650 / −0.04em | `m-fs-title` |
| Section | 16 / 620 / −0.03em | `m-fs-section` |
| Stat value | 17 / 600 / −0.03em | `m-fs-stat` |
| Row title | 15 / 600 / −0.01em | `m-fs-row` |
| Body | 15 / 450, line 1.45 | `m-fs-body` |
| Row sub, badge | 12 / 500 | `m-fs-sub` |
| Caption | 11 / 500, ink-3 | `m-fs-caption` |
| Eyebrow | 10 / 700 / +0.12em uppercase | `fs-eyebrow` |

### 10.3 Components

- **Entity row** (`.row`): leading chip 36 pt (logo, initials or category icon in a tinted
  circle), title, sub, trailing value, trailing sub (change badge or caption). Height 60 pt
  (52 single-line). Dividers are `line-soft`, inset to the title's left edge. Whole row tappable,
  swipe actions on both sides, long press opens the context menu. A disclosure chevron appears
  only on rows without a trailing value (settings, documents); a value row is tappable as a whole
  and keeps that room for its name (at 390 pt a chevron costs a 16-character name its ending). Accounts, positions, bonds,
  properties, loans, policies, transactions, categories, watchlist rows and agenda rows are all
  this row.
- **Card**: `paper`, radius 15, shadow `sh-1` (one step lighter than the desktop card), 16 pt
  gutters; the canvas stays visible between cards. Fewer cards than the desktop: a section is
  often a title and rows directly on the canvas.
- **Hero**: label, display number, change line, period segment, chart. The decorative circle of
  the desktop hero is kept at 60 % size.
- **Stat row** (`.stats-3`): three stats in one row, value 17, label 11, note 10; a long value
  shrinks with the container as on the desktop.
- **Tab bar**: `paper`, hairline `line` on top, 56 pt plus the safe area, Lucide 24 pt stroke
  1.75, active icon and label `ink`, inactive `ink-4`, label 10 / 600. No dark material in the tab
  bar (D5).
- **Nav bar**: large title that collapses to the 20 pt title on scroll; buttons are 44 pt hit
  areas with 24 pt icons; at most one leading and two trailing.
- **Buttons**: primary dark material 48 pt radius 10, label 15 / 600; secondary `paper` with
  `line-strong`; danger text-only; destructive solid only in confirm sheets; full width in sheets
  and under heroes, paired side by side when two.
- **Sheet**: `paper`, top radius 17, drag handle, title 20, one sentence, body, footer on `well`
  with Zrušit left and the primary right; keyboard pushes the footer; Android back closes it.
- **Segmented control**: 36 pt, as on the desktop; scrolls horizontally when it does not fit.
- **Badges, chips, progress, alert, empty state, skeleton, toast**: the desktop rules; toast at
  the bottom above the tab bar, 2.6 s, past-tense verb.
- **Context menu**: the platform's long-press menu styled with the design-system menu (E4).
- **Touch targets** ≥ 44 pt everywhere; side gutter 16 pt; vertical rhythm on the 4 pt scale.

### 10.4 Charts on touch

Same rules as desktop §8 and §9 with these substitutions: hover → scrub (crosshair, value and
date rewrite in the hero, haptic tick per day, snap back on release); tooltip → callout on tap
(marks have 28 pt hit areas); five axis labels → four; chart heights hero 160, trend 120, bars
110, ring 110; the period segment sits between the number and the chart. Calendar-type charts
(bond ladder, insurance 12-month calendar) are replaced by dated lists; the loan trajectory and
the real estate value-vs-equity trends stay. A chart never needs a pinch.

### 10.5 Forms

Short forms are half sheets, long ones full-height. **Required fields first, the rest under
"Další údaje"**: Přidat účet shows name, bank, currency, balance; IBAN, type, interest and the
net-worth switch fold. Smart defaults: date today, price from the current quote, currency from
the account or the quote. Amount fields use the decimal keypad with the unit inside the field and
live thousands grouping. Dates use an inline compact picker. Destructive actions are confirm
sheets with the solid destructive button. Validation errors replace the hint under the field, as
on the desktop.

### 10.6 Dark mode

Both apps ship light and dark from one token set; the phone follows the system setting by
default with the same Systém / Světlý / Tmavý choice as the desktop. The dark values, chart
validation and material rules are defined in `2026-10-16-dark-mode-design.md`; this spec only
requires that every mobile component uses tokens so the switch is free. The prototype shows one
dark screen once that token block exists.

## 11. Data and sync readiness

- Same SQLite schema, same migrations, same SQLCipher encryption, same `attachments.db`. The
  profile folder layout is identical, so a desktop backup zip restores on a phone and vice versa.
- Learned statement formats, categories, rules and milestone states live in the database, so
  they travel with the profile.
- The future sync service syncs one schema; nothing in this design adds phone-only tables except
  the queue's "leave uncategorized for 30 days" mark (`bank_transactions.categorize_snoozed_until`,
  nullable) and notification preferences in `app_config`.

## 12. Technical direction (decided in the plan, recommended here)

Tauri 2 Mobile with the existing Rust core: services, SQLCipher, migrations, importers, FX and
the history rebuild run unchanged on iOS and Android; the mobile UI is a second React app that
shares `shared/schema.ts`, the `tauri-api.ts` wrappers, the i18n JSON and the formatting utils.
One web UI renders identically on WKWebView and Android WebView, which is exactly ruling (1).
Platform pieces (share-in, document picker, biometrics, notifications, shortcuts, privacy screen)
are Tauri plugins, several of which exist. Repo layout to decide in the plan: `apps/mobile` in
this repository with the services extracted into a `moony-core` crate, or a separate repository
depending on a published crate; the recommendation is the monorepo so one change keeps both apps
in step. The alternative, native SwiftUI and Jetpack Compose over the Rust core through UniFFI,
gives the best platform feel at roughly twice the UI work and two UIs to keep identical by hand.

## 13. Phases

- **v1**: lock and biometrics, first run, Přehled with Údržba and notifications, Účty with
  detail, ledger, categorize queue and the import ladder (rungs 1–5), Cashflow with budgets,
  Majetek with every class and detail including camera documents, watchlist, Settings with
  backup and profile transfer, hide amounts, privacy screen, light and dark.
- **v2**: Projekce, Plánování cashflow, Analýza lite, kalkulačky, custom broker formats, widgets,
  watchlist targets in the background, tag management, PDF statement research, tablet layout.
- **Never on the phone**: MCP server, chart drag-range, the regex rule editor, the updater.

## 14. Decisions and tasks

- **D1** Both platforms, one visual language, no platform chrome (owner, 2026-10-16).
- **D2** Four tabs plus avatar; no "More" tab; Budgets merged into Cashflow; Watchlist inside
  Akcie; Settings behind the avatar.
- **D3** Budget limits render as the progress bar of the Cashflow category row; management is a
  pushed page from `···`.
- **D4** The categorize queue is a list with suggestion pills, not a card deck.
- **D5** The tab bar uses ink on paper; dark material stays reserved for the primary button.
- **D6** Hide-amounts toggle only in the Přehled nav bar plus "hide on launch"; state is global.
- **D7** The first run asks for biometrics and for backup, backup recommended on.
- **D8** Import ladder: structured → preset (with catalog refresh and export guides) → guided
  recognition → learned format → contributed format. Manual grid mapping never appears on the
  phone. (Replaces the earlier "mapping in v2".)
- **D9** Dark mode on both apps from one token block; the desktop project defines it.
- **D10** (revised the same day, owner) The second tab is **Finance**: bank accounts, loans and
  insurance policies — contracts with institutions, not wealth. The fourth tab "Majetek" holds
  assets only (investments, real estate, other assets, watchlist) plus a cash row linking to
  Finance. The earlier version had loans and policies under Majetek.
- **T1** Before implementation: verify the phone export paths and formats of the 16 preset banks
  and write them into the presets' `mobileExport` blocks; add banks the owner's users ask for.
- **T2** Decide the repository layout and the plugin set in the implementation plan.
- **T3** The dark token block arrives from the desktop project (spec approved 2026-10-16); the
  prototype's dark frame uses its values. The mobile app reads `tokens.css`, never a copy.

## 15. Prototype

`2026-10-16-mobile-app-prototype.html`, the convention of the three earlier prototypes: one file
on `../design-system/moony.css` (tokens included), a bar with a notes toggle, 20 numbered notes.
Frames at 390 × 844 side by side; the content inside a frame scrolls. Pass 1 (the core jobs,
approved by the owner in chat on 2026-10-16): 01 Zámek, 02 Přehled, 03 Účty, 04 Detail účtu,
05 Zařazení plateb, 06 Import · rozpoznání, 07 Import · oprava pole, 08 Import · hotovo,
09 Cashflow. Pass 2 (written the same day): 10 Majetek, 11 Akcie, 12 Detail pozice, 13 Úvěr,
14 Pojistka, 15 Nastavení, and 16 Přehled · tmavý: frame 02 unchanged except the
`data-theme="dark"` attribute, with a prototype copy of the shared dark token block from
`2026-10-16-dark-mode-design.md` §4.1 (approved by the owner on 2026-10-16; `tokens.css` becomes
the only source once that project lands, T3 done). The prototype's sample data is one consistent
profile (net worth 4 356 920 Kč = assets 6 775 680 − loans 2 418 760) so numbers agree across
frames.
