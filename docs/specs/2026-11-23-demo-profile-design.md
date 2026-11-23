# Demo profile on first run

Draft of 2026-11-23, awaiting the owner's verdict. Not approved, nothing is built.
Prototype: `2026-11-23-demo-profile-prototype.html` (Czech, clickable, light and dark).

## Intent

Someone who has just downloaded Moony has to create a password, save a recovery key and enter
their own money before they see what the app does. A demo profile lets them look first and
decide later. The owner asked for three things: the choice on first run, a clear mark that the
data is synthetic while the person explores, and a way from the demo to the real setup wizard.

## Goals, non-goals, anti-goals

- Goals: one extra screen on first run; the demo opens with no password and no questions;
  the person can change anything in it; an own profile is one click away from anywhere.
- Non-goals (this round): reaching the demo after an own profile exists; a guided tour;
  carrying demo data over into the own profile.
- Anti-goals: a person mistaking synthetic numbers for their own; real data entered in the
  demo and lost without warning; synthetic data leaving the device (sync, backup, MCP).

## Approaches

| | How | Trade-off |
|---|---|---|
| A (recommended) | Generate the demo on first open, in Rust, from the logic of `src-tauri/src/bin/seed_demo.rs`, into its own folder next to the real profile's | History always ends today, follows the current schema, installer does not grow; takes a second or two on first open |
| B | Ship a ready encrypted database as a bundle resource | Opens instantly; dates go stale, every schema change needs it rebuilt and migrated, larger installers on every platform |
| C | Seed the demo into the real profile and convert it later | No second folder; mixes synthetic and real data, which is the main anti-goal |

"Bundled" in approach A means the generator ships inside the app; no data file ships.

## Design (as in the prototype)

1. **First run.** After the brand intro comes "Jak chcete začít?" with two equal cards:
   "Založit vlastní profil" (primary button, leads to the unchanged five-step wizard) and
   "Vyzkoušet na ukázkových datech" (secondary). "Připojit k synchronizaci" and "Obnovit ze
   zálohy" move here from the wizard's first step.
2. **Marking.** One motif: a dashed outline means "sample, not yours" (badge, sidebar card,
   avatar). No new color. A card above the account row with "Založit vlastní profil", plus
   an "Ukázková data" badge in the top bar that explains itself on click (owner's choice,
   2026-11-23; a strip across the top of the content was rejected). The account row reads
   "Ukázkový profil".
   A one-time, dismissable welcome card on the Overview points to three strong places.
3. **Profile menu.** Adds "Založit vlastní profil" and "Obnovit původní data"; "Ukončit
   ukázku" replaces "Zamknout aplikaci" (the demo has no password).
4. **Settings.** A new first section "Ukázka" with the three actions. Security, data and
   backups, sync and the AI assistant are visible but locked, each with one sentence and the
   way to an own profile. General and categorization work as usual.
5. **To an own profile.** No confirmation dialog: the button opens the wizard at step 1,
   whose back button reads "Zpět do ukázky". The demo is deleted when step 3 creates the
   real profile.
6. **Next launch.** While no own profile exists, every launch starts on the choice screen;
   the demo card reads "Pokračovat v ukázce" and offers to start over.

Resetting the demo is the only action with a confirmation, because it is the only one that
discards something.

## Calls made without the owner (to confirm)

1. Own profile is the primary button, the demo the secondary one.
2. Next launch shows the choice screen, not the demo directly.
3. Changes in the demo persist until "Obnovit původní data".
4. The demo is gone for good once an own profile exists.
5. The "already use Moony" links move from wizard step 1 to the choice screen.
6. Sync, backup and restore, password, biometrics and the MCP server are off in the demo.

## Decided by the owner (2026-11-23)

- Marking: the sidebar card and the top-bar badge.
- One universal persona for everyone: **Alex Meyer**, all data in **EUR**. The language of the
  demo profile is the one chosen in the language selector on the choice screen.

## Open before the data is generated

- Prices: real tickers with fetched prices (network calls before any consent screen), or
  frozen synthetic prices that work offline?
- Phone: the same choice in the phone's first run now, or after the desktop?

## Left to the build

