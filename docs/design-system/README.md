# Moony design system

The visual language of the Moony app: design tokens, the component contract, chart rules, copy rules and
the page archetypes the React frontend is built from.

| File | What it is |
|---|---|
| `tokens.css` | All design tokens as CSS custom properties. The only place with raw color values. |
| `moony.css` | Component contract in plain CSS (class per component, states included). The React implementation matches it visually, it does not reuse it. |
| `styleguide.html` | Visual reference: every token, type step, elevation, component and state on one page. Open it in a browser. |
| `shell.js`, `charts.js`, `icons.js` | Style-guide helpers (sidebar from the real IA, SVG charts, Lucide icons). Not production code. |

Open `styleguide.html` directly from the file system; no build step is needed.

## 1. Direction

A light, calm, precise financial instrument designed by a product studio. Not a native macOS clone and
not a colorful fintech app. The interface relies on typography, proportion and material; color carries
meaning only.

Principles:

1. **One dominant number per page.** Net worth, position value, balance. It gets the most space and the
   least noise around it.
2. **Depth comes from surfaces, not borders.** The canvas is one tone darker than content; sidebar, cards
   and the table are layers with soft warm light. No frame around every element.
3. **Color means something or is absent.** Green is only positive change. Red-brown is only negative
   change, destructive actions and validation errors. Data series are monochrome.
4. **Values relate to time.** Where it helps, a chart carries a "time trace": the events (buy, sell,
   dividend, large movement) that explain why a value changed. See §9.
5. **Copy works.** Labels label, buttons name their outcome, empty states say what to do. No exclamation
   marks, no apologies, sentence case everywhere except eyebrows and table heads.

## 2. Tokens

Source of truth: `tokens.css`. Summary of the families (names map 1:1 to the file):

- **Surfaces** `canvas`, `canvas-glow`, `sidebar`, `paper`, `paper-2`, `well`, `well-2`, `well-3`
- **Lines** `line`, `line-soft`, `line-strong`, `line-sidebar`
- **Ink** `ink` … `ink-5`, `ink-inverse`, `ink-inverse-2`
- **Dark material** `dark`, `dark-hi`, `dark-lo` and the two gradients
- **Semantic** `gain`, `gain-soft`, `loss`, `loss-soft`
- **Series** `s1` … `s4`, `s-other`, `chart-line`, `chart-area`, `chart-grid`, `chart-cost`, `chart-axis`
- **Focus** `focus-border`, `focus-ring`
- **Radius** `r-1` 6 · `r-2` 8 · `r-3` 13 · `r-4` 15 · `r-5` 17
- **Elevation** `sh-sidebar`, `sh-1` (stat), `sh-2` (card), `sh-3` (table), `sh-pop`, `sh-modal`, `sh-dark`, `sh-btn`, `scrim`
- **Type** size, weight and tracking per role (`fs-*`, `fw-*`, `ls-*`)
- **Spacing** 4-px scale `sp-1` … `sp-9`, layout widths, control heights
- **Motion** `t-fast` 120 ms, `t-base` 180 ms, one ease curve

Validation that was run (dataviz palette validator, OKLab):

| Check | Result | Consequence in the system |
|---|---|---|
| Series s1–s4 adjacent ΔE | ≥ 17 (normal and CVD) | Max 4 monochrome series, 5th+ folds into "Ostatní". |
| Light series vs paper contrast | s3 2.8:1, s4 1.5:1 | Every series always has a legend with values or a direct label. |
| `gain` vs `loss` deutan ΔE | 3.8 (indistinguishable) | Sign glyph or arrow is mandatory next to every signed value. Color never carries direction alone. |

Dark theme: not in v1. The app ships light only.

## 3. Typography

Inter Variable, bundled with the app (`@fontsource-variable/inter`), no network fonts. Fallback stack
`Inter, ui-sans-serif, system-ui`. Intermediate weights (570, 580, 620, 650) need the variable font;
without it they round to 500/600/700. `font-variant-numeric: tabular-nums` on every number in tables,
stats and heroes.

