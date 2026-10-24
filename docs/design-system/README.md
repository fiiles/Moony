# Moony design system

The visual language of the Moony app: design tokens, the component contract, chart rules, copy rules and
the page archetypes the React frontend is built from.

| File | What it is |
|---|---|
| `tokens.css` | All design tokens as CSS custom properties. The only place with raw color values. |
| `moony.css` | Component contract in plain CSS (class per component, states included). The React implementation matches it visually, it does not reuse it. |
| `mobile.css` | The phone layer of the same contract (§13): phone type scale, screen, nav bar, tab bar, hero, stat row, entity row, segment, buttons, sheet. Matched by `src/mobile/`, never reused. |
| `styleguide.html` | Visual reference: every token, type step, elevation, component and state on one page, with a Světlý/Tmavý switch. Open it in a browser. |
| `mobile.html` | The phone components at 360, 390 and 430 pt with German copy, each frame with a horizontal-overflow check (§13). |
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
3. **Color means something or is absent.** Green is only positive change. Red is only negative change,
   destructive actions and validation errors. Data series use one muted indigo family (§8).
4. **Values relate to time.** Where it helps, a chart carries a "time trace": the events (buy, sell,
   dividend, large movement) that explain why a value changed. See §9.
5. **Copy works.** Labels label, buttons name their outcome, empty states say what to do. No exclamation
   marks, no apologies, sentence case everywhere except eyebrows and table heads.

## 2. Tokens

Source of truth: `tokens.css`. Summary of the families (names map 1:1 to the file):

- **Surfaces** `canvas`, `canvas-glow`, `sidebar`, `sidebar-2`, `paper`, `paper-2`, `well`, `well-2`, `well-3`, `hero-orb`
- **Lines** `line`, `line-soft`, `line-strong`, `line-sidebar`, `line-hover`
- **Ink** `ink` … `ink-5`, `ink-inverse`, `ink-inverse-2`, `ink-hover`
- **Contrast material** `dark`, `dark-hi`, `dark-lo`, `dark-hover-hi`, `dark-hover-lo` and the two gradients
- **Semantic** `gain`, `gain-soft`, `loss`, `loss-soft`, `loss-line`, `loss-hover`, `loss-ring`
- **Series** `s1` … `s4`, `s-other`, `chart-line`, `chart-area`, `chart-grid`, `chart-cost`, `chart-axis`
- **Focus** `focus-border`, `focus-ring`
- **Radius** `r-1` 6 · `r-2` 8 · `r-3` 13 · `r-4` 15 · `r-5` 17
- **Elevation** `sh-sidebar`, `sh-1` (stat), `sh-2` (card), `sh-3` (table), `sh-pop`, `sh-modal`, `sh-dark`, `sh-btn`, `sh-btn-hover`, `sh-seg`, `sh-destructive`, `scrim`
- **Type** size, weight and tracking per role (`fs-*`, `fw-*`, `ls-*`)
- **Spacing** 4-px scale `sp-1` … `sp-9`, layout widths, control heights
- **Motion** `t-fast` 120 ms, `t-base` 180 ms, one ease curve

Main-window page navigation uses a 400 ms entrance in `shell/PageTransition`: opacity 0.2 → 1 and an
8 px upward settle, with an ease-out curve. The sidebar and top bar stay still. Only pathname
changes trigger it; query filters and data refreshes do not. It preserves page component state,
leaves no transform after completion, and skips or cancels the animation for reduced motion.

Validation that was run (dataviz palette validator, OKLab):

| Check | Result | Consequence in the system |
|---|---|---|
| Series s1–s4 adjacent ΔE | ≥ 15.4 (normal and CVD) | Max 4 series of the indigo family, 5th+ folds into "Ostatní". |
| Light series vs paper contrast | s3 4.4:1, s4 2.3:1 | Every series always has a legend with values or a direct label. |
| `gain` vs `loss` CVD ΔE | 9.5 (protan, worst case) | Distinguishable, but a sign glyph or arrow is still mandatory next to every signed value. Color never carries direction alone. |
| `gain` / `loss` text contrast | 5.3:1 / 5.2:1 on paper, 4.7:1 / 4.6:1 on their soft tints | Both work as 10 px badge text. |

Two themes, one set of names. The light values are the `:root` block, the dark values the
`:root[data-theme="dark"]` block of `tokens.css`; `data-theme` is stamped on `<html>` before the first
paint (`public/theme-boot.js`) and switched by `ThemeProvider`. The contrast material `dark` is dark on
light and light on dark (`#e9e7e1`, text `ink-inverse`); depth stays surface-based (canvas < sidebar <
paper, wells deeper = darker in both themes).

Only the color, elevation and focus tokens differ between the themes; radius, type, spacing, layout and
motion are shared and stay out of the dark block. The `dark-grad` gradients are declared once in `:root`
and follow the dark block, which also sets `color-scheme: dark`. Both value sets:

| Token | Light | Dark | Use |
|---|---|---|---|
| `--canvas` | `#eeece7` | `#1a1917` | page background |
| `--canvas-glow` | `#fbfaf7` | `#22211e` | radial highlight top-right |
| `--sidebar` | `#f8f7f3` | `#1f1e1b` | sidebar |
| `--sidebar-2` | `#f5f4f0` | `#1c1b19` | sidebar gradient end |
| `--paper` | `#ffffff` | `#292826` | cards, tables, modals, inputs |
| `--paper-2` | `#fdfcf9` | `#2b2a27` | card gradient end |
| `--well` | `#f7f6f2` | `#232220` | table head, hover row, modal footer, disabled input |
| `--well-2` | `#e9e7e1` | `#1e1d1b` | segmented track, neutral badge, skeleton |
| `--well-3` | `#e3e0d9` | `#181715` | progress track, avatar, switch off |
| `--hero-orb` | `#f0eee8` | `#302f2b` | decorative hero circle (lighter than paper in dark: moonlight, not a dip) |
| `--line` | `#e2dfd7` | `#363531` | card and table borders |
| `--line-soft` | `#efede8` | `#302f2c` | row dividers, chart grid |
| `--line-strong` | `#d9d5cd` | `#45443f` | input and secondary button border |
| `--line-sidebar` | `#e1ded6` | `#2b2a27` | sidebar edge |
| `--line-hover` | `#c9c5bc` | `#56544e` | input hover border |
| `--ink` | `#292822` | `#eceae4` | headings, values |
| `--ink-2` | `#504d46` | `#cdcbc4` | body, table values |
| `--ink-3` | `#817e75` | `#a3a098` | secondary text |
| `--ink-4` | `#98948b` | `#868379` | labels, captions |
| `--ink-5` | `#aaa7a0` | `#6b6962` | axis ticks, placeholders |
| `--ink-inverse` | `#ffffff` | `#1d1c19` | text on the contrast material |
| `--ink-inverse-2` | `#d6d3cc` | `#5f5c54` | secondary text on the contrast material |
| `--ink-hover` | `rgba(41,40,34,.05)` | `rgba(0,0,0,.18)` | nav and ghost hover |
| `--dark` | `#2c2a25` | `#e9e7e1` | contrast material: active nav, primary button, tooltip, checked controls |
| `--dark-hi` | `#3b3932` | `#f3f1ec` | gradient start |
| `--dark-lo` | `#27251f` | `#e2e0d9` | gradient end |
| `--dark-hover-hi` | `#46433b` | `#faf9f5` | hover gradient start |
| `--dark-hover-lo` | `#2f2d27` | `#ebe9e3` | hover gradient end |
| `--gain` | `#107a60` | `#45c6a6` | positive change only |
| `--gain-soft` | `#e6f5ee` | `#203c30` | gain badge tint |
| `--loss` | `#bf4224` | `#ef7f5e` | negative change, destructive, errors |
| `--loss-soft` | `#fcece6` | `#4a2d25` | loss badge tint |
| `--loss-line` | `#f1cfc3` | `#6e3e32` | error alert border, danger-zone card |
| `--loss-hover` | `#a8391d` | `#ff9e84` | destructive button hover |
| `--loss-ring` | `0 0 0 3px rgba(191,66,36,.14)` | `0 0 0 3px rgba(239,127,94,.22)` | invalid field focus ring |
| `--s1` | `#212339` | `#d8dbf0` | series 1 (income bars, first ring slice, progress bar) |
| `--s2` | `#454a73` | `#a2a7ec` | series 2 |
| `--s3` | `#7176a0` | `#7b7ea5` | series 3 (expense bars) |
| `--s4` | `#a6a9c8` | `#4f508e` | series 4 (2.0:1 — legend mandatory) |
| `--s-other` | `#ebe9e3` | `#3a3935` | "Ostatní" bucket, always labeled |
| `--chart-line` | `#3c3f67` | `#b5b9eb` | value line |
| `--chart-area` | `#e1e1ee` | `#393b52` | area fill start (fades to `paper-2` at 0) |
| `--chart-grid` | `#efeeea` | `#302f2c` | grid |
| `--chart-cost` | `#a8a49b` | `#78756d` | dashed "Vloženo" |
| `--chart-axis` | `#aaa7a0` | `#6b6962` | axis ticks |
| `--focus-border` | `#57544d` | `#a09d94` | keyboard focus border |
| `--focus-ring` | `0 0 0 3px rgba(39,38,32,.10)` | `0 0 0 3px rgba(255,255,255,.12)` | keyboard focus ring |
| `--sh-sidebar` | `7px 0 22px rgba(58,53,36,.03)` | `7px 0 22px rgba(0,0,0,.22)` | sidebar |
| `--sh-1` | `0 2px 5px rgba(64,58,43,.05), 0 9px 22px rgba(64,58,43,.02)` | `0 2px 5px rgba(0,0,0,.24), 0 9px 22px rgba(0,0,0,.14)` | stat card |
| `--sh-2` | `0 4px 10px rgba(68,60,43,.05), 0 16px 35px rgba(68,60,43,.03)` | `0 4px 10px rgba(0,0,0,.26), 0 16px 35px rgba(0,0,0,.18)` | card |
| `--sh-3` | `0 4px 11px rgba(68,60,43,.055), 0 20px 38px rgba(68,60,43,.035)` | `0 4px 11px rgba(0,0,0,.28), 0 20px 38px rgba(0,0,0,.2)` | table |
| `--sh-pop` | `0 8px 24px rgba(18,17,13,.12), 0 1px 2px rgba(18,17,13,.06)` | `0 8px 24px rgba(0,0,0,.5), 0 1px 2px rgba(0,0,0,.4)` | menu, tooltip, toast |
| `--sh-modal` | `0 26px 80px rgba(18,17,13,.44)` | `0 26px 80px rgba(0,0,0,.7)` | modal |
| `--sh-dark` | `inset 0 1px rgba(255,255,255,.13), 0 3px 7px rgba(39,35,28,.15)` | `inset 0 1px rgba(255,255,255,.55), 0 3px 7px rgba(0,0,0,.35)` | contrast material |
| `--sh-btn` | `0 1px 2px rgba(52,46,34,.06)` | `0 1px 2px rgba(0,0,0,.3)` | secondary button |
| `--sh-btn-hover` | `0 1px 3px rgba(52,46,34,.1)` | `0 1px 3px rgba(0,0,0,.4)` | secondary button hover |
| `--sh-seg` | `0 1px 3px rgba(34,34,34,.13)` | `0 1px 3px rgba(0,0,0,.45)` | active segmented pill |
| `--sh-destructive` | `inset 0 1px rgba(255,255,255,.12), 0 3px 7px rgba(191,66,36,.2)` | `inset 0 1px rgba(255,255,255,.12), 0 3px 7px rgba(0,0,0,.35)` | destructive button |
| `--scrim` | `rgba(30,29,25,.63)` | `rgba(12,11,9,.7)` | under modals (blur 4 px stays) |

Validation that was run (dark theme, surface `#292826`):

