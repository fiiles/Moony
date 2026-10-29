# Spending plans (Výdajové plány) — design

Date: 2026-10-29. Status: approved by the owner in conversation (principles, then the static
prototype `docs/specs/2026-10-29-spending-plans-prototype.html`, which is the visual reference
for every screen below). Desktop only.

## 1. Why

A friend of the owner budgets per year: at the start of the year the family decides how much
goes to what — 20 or more named pots such as "Dovolená Jeseníky", "Fyzioterapie", "Dárky
Vánoce" — and spends from them through the year. Moony today budgets per category (monthly
limits in Cashflow), which cannot answer "I said the holiday would cost 35 000 Kč; how much did
it cost, and on what (transport, food, lodging)?". A holiday cuts across categories and
accounts.

A **spending plan** (Czech UI: *výdajový plán*) is a named intention to spend a planned amount
in one year. Payments are tagged with a plan **next to** their category. The plan shows planned
vs. spent, what is left, and the spend broken down by category and by month.

The friend's annual envelopes are one use case; the feature must be general (one-off trips,
ongoing care, gifts, a car's yearly service).

## 2. Goals

- Create, edit, close/reopen and delete plans; tens of plans per year.
- Tag payments with a plan by hand: several at once in the account ledger, one by one in the
  ledger, or from a plan's detail through a cross-account picker.
- Overview of a year's plans: how they are doing, which are over or close to their amount.
- Plan detail: planned / spent / left, spend per month, spend per category, the payments.
- Start a new year by taking over last year's plans (names, amounts, look).

## 3. Non-goals

- No automatic assignment: no rules, no learning, no suggestions. Everything is manual.
- No splitting a payment between plans; a payment belongs to at most one plan.
- No link to money on accounts: a plan is not a balance, nothing is reserved or moved.
- No rollover: an unspent or overspent amount is information only; nothing carries into the
  next year.
- No link to category limits (Cashflow budgets). The two lenses coexist and do not interact.
- No mobile UI in this version (the shared Rust backend and migration land on mobile too,
  untouched by its UI).
- No MCP tools for plans in this version (follow-up).
- No plan column in the Cashflow drilldown or other payment lists besides the account ledger.

## 4. Anti-goals (would make it a failure even if it works)

- A plan changing a payment's category, or a category change touching the plan.
- A converted total presented without its currency, or a foreign payment counted 1:1.
- Plans and category budgets that look like the same thing.

## 5. Owner decisions (as decided in the conversation)

| # | Decision |
|---|---|
| O1 | Name: **Výdajové plány** (en "Spending plans"); one plan = "výdajový plán". |
| O2 | A plan belongs to one year. Plans can be created, edited and deleted any time during the year. |
| O3 | Leftover / overspend is informational only; it is a label, nothing transfers. |
| O4 | Last year's and this year's "Fyzioterapie" are two separate plans. Take-over of last year's plans copies them. |
| O5 | A plan can be **closed**: it is no longer offered when assigning payments; it can be **reopened**. The overview distinguishes active and closed plans. |
| O6 | Assignment is purely manual; the plan is another dimension of a payment and never influences categorization. |
| O7 | Multi-select in the payment list (where categorization happens) to add several payments to a plan at once. |
| O8 | Not tied to money on accounts; where the money comes from is irrelevant. |
| O9 | Overview first answers: how are the active plans doing, is any at risk of overspending. Layout A: summary + one table ordered by risk; a free-text search ("dovo" shows only the holidays). |
| O10 | Detail: chart of spend per month above the two widgets "Kategorie" (left) and "Platby" (right); widgets narrowed by a filter carry the dark filter badge of Analýza akcií (`FilterBadge`). |
| O11 | Desktop only for now. |
| O12 | Plans have an **icon and a color**, like categories, chosen from **one shared icon set** (the category set plus eight new icons, available to categories too). |
| O13 | Detail actions: Upravit, Uzavřít/Obnovit plán and **Smazat plán as a separate button** — no ··· menu. |
| O14 | The detail's category widget is called **Kategorie** and shows each category's icon and color. |
| O15 | The detail's payment list looks like the account ledger today: no colored category chips (the category shows its icon in ink, as the ledger's category button does). |
| O16 | "Přidat platby" in the detail opens a picker over **all accounts** (accounts can be several). |