| Role | Size / weight / tracking | Token |
|---|---|---|
| Hero number | 42 / 580 / −0.08em | `fs-display` |
| Page H1 | 36 / 570 / −0.07em | `fs-h1` |
| Modal title | 21 / 600 / −0.05em | `fs-modal` |
| Stat value | 22 / 580 / −0.06em | `fs-stat` |
| Card or table title (H2) | 18 / 650 / −0.045em | `fs-h2` |
| Section inside card (H3) | 16 / 620 / −0.04em | `fs-h3` |
| Navigation | 14 / 600 / −0.015em | `fs-nav` |
| Body | 13 / 450 / line 1.5 | `fs-body` |
| Table name | 12 / 650 | `fs-table`, `fw-table-name` |
| Table value | 12 / 480 | `fs-table`, `fw-table` |
| Caption | 11 / 500, ink-3 | `fs-caption` |
| Micro | 10 / 600, ink-4 | `fs-micro` |
| Eyebrow | 10 / 700 / +0.12em, uppercase | `fs-eyebrow` |
| Table head | 10 / 750 / +0.08em, uppercase | `fs-thead` |

## 4. Elevation and layout

Six levels, differentiated mainly by surface and faint warm shadow (brown-grey, never black):

| Level | Surface | Shadow | Used for |
|---|---|---|---|
| E0 | `canvas` + radial `canvas-glow` top-right | none | page background |
| E1 | `sidebar` gradient | `sh-sidebar` + 1px `line-sidebar` | sidebar |
| E1 | `paper` → `paper-2` gradient | `sh-1` + 1px `line` | stat cards, flat cards |
| E2 | `paper` → `paper-2` gradient | `sh-2` + 1px `line` | cards, hero |
| E3 | `paper` | `sh-3` + 1px `line` | tables (main workspace) |
| E4 | `paper` | `sh-pop` | menus, tooltips, toasts |
| E5 | `paper` | `sh-modal` over `scrim` with 4px blur | modals |
| dark | `dark-grad` | `sh-dark` (inner 1px light + 3/7 shadow) | active nav, primary button |

Layout: sidebar 264 px, page max 1480 px, padding 32 px top and 48 px sides, 56 px bottom. Top bar with
breadcrumb left and data status right (40 px below). Page head: eyebrow, H1, one sentence, actions right
with at most one primary button. Content column grids: `1.4fr .6fr` for detail pages (main + aside),
`1fr 1fr` for reports, `1.15fr .85fr` on the dashboard. Minimum supported width 1080 px (desktop app).

## 5. Navigation

The sidebar is continuous, no boxed groups. The tree is the real IA from
`src/components/common/nav-config.ts`: Přehled · Peníze · Investice · Závazky a pojištění · Nástroje,
with Nastavení pinned above the account row. **Peníze** ends with Cashflow and **"Plánování cashflow"**
(icon `Repeat`), see §7 and §11.

- Row 36 px, 14/600, Lucide icon 16 px stroke 1.75 in `ink-3`; the active row is dark material with white
  text and icon. Hover is 5 % ink. Counts sit right in micro type.
- Group titles are 10 px uppercase eyebrows; a chevron appears on hover and groups collapse. State persists
  per group (existing `moony-sidebar-groups` behavior). **Nástroje is collapsed by default** so the 17 items
  fit a 900 px window; the middle region scrolls, Nastavení and the account stay pinned.
- Account row: avatar initials, name, "Účet a zámek aplikace"; click opens the existing menu (Pravidla
  kategorizace, Zamknout aplikaci).
- Top bar status: dot + text. `status--fresh` green for fresh prices, `status--stale` red-brown for stale.
- Milestones indicator: left of the update status, a neutral `.status` with `ink-2` text, shown only when a deadline (notice deadline, contract or fixed-rate end, maturity, account or promotional-rate end) is at most 14 days away; it names the nearest one ("Spořicí účet · za 10 dní", "+N" for more) and opens a popover with those deadlines. Information, data upkeep and target prices never appear there.