| Check | Result | Consequence |
|---|---|---|
| Series s1–s4 adjacent ΔE (normal vision) | 16.1 · 15.1 · 15.0 (floor 15) | Max 4 series, 5th+ folds into "Ostatní" (unchanged). |
| Series adjacent ΔE (protan/deutan, Machado 1.0) | ≥ 14.7 (target 8) | Passes without secondary encoding; legend stays. |
| Series vs paper contrast | s1 10.7, s2 6.5, s3 3.8, s4 2.0:1 | s4 below 3:1 (light: 2.3:1): every series always has a legend with values or a direct label. |
| Ordinal-ramp check (one hue, monotone L, ΔL ≥ 0.06, light end ≥ 2:1) | pass | The family reads as one ramp. |
| `gain` / `loss` text contrast | 6.9:1 / 5.5:1 on paper; 5.7:1 / 4.7:1 on their soft tints | Both work as 10 px badge text. |
| `gain` vs `loss` CVD ΔE | 9.4 (deutan, worst case; light 9.5) | ▲ / ▼ glyph mandatory as today. |
| Ink on paper | ink 12.2, ink-2 9.1, ink-3 5.6, ink-4 3.9, ink-5 2.7:1 | ink-4 is the smallest text allowed for labels (light: 3.0:1). |
| Contrast material | `ink-inverse` on `dark` 13.8:1, `ink-inverse-2` 5.4:1 | Tooltip second line and nav counts stay readable. |
| Logos (`ink-inverse` on s1–s3, `ink` on s4) | 12.4 / 7.5 / 4.4 / 6.1:1 | `.logo--4` keeps `ink` text as in light. |
| Category presets, glyph lifted 30 % to ink | all 14 ≥ 4.9:1 (raw min 3.1:1) | D7. |
| `focus-border` on paper | 5.4:1 | — |

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

Six levels, differentiated mainly by surface and faint warm shadow (brown-grey in the light theme, black-based in the dark one):

| Level | Surface | Shadow | Used for |
|---|---|---|---|
| E0 | `canvas` + radial `canvas-glow` top-right | none | page background |
| E1 | `sidebar` gradient | `sh-sidebar` + 1px `line-sidebar` | sidebar |
| E1 | `paper` → `paper-2` gradient | `sh-1` + 1px `line` | stat cards, flat cards |
| E2 | `paper` → `paper-2` gradient | `sh-2` + 1px `line` | cards, hero |
| E3 | `paper` | `sh-3` + 1px `line` | tables (main workspace) |
| E4 | `paper` | `sh-pop` | menus, tooltips, toasts |
| E5 | `paper` | `sh-modal` over `scrim` with 4px blur | modals |
| contrast material (`dark`) | `dark-grad` | `sh-dark` (inner 1px light + 3/7 shadow) | active nav, primary button |

In the dark theme the shadows are black-based (table in §2) and the card is carried by its 1 px `line`
and the paper/canvas step; the shadow only settles the bottom edge.

Layout: sidebar 264 px, page max 1480 px, padding 32 px top and 48 px sides, 56 px bottom. Top bar with
breadcrumb left and data status right (40 px below). Page head: eyebrow, H1, one sentence, actions right
with at most one primary button. Content column grids: `1.4fr .6fr` for detail pages (main + aside),
`1fr 1fr` for reports, `1.15fr .85fr` on the dashboard. Minimum supported width 1080 px (desktop app).

## 5. Navigation

The sidebar is continuous, no boxed groups. The tree is the real IA from
`src/components/common/nav-config.ts`: Přehled · Peníze · Investice · Závazky a pojištění · Nástroje,
with Nastavení pinned above the account row. **Peníze** ends with Cashflow and **"Plánování cashflow"**
(icon `Repeat`), see §7 and §11.

- Row 36 px, 14/600, Lucide icon 16 px stroke 1.75 in `ink-3`; the active row is the contrast material with
  text and icon in `ink-inverse`. Hover is 5 % ink. Counts sit right in micro type.
- Group titles are 10 px uppercase eyebrows; a chevron appears on hover and groups collapse. State persists
  per group (existing `moony-sidebar-groups` behavior). **Nástroje is collapsed by default** so the 17 items
  fit a 900 px window; the middle region scrolls, Nastavení and the account stay pinned.
- Account row: avatar initials, name, "Účet a zámek aplikace"; click opens the existing menu (Pravidla
  kategorizace, Zamknout aplikaci).
- Top bar status: dot + text. `status--fresh` green for fresh prices, `status--stale` red for stale.
- Milestones indicator: left of the update status, a neutral `.status` with `ink-2` text, shown only when a deadline (notice deadline, contract or fixed-rate end, maturity, account or promotional-rate end) is at most 14 days away; it names the nearest one ("Spořicí účet · za 10 dní", "+N" for more) and opens a popover with those deadlines. Information, data upkeep and target prices never appear there.

## 6. Components

Each entry names the CSS class in `moony.css` and the React component that implements it.

| Component | Class | React component | Rules |
|---|---|---|---|
| Button | `.btn`, `--primary`, `--ghost`, `--danger`, `--destructive`, `--sm`, `--icon` | `ui/button.tsx` (`cva` variants) | 36 px (32 small), radius 8, 11/650 label as a verb. One primary per page. Exception (owner decision 2026-10-10): the bank account detail head has two, Zaznamenat zůstatek and Importovat výpis. Danger is text-only; destructive solid only inside confirm dialogs. Loading spins the icon, keeps the label. |
| Segmented control | `.seg`, `.seg__btn.is-active`, `--lg` | `ui/segmented.tsx` for period and 2–4-value filters; `ui/tabs.tsx` for page tabs | Paper pill with shadow on a `well-2` track. An option may carry a divider (`divider`: a 1 px `line-strong` rule, 14 px tall, before it) that separates kinds of choices, as in Rozdělit podle on Stocks analysis: Nerozdělovat, the tag groups the user made, then the automatic ones. |
| Field | `.field`, `.field__label`, `.field__hint`, `.field__error` | `ui/form.tsx` + `label.tsx` | Label above, 11/700; optional marker inline; hint only when useful; error replaces hint, red with icon, says what to do. |
| Input, select, textarea | `.input`, `.select` in `.select-wrap`, `.textarea`, `.input-wrap` with `.unit` | `ui/input.tsx`, `select.tsx`, `textarea.tsx` | 39 px, radius 8, unit inside right. Focus: `focus-border` + `focus-ring`. Invalid: `loss` border. |
| Checkbox, switch | `.check`, `.switch` | `ui/checkbox.tsx`, `switch.tsx` | Contrast material when on. The off thumb is `paper` in light and `ink-2` in dark (the one theme-specific component rule). |
| Search | `.search.input-wrap` | `ui/input.tsx` with a leading icon | 240 × 35 px with leading icon. |
| Card | `.card`, `--flat`, `--table`, `.card__head/body/title/sub` | `ui/card.tsx` | E2 default, E1 flat, E3 table. Title 18/650, sub 10/500. |
| Hero | `.hero`, `.hero__label/value/delta/aside` | `common/HeroCard.tsx` | One number, change with sign + context, period segment top right, chart below. Decorative circle is part of the material. |
| Stat | `.stat`, `.stats`, `.stats--3`, `.stats--2x2` | `common/Stat.tsx` | Label 11, value 22, note 10. Three or four per row; four stats in a narrow column (calculator results beside the input form) sit 2 × 2 (`columns={2}`). The card is a size container: the value keeps 22 px down to a 163 px content box, then shrinks with the card (13.5 cqi, floor 16 px) so a long amount ("+ 16 682 338 Kč") never leaves the card at the 1080 px minimum width; every card in a row shares one size. |
| Table | `.table`, `.table__head/row/group/foot`, `.cell--num`, `.cell--name`, `.entity`, `.logo`, `.row-actions` | `ui/table.tsx` + `InvestmentsTable` etc. | Head 40 px on `well`; rows 56 px (46–52 dense); hover `well`; whole row clickable; "···" on hover only; numbers right and tabular; no zebra, no vertical lines. Category icon chips (`.cat-chip`) keep the user's hue; the dark theme lifts the glyph 30 % towards ink. |
| Badge, chip | `.badge`, `--gain/--loss/--dark/--outline`, `.chip.is-active` | `ui/badge.tsx`, `ui/chip.tsx` | 20 px, no border. Green/red only for positive/negative state. |
| Change badge | `.badge--gain` / `.badge--loss` with a ▲ / ▼ glyph | `common/ChangeBadge.tsx` (`ChangeBadge`, `ChangeNote`) | Every signed percentage change: "▲ 0,42 %" green, "▼ 1,3 %" red, no glyph and neutral when it rounds to zero. The glyph carries the direction, the number is unsigned (assistive tech hears the sign). In a gain column the badge sits on the first line and the signed amount under it in `ink-4`; in a stat note `ChangeNote` puts the badge into the translated sentence and the note stays neutral (the stat value keeps its green/red). Signed amounts stay text. |
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
| Detail | Stock detail | Back link, eyebrow, H1, action row, hero with time trace, 3–4 stats, ledger table + aside cards, modals. |
| Detail (bank account) | Bank account detail | Back link, eyebrow, H1, two actions + ··· menu; sections by data: balance hero (line through dated balance records of the last year, axis to Dnes, line ending at the newest record) with 3 balance stats, a flows card (12-month bar window shifted by arrows, the newest month with movements selected), the full-width ledger with its own period filters; balance history, import history and interest tiers are modals from the ··· menu. |
| Report | Cashflow | Actual income and expenses from bank transactions for a month, quarter or year: stats, bar chart card, income and expenses by category, money transfers in their own card, recategorization from the drilldown. |
| Plan | Cashflow planning | Planned recurring items in personal and investment sections; every non-empty category is a collapsible row (chevron, name, "N položek · M mimo plán · automaticky z Úvěry", included total), items under it with origin and original amount in the sub-line; any row or whole category can be left out of the plan from its "···" menu (stays visible, muted, "Mimo plán" badge, not counted); monthly / yearly switch, a 100 % flow bar instead of a Sankey diagram. |
| Planning | Budgets | Month navigation, account scope and one primary "Spravovat rozpočty" in the head (no view switch); stats, category table with progress bars and state badges, spending without a limit; every category name has its icon chip (`CategoryIconChip`, the icon in a 28 px circle tinted with the category color, as in Settings). The manage modal (720 px) lists one row per category: the limit in effect or the first scheduled one ("Naplánováno" badge), its validity and upcoming changes in the sub-line; earlier limits fold out under a chevron (edit / delete one interval); gaps without a limit are never rows; a budget whose limit has run out is an ordinary row with its date range, listed last (no separate "ended" state). Row menu: Upravit limit, then Smazat interval (only when the budget has more than one interval) and Smazat celý rozpočet (deletes every limit of the category, past months included, after a confirmation that says so). Footer: Zavřít left, Přidat rozpočet right. The limit form has Od / Do months side by side; the newest save wins: it replaces every limit it overlaps and shortens one it overlaps only in part. Do is optional ("Bez konce" clears it): without it the limit runs for good, with it the end month is included and afterwards the limit that applied before returns. Before replacing other limits the form lists them in a loss-tinted note and the primary turns into "Přepsat a uložit"; the second click saves. Equal neighbouring limits merge into one interval. |
| Settings | Settings | Left sub-navigation (existing routes), stacked flat cards, danger zone last. |
| Outlook | Projection | Horizon segment, hero with expected line, ± 2 p.p. scenario band, dashed "today + contributions" reference and future milestones (mortgage payoff, round net-worth marks), asset-class table, three scenarios, parameters modal. |
| Analysis | Stocks analysis | Performance first; no stats row and no allocation ring. A flat scope card "Které pozice analyzovat" holds the chip rows (one row per tag group, one tag or "Vše" per group, AND across groups; tags without a group in a "Štítky" row; from the fourth row on the rest hides behind "Další filtry" and an active hidden filter shows as a removable chip), a live summary with "Zrušit výběr" and a rule sentence. The full-width performance card in hero material carries three figures (time-weighted return of the whole portfolio in display size, money-weighted return with its gap to the TWR in p. b., change of value split into net deposits and portfolio gain; they and the dashed portfolio line stay whole-portfolio whatever the scope), the toolbar ("Rozdělit podle": starts with "Nerozdělovat", which draws the whole portfolio as the one solid `chart-line` (nothing to compare it with) or, with a filter on, the selection as one solid `s1` line against the dashed portfolio; then a divider, the tag groups, a divider, the automatic Sektor and Měna, which are offered only when they split the portfolio; with more than four of those the control is a select; an optional index, S&P 500 or MSCI World, as a dotted `ink-4` line; periods 3 měsíce · Letos · Rok · 3 roky · Vše · Vlastní, where Vlastní shows Od/Do date inputs and a range can also be dragged in the chart), a caption saying what each line adds up, the TWR chart (280 px, up to four parts in `s1`–`s4`, the portfolio dark dashed next to its parts and solid when it is the only line) and a drawdown panel on one date axis with labels at real dates. Hover draws the crosshair and rewrites the figures and the board to that day. The board under the chart doubles as its legend: return for the period, difference to the portfolio, contribution in p. b., maximum drawdown, share and value per part, undrawn parts with a hatched swatch. Below it the contribution card (its folded "Ostatní" row is a button that opens a 560 px modal listing every folded position or part with the same rows, bars scaled to them, and their sum) and the deposits-and-portfolio-gain card side by side at equal height, the tags table (period return and unrealized gain as of today, each a signed amount with its percent as a change badge that wraps under the amount in a narrow window; a group with at least two tags holding a position ends with a `well` "Celkem" row that sums its positions once, its period return from the group's own series), the holdings table (the same two columns, concentration in the subtitle), manage and assign modals (one choice per tag group). Every widget the chips narrow carries a dark "Filtr: Růst · USA" badge (the active chip's material) in its head: the performance card top right, in the hero's aside (not in the toolbar, which has no room to spare; its hover says the hero and the dashed line stay whole-portfolio), the contribution card and both tables; the deposits card does not follow the chips and has none. |
| Watchlist | Stock monitor and its detail | Native-currency prices, day change vs previous close, inline 52-week range bar with current price and target tick, target state badge; detail with target line on the chart, watch-start marker, markdown notes, key data. |
| Calculator | Annuity calculator, real estate investment calculator | Inputs in flat cards left, live hero result right (stats full width under both columns, or 2 × 2 under the hero when they stay in the results column), chart with milestones (principal overtakes interest, half repaid; positive cashflow, half of loan, sale), yearly tables. Annuity adds an optional extra yearly payment with the saving it brings. |
| Crypto | Crypto, crypto detail | List archetype of Stocks with a share column and buy/sell markers on the aggregate trend (stocks show the same trade days); the detail mirrors Stock detail: cost-basis step line, buy/sell events, realized gain, holding period, "since first buy" card. |
| Bonds | Bonds | Nearest maturity and weighted time to maturity as stats, a maturity-and-coupon ladder (principal returned and coupons per year), per-bond yearly coupon, "do roka" badge, matured issues behind a toggle. |
| Real estate | Real estate, real estate detail | Appreciation, rental yield and equity / LTV per property, value-vs-equity trend with every purchase (▲) and revaluation (○) marked; the detail has a valuation trace starting with the purchase (optional purchase date) followed by every estimate (the first estimate is recorded when the property is created), revaluations as events, cash-on-cash return, an annual balance that separates principal paydown from cashflow, a link to the real estate investment calculator and a "Přecenit" modal. |
| Other assets | Other assets | Gain/loss stat, per-row gain and annual yield, "oceněno" date, valuation-history trace shared with real estate, edit action. |
| Loans | Loans, loan detail | Debt trajectory (actual solid, schedule dashed) with every new loan marked ▲ where the debt steps up and milestones (car paid off, fixation end, half repaid, payoff), repaid progress per loan; the detail adds the schedule table (next 12 / per year), an extra-payment what-if (shorten the term or lower the payment) and an events list. |
| Insurance | Insurance, insurance detail | Policy (type · insurer underneath), premium, coverage total with the largest limit, next payment and anniversary per policy, a 12-month payment calendar; the detail has a cumulative-premiums trace with contract changes, a limits table, documents and a contract card. |
| Onboarding | First run | Two-column first run: the step form on the left, a live preview of the future overview on the right that reacts to the chosen areas. |
| Lock screen | Lock screen | One card: unlock with a decrypting state and inline error, a three-step recovery flow, language segment. |
| CSV import | CSV import wizard | Four-step wizard in a wide modal; the preview step shows duplicates and the category each row would get, uncategorized rows are fixed inline before the import; the done step lists next steps. |
| Categorization rules | Categorization rules | Evaluation-order strip, automation rate, hit counts per rule (90 days) in all three tabs, a rule editor with a live "matches N existing transactions" preview and a re-categorize option. |