Accepted proposals (made by me in the conversation, confirmed by the owner's "takhle ok"):

| # | Decision |
|---|---|
| P1 | One plan per payment (nullable plan id on the payment). |
| P2 | An incoming payment in a plan (a refund, a friend's share) reduces the plan's spend: spend is **net** (debits minus credits). |
| P3 | One planned amount per plan; the category breakdown is actual spend only. |
| P4 | A plan has a currency, default the main currency; payments in another currency are converted at the rate of their booking day (ADR 0012). |
| P5 | A payment counts towards the plan it is tagged with regardless of its date (the February balance of a holiday can sit in last year's plan). |
| P6 | Deleting a plan only removes the plan from its payments; payments and categories stay. |
| P7 | Thresholds: **over** = spent > planned; **near** = spent ≥ 90 % of planned and not over. No pace / time-based warnings (one-off spending is lumpy). |
| P8 | Sidebar item **Výdajové plány** in the Peníze group after Cashflow; year stepper in the page head. |

## 6. Data model

New migration `026_spending_plans` (next free number at the time of writing; the builder takes
whatever is next).

```sql
CREATE TABLE spending_plans (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    year INTEGER NOT NULL,
    amount TEXT NOT NULL,          -- planned amount, decimal TEXT, > 0
    currency TEXT NOT NULL,        -- the plan's currency (P4)
    icon TEXT NOT NULL,            -- key of the shared icon set
    color TEXT NOT NULL,           -- #RRGGBB
    note TEXT,
    closed_at INTEGER,             -- NULL = active; unix epoch when closed (O5)
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_spending_plans_year ON spending_plans(year);

ALTER TABLE bank_transactions ADD COLUMN spending_plan_id TEXT
    REFERENCES spending_plans(id) ON DELETE SET NULL;
CREATE INDEX idx_bank_transactions_spending_plan ON bank_transactions(spending_plan_id);
```

- Name uniqueness: a plan name is unique within a year, compared case-insensitively with
  Unicode lowercasing **in the service** (SQLite `NOCASE` is ASCII only). Violation:
  `validation.spendingPlanNameTaken`.
- Deleting a plan sets `spending_plan_id = NULL` on its payments explicitly in the same
  transaction (do not rely on the FK action alone).
- Pending → booked promotion is an `UPDATE` of the same row (`promote_pending_transaction`), so
  the plan survives it. Deleting an import batch deletes its rows and with them their tags.
- Regenerate the schema snapshot; add the table to `docs/architecture/database.md` and the
  domain map in `docs/architecture/overview.md`.
- The JSON export dumps all tables already; nothing to do. The local anonymizer
  (`src-tauri/src/bin/local_anonymize_profile.rs`) must scrub plan names and notes and scale
  `spending_plans.amount` like other money columns.

## 7. Money semantics

- **Spent** of a plan = Σ over its booked payments of (debit amount − credit amount), each
  converted from the payment's currency into the **plan currency** at the payment's booking day
  via `fx::History::convert_on_tracked` (ADR 0012). Same currency is identity.
- Pending payments may be tagged and are listed, but are **not counted** (booked only, as in
  Cashflow); the ledger badge "Čekající" already marks them.
- A payment that cannot be converted is left out of the sums and named in the response's
  `fxQuality` (`missingCurrencies`, `missingCount`). Never 0, never 1:1.
- Monthly series and the category breakdown use the same converted values, bucketed by the
  booking day's UTC month; months outside the plan year appear when a payment falls there (P5).
- The overview's totals (planned, spent, left in active plans) sum plans of possibly different
  currencies: each plan's planned and spent are converted from the plan currency into the
  **report currency** (the main currency, passed by the frontend) with the current snapshot
  (`fx::current()`); misses go to `fxQuality`. Per-plan values stay in the plan currency.
- Converted wire amounts are `Option<String>` rounded with `reporting::money_in_currency`.

## 8. Backend

New domain `spending_plans`: `src-tauri/src/models/spending_plans.rs`,
`src-tauri/src/services/spending_plans.rs` (all logic, in-memory tests),
`src-tauri/src/commands/spending_plans.rs` (thin). Follow the 7-step type contract.

### Types (wire names camelCase)

- `SpendingPlan` — `id, name, year, amount, currency, icon, color, note, closedAt, createdAt`.
- `InsertSpendingPlan` — `name, year, amount, currency, icon, color, note` with `validate()`:
  name trimmed, 1–100 chars (`validation.spendingPlanNameRequired` / `…NameTooLong`); year
  2000–2100 (`validation.spendingPlanYearInvalid`); amount a positive decimal
  (`validation.spendingPlanAmountInvalid`); currency a known code (reuse the existing currency
  validator); icon 1–40 chars of `[a-z0-9-]`; color `#RRGGBB` (`validation.spendingPlanColorInvalid`);
  note ≤ 500 chars.
- `SpendingPlanSummary` — the plan plus `spent: Option<String>` (plan currency, `None` only if
  nothing could be converted and there are payments), `paymentCount`, `lastPaymentDate`
  (booking day, epoch), `fxQuality`.
- `SpendingPlansOverview` — `year, currency` (report), `fxQuality`, `plans:
  Vec<SpendingPlanSummary>`, `totals { planned, spent, remainingActive }` (each
  `Option<String>`, report currency), `yearsWithPlans: Vec<i32>`.
- `SpendingPlanDetail` — `plan: SpendingPlanSummary`, `months: Vec<{ month: "YYYY-MM", spent }>`
  (only months that have payments; the frontend lays out the axis), `categories: Vec<{ categoryId:
  Option<String>, spent, count }>` (uncategorized grouped under `None`), `accountCount`, and `cells: Vec<{ month, categoryId, spent, count }>` (month × category, so the filtered
  views are sums of backend-converted values).
- `SpendingPlanAssignment` — `{ transactionId, planId: Option<String> }`.
- `BankTransaction` gains `spendingPlanId: Option<String>` (serialized always, like
  `flowKind`), filled by every query that builds it.

### Commands

| Command | Does |
|---|---|
| `get_spending_plan_index()` | every plan of every year with its spent (ledger chips, plan picker) |
| `get_spending_plans_overview(year, report_currency)` | plans of the year with spent, totals, years with plans |
| `get_spending_plan_detail(id)` | detail aggregates in the plan currency |
| `get_spending_plan_transactions(id)` | the plan's payments as `BankTransaction`s, newest first |
| `search_plan_candidate_transactions(filters)` | cross-account payments for the "Přidat platby" dialog: `{ planId, from, to (epoch days, optional), accountId?, onlyUnassigned, query?, limit }`; excludes the plan's own payments; newest first; capped (default 500) with a `truncated` flag |
| `create_spending_plan(data)` / `update_spending_plan(id, data)` | validate, unique name per year |
| `create_spending_plans(items: Vec<InsertSpendingPlan>)` | take-over of last year's plans, one transaction, all-or-nothing |
| `set_spending_plan_closed(id, closed)` | sets/clears `closed_at` |
| `delete_spending_plan(id)` | clears the tags, deletes the plan |
| `set_transaction_spending_plans(assignments)` | one transaction; assigning to a **closed** plan fails with `validation.spendingPlanClosed`; unknown ids → `NotFound`. Used for add, move, remove **and** undo (the frontend sends the previous values back). |

All errors are `AppError`; every validation key lands in all ten locales (`validation.*` in
`common.json`). Log nothing personal above `debug`.

### Tests (service, in-memory DB)

CRUD and validation; name uniqueness with Czech case (`Dovolená` vs `DOVOLENÁ`); close/reopen;
assign/move/remove/undo; closed-plan rejection; delete clears tags; spent nets credits; pending
excluded; foreign payment converted at its day with `fx` test quotes; missing rate excluded and
reported; months bucket incl. a payment in the following year; category breakdown incl.
uncategorized; overview totals in a report currency with a plan in EUR; candidate search filters
(account, period, only unassigned, query, excludes own); promotion keeps the tag.

## 9. Frontend

### Shared pieces

- `src/lib/tauri-api.ts`: `spendingPlansApi` (typed wrappers); types mirrored in `shared/schema.ts`.
- Hooks: `src/hooks/use-spending-plans.ts` (overview, detail, transactions, candidates) and
  `src/hooks/use-spending-plan-mutations.ts`. Query keys: `["spending-plans", year, currencyCode,
  fxRevision]`, `["spending-plan", id, fxRevision]`, `["spending-plan-transactions", id]`,
  `["spending-plan-candidates", …filters]`. Mutations invalidate `["spending-plans"]`,
  `["spending-plan"]`, `["spending-plan-transactions"]`, `["spending-plan-candidates"]` and
  `["bank-transactions"]`. Tagging does **not** change money or asset state, so the
  net-worth snapshot / `portfolio-metrics` / `cashflow-report` part of rule 7 does not apply
  (state that in the hook's comment). Rate refresh: add the overview/detail keys to
  `invalidateAfterPriceRefresh`.
- Pure helpers with tests in `src/utils/spending-plans.ts`: plan status (`over | near | ok |
  empty`, P7 thresholds, closed handled by the caller), risk ordering for the overview, month
  axis (plan year Jan–Dec, extended to months with payments outside it), name search
  (case- and diacritics-insensitive, `dovo` matches "Dovolená").
- **Icon set (O12):** extend `ICON_MAP` in `src/components/common/category-icons-map.ts` with
  `mountain, tree-pine, tree-palm, snowflake, party-popper, book-open, stethoscope, tent`, so
  categories and plans pick from one set. Extract a shared `IconColorPicker` (icon grid + 14
  color presets + custom color + preview) from `CategoryFormDialog` and use it in both forms.
  Plan chip: `CategoryIconChip`'s look with a **rounded-square** shape (category chips are
  round), e.g. a `shape="square"` prop.
- i18n: new namespace `spending_plans` in all ten locales; `nav.spendingPlans` in `common.json`;
  validation keys in `common.json`. English is the source; Czech copy from the prototype.

### Navigation and route

`/spending-plans` (overview) and `/spending-plans/:id` (detail), both under `ProtectedRoute`.
Sidebar: Peníze group, after Cashflow, icon `ClipboardList`, no `prefKey`.

### Overview page (`src/pages/SpendingPlans.tsx`)

As in the prototype:
- Page head: eyebrow "Peníze", H1, the one-line explanation, year stepper (‹ 2026 ›, from the
  earliest year with plans or the current year − 1, to the current year + 1), "Převzít plány z
  roku N−1" (only when that year has plans and this one has some — the empty state carries it
  otherwise), primary "Nový plán".
- Four stats: Plánováno na rok N (count, closed count), Utraceno (% of planned), Zbývá v
  aktivních plánech (count of active), Vyžaduje pozornost (n přečerpané / "Vše v mezích", n
  blízko limitu). Totals in the report currency.
- Table card "Plány N": search field, segment Aktivní n · Uzavřené n · Vše n (default Aktivní),
  columns Plán (chip, name, "N plateb · poslední d. m."), Plánováno, Utraceno, Čerpání (% +
  progress, over in `loss`), Zbývá (negative in `loss` with "−"), Stav (Přečerpáno badge with
  arrow / Blízko limitu / V pořádku / Bez plateb / Uzavřený outline badge), row ··· menu
  (Otevřít, Upravit, Uzavřít|Obnovit, Smazat). Order: over, near, then by % desc; closed last in
  "Vše". Footer: count, planned and spent of the shown rows; "Blízko limitu: utraceno 90 % a více".
  Row click opens the detail. Amounts of a plan in another currency show that currency.
- Empty year: icon, "Na rok N zatím nemáte žádné plány", take-over primary (when N−1 has plans)
  and "Nový plán". Empty segment/search states as in the prototype.

### Detail page (`src/pages/SpendingPlanDetail.tsx`)

- Back link "Výdajové plány N", eyebrow "Výdajový plán · N · CUR", large plan chip + H1, note
  under it. Actions: Upravit, Uzavřít plán | Obnovit plán, **Smazat plán** (danger text button,
  confirm dialog). Closed plan: the inline alert "Plán je uzavřený …" with Obnovit.
- Three stats: Plánováno; Utraceno with the status badge (or the plain %); Zbývá / "Přečerpáno o".
- **Útrata v čase** card: bars = net spend per month (plan currency), a cumulative line up to
  today's month and a dashed line at the planned amount; months after today shaded "Zbytek
  roku"; value labels on bars. Click a month → month filter (click again clears). With a
  category filter the bars show that category only and the line, plan line and legend hide.
  Recharts, design-system series colors (`s3` bars, `chart-line`, `chart-cost` dashed).
- **Kategorie** card (left): rows with the category chip (icon + color), name, "N plateb",
  amount, share %, progress; total row. Click → category filter (toggle). Narrowed by the month
  filter.
- **Platby** card (right): rows like the account ledger (day group headers, counterparty with
  the round icon and "Účet · detail" sub-line, category as icon + name in ink, signed amount,
  credits in `gain`; foreign payments show the native amount in the sub-line). Checkboxes and a
  bulk bar: "Přesunout do…" (plan picker without the current plan) and "Odebrat z plánu".
  Head button **Přidat platby** opens the cross-account dialog. Narrowed by both filters.
- **Filter badges (O10):** reuse/generalize `FilterBadge` from `components/stocks-analysis/`
  (dark badge, list-filter icon, "Filtr: červenec · Ubytování") with an × that clears the
  filters it names. Chart shows the category filter, Kategorie the month filter, Platby both.
  A narrowed card gets the stronger border.
- Empty plan: chart with zero bars, Kategorie "Až do plánu přidáte platby…", Platby empty state
  with primary "Přidat platby".

### "Přidat platby do plánu" dialog

Wide modal: search (counterparty, category), account select (Všechny účty + each account),
period select (Celý rok N — default —, each month of N up to today, Všechna období), switch "Jen
platby bez plánu" (on). Table: checkbox, date, counterparty + account (+ "teď v plánu X"),
category, plan (chip or "Bez plánu"), amount. Row click toggles. Footer: "Vybrány n platby ·
± sum · k se přesune z jiného plánu", primary "Přidat n plateb". Closed plans' payments can be
moved out (the target is the open plan); the target itself must be active.

### Account ledger (`BankAccountDetail.tsx`)

- A checkbox column (header checkbox selects the current page) and a column **Výdajový plán**:
  a chip with the plan's small square icon and name (click → plan picker for the row) or, on
  row hover/focus, a dashed "Přidat do plánu" button.
- With a selection a bulk bar appears under the card head: "Vybrány n platby · ± sum", primary
  "Přidat do plánu" (picker), "Odebrat z plánu" (disabled when none has a plan), ghost "Zrušit
  výběr". Selection clears on page/period/filter change.
- **Plan picker** (popover): search, active plans grouped by year (newest first) with chip and
  "zbývá X z Y" / "přečerpáno o X", a check on the current plan when all chosen rows share it,
  note "Uzavřené plány se nenabízejí", "Odebrat z plánu" (when any chosen row has a plan) and
  "Nový výdajový plán…" (opens the plan form; on save the rows are assigned to the new plan).
- After any assignment a toast "n platby přidány do plánu X" / "… odebrány z plánu" with
  **Vrátit zpět** (sends the previous plan ids back).

### Plan form dialog

Název; Plánovaná částka (unit follows the currency) · Rok · Měna; hint "Platby v jiné měně se do
plánu převedou kurzem ECB ke dni platby."; Poznámka (nepovinné); Ikona; Barva; preview. Create
or "Uložit změny"; no delete in the form. Defaults for a new plan: the page's year, main
currency, icon `wallet`, color `#6366F1`. react-hook-form + zod with `validation.*` keys.

### Take-over dialog ("Převzít plány z roku N−1")

Table of last year's plans: checkbox (preselected: not closed and no same-named plan this year),
chip + name + "Aktivní/Uzavřený · v roce N už existuje", Plánováno N−1, Utraceno N−1 (over in
`loss`), editable "Částka na N". Footer: "n plánů · sum", primary "Převzít n plánů". Copies
name, amount (edited), currency, icon, color, note; never payments.

## 10. Copy (Czech source in the prototype; English)

Page "Spending plans" — "How much you want to spend on what this year, and how much you have
spent. Tag payments with a plan in an account's payment list; categories stay as they are."
Badges: Over plan / Close to limit / On track / No payments / Closed. Buttons: New plan, Take
over plans from N, Close plan, Reopen plan, Delete plan, Add payments, Add to plan, Remove from
plan, Move to…, New spending plan…, Undo. The builder writes the rest following the prototype's
Czech and translates per `docs/standards/i18n-glossary.md` (add "spending plan" to the glossary).

## 11. Verification

- All gates of `docs/standards/workflow.md`; locale parity test; generated-types drift.
- Run the app (`npm run tauri dev`, demo profile on a free port per the dev-server notes) and
  walk: create a plan, tag payments from two accounts (ledger bulk + detail dialog), filter the
  detail by month and category, a EUR payment in a CZK plan, close/reopen, take-over into the
  next year, delete; light and dark; one non-Czech language for layout.

## 12. Calls made after the builder check (mine, not the owner's)

| # | Question | Call |
|---|---|---|
| C1 | Does a tagged transfer / income / loan repayment count as spend? | Yes. A plan is a label (O3, O8): spent = debits − credits of every booked tagged payment, whatever the category role or flow kind. The plan's numbers may differ from Cashflow by design; the picker does not hide transfers. |
| C2 | Changing a plan's currency after payments are tagged | Allowed. Amounts are native facts converted at read time, so spent is simply recomputed in the new currency; the planned amount is taken as typed next to the new currency's unit. No conversion of the planned amount. |
| C3 | Do `paymentCount` / `lastPaymentDate` include pending payments? | Yes — they describe what is tagged (the list shows them). Only `spent` is booked-only. Status `empty` = no tagged payment at all. |
| C4 | Status when spent cannot be fully converted | Status and % use the converted part; a plan with excluded payments shows the app's compact missing-rate marker next to Utraceno (row and detail). `spent = None` (nothing convertible) → "—", status `unknown`, ordered after over/near, before ok. |
| C5 | A ledger row whose plan is closed | The chip is still shown (closed plan's name, tooltip "Uzavřený plán") and clickable: the picker offers the active plans and "Odebrat z plánu". Backend: a **closed target** is rejected (`validation.spendingPlanClosed`) unless the call is an undo — `set_transaction_spending_plans(assignments, restore: bool)`; `restore = true` (only the toast's Vrátit zpět sends it) may write a closed plan id back. Closing a plan needs no confirmation (it is reversible). |
| C6 | Take-over with a name that already exists in year N | Such rows are shown disabled with "v roce N už existuje" and cannot be checked; names are not editable in the dialog. The backend still validates the whole batch and fails all-or-nothing with `validation.spendingPlanNameTaken` (defence only). |
| C7 | Ledger scope | One ledger component serves every account type; the columns are added there. The app's minimum window width (1080 px) is the layout floor; the Detail column gives way first (truncates). |
| C8 | Migration number | Last applied is `025_crypto_yahoo_symbol`, so `026_spending_plans`; procedure per `docs/playbooks/add-db-migration.md`. |

## 13. Left to the builder

Component split, exact recharts composition, how the ledger's existing sort/pagination state
coexists with selection (selection is per page), the candidate query's SQL, and the remaining
copy within the prototype's intent.