## 6. Components

Each entry names the CSS class in `moony.css` and the React component that implements it.

| Component | Class | React component | Rules |
|---|---|---|---|
| Button | `.btn`, `--primary`, `--ghost`, `--danger`, `--destructive`, `--sm`, `--icon` | `ui/button.tsx` (`cva` variants) | 36 px (32 small), radius 8, 11/650 label as a verb. One primary per page. Danger is text-only; destructive solid only inside confirm dialogs. Loading spins the icon, keeps the label. |
| Segmented control | `.seg`, `.seg__btn.is-active`, `--lg` | `ui/segmented.tsx` for period and 2–4-value filters; `ui/tabs.tsx` for page tabs | Paper pill with shadow on a `well-2` track. |
| Field | `.field`, `.field__label`, `.field__hint`, `.field__error` | `ui/form.tsx` + `label.tsx` | Label above, 11/700; optional marker inline; hint only when useful; error replaces hint, red-brown with icon, says what to do. |
| Input, select, textarea | `.input`, `.select` in `.select-wrap`, `.textarea`, `.input-wrap` with `.unit` | `ui/input.tsx`, `select.tsx`, `textarea.tsx` | 39 px, radius 8, unit inside right. Focus: `focus-border` + `focus-ring`. Invalid: `loss` border. |
| Checkbox, switch | `.check`, `.switch` | `ui/checkbox.tsx`, `switch.tsx` | Dark material when on. |
| Search | `.search.input-wrap` | `ui/input.tsx` with a leading icon | 240 × 35 px with leading icon. |
| Card | `.card`, `--flat`, `--table`, `.card__head/body/title/sub` | `ui/card.tsx` | E2 default, E1 flat, E3 table. Title 18/650, sub 10/500. |
| Hero | `.hero`, `.hero__label/value/delta/aside` | `common/HeroCard.tsx` | One number, change with sign + context, period segment top right, chart below. Decorative circle is part of the material. |
| Stat | `.stat`, `.stats`, `.stats--3` | `common/Stat.tsx` | Label 11, value 22, note 10. Three or four per row. |
| Table | `.table`, `.table__head/row/group/foot`, `.cell--num`, `.cell--name`, `.entity`, `.logo`, `.row-actions` | `ui/table.tsx` + `InvestmentsTable` etc. | Head 40 px on `well`; rows 56 px (46–52 dense); hover `well`; whole row clickable; "···" on hover only; numbers right and tabular; no zebra, no vertical lines. |
| Badge, chip | `.badge`, `--gain/--loss/--dark/--outline`, `.chip.is-active` | `ui/badge.tsx`, `ui/chip.tsx` | 20 px, no border. Green/red-brown only for positive/negative state. |
| Progress | `.progress`, `--over`, `.progress__mark` | `ui/progress.tsx` | 6 px (10 px overview). Over limit uses `loss`. |
| Modal | `.modal-scrim.is-open`, `.modal`, `--sm`, `.modal__head/body/foot/close` | `ui/dialog.tsx`, `alert-dialog.tsx` | 560 px (confirm 440), radius 17, 180 ms open. Title + one sentence; footer on `well` with ghost "Zrušit" left and the primary named like the title right. A destructive action of the same modal ("Smazat cíl", "Odebrat ruční cenu") is a text-only danger button directly left of the primary; "Zrušit" stays at the left edge whether it is shown or not. Esc closes, first field focused. |
| Menu | `.menu`, `.menu__item`, `--danger`, `.menu__sep` | `ui/dropdown-menu.tsx` | E4, 190 px min, 12/550 items. |
| Tooltip | `.tip` | `ui/tooltip.tsx` and chart tooltips | Dark material, 10 px, bold value + muted lines. |
| Toast | `.toast.is-on` | `ui/toaster.tsx` (sonner) | Bottom right, 2.6 s, past-tense verb ("Účet přidán"). |
| Alert | `.alert`, `--error` | `ui/alert.tsx` | Inline, next to the cause, with an action. |
| Empty state | `.empty` | `common/EmptyState.tsx` | Icon in ink-5, title, one sentence, button. |
| Milestone row | — | `milestones/MilestoneRows.tsx` | Agenda row: a 60 × 42 px date chip on `well` (day and month 12/650, distance under it 10/500 `ink-4`; dark material only for a deadline at most 7 days away, where the distance is `ink-inverse-2`; a crossed target's chip reads "Dnes / cíl"), title 12/650, one sub-line 10/500 `ink-4`, a ghost `icon-sm` '···' (always visible, `ink-4`, named "Možnosti připomínky: {title}" for assistive tech) next to the link, never inside it, opening Skrýt / Připomenout za týden (upkeep: za měsíc; not offered for an item at most 7 days away) / Nepřipomínat …. Upkeep rows use an icon in a `well` square instead of the chip. |
| Skeleton | `.skeleton` | `ui/skeleton.tsx` | Shaped like the content it replaces; no page spinners. |
| Breadcrumb, status, back link | `.topbar`, `.crumb`, `.status`, `.back` | `shell/TopBar.tsx`, `shell/DataStatus.tsx` | See §5. |

Carousel, menubar, navigation-menu, hover-card, aspect-ratio, input-otp and drawer have no place in a
desktop-only app and are not used. Radix primitives provide the behavior; the design system restyles them.

## 7. Page archetypes

Every page follows one of these structures; the pages named in the second column are the reference
implementations.

| Archetype | Pages | Structure |
|---|---|---|
| Overview | Dashboard | Greeting H1, period segment, hero (net worth + chart), 3 stats, "Co vás čeká" (a two-column agenda with date chips filled top to bottom, data upkeep folded into one line under it, hidden when empty), allocation ring + recent moves. |
| List | Stocks, bank accounts | 3–4 stats, a trend card (`TrendCard`: title, one sentence, horizon segment, chart 170 px, legend; the same chrome and height on every list page), table card with search and toggles, add modal. Tables use a fixed layout so long text ends with an ellipsis instead of widening the table; no horizontal scroll at the minimum width. |
| Detail | Stock detail, bank account detail | Back link, eyebrow, H1, action row, hero with time trace, 3–4 stats, ledger table + aside cards, modals. |
| Report | Cashflow | Actual monthly income and expenses from bank transactions: stats, bar chart card, breakdown by category and by source with change versus the previous period. |
| Plan | Cashflow planning | Planned recurring items in personal and investment sections, items derived from Úvěry / Pojištění / Nemovitosti / Akcie marked as automatic, monthly / yearly switch, a 100 % flow bar instead of a Sankey diagram. |
| Planning | Budgets | Month navigation, stats, overview progress, category table with progress bars and state badges. |
| Settings | Settings | Left sub-navigation (existing routes), stacked flat cards, danger zone last. |
| Outlook | Projection | Horizon segment, hero with expected line, ± 2 p.p. scenario band, dashed "today + contributions" reference and future milestones (mortgage payoff, round net-worth marks), asset-class table, three scenarios, parameters modal. |
| Analysis | Stocks analysis | Tag groups as chip filters that narrow everything below them (ring, TWR tag series, tag table, holdings; the stats stay portfolio-wide), allocation ring by strategy and the TWR card at equal height, TWR in percent per tag with the whole portfolio as the dark dashed reference and its return in the card head, legend with values under the chart, tag table, holdings with tags, manage and assign modals. |
| Watchlist | Stock monitor and its detail | Native-currency prices, day change vs previous close, inline 52-week range bar with current price and target tick, target state badge; detail with target line on the chart, watch-start marker, markdown notes, key data. |
| Calculator | Annuity calculator, real estate investment calculator | Inputs in flat cards left, live hero result right, chart with milestones (principal overtakes interest, half repaid; positive cashflow, half of loan, sale), yearly tables. Annuity adds an optional extra yearly payment with the saving it brings. |
| Crypto | Crypto, crypto detail | List archetype of Stocks with a share column and buy/sell markers on the aggregate trend (stocks show the same trade days); the detail mirrors Stock detail: cost-basis step line, buy/sell events, realized gain, holding period, "since first buy" card. |
| Bonds | Bonds | Nearest maturity and weighted time to maturity as stats, a maturity-and-coupon ladder (principal returned and coupons per year), per-bond yearly coupon, "do roka" badge, matured issues behind a toggle. |
| Real estate | Real estate, real estate detail | Appreciation, rental yield and equity / LTV per property, value-vs-equity trend; the detail has a valuation trace starting with the purchase (optional purchase date) followed by every estimate (the first estimate is recorded when the property is created), revaluations as events, cash-on-cash return, an annual balance that separates principal paydown from cashflow, a link to the real estate investment calculator and a "Přecenit" modal. |
| Other assets | Other assets | Gain/loss stat, per-row gain and annual yield, "oceněno" date, valuation-history trace shared with real estate, edit action. |
| Loans | Loans, loan detail | Debt trajectory (actual solid, schedule dashed) with every new loan marked ▲ where the debt steps up and milestones (car paid off, fixation end, half repaid, payoff), repaid progress per loan; the detail adds the schedule table (next 12 / per year), an extra-payment what-if (shorten the term or lower the payment) and an events list. |
| Insurance | Insurance, insurance detail | Policy (type · insurer underneath), premium, coverage total with the largest limit, next payment and anniversary per policy, a 12-month payment calendar; the detail has a cumulative-premiums trace with contract changes, a limits table, documents and a contract card. |
| Onboarding | First run | Two-column first run: the step form on the left, a live preview of the future overview on the right that reacts to the chosen areas. |
| Lock screen | Lock screen | One card: unlock with a decrypting state and inline error, a three-step recovery flow, language segment. |
| CSV import | CSV import wizard | Four-step wizard in a wide modal; the preview step shows duplicates and the category each row would get, uncategorized rows are fixed inline before the import; the done step lists next steps. |
| Categorization rules | Categorization rules | Evaluation-order strip, automation rate, hit counts per rule (90 days) in all three tabs, a rule editor with a live "matches N existing transactions" preview and a re-categorize option. |

## 8. Charts

Charts use Recharts with these rules (`charts.js` draws the same shapes by hand for the style guide):

- One y-axis, never two. Grid in `chart-grid`, value ticks right-aligned in `chart-axis` 10 px, time axis
  below with five labels, the last being "Dnes". Ticks are nice numbers (20 / 25 / 50 steps).
- Value line 2.25 px `chart-line`, area gradient `chart-area` → transparent, last point a white ring.
  "Vloženo" (cost basis) is a dashed step line in `chart-cost`; the gap between lines is the return.
- Bars: income `s1`, expense `s3`, 3 px rounded ends, 2 px gap, no borders, zero baseline.
- Allocation ring: `s1`–`s4` + `s-other`, legend with amounts and percentages, never color alone.
- Hover always: crosshair + tooltip on lines, tooltip on bars. Tooltip is `.tip`. Value charts offer the horizons 30 dní · 3 měsíce · Letos · Rok · Vše, bounded on UTC days.
- Baseline: zero for "Vše" and bars; truncated for short periods with 15 % headroom below.
- Only one hue family per chart; green/red-brown never appear in series.

## 9. Time trace

On charts of a **single entity** (stock position, crypto position, bank account, loan, real estate),
discrete events are drawn on the value line:

| Event | Mark | Color |
|---|---|---|
| Buy | ▲ filled, 12 px, 2 px white ring | `chart-line` |
| Sell | ▽ outlined | `chart-line` |
| Dividend, income | ○ ring 9 px | `gain` |
| Large expense, payment | ○ ring 9 px | `chart-line` |

Behavior: hovering a mark shows the event tooltip (type, quantity, price, total, date); hovering a row in
the history table highlights its mark (`is-hot`, scale 1.35); clicking a mark scrolls to the row. Only
events inside the selected period are drawn. Aggregate charts (net worth, cashflow) never draw events; the
exceptions are the stocks and crypto list trends (one mark per trade day and direction, explaining the
jumps a buy or a sell causes) and the loans trajectory (new loans and milestones). Bank accounts mark only movements above a threshold (an absolute floor of about 400 EUR, or 10 % of balance). If more
than ~24 events fall in view, cluster neighbors into one mark with a count ("3 nákupy").

## 10. Copy and formatting

- Money through `useFormat().money()`: thin non-breaking thousands, comma decimals, unit after the number
  with a non-breaking space. Signed changes: "+ 60 110 Kč", "− 15 200 Kč" (true minus U+2212). Axis ticks
  shorten to "tis." / "mil.".
- Dates "2. 10. 2026"; lists use "Dnes", "Včera", "30. zář". Times "10:32".
- Uppercase only in eyebrows and table heads. Sentence case everywhere else. No exclamation marks.
- Buttons are verbs that name the outcome, identical to the modal title ("Přidat účet" → toast "Účet
  přidán"). Never "OK", "Odeslat", "Submit".
- Every string in both `cs/` and `en/` locale files (`docs/standards/i18n.md`).

## 11. Decisions

1. **Dark theme is not part of v1.** The app ships light only; there is no theme switch and no `.dark`
   token block. It can be revisited after launch.
2. **Time trace on every single-entity detail page**, as specified in §9. Aggregate charts stay free of
   events.
3. **Inter Variable is bundled** via `@fontsource-variable/inter`. No network fonts and no system-font
   fallback design.
4. **Cashflow is two pages.** *Cashflow* (`/reports/cashflow`) shows actual flows from bank transactions:
   monthly sums by direction, by category and by income source for 6 months, 12 months or the year to date,
   next to the same sums for the previous range, converted to CZK with the dated rate like the ledger.
   *Cashflow planning* (`/reports/cashflow-planning`, label key `nav.cashflowPlanning`, cs "Plánování
   cashflow", en "Cashflow planning") shows the planned recurring items; items derived from other domains
   (`isUserEditable = false`: Úvěry, Pojištění, Nemovitosti, Akcie, Dluhopisy) carry a source badge instead
   of row actions, and a 100 % flow bar (expense groups `s1`–`s4` + Ostatní, hatched remainder) replaces
   the Sankey diagram. The two pages link to each other in the page subtitle; the projection reads the
   planned report.

## 12. How the React app maps to this

- **Tokens.** The values of `tokens.css` live in `src/index.css` (keep the two files identical) and are
  exposed as Tailwind names (`canvas`, `paper`, `well`, `ink-*`, `gain`, `loss`, `s1`–`s4`, radius and
  shadow scales) in `tailwind.config.ts`. Components never use raw color values.
- **Behavior layer.** shadcn/ui with Radix primitives stays in `src/components/ui/`. Keyboard handling,
  focus management and accessibility come from Radix; the look comes from `cva` variants and classes that
  use the tokens. react-hook-form + zod, sonner (restyled), Recharts (wrapped) and lucide-react stay.
- **Charts.** `MoonyLineChart` and `MoonyBarChart` (`src/components/charts/`) wrap Recharts and implement
  §8; an event-markers layer (`EventMarkers.tsx`) implements §9. Marks are drawn as Recharts reference dots,
  never as a graphical item with its own data (Recharts 3 would then resolve the tooltip against the marks).
  `TrendCard` is the list-page chart card.
- **Font.** `@fontsource-variable/inter`, imported once in `src/main.tsx`.
- **Two-column pages.** The shorter column receives the content that would otherwise sit below both
  columns (see the two calculators), never an empty stretch.