## 8. Charts

Both themes use the same token names; the dark values are re-stepped (s1 lightest) and validated in §2.
The end-point ring and event marks are `paper` (never `#fff`), the area fade ends in `paper-2`.

Charts use Recharts with these rules (`charts.js` draws the same shapes by hand for the style guide):

- One y-axis, never two. Grid in `chart-grid`, value ticks right-aligned in `chart-axis` 10 px, time axis
  below with five labels, the last being "Dnes". Ticks are nice numbers (20 / 25 / 50 steps).
- Time axes of value charts put their labels at real calendar dates, not at evenly spread points: month starts, the 1st and 15th for short spans, quarters and years for long ones, with the year written on January.
- Value line 2.25 px `chart-line`, area gradient `chart-area` → transparent, last point a ring in `paper`.
  "Vloženo" (cost basis) is a dashed step line in `chart-cost`; the gap between lines is the return.
- Bars: income `s1`, expense `s3`, 3 px rounded ends, 2 px gap, no borders, zero baseline.
- Allocation ring: `s1`–`s4` + `s-other`, legend with amounts and percentages, never color alone.
- Hover always: crosshair + tooltip on lines, tooltip on bars. Tooltip is `.tip`. Value charts offer the horizons 30 dní · 3 měsíce · Letos · Rok · Vše, bounded on UTC days.
- Baseline: zero for "Vše" and bars; truncated for short periods with 15 % headroom below.
- Only one hue family per chart: the indigo series `s1`–`s4` (decision 2026-10-10: the warm greys read
  as unfinished and `s4` was nearly invisible; plain blue felt cold on the warm canvas). Green/red never
  appear in series, and value lines stay `chart-line` whatever the direction of the period.

## 9. Time trace

On charts of a **single entity** (stock position, crypto position, bank account, loan, real estate),
discrete events are drawn on the value line:

| Event | Mark | Color |
|---|---|---|
| Buy | ▲ filled, 12 px, 2 px `paper` ring | `chart-line` |
| Sell | ▽ outlined | `chart-line` |
| Dividend, income | ○ ring 9 px | `gain` |
| Large expense, payment | ○ ring 9 px | `chart-line` |