Backend state for "a demo exists" next to `needs_setup`, the demo folder and its fixed key,
commands to open, reset and close the demo, the gates that switch off the features in call 6,
all copy in ten locales, tests for the generator. The data set itself (which accounts,
positions, loans, how many years) gets its own short design once the above is agreed.

## Data set: Alex Meyer (designed 2026-11-23)

Assumed without the owner's answer, because the question was left open: real tickers with
prices the app may refresh online, and built-in starting prices so everything works offline.

Everything is generated relative to the day the demo is opened ("today"); `Mn` means n months
before today. Money is EUR unless said otherwise. All free text (account, property, loan,
policy, asset, plan and tag names, payment descriptions) is written in the chosen language;
merchant and company names are the same everywhere. System categories are not renamed, so the
app translates them.

**Person.** Alex Meyer, employee of the fictional Northwind Labs, owns a flat with a mortgage,
drives an older car, invests monthly. Four years of bank history, five years of property and
loan history.

**Accounts (3).** Current account (EUR, salary and all spending). Savings account (EUR, 2.6 %,
rate valid for about two more months, so a deadline shows; it funds the car, the parking space,
the gold, the bond and every single-stock and crypto purchase). Travel account at Wise (USD, used
on two trips). Month-end balance records for all four years, consistent with the payments.

**Payments (about 45 a month, roughly 2 200 in total), all categorized.**
- Income: salary on the 1st rising yearly (3 350 → 3 550 → 3 850 → 4 100), a bonus every
  March, a tax refund every May, 95 a month rent from the parking space, savings interest.
- Fixed: mortgage 791, kitchen loan 213 (from M20), building fee 185, electricity and heating
  with a winter peak, internet, phone, streaming, gym, transit pass, life insurance monthly,
  home, car, liability and travel insurance yearly, property tax yearly.
- Variable: groceries, dining, fuel, shopping, pharmacy and dentist, culture, rides; December
  gifts; two trips a year (flights a month ahead, lodging, USD card spending on the long trip).
- Transfers: standing order to savings, monthly deposit to the broker, occasional crypto
  purchases, top-ups of the USD account, one extra mortgage repayment of 5 000 at M18.

**Budgets.** Monthly limits for groceries, dining, shopping, entertainment and transport, with
one raise a year ago so past months keep their old limit.

**Spending plans.** This year: Holiday, Christmas and gifts (600), Car. Last year: Holiday
(closed). Amounts are sized from the tagged payments so the plans end up mostly used, not over. Payments are tagged to them.

**Cashflow planning.** Salary, groceries and subscriptions as manual items; loans, insurance,
property costs, rent, interest and dividends come from their sources automatically.

**Stocks (fee 1 per trade).** Vanguard FTSE All-World (VWCE.DE, monthly purchases for 46
months), Microsoft, Apple, ASML, Allianz, NVIDIA (bought before the 2024 split, part sold at a
gain), PayPal (bought and sold at a loss, a closed position). Dividends where the company
pays them. One tag group "Strategy" with Core, Growth and Dividend. Watchlist: Alphabet,
Amazon, Siemens, Novo Nordisk, two with target prices.

**Crypto.** Bitcoin (three purchases), Ethereum (two purchases, one partial sale); USD.

**Bonds.** One euro government bond, 6 × 1 000, 2.9 %, maturing in about four and a half years.

**Property.** The flat (bought M60 for 248 000, six valuations up to 312 000, building fee and
property tax as recurring costs, the kitchen as a one-time cost, purchase contract attached).
A parking space as an investment property (bought M33 for 21 500, rent 95 a month).

**Loans.** Mortgage 165 000 from M60 at 3.1 %, fixed for 14 more months, linked to the flat,
with the extra repayment. Kitchen loan 9 000 from M20 at 6.4 % for four years.

**Insurance.** Home (linked to the flat), life, car (anniversary within weeks, so a deadline
shows), liability, travel. One policy document attached.

**Other assets.** Volkswagen Golf (bought about three years ago, on the day its insurance
anniversary falls, yearly valuations going down), two one-ounce
gold coins (bought M25, valuations going up).

**History.** Net worth for every day of the five years, derived by the app's own rules from
the dated facts above and from built-in daily prices; USD/EUR rates built in for the same span.

**Not in this round.** Photos of the property (a generated placeholder would look fake; real
photos would have to be supplied), statement import batches, custom categories and rules.
