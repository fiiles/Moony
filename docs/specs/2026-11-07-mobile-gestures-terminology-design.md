# Phone: one gesture rule and the desktop's words

Date: 2026-11-07 · Audit items C2 and D (`docs/audits/2026-11-06-mobile-ux-audit.md`), work order
item 6 · Mobile spec principles 2 ("Fingers, not a pointer") and 6 ("The same language as the
desktop") · Reference: the desktop (owner, 2026-11-06) · The owner delegated the decisions; the
calls marked **(call)** are mine.

## Why

- **C2.** Swipe left means something different on every screen: Kategorie in the ledger and the
  Cashflow breakdown, Nechat in the categorize queue (no way back), Skrýt in Co vás čeká, Smazat
  in Rozpočty. Destructive panels look like the harmless ones. The main rows (accounts, loans,
  policies, positions, coins, watched tickers, properties, other assets) have neither a swipe nor
  a long press. Where a long press exists, its `···` trigger is screen-reader only
  (`RowMenu.tsx:42` and five copies), so a sighted person cannot find it — and for some rows the
  menu is the only way to an action (deleting a loan event, a bond, a budget).
- **D.** The phone says "Pohyby", "Volné peníze", "Aktiva", "za 3 měsíce", "Dividendy / rok",
  "ID banky" where the desktop says "Transakce", "Volné prostředky", "Celková aktiva", "za
  posledních 90 dní", "Odhad dividend", "ID bankovní transakce". Inside the phone the same act has
  two names (Vymazat/Smazat hledání, Zrušit filtr/filtry, Odstranit/Smazat). Some confirm titles
  lack their "?". One sentence shared with the desktop reads "…transakci typu koupit…".

## Goals

1. One swipe rule, written in the design system (README §13) and applied to every swiping row.
2. Every row with a menu: a long press, and a visible `···` wherever the menu holds an action that
   nothing else on the screen (or behind the row's tap) offers.
3. Main rows get a long-press menu with their detail's key actions.
4. Destructive swipe panels in `loss`, never firing a delete without a confirm sheet.
5. The phone uses the desktop's words for every pair of the audit's D table; the phone's own
   inconsistencies are gone; where the desktop disagrees with itself, one word wins in both shells,
   in all ten locales.

## Non-goals

- New gestures (pull-to-refresh is the finance branch's; edge swipe back exists).
- Swipe actions on the main rows (see the calls).
- Translation quality review of untouched strings.
- Desktop UI changes beyond the shared strings.

## The gesture rule

| Gesture | Meaning | Where |
|---|---|---|
| Tap | Opens the row (its detail, sheet or the row's own primary act). | Every interactive row. |
| Swipe right (panel on the left) | Look closer or later: **Podrobnosti**, **Upravit**, **Připomenout později**. Never destructive. | Payments (Podrobnosti), Rozpočty (Upravit limit), Co vás čeká (Připomenout). |
| Swipe left (panel on the right) | Deal with the row. On a **payment** (ledger, Cashflow breakdown, categorize queue) it is always **Kategorie**. Elsewhere it takes the row out of the list: a **reversible** removal (Skrýt) fires at once with a "Vrátit" toast on dark material; a **destructive** one (Smazat, Zapomenout) has a `loss` panel and opens the confirm sheet. | Payments, Co vás čeká, Rozpočty, pravidla, naučení příjemci, kategorie. |
| Long press (or right click) | The row's menu: every action of the row, swipes included. | Every row with actions. |
| `···` | The menu's trigger. Visible (44 pt, `ink-4`, at the row's end) when the menu holds an action that is neither on the row nor behind its tap; otherwise screen-reader and keyboard only. | See the table below. |

A swipe fires when released past 60 px, as today; what makes it safe is that nothing destructive
happens without the confirm sheet, and the one instant removal (Skrýt) has its undo.

Rows and their `···`:

| Row | Tap | Menu | `···` visible |
|---|---|---|---|
| Ledger, Cashflow breakdown | Transaction sheet (category, delete) | Podrobnosti, Kategorie (+ Smazat) | no |
| Categorize queue | Category picker | Podrobnosti, Kategorie, Nechat | yes (Podrobnosti, and Nechat on a suggested row, are nowhere else) |
| Co vás čeká | Source page | Skrýt, Připomenout, Nepřipomínat… | yes |
| Rozpočty, pravidla, naučení příjemci, kategorie | Edit / nothing | …, Smazat / Zapomenout | yes |
| Loan events, bonds, trades, dividends, documents, costs, import history, linked loans and policies of a property | Menu / unfold / viewer / edit | Upravit, Smazat, Odpojit… | yes |
| Main rows (accounts, loans, policies, positions, coins, watched, properties, other assets) | Detail | The detail's key actions | no (all of them are visible in the detail) |
| Cashflow category, spending plan | Its page | Open, limit / edit | no |

Main-row menus (the detail's key, non-destructive actions):

| Row | Menu |
|---|---|
| Account | Zaznamenat zůstatek, Importovat výpis, Přidat transakci |
| Loan | Zadat zůstatek k datu, Zapsat změnu sazby |
| Policy | Upravit pojistku, Ukončit pojistku (a running one) |
| Stock position, coin | Koupit, Prodat (while something is held) |
| Watched ticker | Nastavit / Upravit cíl, Koupit |
| Property | Přecenit |
| Other asset | Koupit, Prodat (while held), Přecenit |

An item opens the detail with that act already open: the push carries an **intent** in the history
state (`mobile/lib/row-intent.ts`), the detail consumes it once (`useRowIntent`) and drops it from
the entry, so back and forward never reopen it. "Importovat výpis" opens the global import sheet in
place.

## Terminology

| Pair (audit D) | Phone becomes | How |
|---|---|---|
| Ledger item | "Transakce", "N transakcí", "Žádné transakce neodpovídají", "Transakce se nepodařilo načíst." | desktop keys `bank_accounts:transactions`, `ledger.footer_*`; mobile empty/error texts reworded in every locale |
| Nezařazené | "Nezařazené transakce" (Finance row); the per-row badge stays "Bez kategorie" (the desktop's import preview word for one row) | `bank_accounts:metrics.uncategorized`; `reports:actuals.uncategorized` = `budgeting:uncategorized` in every locale |
| Volná hotovost | "Volné prostředky · na bankovních účtech" | `dashboard:stats.freeCash`, `freeCashNote` |
| Aktiva | "Celková aktiva" | `dashboard:stats.totalAssets` |
| Change periods | "za posledních 30 dní", "za posledních 90 dní", "od začátku roku", "za poslední rok" | `dashboard:changePeriod.*` (mobile keys removed) |
| Správa limitů | "Spravovat rozpočty" in both shells | `budgeting:manage.title` (the reports key removed) |
| Stav limitu | already the desktop's keys (Cashflow parity) | — |
| Dividends | "Odhad dividend" in both shells | `stocks:summary.estimatedDividend`; `stocks:detail.dividends` reworded |
| Manual price | "Zadat cenu ručně" (stocks) beside "Zadat kurz ručně" (crypto), both shells | `stocks:detail.menu.editPrice` reworded and used by the desktop table |
| Transaction technical fields | "ID bankovní transakce", "Datum valuty", "Původní soubor", "Řádek souboru"; the bank's text is "Popis", not "Poznámka" | `bank_accounts:readiness.detail.*`, `transaction.description` |

Inside the phone: "Vymazat hledání" everywhere (`common:a11y.clearSearch`); "Zrušit filtr"
(`common:filterBadge.clear`, the app calls the whole set "Filtr"); "Zavřít" on every sheet that only
closes (the transaction sheet), "Hotovo" only where it commits a draft (Účty v přehledu); "Smazat
událost" (both shells); a detail's `···` names its entity ("Upravit úvěr", "Upravit pojistku",
"Upravit nemovitost", "Upravit aktivum", "Smazat aktivum"), a row menu uses the bare verb; every
confirm title is a question; the stocks summary flips "Nerealizovaný výnos / ztráta" as crypto and
the desktop do; the trade delete sentence names the trade ("Tím trvale smažete nákup 10 × AAPL …")
in both shells.

## Calls

- **(call)** Payments: swipe left = Kategorie also in the queue, so "Nechat" leaves the swipe. It
  stays as the pill of a row without a suggestion and in the menu (whose `···` is now visible). If
  wrong: a person used to swiping a payment away has to tap twice. The finance branch's undo toast
  for Nechat is untouched by this.
- **(call)** The queue's tap stays the category picker (its job); swipe left does the same. If wrong:
  one redundant gesture, nothing lost.
- **(call)** Main rows get no swipe: two swipe actions per row are not enough for an account or a
  position, and the detail is one tap away. If wrong: an extra gesture can be added without
  changing the rule.
- **(call)** Main-row menus hold no Smazat: a delete from a list would need its own confirm and
  leave-screen handling per class, and the detail keeps it one tap away. If wrong: a person has to
  open the detail to delete.
- **(call)** Visible `···` only where the menu is the only way to an action; on a row whose tap
  opens everything, the trigger stays for screen readers, so the account and position lists keep
  the room the value needs (README §13: a value row keeps the chevron's room for its name). If
  wrong: a sighted person does not learn the long press shortcut on main rows; nothing becomes
  unreachable.
- **(call)** Skrýt (Co vás čeká) is a reversible removal: dark panel, instant, "Vrátit" toast (it
  already has one). If wrong: a stray swipe hides an item for a few seconds.
- **(call)** "Hotovo" stays on Účty v přehledu: it commits a draft, it does not just close.
- **(call)** "Bez kategorie" stays the badge of one uncategorized row: "Nezařazené" is the plural of
  a filter and reads wrong on one row; the desktop's import preview uses "Bez kategorie" per row.
- **(call)** Manual price keeps each class's noun: shares have a "cena", crypto has a "kurz" in
  every desktop string of the class; the verb is now the same.
- **(call)** "Spravovat rozpočty" over "Spravovat limity": the dialog, the phone page and the help
  text already say it.
- **(call)** "Pohyb" stays where the desktop says it (import wizard, statement checks, "Poslední
  pohyb" sort): the audit's pair is the ledger.