Behavior: hovering a mark shows the event tooltip (type, quantity, price, total, date); hovering a row in
the history table highlights its mark (`is-hot`, scale 1.35); clicking a mark scrolls to the row. Only
events inside the selected period are drawn. Aggregate charts (net worth, cashflow) never draw events; the
exceptions are the stocks and crypto list trends (one mark per trade day and direction, explaining the
jumps a buy or a sell causes), the real estate list trend (every purchase and every revaluation, the
initial estimate that repeats the purchase left out), the loans trajectory (new loans and milestones) and the net-worth line of the overview: a neutral ○ only on a big day-over-day step (at least 5 % of the period's largest net worth, at most 12 per period; a step only stocks or crypto made without a trade that day needs 10 % and reads "Pohyb trhu"). Its tooltip names the classes that moved and the dated facts behind each ("Nemovitosti +3,12 mil. · nákup Chlumec", "Úvěry −1,97 mil. · začátek Chlumec - byt"); a class without a fact shows its change alone. Bank account charts draw no movement events; dated balance records are drawn as marks (○) on the balance line. If more
than ~24 events fall in view, cluster neighbors into one mark with a count ("3 nákupy").

## 10. Copy and formatting

- Money through `useFormat().money()`: thin non-breaking thousands, comma decimals, unit after the number
  with a non-breaking space. Signed changes: "+ 60 110 Kč", "− 15 200 Kč" (true minus U+2212). Axis ticks
  shorten to "tis." / "mil.".
- Dates "2. 10. 2026"; lists use "Dnes", "Včera", "30. zář". Times "10:32".
- Uppercase only in eyebrows and table heads. Sentence case everywhere else. No exclamation marks.
- Buttons are verbs that name the outcome, identical to the modal title ("Přidat účet" → toast "Účet
  přidán"). Never "OK", "Odeslat", "Submit".
- Every string in every locale folder — all ten languages (`docs/standards/i18n.md`).

## 11. Decisions

1. **Dark theme was not part of v1** (2026-10-03). **Amended 2026-10-16:** the dark theme is in, as a
   second value block on the same token names, with the preference in Settings → General (System /
   Light / Dark, stored in the profile and mirrored on the device); spec
   `docs/specs/2026-10-16-dark-mode-design.md`.
2. **Time trace on every single-entity detail page**, as specified in §9. Aggregate charts stay free of
   events, except the ones §9 lists (the net-worth line marks only its big steps).
3. **Inter Variable is bundled** via `@fontsource-variable/inter`. No network fonts and no system-font
   fallback design.
4. **Cashflow is two pages.** *Cashflow* (`/reports/cashflow`) shows actual flows from bank transactions
   for one calendar month, quarter or year, chosen with the same period control as Budgets (default:
   the current year), with no comparison period. Three stats (income, expenses, savings with the savings
   rate), a twelve-month chart that mutes the months outside the period, then a two-column grid: expenses
   by category on the left, income by category and *Investice, spoření a převody* (money transfers by
   category) stacked on the right. Every breakdown row is clickable as a whole and opens its
   transactions, which can be recategorized in place. Amounts are calculated in the selected report
   currency with historical pair rates. Same-currency movements need no conversion. Currency is part of the
   amount; ordinary reports do not carry persistent technical FX explanations. Show compact
   warnings only when missing data affects the result.
   *Cashflow planning* (`/reports/cashflow-planning`, label key `nav.cashflowPlanning`, cs "Plánování
   cashflow", en "Cashflow planning") shows the planned recurring items. Every non-empty category is a
   collapsible group row with its included total (collapsed by default, remembered per browser); every
   row has one "···" menu — automatic rows (`isUserDefined = false`: Úvěry, Pojištění, Nemovitosti, Akcie,
   Dluhopisy, Bankovní účty) offer Nezapočítávat / Započítat, Nahradit vlastní částkou… and Otevřít v
   modulu, manual rows Upravit / Nezapočítávat / Smazat, category rows Nezapočítávat kategorii / Přidat
   položku do kategorie; the add dialog offers every category in one select grouped by section and
   income / expense, and a replacement is set only from the row menu (shown in the dialog as a note).
   A row left out stays listed, muted, with a "Mimo plán" badge and does not count
   in totals, stats or the flow bar (persisted in `cashflow_plan_exclusions`, honoured by the MCP
   `cashflow_report` tool too). A 100 % flow bar (expense groups `s1`–`s4` + Ostatní, hatched remainder)
   replaces the Sankey diagram. The two pages link to each other in the page subtitle.
5. **Dark theme principles (2026-10-16).** Warm near-black, never `#000`; depth from surfaces with wells
   deeper = darker in both themes; the contrast material inverts; series re-stepped and validated (s4
   2.0:1, legend mandatory); category glyphs lifted towards ink.

## 12. How the React app maps to this

- **Tokens.** The values of `tokens.css` live in `src/index.css` (the same declarations in both theme
  blocks; `src/index.test.ts` guards it) and are exposed as Tailwind names (`canvas`, `paper`, `well`,
  `ink-*`, `gain`, `loss`, `s1`–`s4`, radius and shadow scales) in `tailwind.config.ts`. Components
  never use raw color values. `ThemeProvider` (`src/theme/`) stamps `data-theme`, resets the chart token
  cache and exposes `useTheme()` / `useChartTokens()`. On Linux the native window theme is left to GTK
  (tao's cleared override would force light); the content follows `data-theme` and `color-scheme` on every
  platform.
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

## 13. Mobile

The phone app (spec `docs/specs/2026-10-16-mobile-app-design.md` §5, §6 and §10, approved prototype
`docs/specs/2026-10-16-mobile-app-prototype.html`) is this design system with a second component layer:
the same tokens, no platform-specific token name, and the same look on iOS and Android (Moony's own tab
bar, sheets and segment, Inter bundled). The reference width is 390 pt; every layout is checked at 360
(the narrowest common Android) and 430.

- **`mobile.css`** is the phone component contract, as `moony.css` is the desktop's: plain CSS on the
  tokens, every class prefixed `m-` so the two layers never collide. Its own `:root` defines the phone
  tokens (safe areas, bar heights, type scale); `src/index.css` carries the same block (keep the two
  identical). Its other sources are the tokens of `tokens.css` only, including `hero-orb`, `sh-seg`,
  `sh-destructive` and `loss-ring`, which exist in both themes (table in §2). `src/mobile/` matches
  `mobile.css` visually and never imports it. The phone is light only until Phase 6: `src/main.tsx`
  overwrites the `data-theme` that `theme-boot.js` stamped with `light` before the first render, the
  shell has no `ThemeProvider` (the shared toaster falls back to light through `useResolvedTheme()`),
  and `MobileApp` shows the window itself after its first render (`showWindow()`, capability
  `core:window:allow-show`) instead of waiting for the provider that does it on the desktop. tao
  ignores `visible: false` on iOS and Android, so the phone window is up from launch and the call is
  a no-op there today.
- **`mobile.html`** shows the components at 360, 390 and 430 with German copy (German runs longest): the tab root with Údržba, hero, stats, agenda rows (a dark date chip, a swiped row), the allocation ring and legend and the entity rows, the Finance tab root (compact hero, accounts with every kind of trailing sub and the badge in a title, the loan and policy heads with their one number, the trend card), the account detail (the nav sub-line, the button pair, the hero with a step line, two stats, the flows strip that scrolls while the page does not, the search, the toolbar with the dotted filter button, two day groups under sticky heads with badges in the sub-lines and a row swiped onto "Kategorie"), the Cashflow tab (period bar, savings hero, bars, budget note and category rows with their bars), Spravovat rozpočty and the limit sheet, the sheet with its amount fields, the filter sheet's pick rows, the transaction sheet (amount, label-and-value rows, the meaning segment, the folded details) and the category picker (search, suggestion pill, recent and all categories, the pinned learning switches).
  Under each frame a check reports anything that extends past the screen or scrolls it sideways, and
  lists the texts that end with an ellipsis. Open it after changing a phone component: nothing may
  overflow at 360. It also shows Majetek's tab root (the compact hero with the allocation bar, class rows with their change lines, an empty class and a class without a rate), the Akcie class page (the pushed screen with its sub-line, the hero with legend, the Pozice · Sledované segment, the toolbar with the sort pill and the position rows) and the position detail (the pushed screen of a position: the sub-line, the hero with the dashed cost line and its legend, the button pair, the six stats in two rows, the trade rows — one of them mid-flash — with the folded list, and the key-value card of the company). The import sheet is shown as its preview (a statement of another account: the file bar, the red box with its button and switch, the balance check, the counts, rows with a category, a manual choice, a missing category, a duplicate and an error, the footer's wrapping primary), its done step (stats, sentences, the primary and the next steps), its account stage (pick rows with a second line), the category choice over it, and the export guide (under the picker with a web route's button, as the list of banks when a file cannot be read, and one bank's guide opened in place with an app route). The categorize queue is shown as its page (the nav bar, the lead line, the count, the progress row, and eight rows: one with each pill row — a suggestion with its source, a long category name, no suggestion, a credit — a done row on `well` with its expansion, and one put off) at the three widths.

Phone type scale (Inter Variable, tabular numerals on every number; Tailwind `text-m-*`, which `cn()`
knows as type roles):

| Role | Size / weight / tracking | Token | Tailwind |
|---|---|---|---|
| Display (hero number) | 36 / 580 / −0.06em | `m-fs-display` | `text-m-display` |
| Large title | 30 / 600 / −0.05em | `m-fs-title-lg` | `text-m-title-lg` |
| Title (collapsed nav bar, sheet) | 20 / 650 / −0.04em | `m-fs-title` | `text-m-title` |
| Section | 16 / 620 / −0.03em | `m-fs-section` | `text-m-section` |
| Stat value | 17 / 600 / −0.03em | `m-fs-stat` | `text-m-stat` |
| Row title and value | 15 / 600 / −0.01em | `m-fs-row` | `text-m-row` |
| Body | 15 / 450, line 1.45 | `m-fs-body` | `text-m-body` |
| Row sub, badge | 12 / 500 | `m-fs-sub` | `text-m-sub` |
| Caption | 11 / 500, ink-3 | `m-fs-caption` | `text-m-caption` |
| Eyebrow | 10 / 700 / +0.12em, uppercase | `fs-eyebrow` | `text-eyebrow` |

The sentence under a large title or a sheet title is 13 / 450 `ink-3` (prototype). With the system text
size the text roles will scale up to 130 % and display and stat values up to 110 % (spec §10.2).

**Screen.** A screen is the nav bar (44 pt under the top safe area) over one scroll area with a 16 pt
gutter and room at the bottom for the tab bar (56 pt above the bottom safe area), which is fixed over it.
Safe areas come from `env(safe-area-inset-*)` as `--safe-top` / `--safe-bottom` (`index.html` asks for
`viewport-fit=cover`). The phone shell sets `data-shell="mobile"` on `<html>`; under it `src/index.css`
drops the desktop's 1080 px minimum body width and the tap highlight. Touch targets are at least 44 pt.
The document itself never scrolls: `#root` is one fixed box and every screen (`Screen`, the lock, the
first-run wizard) scrolls inside it. WKWebView gives a `viewport-fit=cover` page a native scroll range of
the safe-area insets, and a focused field used to slide the whole page up under the status bar. While the
on-screen keyboard is up, `src/mobile/lib/use-visible-height.ts` publishes the free height as `--m-vh` on
`<html>` and the box shrinks to it, so a field scrolls above the keyboard inside its own screen. Screens
outside the tabs (`<Screen tabBar={false}>`) keep no room for the tab bar.

**Lock and first run** (`mobile/pages/Lock.tsx`, `mobile/pages/FirstRun.tsx`, spec §8.1–8.2). One page
for `/auth`: the phone first run, the lock, and the three-step recovery as pushed screens with a back
button. The first run (`FirstRun.tsx`, one file per step under `mobile/pages/first-run/`, the flow in
`mobile/hooks/use-first-run.ts`, the step order in `mobile/lib/first-run.ts`) is the desktop's five steps
in one column without the live preview — Soukromí (currency and country as pick-row sheets), Účet, Klíč
(the key box with Zkopírovat and "Uložit do Souborů", two groups to verify), Co sledovat (switch rows),
První účet (name, amount field, currency) — then Biometrie ("Odemykat Moony obličejem?", Zapnout / Teď
ne; left out when the phone has no usable biometry) and Záloha ("Telefon je jediná kopie vašich dat…",
the first backup now / Teď ne). Every field and primary is 48 pt, text links 44 pt, the step progress
caption ("2 / 7 · Účet") sits over a 3 pt bar, and there is no way back once the database exists. The
desktop `OnboardingWizard` is not used on the phone. The lock is centred: brand, a 76 pt rounded paper
square with the lock glyph (the biometric button while biometric unlock is on), a 22 / 650 greeting, a
12 pt caption (`ink-3`), a 48 pt password field, the 48 pt primary button, the "forgot" link (44 pt
tall), and at the bottom the language menu (`LanguageSelect`, never a segment of flags) and the 11 pt
caption. Credential failures show under the field, everything else as a toast.

| Component | Class | React | Rules |
|---|---|---|---|
| Screen | `.m-screen`, `__body` | `mobile/shell/Screen.tsx`, `mobile/pages/CashflowCategory.tsx`, `mobile/pages/Budgets.tsx` | Fills the viewport (the fixed `#root` box, `h-full`). A tab root is `large`: its 30 pt title sits in the content, wraps and hyphenates. A page pushed on a tab is not: the back button names the tab ("Cashflow"), the title stays in the bar, and the body opens with one 13 / 450 `ink-3` line (the Cashflow drilldown: the period and the counts; Spravovat rozpočty: what a limit covers), which keeps the tab bar. Spravovat rozpočty also has a `+` (Přidat rozpočet) at the end of the bar, disabled while every expense category has a limit. |
| Nav bar | `.m-nav`, `--large`, `.is-collapsed`, `__side`, `__title`, `__btn`, `__back`; `.m-avatar` | `mobile/shell/NavBar.tsx` | At most one leading and two trailing 44 pt buttons (24 pt icons); the 20 pt title is centred between them and ends with an ellipsis. On a tab root the bar title and a hairline `line` appear once the large title has scrolled away; elsewhere the title always shows and the hairline appears on scroll. A back label ends with an ellipsis at 120 px. No blur. |
| Tab bar | `.m-tabbar`, `.m-tab`, `.is-active`, `__label` | `mobile/shell/TabBar.tsx` | Paper, hairline `line` on top, four equal tabs, Lucide 24 at stroke 1.75 in both states, label 10 / 600; colour alone marks the active tab (`ink`, the rest `ink-4`); no dark material (D5). Stays on tab roots and details, hides on the lock screen and under sheets. A tab switch is a history entry (Android back walks back); tapping the tab you are on at its root adds none. |
| Card | `.m-card`, `--well`, `--hero` | — | Paper, radius 15, `sh-1`; fewer cards than the desktop, a section is often a head and rows on the canvas. |
| Section head | `.m-sec`, `__t`, `__a`, `__n` | `mobile/components/SectionHead.tsx` | 16 / 620 title, 12 / 600 `ink-3` action (`__a`, a link whose hit area reaches 44 pt) or the section's one number (`__n`, the `note` prop: "Restschuld 2 418 760 Kč"; nothing to tap, never beside an action). |
| Hero | `.m-card--hero`, `.m-hero__label`, `__value`, `__delta`, `__seg`, `__chart` | `mobile/components/Hero.tsx`, `ScrubChart.tsx` | Label, display number, change line (badge + context), period segment, chart 160 pt; the desktop circle at 60 %. `step` (the account detail's balance records) draws a step line (`stepAfter`): a value holds until the next point, and the ○ marks sit on it (`stepValueAt`), not on an interpolated slope. `children` follow the chart, or in the compact hero (below) the change line: Majetek's allocation bar and legend, a chart legend. A position's hero adds the dashed "Vloženo" (`ScrubChart`'s `cost`: a 1.5 pt `stepAfter` line in `chart-cost`, dash `4 4`, drawn under the value line, the value domain widened to hold it) and the mark tap: a tap on a buy or sell mark opens its callout and calls `onMarkTap` with the events, so the page can flash the matching rows. |
| Compact hero | `.m-card--hero.m-card--compact` | `mobile/components/Hero.tsx` (without `points`) | Spec §8.4: Finance's "Celkem na účtech" is the hero without its segment and chart — label, number, change line (or "bez pohybů za 30 dní"), the card ending 14 px under the change line. Majetek's tab root is one too, with the allocation bar and its legend under the change line. |
| Stat row | `.m-stats`, `--2`, `--1`, `.m-stat`, `--flat`, `__l`, `__v`, `__n` | `mobile/components/StatRow.tsx` | Three (or two, or one — the account detail shows only the stats that have data) stats: label 11 (may wrap, hyphenated), value 17, note 10; values sit on one line across the row. The value keeps 17 px down to the 86 px box of a third at 390 pt, then shrinks with the card (19.7 cqi); a third holds about nine characters ("6,78 mil.", "214 380"), a longer amount goes in a row of two. A signed result takes the tone of its sign on the value (`.m-stat__v--gain` / `--loss`, `StatItem.tone`); the sign stays in the text. |
| Entity row | `.m-rows`, `.m-row`, `--single`, `--noicon`, `--chev`, `--bar`, `__chip` (`--sq`, `--dark`, `--gain`), `__main`, `__line`, `__t`, `__s`, `__v` (`--gain`, `--loss`), `__vs`, `__bar`, `__chev`, `--act`, `--muted` | `mobile/components/EntityRow.tsx`, `AgendaRow.tsx`, `mobile/pages/Budgets.tsx` | See the row rule below. A budget row of Spravovat rozpočty: the limit as the value, "Naplánováno" / "ukončen" as the trailing sub, and an ended budget `--muted` (the whole row and its divider at half strength). A row whose one action is a small button at the end (`--act`: "Výdaje bez limitu" with "Nastavit limit") is not a tap target itself; the button keeps its 44 pt hit area, so it is laid out like an Údržba row, not through the value slot (which clips). A row may carry a second block inside the same box (`EntityRow`'s `below`, the categorize queue's pill row): the row keeps its own 60 pt, its swipe panels and its menu, the block does not slide, and the divider and the background belong to both. |
| Badge | `.m-badge`, `--gain`, `--loss` | — | 20 px, 12 / 500; the desktop meaning (green and red only for a signed change, always with ▲ / ▼). |
| Row badge | `.m-row__badge`, `--loss` | inline in `mobile/pages/Finance.tsx`, `mobile/components/LedgerRow.tsx` | A badge inside a row's title ("Vyloučeno" after an excluded account's name): 18 px, 11 / 600 `ink-3` on `well-2`, 6 px after the name; the title still ends with an ellipsis. In a sub-line ("Bez kategorie", "Čekající", "Vratka") it has no leading gap and joins the other parts with " · ". `--loss` (red on `loss-soft`) is the import preview's "Chyba". |
| Segment | `.m-seg`, `__btn.is-active` | `mobile/components/Segment.tsx`, `mobile/pages/Stocks.tsx`, `mobile/components/wealth/PositionLedger.tsx` | 36 pt as on the desktop; scrolls sideways when it does not fit. Radix ToggleGroup (single): arrow keys move, the active option cannot be switched off. The hero and the trend card use it for the period. Akcie's Pozice · Sledované and a position's Vše · Nákupy · Prodeje are segments too. |
| Trend card | `.m-trend`, `__chart` | `mobile/components/TrendCard.tsx` | Spec §8.4 "Vývoj hotovosti": a section head (its note is the period's context, and while a finger scrubs the scrubbed value and date), then a `.m-card` holding the period segment (8 px under it) and the 120 pt scrub chart (the hero's is 160). |
| Buttons | `.m-btn`, `--primary`, `--secondary`, `--ghost`, `--danger`, `--destructive`, `--block`, `--sm`, `--wrap`; `.m-btn-pair` | — | 48 pt, radius 10, label 15 / 600. One primary (dark material) per screen; secondary paper with `line-strong`; danger text-only; destructive solid only in confirm sheets. Full width in sheets and under heroes; a pair shares the row and its labels wrap to a second line (13 px) instead of overflowing. Small (34 pt, inline in a card row) keeps a 44 pt hit area. `--wrap` is a footer primary whose label may be long (the import sheet's "48 Transaktionen importieren" beside "Abbrechen" at 360): it wraps to a second line and the button grows from 48 pt instead of ending in an ellipsis. |
| Button pair | `.m-btn-pair` | `mobile/components/ButtonPair.tsx` | The account detail's two buttons under the nav bar (spec §8.4, the kept exception to one primary per screen): primary "Zaznamenat zůstatek" and secondary "Importovat výpis", 48 pt, sharing the row 10 pt apart; a label that does not fit wraps to a second line at 13 px (hyphenated). A button of a pair can be `disabled` (`ButtonPair`'s `disabled`: 45 % opacity, no pointer events, as `.m-btn:disabled`): the Sell of a position that has been sold out. |
| Nav sub-line | `.m-nav__sub` | inline in `mobile/pages/finance/AccountDetail.tsx`, `mobile/pages/Stocks.tsx` and `mobile/pages/PositionDetail.tsx` | A pushed screen's second line ("Fio · CZK · 2100123456/2010"): the nav bar keeps its one 20 pt title, so the line is the first of the body, 11 / 500 `ink-3`, centred, one line ending with an ellipsis. |
| Flows strip | `.m-flows`, `__col`, `.is-muted`, `__bars`, `__bar--in`, `--out`, `__l` | `mobile/components/FlowStrip.tsx` | Spec §8.4 "Toky", in a card: 12 months, one 44 pt wide button per month — incoming (`s1`) and outgoing (`s3`) bars side by side (10 px, 2 px apart, radius 2) in a 110 pt area scaled to the strip's largest month, the bare short month name 10 / 500 `ink-4` under them ("Úno"; `fmt.month` gets `year: undefined`, since with a year Czech writes the long month) and January's two-digit year on a second line; a label ends with an ellipsis rather than outgrow its column. It scrolls sideways and opens at its end, the newest month at the right; the page never scrolls sideways. A tap selects a month (`aria-pressed`) and filters the ledger to it; the others fade to 45 %. The section head's number is the selected month, the card's caption its line ("+ 61 200 Kč příchozí · − 31 980 Kč odchozí · 41 pohybů"). |
| Search field | `.m-search`, `__i`, `__ic`, `__clear` | `mobile/components/SearchField.tsx` | 44 pt on `well-2`, radius 12, the 16 pt magnifier `ink-4` on the left, a 44 pt clear button on the right while there is text; the keyboard's key says "Search", no autocorrect. The placeholder is its accessible name. |
| Toolbar, icon button | `.m-toolbar`; `.m-iconbtn`, `__dot` | inline in `mobile/pages/finance/AccountDetail.tsx` and `mobile/pages/Stocks.tsx` (`NavIconButton`) | The ledger's row under the search: the segment takes the row, a 44 pt icon button on paper (`line-strong`, radius 12, 20 pt icon) opens the filter sheet; a 6 pt `dark` dot while a filter in the sheet is on (category or period, not the segment or the search). |
| Day head | `.m-dayhead` | `mobile/components/DayHead.tsx` | "Dnes", "Včera", "14. 10." over a day's rows: 11 / 600 uppercase `ink-3`, sticky at the top of the scroll area while the day passes (each day is its own block), reaching the screen edges on `canvas`, which hides the rows sliding under it; z 3, above the rows' dividers. |
| Ledger row | `.m-row` (the entity row) | `mobile/components/LedgerRow.tsx` | A movement: the counterparty's initials (a credit's chip in `gain`, a transfer's arrows), the name, the sub "Kategorie · detail" with the badges, the signed amount in its own currency ("− 1 249 Kč", a credit "+ …" in `gain`). Tap and swipe right open the details, swipe left the category picker; a long press opens the row menu (Podrobnosti, Kategorie, Smazat). Rows load page by page (100) as the end of the list nears the screen's bottom. |
| Pick row | `.m-pick`, `__chip`, `__main`, `__l`, `__s`, `__check` | `mobile/components/PickRow.tsx` (`PickGroup`, `PickRow`, `NeutralChip`) | A radio row of a picker sheet (the ledger filter: Kategorie, Období; the category picker's lists): 44 pt, an optional 28 pt chip, the label 15 / 450 with an ellipsis (with `sub`, a second 12 / 500 `ink-4` line under it in the same column: an account's "Fio · CZK", a statement's currency and payment count), a check on the chosen one; rows `line-soft` apart. "Vlastní" adds two 48 pt date fields (`.m-field__i`). The sheet edits a copy: the primary applies it, the ghost resets the filter. |
| Label and value | `.m-kv`, `--tap`, `--stack`, `__l`, `__r`, `__v`, `__s`, `__chev`; `.m-kvcard` | `mobile/components/KeyValueRow.tsx`, `mobile/components/wealth/CompanyCard.tsx` | The transaction sheet's row: 44 pt at least, rows `line-soft` apart. The label is 12 / 500 `ink-3` at the left (it wraps, up to 40 % of the row), the value 15 / 450 `ink` at the right, one line ending in an ellipsis, with an optional 11 `ink-4` line under it. `--tap` makes it a button with `well` while pressed and a 16 pt `ink-5` chevron: **the contract's chevron exception for settings-style rows** — Kategorie has a value *and* opens a picker, like the rows of Nastavení and Dokumenty, so the chevron says "this opens". Other rows (Datum, Měna…) are inert and have none. `--stack` puts the label over a full-width value that wraps (the note, the Význam segment with the meaning it comes to as its sub-line). The company card ("O společnosti") is a `.m-card` of such rows, 4 pt of padding above and under and 16 at the sides, `line-soft` between the rows: the name and the currency, then what Yahoo reported (rows without a figure left out) or why there is nothing — a skeleton while it comes, a note for an ETF, a note for a ticker Yahoo does not know, "could not be loaded" with a retry. |
| Disclosure row | `.m-disclose` | inline in `mobile/sheets/TransactionSheet.tsx` | The "Podrobnosti" fold under the sheet's rows: 44 pt, the label 15 / 600 `ink-2`, a 16 pt `ink-4` chevron down (folded) or up (open), `aria-expanded`. It folds `.m-kv` rows (bank id, value date, file, line, source, status, currency; the ones the payment does not have are left out) and opens folded for every row. |
| Switch row | `.m-switchrow`, `__main`, `__t`, `__s`, `__sw` | `mobile/components/SwitchRow.tsx` | A setting with a switch: 44 pt at least, the label 15 / 450 `ink` and a 12 `ink-4` sentence under it (it wraps) at the left, the 34 × 20 `Switch` (dark material when on) at the right. The whole row is the `<label>`, so a tap anywhere toggles it, and the sentence describes the switch to a screen reader. A switch that cannot be on is off and disabled (45 %), the sentence says why. |
| Suggestion pill | `.m-pill-row`, `.m-pill`, `__chip`, `__x` | inline in `mobile/sheets/CategoryPickerSheet.tsx` | The category an MCP client suggested: a 36 pt pill (radius 10, dashed `line-strong` on paper) with the sparkles, the category's 24 pt chip and its name; a tap accepts it (hit area 44 pt). Beside it a 44 pt × declines. Shown only while the row has no category. |
| Queue page | `.m-queue__lead`, `__count`, `__prog`, `__cap`; `.m-pbar` | `mobile/pages/finance/CategorizeQueue.tsx`, `mobile/hooks/use-categorize-queue.ts`, `mobile/lib/categorize-queue.ts` | Spec §8.5, prototype frame 05, `/finance/uncategorized` (`?account=<id>` for one account): the nav bar's title is "Nezařazené platby" with "Finance" as the back label; the body opens with the lead line (13 / 450 `ink-3`: the account and "z výpisu 10. 10.", or "3 účty", then "· 3 už vyřízeny"), the count as a 20 / 650 heading ("9 plateb bez kategorie" — the prototype's large count, kept in the body because the nav bar already carries the title), the progress row (`.m-pbar`, "3 z 12" 11 `ink-3`) and the rows, then the frame's caption. The rows are a snapshot taken when the page opens: a row that is dealt with stays where it is (green pill on `well`, or "nechána na 30 dní"), so nothing jumps under the finger, and counts in the progress. Empty: the check, "Všechno zařazeno", one sentence and the one primary "Zobrazit cashflow" (it replaces count and list — except that the done rows stay under it while an expansion can still be undone, because the last accept may have taken other payments along). Údržba's "Zařadit" and the Finance row open it, of one account when exactly one holds the payments. |
| Queue row | `.m-queue__item` (`is-done`, `is-left`), `.m-pillrow`, `.m-pill` (`--dark`, `--ghost`, `--done`, `__t`), `.m-queue__note`, `.m-queue__more` | `mobile/components/QueueRow.tsx`, `mobile/components/queue/PickRunner.tsx`, `mobile/components/EntityRow.tsx` (`below`) | One payment of the queue: the ledger's entity row (initials, counterparty, "den · detail", signed amount in its own currency, hidden with the eye like every amount) and, in a second block of the same row, the pill row of 36 pt pills (radius 18, 12 / 600, hit area 44 pt, wrapping when a name is long). With a suggestion: the dark pill (check, category, its source in 10 / 500 — "návrh z MCP", "naučené", "podle pravidla", "balíček pravidel"; a tap accepts it, with a haptic tick) and ghost "Jiná…"; without: the plain "Vybrat kategorii…" and ghost "Nechat". Done: the green `gain-soft` pill with the category's name (not a button) on `well`; after a pick that took other payments along, once, a hairline and a switch row "Zařazeny i další 3 platby od Lidl" (on; off restores them and the switch stays off). A later write that reaches one of those payments — its own pick, an accepted suggestion, another pick's batch — takes it from the switch, which then restores only the payments it still holds; a done row shows the category of the newest write that reached it (`withPick`, `releaseRow`, `withBatch`). Put off: the clock sentence on `well`. A tap on the row opens the category picker, swipe right "Podrobnosti", swipe left "Nechat" (30 days, no undo); the long-press menu has all three. |
| Transaction sheet | `.m-sheet--full`, `.m-sheet__amount`, `.m-kv`, `.m-disclose` | `mobile/sheets/TransactionSheet.tsx` | Spec §8.4. Full-height sheet: the counterparty as the title (the ledger row's own title) and the long date under it; the amount in the display type on one line (it shrinks to fit), signed and in its own currency, a credit in `gain`; rows Protistrana (the account number as its sub-line), Datum, Kategorie (opens the picker over this sheet), Význam pohybu (Automaticky · Příjem · Výdaj · Přesun peněz as a segment; on Automaticky the meaning it comes to and why, "Výdaj · směr pohybu") and Poznámka, then "Podrobnosti" folded. The footer: Smazat transakci as a text danger button on the left (it asks in a confirm sheet), the primary Hotovo on the right. A change of meaning saves at once (toast "Význam uložen"). |
| Category picker | `.m-sheet`, `.m-search`, `.m-pill`, `.m-pick`, `.m-learn`, `.m-switchrow` | `mobile/sheets/CategoryPickerSheet.tsx`, `mobile/hooks/use-categorize.ts` | Spec §8.4. A half sheet: the search, the suggestion pill (head "Návrh"), "Naposledy použité" (up to five distinct categories of the account's newest categorized rows — the whole history, not the filtered ledger; only while the search is empty), "Všechny kategorie" (top-level, in sort order, the current one checked), "Nová kategorie" as a plain action row (it opens the desktop form over the sheet and picks the new category). A tap on a category saves it at once and closes the sheet; a failed save leaves it open. Under the list, pinned to the bottom of the body (`.m-learn`) so they are in view however long the list — they follow the list instead while the search has text, because with the keyboard up the list needs the room — are the desktop's two learning options: "Zapamatovat pro tento účet a směr" (on; off and disabled with the reason for an own-account transfer or a payment without payee or account; "Změnit kategorii tohoto pravidla" when a custom rule decides) and "Zařadit i dalších N plateb" (on; only while remembering and other payments match, its sub-line the breakdown "12× Potraviny · 2× bez kategorie"). After the save, learning runs on its own and says so in a toast with "Vrátit zpět" (8 s). A caller that passes `onApplied` (the categorize queue) takes what the learning categorized itself — its expansion has the undo — and the sheet leaves out the toast of a pick that took other payments along (a pick that took none, and a re-pointed custom rule, are still said). The sheet stacks over the transaction sheet; Android back closes the newest sheet only (`lib/sheet-history.ts`). |
| Import sheet | `.m-sheet--full`, `.m-pick`, `.m-note`, `--ok`, `.m-box--loss`, `.m-imp-warn`, `.m-imp-act`, `.m-imp-confirm`, `.m-imp-lead`, `.m-imp-sub`, `.m-imp-wait`, `.m-btn--wrap` | `mobile/sheets/ImportSheet.tsx`, `mobile/hooks/use-import-sheet.tsx` (provider), `mobile/hooks/use-import-flow.ts` (the stages), `mobile/components/import/StageBodies.tsx`, `PreviewBody.tsx`, `ImportNote.tsx`, `ReceivedFileHost.tsx`, `mobile/lib/import-flow.ts`, `mobile/lib/received-queue.ts` | Spec §8.6. One tall sheet whose stages swap the title, the sentence, the body and the footer; one primary in the footer at most, and none that the stage cannot act on. `ImportSheetProvider` (above the routes, mounted only while unlocked) owns it and `useImportSheet().openImport({ accountId?, path?, source })` opens it from an account's "Importovat výpis", the Údržba card, Nastavení → Import or a shared file; a sheet that is already open is left as it is, a lock closes it. A statement shared into the app (iOS share sheet, "Otevřít v Moony") reaches it through `ReceivedFileHost`, which draws nothing and is mounted once in the shell: Rust keeps the file until the app is unlocked, the first drain (`receivedFilesApi.take()`) runs at the unlock, "received-files" asks for another, drains run one at a time, and `createReceivedQueue` opens each path once and one at a time — a path that arrives while a sheet is open waits for it to close. **Pick** ("Vybrat soubor": the Files picker with no type filter — a bank's export may be `.STA`, `.940` or have no extension, with the account's bank guide under it, see Export guide), **reading** (the file bar and a turning arrow), **statement** (several statements, and for QIF how the dates are written, as radio rows; "Pokračovat" confirms them), **account** (the open accounts as radio rows, a tap chooses and goes on; "Nový účet…" opens the desktop's account form and chooses the new account — asked only when the file, the bank's preset or the entry point cannot tell), **recognize** and **correct** (an unknown CSV, see Import recognition), **preview**, **importing**, **done**, **unreadable**. `unreadable` says plainly what the file is (a PDF, a photo or scan, no statement, or a read error already translated — never a raw message) and what can be read; its primary is "Vybrat jiný soubor", and under it the list of bank guides (Export guide). A statement is told from its content (`inspect_statement_file`); a CSV a bank preset describes goes straight on (`presetReadable`: rung 2 is a preset match only), any other CSV with two named columns and a row is recognized from its content first (`inferCsvColumns`, then the **recognize** stage and, when the person says so or the inference could not tell, **correct**) and only a file with no such table — an empty one, one column, a bare header — is `unreadable`. Duplicates are skipped (no "import anyway" on the phone) and a QIF file's source categories are not mapped; the rules categorize. The copy iOS made of a shared or picked file is discarded when the sheet closes (any stage), when another file replaces it and when the sheet is unmounted; Rust accepts only files in its Inbox folders. |
| File bar | `.m-filebar`, `__ic`, `__main`, `__t`, `__s`, `__s--wrap` | `mobile/components/import/FileBar.tsx` | The file the sheet is working on, first in every stage after the picker: a 36 pt paper chip with the file glyph, the name 15 / 600 and a 12 / 500 `ink-4` line under it ("CAMT.053 · CZ65 0800 …", "Fio · CSV"), both ending in an ellipsis, on `well`, radius 12, 52 pt at least. The recognition's line wraps instead (`FileBar wrapMeta`, `--wrap`): all of it is news. |
| Export guide | `.m-guide`, `__t`, `__steps`, `__n`, `__fmt`, `__back`, `.m-pick__chip--ini`, `.m-imp-act`, `.m-btn--secondary` | `mobile/components/import/ExportGuide.tsx` (`ExportGuide`, `BankGuides`), `mobile/lib/export-guide.ts`, `mobile/components/import/StageBodies.tsx` | Spec §8.6 rung 2: how to get a statement file out of one bank on a phone. The steps are data — the `mobileExport` block of the bank's preset (`src-tauri/resources/csv-presets/*.json`, a `route` of `app` or `web`, the `format` to pick, an `https://` `webUrl`, three steps in each of the ten languages, the person's language else English; `appUrlScheme` stays `null` until a bank documents one) — and the sheet only shows them: the section head (16 / 620, wrapping: "Jak získat výpis: Česká spořitelna"), the steps as a numbered list (a 22 pt `well-2` number chip, the step 13 / 450 `ink-2`, wrapping), the format as a caption (11 / 500 `ink-3`) and, for a `web` route, a secondary 44 pt button "Otevřít internetové bankovnictví" with the external-link glyph that opens `webUrl` in the system browser (the opener plugin; the only network use, on a tap). Under "Vybrat soubor" it is the guide of the bank of the account the import was asked for (`guideForAccount`, by institution; nothing without one). When a file cannot be read the banks that have a guide follow the "Vybrat jiný soubor" sentence as "Návody bank" (`guidesList`: country, then name), plain 44 pt `.m-pick` rows with the bank's initials in the 28 pt chip and its country as the second line; a tap opens that bank's guide in place under a 44 pt text button back to the list. |
| Import recognition | `.m-recog`, `__row` (`--no`), `__ic`, `__f`, `__v`, `__warn`; `.m-filebar__s--wrap`; `.m-rows` | `mobile/components/import/RecognizeStep.tsx`, `field-text.ts`, `mobile/hooks/use-recognition.ts`, `mobile/lib/recognition.ts` | Spec §8.6 rung 3, prototype frame 06: an unknown CSV, never a mapping grid. Under the file bar ("48 řádků · Oddělovač „;“ · kódování UTF-8 · formát zatím neznám", it wraps) the section head "Takhle tomu rozumím" (16 / 620), "Tři pohyby ze souboru tak, jak je načtu:" (12 / 500 `ink-3`), three real rows of the file drawn as entity rows by `PreviewRows sample` (`previewCsvImport` with `previewLimit: 3` against the anchor account — the preselected one, else the first open one; the sub-line is "datum · k čemu platba byla", a duplicate is no duplicate here, a row that cannot be read says so; a failed preview leaves the list and one quiet sentence, never a raw error) and the list of what was recognized: a 16 pt check in `gain` (a minus in `ink-5` for a field that is not read), the field's name 13 / 600 (the desktop's `transaction.*` / `fields.*` strings, "ID pohybu" its own) and the columns it reads 12 / 500 `ink-3`, in a 20 pt / 34 % / rest grid, rows `line-soft` apart, every text wrapping. A field that is not read is at `ink-4`: "nenalezeno" (the inference found nothing) or "nepoužito" (the person left it out), and what that means — ID pohybu "duplicity poznám podle data a částky", Popis "platby se hůř zařazují do kategorií", Protistrana, Měna "měna se vezme z účtu", Zůstatek. The amount line adds "znaménko ověřeno podle zůstatku" when the balance proved the sign, and — `loss`, with the warning glyph — "výdaje v souboru vypadají kladně — zkontrolujte ukázku" when it shows them inverted; an amount read from a debit and credit pair names its sides ("Výdaje: Odchozí · Příjmy: Příchozí"). The import cannot flip signs, so a file the balance proves inverted cannot be imported as read: under the rows the red box (`.m-box--loss`) says so plainly ("Soubor, ve kterém jsou výdaje kladně, zatím Moony naimportovat neumí. Pokud částku obsahuje jiný sloupec, vyberte ho, jinak import zrušte.") with "Opravit…" as the action that resolves it, the footer is "Zrušit" and "Vypadá to správně" disabled, and the preview's import stays blocked too (`signsBlockImport`); another amount column lifts it. Footer otherwise: "Opravit…" ghost and "Vypadá to správně" primary, which chooses the account (as for a preset file) and goes on with the mapping the person saw. The recognition **opens on a correction** instead when there is no date, when there is neither an amount nor a debit and credit pair (the app never guesses either silently), or when the day and month of the dates cannot be told (the QIF date-order question): its footer is "Zrušit" and "Použít"; the questions come one after the other — the date, then the amount, then the order of an ambiguous date — so a file missing both is asked both, and only a required field still empty after its own question (in practice the person cancelling) is `unreadable` ("Tento soubor neobsahuje výpis, který bych uměl přečíst" and the bank guides). Tapping "Opravit…" while "Vypadá to správně" is still fetching the accounts supersedes it (every question opened or answered bumps the sheet's read generation, and the answer is used only on the recognition's own stage: `recognitionAnswerWanted`). The CSV is imported with the accepted mapping (`decimalSeparator` left to detect). Taps from the share sheet to "Hotovo" for an unknown CSV: share, "Vypadá to správně", ("Který účet?"), "Importovat", "Hotovo" = 4–5; with one deliberate correction "Opravit…", the field, the column, "Použít" make it 8 (9 when the account has to be asked); a correction the recognition asks itself costs one or two taps — "Použít" alone for a guessed date order (the guess is checked), the column and "Použít" for a missing date or amount. |
| Column option | `.m-colopt`, `__radio`, `__t`, `__cap`, `__s`; `.m-row--chev` | `mobile/components/import/CorrectStep.tsx`, `mobile/lib/recognition.ts` (`columnOptions`) | Spec §8.6 rung 3, prototype frame 07: "Opravit…" lists the fields as entity rows with a chevron (a rounded-square chip with the field's glyph, what it reads now; Datum, Částka, Popis, Protistrana, Měna, Zůstatek, ID pohybu); a field opens its question ("Co je částka?") with one sentence under it and the file's columns as radio options in a `radiogroup`. An option: 56 pt at least, paper with a `line` hairline, radius 12, 8 pt apart; a 22 pt radio circle (`line-strong`; the chosen one `ink` with a white check) and the column's header 15 / 600 (it wraps) over its two sample values 12 / 500 `ink-4` (tabular, one line, an ellipsis ends a long one); the chosen option has the focus border and ring. A pair of columns (debit and credit) is titled by its sides, "Výdaje: Odchozí · Příjmy: Příchozí", and carries "(dvojice sloupců)" after them; its samples read "out / in". The inferred pair is offered the way the inference oriented it; neighbouring numeric columns of which exactly one is filled per row are offered the way a header names a side, else both ways round — nothing tells which is the money out, so the person says it; the description offers the columns it was inferred from together; an optional field has "— nepoužít —" first, a required one (date, amount) never; a date column whose day and month cannot be told — the inferred one by the inference's own answer, any other column by its sample cells (all with the day and month at most 12; the sample rows can only over-ask, never guess) — shows the column twice, day-first and month-first, each with its date spelled out first ("3. dubna 2026 · Datum zaúčtování", the line wraps) — the question the QIF import asks, reused; choosing a column that reads both ways is choosing one of its two readings, so no date order is ever left to the import's guess and the recognized date line names the order. Under the options "Ukázka po výběru:" and the file's first row re-read with the choice (a sentence instead while the choice would leave no date or amount); on the amount question a single column whose sample amounts are all positive and that nothing proves (any column but the inferred one) adds under it, in `loss` with the warning glyph, "Všechny částky jsou kladné — výdaje by se uložily jako příjmy." (`amountsAllPositive`); footer "Zpět" (to the list of fields) and "Použít" (disabled until an option is checked) — "Zrušit" instead of "Zpět" when the recognition asked the question itself. While amounts are hidden (`maskedSample`) every digit of the cells of a money column shows the mask — the inferred amount, debit, credit and balance, the columns the mapping reads as one now (also after a correction), the columns of a pair that looks like a debit and a credit, and every numeric column on the amount and balance questions and while there is no amount yet (the date question of a file missing both: "25000" says nothing of itself) — and in any other column so does a cell that says it is an amount: a sign, a decimal mark, a thousands space, a currency or "1500,-" (`readsAsAmount`: "−1 249,00", "5 000", "-1249"). Dates, text and a plain integer that may be an id ("26100000001") stay, so on the amount question the amount can still be told from the date. |
| Import preview rows | `.m-imp-count`, `.m-imp-meta`, `__loss`, `.m-imp-hint`, `.m-imp-more`, `.m-imp-rows`, `.m-row`, `--muted`, `.m-row__badge`, `--loss` | `mobile/components/import/PreviewRows.tsx`, `PreviewBody.tsx`, `mobile/lib/import-flow.ts` (`previewAmount`) | The preview keeps the desktop's guarantees on one column: under the file bar what the statement says about itself — whose account it is (a statement of another account is the red box with "Importovat do účtu X", a 44 pt text button that switches the target and reads the file again for it, and a switch "Importovat i tak do tohoto účtu" that must be on before the import; a statement of none is the same box with the switch), whether its balances add up (the quiet check, or the red difference), a currency that differs and the reader's warnings — then "Naimportuje se N transakcí · M duplicit přeskočeno" 15 / 600, the period and the counts with and without a category 12 `ink-3`, errors in `loss`, and the first 50 rows as entity rows: the counterparty's initials (a credit's chip in `gain`), its name, "datum · kategorie" ("Bez kategorie" as the ledger's badge, a category the person chose marked "ručně"), the signed amount in the row's own currency, masked while amounts are hidden with the sign kept (`signedAmountText`). A duplicate or a skipped row is at half strength with its reason as a badge; an error row says what is wrong; a row that will be written opens the category choice on a tap. "a N dalších" counts the rest. The primary says "Importovat N transakcí" and waits for the preview. |
| Import done | `.m-stats`, `.m-imp-facts`, `.m-imp-primary`, `.m-btn--block`, `.m-rows`, `.m-row--chev`, `--muted` | `mobile/components/import/DoneStep.tsx`, `mobile/hooks/use-auto-categorize.ts` | Spec §8.6. Three flat stats (Importováno, Duplicity, Chyby), the sentences under them (the statement's balance, or the balance after the last payment; how many payments the rules categorized and how many are left; pending payments that were booked — all amounts masked while hidden), the one primary "Zařadit M plateb" (only while M > 0; it closes the sheet and pushes the categorize queue of the account, after the sheet's history entry is off the stack) and the desktop's next steps as rows with a chevron: "Zobrazit importované" (the account), "Zapnout balíček pravidel" (turns the suggested pack on, then runs the rules over the account's payments without a category — the desktop's auto-categorize) and "Nastavit zůstatek" (a confirmation sheet with the desktop's sentence, then the dated balance from the statement; blocked with the reason when the currency differs). A step that was carried out is ticked and at half strength. The footer is "Hotovo" alone. |
| Category choice | `.m-sheet--full`, `.m-search`, `.m-pick`, `__main`, `__s` | `mobile/sheets/CategoryChoiceSheet.tsx` | A plain choice for the preview's rows, stacked over the import sheet: the search, "Podle pravidel" first (it takes the person's choice back; the rules decide again), then the top-level categories with a check on the chosen one. A tap chooses and closes; nothing is saved and nothing is learned (unlike the category picker), and the choice rides with the import (`categoryOverrides`). |
| Date chip | `.m-row--date`, `.m-dchip`, `--soon` | `mobile/components/AgendaRow.tsx` | The agenda row's 54 × 42 chip on `well` (radius 9): the day 12 / 650, its distance under it 10 / 500 `ink-4`; dark material for a deadline at most 7 days away. The row's first column is 54 px, so its divider insets to 80. |
| Swipe actions | `.m-swipe`, `__panel`, `--start`, `--end` | `mobile/components/EntityRow.tsx`, `mobile/hooks/use-swipe-actions.ts` | The row slides over a 98 px panel of dark material that waits under it: 18 px icon over an 11 / 650 label. It follows the finger up to the panel's width and fires the action when released past 60 px, then snaps back; a vertical drag scrolls the page. The `li` clips the panel and carries the divider (`--m-row-inset`). A long press opens the row's menu. |
| Row menu | (Radix `DropdownMenu`, `components/ui/dropdown-menu.tsx`) | `mobile/components/cashflow/RowMenu.tsx`, `AgendaRow.tsx` | The context menu of an entity row, the agenda row's pattern: the row's long press (or right click) opens it, under the row's right edge, 220 pt wide at least; items 44 pt tall at the body size with 18 pt icons (`RowMenuItem`). Its `···` trigger is the row's `accessory`, outside the part that slides: 44 pt, hidden until it has keyboard or screen-reader focus, named for the row, so no action is reachable by gesture alone. |
| Allocation | `.m-alloc`, `.m-ring`, `__c`, `.m-leg` | `mobile/components/AllocationRing.tsx` | In a card: the 110 px ring (22 px thick, `s1`–`s4`, the folded bucket in `s-other`, at most four classes and "Ostatní") on the left, its label in the middle, the legend (swatch 10, name 12 / 600, share) on the right. The share is always written. |
| Period bar | `.m-period`, `.m-stepper`, `__l`, `__btn` | `mobile/components/cashflow/PeriodBar.tsx` | Cashflow (spec §8.7). The full-width 36 pt segment Měsíc · Čtvrtletí · Rok (as the hero's) and under it the stepper: two 44 pt icon buttons (`.m-nav__btn`, 24 pt icons at stroke 1.75) around the period's name, 15 / 650 `ink`, tabular ("Říjen 2026", "Q4 2026", "2026"). "Next" is disabled at the current period; a switch of granularity returns to the current period. |
| Cashflow hero | `.m-card--hero`, `.m-hero__value--loss`, `.m-hero__delta--note` | `mobile/components/cashflow/CashflowHero.tsx` | The hero without segment and chart: label, the display number (green for a surplus, red for a deficit, "—" without a figure) and one 20 pt note line ("míra úspor 24 %"). |
| Bars | `.m-bars`, `__taps`, `__tap` | `mobile/components/cashflow/CashflowBars.tsx` | Twelve months of income (`s1`) and expenses (`s3`), 110 pt: the desktop bars' rules (README §8), months outside the selected period at 35 % opacity, no tooltip. A transparent button per month lies over the plot at its full height (`aria-pressed` for the months in the period) and a tap opens that month; the value axis shows the mask while amounts are hidden. A short month name stands under every third bar, the legend under the axis. |
| Progress bar | `.m-pbar`, `--share`, `--over`; `.m-row--bar`, `.m-row__bar` | `mobile/components/cashflow/ProgressBar.tsx`, `EntityRow.tsx` (`bar`) | 6 pt `well-3` track, radius 3; the fill is `s1` (used part of a limit), `s3` (a category's share) or `loss` (limit exceeded, full). It is decoration: the sub-line says the number. A row with a bar is the third line of the row's main column, 6 pt under the sub-line, about 69 pt tall and never single-line. The sub-line of a category row puts what matters first and the count last, so an ellipsis drops the count: the share ("47 % výdajů · 3 pohyby") or a limit's progress ("3 206 / 3 000 Kč", `loss` when over) · the state word ("blíží se limitu", "nad limitem") · the count; the amount of an exceeded limit turns `loss` too (`valueTone`, `.m-row__v--loss`). |
| Note | `.m-note` | `mobile/components/cashflow/BudgetSummaryNote.tsx` | A quiet line on `well`, radius 11, at least 44 pt, 12 / 500 `ink-2` with a 15 pt `ink-3` info glyph; wraps rather than ends in an ellipsis; a link when it leads somewhere. The budget summary under the bars ("Rozpočty: 5 v mezích · 1 nad limitem · 2 blíží se limitu") shows only for a month with limits, and its counts stay while amounts are hidden. |
| Category chip | `.m-row__chip--cat` | `mobile/components/cashflow/CategoryChip.tsx` | The 36 pt round chip with the category's icon (16 pt) in its own hue at 15 % over paper (the inline background, hex alpha 26, the glyph `--cat`; the dark theme lifts the glyph 30 % towards ink, as the desktop's `cat-chip`). Without a hue the icon sits on `well-2`; the uncategorized group shows a tag. |
| Allocation bar | `.m-abar`, `__other`, `.m-legend`, `__item`, `__swatch` | `mobile/components/wealth/AllocationBar.tsx` | Under the change line of the compact hero (Majetek): an 8 px bar of one share per class (the four largest in `s1`–`s4`, the rest folded into "Ostatní" in `s-other` with a `line` ring), then a wrapping legend of swatch 10, name 12 / 600 and share. The share is always written; the bar's accessible name lists the shares with their amounts. It counts the hero's total, so a class the menu preference hides is in it. |
| Change line | `.m-change`, `__badge`, `--gain`, `--loss` | `mobile/components/wealth/ChangeLine.tsx` | The trailing sub of a row: the 18 px badge (▲ / ▼ and the share, green or red; neutral at exactly zero; none without a share to write) and then the signed amount, or a date where the amount would be ("▲ 1,3 % 5. 10."). Nothing when the value has no base or could not be valued. |
| Sort pill | `.m-sortpill` | `mobile/components/wealth/SortPill.tsx` | The sort of a list (Akcie: Podle hodnoty / výnosu / názvu): 34 pt on `well-2`, radius 10, the chosen option 12 / 600 `ink-2` and a 14 pt chevron; a 44 pt hit area reaches past it. It opens a radio menu (Radix `DropdownMenuRadioGroup`, items 44 pt) headed by the control's name, and the pill's accessible name is "Řazení: Podle hodnoty". In Akcie's toolbar it sits at the start and the search toggle (an `.m-iconbtn`, `aria-pressed`) at the end; the search field opens under the toolbar, and closing it clears the search. |
| More row | `.m-rows__more` | `mobile/components/wealth/MoreRow.tsx` | The last row of a folded list ("Zobrazit všech 12 pozic" after the first eight, "Zobrazit všech 28" under a position's transactions): 52 pt, the label centred 15 / 600 `ink-2`, its divider inset 14 pt like any row without a chip; a tap unfolds the list in place. |
| Trade row | `.m-row`, `.m-row--flash`, `.is-hot`, `__chip--gain`, `__v--gain` | `mobile/components/wealth/TradeRow.tsx` | The entity row of a buy or a sell of a position: a ▲ chip (▽ on `gain` for a sell), "Nákup · 12 ks" over the day and the price in the trade's currency, the total at the rate of the trade's day as the value ("− 32 750 Kč"; "+ 12 880 Kč" in `gain` for a sell) over the total in the trade's own currency. Without a rate for that day the native total stands in the value and the sub says "bez kurzu pro EUR". A tap or a long press opens the row's menu (Upravit, Smazat). A tap on the trade's mark on the chart flashes the row: it takes `well` at once (`.is-hot`) and fades back to paper over 1.2 s (`.m-row--flash` carries the transition, so the press feedback of other rows stays instant; reduced motion drops it). |
| Údržba card | `.m-card--well`, `.m-sec__n`, `.m-todo`, `__ic`, `__main`, `__t`, `__s` | `mobile/components/UpkeepCard.tsx` | Spec §8.3 item 1. An action card on `well`, first on Přehled, present only while something is due. Head: the section title and the count ("3 věci"). One row per item: a 34 px icon square on `well` (Lucide 17), the title 14 / 600 (wraps), a 12 `ink-4` sub-line (one line, ellipsis) and the verb at the end as a small button — primary on the first row, secondary on the rest. Rows are `line-soft` apart. Zaznamenat opens the record-balances sheet; Importovat opens the import sheet for that row's account (`openImport({ accountId, source: 'upkeep' })`); Zálohovat opens Nastavení → Záloha a přenos (`/settings/backup`); Zařadit opens the categorize queue (`/finance/uncategorized`, of that account when exactly one holds the payments; the count is the queue's); Přecenit goes to the Majetek tab. |
| Field | `.m-field`, `--signed`, `__l`, `__w`, `__i` (`--month`), `__u`, `__e`, `__s`, `__h`, `__lr`, `__a`; `.m-field-grid` | `mobile/components/AmountField.tsx`, `mobile/components/cashflow/MonthField.tsx` | Label 12 / 600 above, a 48 px field (radius 10, `line-strong`; focus `focus-border` and ring; invalid `loss`), text 15 tabular, the currency inside on the right, the decimal keypad, the last value as the placeholder. A plain text field: `parseAmountInput` reads the comma or point the system keypad types. A field that takes negative amounts (a balance) has the ± toggle on the left (`--signed`): a 32 px chip in a 48 px hit area that keeps the keypad open, ± on `well-2`, a dark "−" while pressed (`aria-pressed`, named "Záporná hodnota"), because the iOS decimal pad has no minus key; `signedAmountInput` adds the sign (a typed or pasted minus counts too). Invalid and focused, the ring is `loss-ring`. An error is 12 / 600 `loss` under the field; without one a hint may stand there (11 `ink-3`, the budget limit's six-month average). A month field (`MonthField`) is the same field with the platform's month picker (`input type="month"`: iOS and Android show their own wheel), with an optional text action beside its label ("Bez konce" clears the end month). Two month fields sit side by side in `.m-field-grid` (12 px gap) and their label rows are two lines high (`__lr`, 32 px) with or without the action, so the inputs stay level; the action (`__a`) reaches 44 pt by extending 12 pt above its row, never over the input. |
| Sheet | `.m-scrim`, `.m-sheet`, `--full`, `__handle`, `__head`, `__t`, `__s`, `__body`, `__foot` | `mobile/components/Sheet.tsx`, `mobile/sheets/RecordBalancesSheet.tsx`, `mobile/sheets/CashflowCategoryPickerSheet.tsx` | Paper, top radius 17, drag handle 36 × 5 `well-3`, title 20 / 650, one sentence, a body that scrolls, a footer on `well` with a hairline: the ghost Zrušit left, the primary right (it takes the rest of the row). Covers the tab bar. Closes on the scrim, Escape, its own buttons and Android back (it pushes one history entry while open, `use-sheet-history.ts`; sheets stack — the picker over the transaction sheet, a confirmation over either — and back closes the newest only). Rises 260 ms, leaves 200 ms. Portalled into `#root` so it shares the box `--m-vh` shortens: the footer stays above the keyboard. The first field is not focused on opening. The category picker of the Cashflow drilldown is a tall sheet (`--full`): the categories as entity rows grouped under section heads (Výdaj · Příjem · Převod), the payment's current one ticked (a value row, so no chevron), a spinner on the tapped row until the change has taken, and the lone ghost Zrušit in the footer (to be replaced by Finance's `CategoryPickerSheet`, Phase 3b). |
| Confirm sheet | `.m-sheet`, `.m-btn--destructive`, `.m-btn--ghost` | `mobile/sheets/ConfirmSheet.tsx`, `mobile/sheets/BudgetDeleteSheet.tsx` | The phone's confirm dialog (spec §10.5, the desktop's `ConfirmDeleteDialog`): the question as the sheet's title ("Smazat celý rozpočet?"), one sentence under it that says what else goes ("minulé měsíce přijdou o limit"), no body, and the footer — the ghost Zrušit on the left, the solid `destructive` button taking the rest and naming the deed ("Smazat celý rozpočet", "Smazat"). The one place the solid destructive button is used; it spins while the delete runs. The caller closes it once the action has succeeded; it closes on the scrim, Escape and Android back like any sheet. |
| Placeholder sheet | `.m-sheet` | `mobile/sheets/PlaceholderSheet.tsx` | A sheet that is not built yet: its title, one sentence saying when it comes, Zavřít as the footer's one button. |
| Form box | `.m-box`, `--loss`, `__t` | `mobile/sheets/BudgetLimitSheet.tsx` | A box in a sheet's form: the scheduled changes on `well` (`line` border, `ink-3`, radius 8), and after the first tap on save the limits that the save replaces in `loss` (`loss-soft`, `loss-line`, `role="alert"`) — a bold 12 line, then one line per item. The primary button then reads "Přepsat a uložit"; a second tap saves. |
| Toast | (sonner, `components/ui/toaster.tsx`) | `mobile/MobileApp.tsx` | At the bottom, centred between the 16 pt gutters and 12 pt above the tab bar and the bottom safe area (`--m-tabbar-h` + `--safe-bottom`), 2.6 s, so "Vrátit" is never under the bar. Past-tense verb, as on the desktop. |
| Settings row | `.m-row--chev`, `__chip--sq` | `mobile/components/SettingsRow.tsx`, `mobile/pages/Settings.tsx` | A row of Nastavení (spec §8.9, prototype note 20): the entity row with a rounded-square icon chip (Lucide 16), the setting's name, its current state in the sub-line ("Odemykání obličejem · zámek po 15 minutách", one line ending in an ellipsis) and the chevron. `danger` draws the chip and the title in `loss` (Smazat všechna data). A row pushes a page (`href`, `/settings/:section`) or runs an action (`onClick`: the lock, a sheet; Import opens the import sheet with no account and "CSV, CAMT.053, MT940, OFX, QIF" in the sub-line). The root ends with the version caption. |
| Section page | `.m-screen` | `mobile/components/SectionPage.tsx`, `mobile/pages/settings/*Section.tsx` | A page pushed on Nastavení: the nav bar with "Nastavení" as the back label, no tab bar, one lead sentence (13 / 450 `ink-3`) as the first body line, then switch rows, key-value rows, a segment or 48 pt buttons on paper cards (Profil, Zabezpečení, Připomínky, Oznámení, Záloha a přenos). |
| Step progress | — | `mobile/components/StepProgress.tsx` | The first run's place in its steps: the caption "2 / 7 · Účet" (11 `ink-3`) over a 3 pt bar on `well-2` with the `dark` fill at index / total. The caption carries the meaning; the bar is `aria-hidden`. |
| Picker sheets | `.m-sheet`, `.m-pick`, `.m-search` | `mobile/sheets/LanguagePickerSheet.tsx`, `DayPickerSheet.tsx`, `CurrencyPickerSheet.tsx`, `CountryPickerSheet.tsx` | One choice from a list: pick rows with a check on the current one and Zrušit as the footer's one button; a tap picks and closes. The language rows carry `lang` (a `PickRow` prop) so 日本語 and 简体中文 get the CJK font rule of `index.css`; the currency sheet has a search over codes and names; the day sheet lists 1–28. |
| Restore sheet | `.m-sheet`, `.m-btn--destructive` | `mobile/sheets/RestoreBackupSheet.tsx` | The restore confirmation: the question, the archive's date, version, attachment count and size in the phone's own sentence (`backup.restoreConfirmDescription`: it replaces the data and says to back up first; the desktop's "pre-restore folder" promise is not made, nobody can reach that folder on a phone), the "newer than this app" line in `loss` with the button disabled, Zrušit and the destructive Obnovit; not dismissable while restoring. |
| Delete sheet | `.m-sheet`, `.m-btn--destructive` | `mobile/sheets/DeleteProfileSheet.tsx` | Smazat všechna data: the question, what goes and the backup reminder, a 48 pt field that must spell DELETE (the desktop's word), Zrušit and the destructive button enabled only then; on success the pending notifications are cancelled (iOS, best effort), the device storage is cleared and the app reloads into the first run. |

**Hide amounts** (spec §4 principle 7, D6). The eye on Přehled covers every amount in the app, so
phone code never calls `fmt.money` itself; `src/mobile/components/Money.tsx` documents the four routes
to the one switch, by what the place needs: `<Money>` where an element will do, `useMoneyText()` where a
plain string is needed (and as the formatter handed to shared hooks), `ConvertedMoney` with its `format`
prop (it keeps the missing-rate marker), and `maskMoney` / `MASK` for text that `fmt.money` did not build
(the chart's value axis, the record-balances hint). `Hero.formatValue` and `StatItem.value` are strings
for that reason: they size themselves to the shown text, so while amounts are hidden the fit follows the
mask. **The sign stays and the number is never compact:** "+ •••• Kč", "− •••• Kč" (the ledger row and
the transaction sheet compose them with `signedAmountText`), and `useMoneyText` drops `compact` while
hidden, so "6,78 mil. Kč" masks to "•••• Kč" and never leaks "mil.". Inputs are never masked; counts,
percents and dates stay visible. `src/mobile/pages/Overview.tsx` is the reference, and the check for a
new screen is that `grep -rn "fmt.money(" src/mobile --include='*.ts' --include='*.tsx'` lists only
`src/mobile/hooks/use-money-text.ts`.
**The one exception** is the company card's market cap ("O společnosti", `CompanyCard.tsx`): it is public
data about the company, not an amount of the user's, so it stays visible while amounts are hidden; the
prices on the same card (the 52-week range) go through the eye.

**Backup export on iOS** (`mobile/hooks/use-backup.ts`, `use-export-text-file.ts`,
`src-tauri/src/commands/backup.rs`). The dialog plugin's `save()` on iOS exports whatever file already sits
at `Documents/<fileName>` through the Files picker, so an export is staged first: `stage_backup_export`
writes the archive there without recording a backup, `save` opens the picker, and `finish_backup_export`
removes the staged copy and records the backup only when the picker returned a destination. The recovery
key text goes the same way through `stage_text_export` / `discard_text_export` under a fixed allow-list
of names. The phone capability grants the webview no fs command, and a startup sweep removes any staged
file an interrupted export left behind. Exports are iOS-only until an Android path writes into the
returned content URI. Restore (`inspect_backup` + `restore_backup` on a picked file, the picked copy read
by Rust) is verified on iOS only: there the picker hands over `file://` URLs, which `dialogPathToFsPath`
turns into paths, while Android's `open()` returns `content://` URIs that `inspect_backup` cannot read,
so Android restore is a follow-up. Where it runs, restore switches biometric unlock off first, clears the
enrolment flag, cancels the old profile's pending notifications and reloads into the lock, so the next
password unlock offers biometrics again and plans the restored profile's notifications.
The phone's data folder is excluded from iOS device backups (iCloud, Finder: `isExcludedFromBackup`, set
at every start and again after setup and restore through the keychain plugin's Rust-only
`exclude_from_backup`), so the backup the person saves here is the only copy outside the phone, and the
Záloha note and the first-run Záloha lead say so.

**Notifications** (`mobile/lib/notification-plan.ts`, `mobile/hooks/use-notification-schedule.ts`).
Scheduling runs on iOS only (ruling 16 of the Phase 5 plan; `schedulesOnThisDevice`): the Android
plugin reads `schedule.at.date` as UTC, so the local digits written for iOS would fire hours off there.
Android needs its own UTC encoding of the date and an Android build to verify it on, which is a
follow-up. Planned by a pure function on every unlock from the milestones and the mute list: T−14 and
T−1 for deadlines, the monthly upkeep on the chosen day (1–28), the backup reminder the morning after a
stale backup is found, all at 09:00 local, at most 60 pending. The iOS plugin reads `schedule.at.date`
as local wall-clock digits with a literal `Z`, so `scheduleDateString` emits exactly that and never a
`Date`; items already past are dropped. Smazat všechna data and a restore call
`cancelAllNotifications()` before the reload, so the notifications of a deleted or replaced profile do
not outlive it. Nothing is scheduled until the person allows notifications from
Nastavení → Oznámení (the ten reminder groups of Připomínky are the desktop's mute list); the
notification preferences live in `localStorage` (`moony-notifications`), like the background lock and
hide-on-launch flags.

**The row rule.** One row pattern replaces tables on the phone: accounts, positions, bonds, properties,
loans, policies, transactions, categories, watchlist and agenda rows. A 36 pt chip (logo, initials or an
icon in a tinted circle), then two lines: the title 15 with the trailing value, the sub 12 with the
trailing sub 11 (a change badge or a caption). 60 pt high, 52 single-line (8 pt vertical padding around the chip); dividers `line-soft`, inset to
the title's left edge; the whole row is the tap target. Each line shares out its own width: the value and
the trailing sub keep theirs, the title and the sub take the rest and end with an ellipsis, so a long
trailing sub never cuts the name (the prototype's two columns did, in German at 360). **A chevron appears
only on rows without a trailing value** (settings, documents); a value row is tappable as a whole and
keeps that room for its name (`.m-row--chev` with a value drops the chevron).

Where the contract differs from the prototype it follows the spec: badges 12 / 500 (§10.2; the prototype
drew 9–10 px), the hero chart 160 pt (§10.4; 150), the collapsed nav title 20 pt (§10.3; the prototype's
detail title was 17 with a sub-line), and the entity row's two lines above. Components use tokens only and
no `.dark` classes, so the dark token block switches them for free (§10.6).
