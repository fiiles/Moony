# Dark mode (desktop) — design

Date: 2026-10-16. Status: **prototype approved** by the owner in chat on 2026-10-16
(`docs/specs/2026-10-16-dark-mode-prototype.html`); this document is up for the owner's review
before the plan. The plan will be `docs/plans/2026-10-16-dark-mode.md`. This spec is also the
hand-off brief: an implementer should not need the chat.

## 1. Problem

Moony ships light only. `tokens.css` says "Light theme only", the design-system README §2 says
"Dark theme: not in v1" and §11.1 records the decision. A mobile app (iOS + Android) with the
same visual language is being designed in parallel (`docs/specs/2026-10-16-mobile-app-design.md`,
written by another session and not yet in this tree); on a phone a dark theme is expected, and
the two apps must share **one** set of dark tokens so the brand reads the same on both. The
desktop app goes first; what it hands the mobile design is the token table of §4.1 as it lands
in `tokens.css`, the README and the style guide.

Constraints that shape the design:

- Every component already reads colors through tokens (`src/index.css` → Tailwind names in
  `tailwind.config.ts`; `docs/design-system/tokens.css` for the style guide). A dark theme is
  therefore mostly a second value per token, provided the few raw literals left in components
  and `moony.css` are tokenized first.
- Charts read tokens once into plain strings (`src/lib/tokens.ts`, Recharts needs hex), so a
  theme switch must invalidate that cache and re-render the charts.
- The lock screen renders before the database is decrypted, so the theme must be known from a
  device-level store before the profile is readable, and before the first paint (no light flash).
- `prefers-color-scheme` must be honoured only when the user chose "System".

## 2. Goals and non-goals

Goals:

- A dark theme for every screen of the desktop app, built from the same token names as the
  light theme, with the design principles kept: depth from surfaces, color means something,
  one muted indigo series family, warm neutrals.
- A theme preference in Settings → General: System / Light / Dark (default System), stored in
  the profile, mirrored on the device for the lock screen, applied before the first paint.
- A validated palette: series, gain/loss and ink contrast measured with the dataviz validator
  (OKLab), results recorded in the README like the light ones.
- The shared dark token set documented in `tokens.css`, the README and the style guide (with a
  theme switch), so the mobile design can consume it.

Non-goals:

- A third theme, per-page themes, scheduled switching (sunset), or high-contrast mode.
- Theming of attachment viewers' content (a PDF page stays white paper; photos are photos).
- Changing the light theme's values. The light theme must look exactly as today; the only
  light-side changes are literal-to-token replacements that resolve to the same colors
  (one accepted nuance: D13).
- Dark app icons, dark DMG background, or a dark variant of the website.
- The mobile implementation. Only the token table is shared; mobile components are its own.
  A machine-readable token export (JSON, Swift, Kotlin) is not part of this spec.
- Retouching decorative Tailwind palette classes that remain in three files (file-type icons in
  `RealEstateDocuments.tsx`, the heart in `AboutModal.tsx`, the amber hint in
  `CategorySelector.tsx`): decorative, ≥ 3:1 on both papers, left as they are and listed in D13.

## 3. Decisions

| # | Decision |
|---|---|
| D1 | **One token set, two value blocks.** The dark theme is `:root[data-theme="dark"] { … }` with exactly the token names of `tokens.css` (no new names). `tokens.css` and `src/index.css` are first reconciled (the nine names only `index.css` has move into `tokens.css`), then both get the same dark block. "Identical" means: the same declarations (names and normalised values) in the color, elevation and focus groups of both blocks, guarded by a test (§5); formatting may differ (`index.css` is Prettier-formatted) and the spacing scale `--sp-*` stays only in `tokens.css` (Tailwind's spacing covers the app). |
| D2 | **Warm near-black, depth from surfaces.** canvas `#1a1917` < sidebar `#1f1e1b` < paper `#292826`; never `#000`, same warm hue as the light neutrals. Wells go deeper = darker in both themes (well < paper), so the hover row, table head, modal footer and segmented track all sink, as they do in light. Black-based shadows with higher alpha; a card is carried by its 1 px `line` and the paper/canvas step, not by the shadow. |
| D3 | **The contrast material inverts.** `--dark` becomes the light material (`#e9e7e1`) and `--ink-inverse` becomes dark ink. Active nav, primary button, tooltip (`.tip`), checked checkbox/switch, `badge--dark`, active chip and the moon mark invert with it without any component change. The token keeps its name `dark` because the name is part of the shared set; the README defines it as "the contrast material". |
| D4 | **Series re-stepped, same family.** s1–s4 stay one indigo family (OKLCH hue 281°) with the lightness ladder reversed (s1 lightest). Validated: adjacent ΔE 16.1 / 15.1 / 15.0 (floor 15), CVD ΔE 14.7 (target 8), contrast vs paper 10.7 / 6.5 / 3.8 / 2.0:1. s4 below 3:1 keeps today's rule: every series always has a legend with values. |
| D5 | **gain/loss re-stepped.** `gain #45c6a6`, `loss #ef7f5e`; text 6.9:1 and 5.5:1 on paper, 5.7:1 and 4.7:1 on their soft tints (`#203c30`, `#4a2d25`), CVD ΔE 9.4. The ▲/▼ glyph next to every signed change stays mandatory. |
| D6 | **Ink inverse and warm.** `ink #eceae4` (12.2:1 on paper) … `ink-5 #6b6962` (2.7:1, axis ticks and placeholders; light has 2.4:1). Never `#fff` for text. |
| D7 | **Category icons: hue kept, glyph lifted.** The chip keeps the user's hue and the 15 % tint over paper; in the dark theme the glyph color is `color-mix(in oklab, <hue> 70%, var(--ink))`, which puts all 14 preset colors at ≥ 4.9:1 (raw, three of them sit at 3.1–3.5:1). Instrument and bank logos (`AssetLogo`, `.logo`) need no rule: they are s1–s4 discs with `ink-inverse` text and invert together with the series. |
| D8 | **Preference in the profile, mirrored on the device.** `user_profile.theme TEXT NOT NULL DEFAULT 'system'` (migration `022_profile_theme`), values `system` / `light` / `dark`, exposed on `UserProfile.theme` and `UpdateUserProfile.theme`. `localStorage['moony-theme']` mirrors it so the lock screen and the first paint know it; after unlock the profile wins (same pattern as language). Not `app_config`: that table is a technical store excluded from backups. |
| D9 | **System only when chosen.** The resolved theme is `pref === 'system' ? (matchMedia dark ? 'dark' : 'light') : pref`. Under System the native window theme override is cleared (`setTheme(null)`, so the webview's `prefers-color-scheme` follows the OS again) and the OS change is followed live through the media query, with Tauri's `onThemeChanged` as a second trigger to re-read it. Under Light or Dark the override is set (`setTheme(resolved)`), which pins the webview's media query to the choice: exactly what "only when chosen" means. |
| D10 | **No flash.** `public/theme-boot.js`, a classic blocking script in `<head>`, stamps `data-theme` on `<html>` before the stylesheet applies; CSS has a single dark block and no `@media (prefers-color-scheme)` duplicate. The main window is created hidden (`visible: false`) and shown by the frontend right after the stamp; a Rust fallback shows it after 3 s if the frontend never did. `color-scheme: dark` on the dark root makes native pickers, selects and scrollbars dark. The native window theme follows the choice (`setTheme(resolved)`, or `null` under System, D9). As a nicety, not a guarantee, `WebviewWindow.setBackgroundColor(canvas)` (window and webview layer; macOS has no webview layer) colors the overdraw during resizes; `visible: false` + `show()` is what prevents the start-up flash. |
| D11 | **Charts re-read tokens on switch.** `src/lib/tokens.ts` gets `resetTokenCache()`; a `useChartTokens()` hook (theme version from the provider) replaces every `useMemo(chartTokens, [])`, so Recharts gradients, fills and strokes repaint on a switch. Every other memo that calls `token()` (today `PerformanceChart`'s `colors`, line 367, through `resolveColor`) takes the theme version as a dependency. SVG that already uses `var(--…)` needs nothing. |
| D12 | **Literals become tokens, light unchanged.** The raw `#fff`, `rgba(41,40,34,.05)`, `#f0eee8`, `#c9c5bc`, `#f1cfc3`, `#a8391d` and shadow literals in `moony.css`, `charts.js`, `styleguide.html` and three React files resolve to the identical light colors through tokens (list in §4.4). |
| D13 | **Accepted nuances in the light theme.** (a) The avatar's inset highlight goes from `#fff` to `rgba(255,255,255,.13)` (the logo's value); on the light `well-3` disc the 1 px difference is invisible, on dark a solid white edge would glare. (b) The switch thumb is the one component with a theme-specific rule: off-state thumb `ink-2` in dark (paper would sink into the dark track); on-state thumb `ink-inverse` in both. (c) Style guide only: `moony.css` still carries the pre-redesign brown `rgba(148,92,77,…)` in the invalid-focus ring and the destructive shadow; through `--loss-ring` / `--sh-destructive` it takes the app's real `rgba(191,66,36,…)` values. `.fmt .bad` in `styleguide.html` becomes `color-mix(in srgb, var(--loss) 50%, transparent)`. (d) The buy/sell label in `OtherAssetTransactionsModal.tsx` (lines 115–116) wears Tailwind `text-green-600` / `text-red-600`; it moves to `text-gain` / `text-loss` (red-600 is 3.05:1 on dark paper, and the design system allows only the two semantic colors). The three decorative uses named in the non-goals stay. |
| D14 | **Settings UI.** A new card "Vzhled" (Appearance) in Settings → General, after "Jazyk a formáty": one row with a segmented control Systém / Světlý / Tmavý. The theme applies immediately on click (optimistic), the profile is saved without a success toast; a failed save shows the usual error toast and reverts. |
| D15 | **Docs are revised, not rewritten.** README §2 ("Dark theme: not in v1") and §11.1 get a dated amendment ("2026-10-16: dark theme added …") instead of deletion; §2 gains the dark token table and validation; §4, §6, §8, §12 get the dark rules. `styleguide.html` gets a Světlý/Tmavý switch in its top bar. The prototype and this spec stay frozen snapshots. |
| D16 | **No MCP change, no backup change.** `theme` rides along in `user_profile` exports/restores like `language`; a backup without the column restores to the default. MCP does not expose it. |

## 4. Design

### 4.1 The shared token set

Only the color, elevation and focus tokens change. Radius, type, spacing, layout and motion are
theme-independent and stay out of the dark block. The `--dark-grad` / `--dark-grad-hover`
gradients are declared once in `:root` with `var(--dark-hi)` etc. and follow the dark block.

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

The dark block also sets `color-scheme: dark` (D10). The nine tokens that today exist only in
`src/index.css` (`hero-orb`, `line-hover`, `ink-hover`, `loss-line`, `loss-hover`, `loss-ring`,
`sh-btn-hover`, `sh-seg`, `sh-destructive`) are added to `tokens.css` with their light values
(D1). The `tokens.css` header changes from "Light theme only (dark theme dropped for v1, see
README.md §11)" to "Two themes: the `:root` block is light, `:root[data-theme="dark"]` is dark;
same names, see README.md §2 and §11".

### 4.2 Validation (dataviz palette validator, OKLab, surface = dark paper `#292826`)

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

The validator's lightness-band and chroma-floor checks fail by design for a monochrome family
(they did for the light series as well): those checks target multi-hue categorical palettes.
The commands to repeat the run are in §5.

### 4.3 Rules

**Surfaces and elevation (README §4).** The six levels keep their surfaces; in the dark theme
the shadow column is black-based (table above) and the sentence "differentiated mainly by
surface and faint warm shadow" gains "in the dark theme by the surface step and the 1 px line;
the shadow only settles the bottom edge". E0 canvas is the darkest surface in both themes.

**Contrast material (README §4 "dark" row, §5, §6).** "Dark material" is renamed in prose to
"contrast material (`dark`): dark on light, light on dark". Rows that say "dark material when
on" (checkbox, switch), "active row is dark material" and "Tooltip: dark material" are read
through that definition; the only wording change is the definition itself.

**Charts (README §8).** Unchanged rules; one added sentence: "Both themes use the same token
names; the dark values are re-stepped (s1 lightest) and validated in §2. The end-point ring and
event marks are `paper` (never `#fff`), the area fade ends in `paper-2`." Recharts components
read tokens through `useChartTokens()` (D11). `charts.js` and `styleguide.html` replace their
`#fff` / `#fbfaf8` literals with `var(--paper)` / `var(--paper-2)`.

**Logos and category icons (README §6 Table row, Budgets row).** `AssetLogo` and `.logo`
unchanged. `CategoryIconChip` becomes a CSS-driven chip: the component sets `--cat: <hex>`
inline; `src/index.css` defines `.cat-chip { color: var(--cat); background: color-mix(in srgb,
var(--cat) 15%, transparent) }` and `:root[data-theme="dark"] .cat-chip { color: color-mix(in
oklab, var(--cat) 70%, var(--ink)) }`. The preview chip of `CategoryFormDialog.tsx` (lines
226–232, the same inline `${color}26` tint) renders `CategoryIconChip` instead of its own markup,
so the preview and the real chip never differ. `CategorySelector` and `CategoryFormDialog` keep the raw
preset hexes (they are data, the user's choices), as does the `#9E9E9E` / `#6b7280` fallback for
a category without a color.

**Native controls.** `color-scheme: dark` on the dark root (D10). The date input rules in
`index.css` already use ink tokens. The sonner `Toaster` (`toaster.tsx`) passes
`theme="light"` today; it passes the resolved theme from `useTheme()` instead (it is unstyled
with token class names, so this only keeps sonner's internals consistent).

**Photos and documents.** The real-estate photo lightbox keeps its white controls over the
image; `PdfPages` keeps `bg-white` for the page itself (a rendered page is white paper in both
themes); the attachments viewer chrome is tokens and follows the theme.

### 4.4 Literal-to-token replacements (D12, D13)

`docs/design-system/moony.css`:

| Rule | Today | Becomes |
|---|---|---|
| `.hero::after` | `background: #f0eee8` | `var(--hero-orb)` |
| `.nav__item:hover`, `.account:hover`, `.btn--ghost:hover` | `rgba(41,40,34,.05)` | `var(--ink-hover)` |
| `.input:hover`, `.select:hover`, `.textarea:hover` | `#c9c5bc` | `var(--line-hover)` |
| `.input.is-invalid:focus` | `rgba(148,92,77,.14)` ring | `var(--loss-ring)` |
| `.btn:hover` | `0 1px 3px rgba(52,46,34,.1)` | `var(--sh-btn-hover)` |
| `.seg__btn.is-active` | `0 1px 3px rgba(34,34,34,.13)` | `var(--sh-seg)` |
| `.check input:checked::after` | `border: solid #fff` | `var(--ink-inverse)` |
| `.switch input::after` | `#fff` | `var(--paper)`; `:checked::after` → `var(--ink-inverse)`; dark off-state → `var(--ink-2)` (D13b) |
| `.avatar` | `inset 0 1px #fff` | `inset 0 1px rgba(255,255,255,.13)` (D13a) |
| `.logo` | `color: #fff` | `var(--ink-inverse)` (`.logo--4` keeps `var(--ink)`) |
| `.badge--dark`, `.chip.is-active`, `.tip`, `.tip b` | `#fff` | `var(--ink-inverse)` |
| `.btn--destructive` | `color: #fff`, shadow literal, hover `#a8391d` | `var(--ink-inverse)`, `var(--sh-destructive)`, `var(--loss-hover)` |
| `.alert--error` | `border-color: #f1cfc3` | `var(--loss-line)` |
| `.proto-tag` | `rgba(250,249,246,.92)` | `var(--paper)` at 92 % via `color-mix` or simply `var(--paper)` (style-guide chrome; builder's call) |

`docs/design-system/charts.js`: every `fill="#fff"` / `stroke="#fff"` (11 places) →
`var(--paper)`; `stop-color="#fbfaf8"` → `var(--paper-2)`. `docs/design-system/styleguide.html`:
the legend swatches with `#fff` (line 279), the "Tmavý materiál" swatch `color:#fff` (line 139),
`.toc a:hover` (`rgba(41,40,34,.05)` → `var(--ink-hover)`) and `.fmt .bad`
(`rgba(148,92,77,.5)` → `var(--loss-line)`) → tokens; the `swatches()` calls (lines 332 ff.)
carry the light hex as data today and change to read both values from the computed style of
the two blocks; plus the theme switch (§4.8).

React (`src/`):

| File | Change |
|---|---|
| `components/ui/avatar.tsx` | `shadow-[inset_0_1px_#fff]` → `shadow-[inset_0_1px_rgba(255,255,255,0.13)]` (D13a) |
| `components/ui/switch.tsx` | thumb `bg-paper data-[state=checked]:bg-ink-inverse` plus the dark off-state rule in `index.css` (D13b) |
| `components/common/CategoryIcon.tsx` | `CategoryIconChip` → `.cat-chip` with `--cat` (§4.3) |
| `components/categories/CategoryFormDialog.tsx` | preview chip → `CategoryIconChip` (§4.3) |
| `components/other-assets/OtherAssetTransactionsModal.tsx` | `text-green-600` / `text-red-600` → `text-gain` / `text-loss` (D13d) |
| `components/ui/toaster.tsx` | `theme="light"` → the resolved theme (§4.3) |
| `index.css` | dark block, `.cat-chip` rules, the switch dark rule |
| `lib/tokens.ts` | `resetTokenCache()`; the doc comment loses "Light theme only; nothing here changes at runtime" |
| `components/charts/{MoonyLineChart,MoonyBarChart,MoonyBandChart,MoonyLinesChart}.tsx`, `components/stocks-analysis/PerformanceChart.tsx` | `useMemo(chartTokens, [])` → `useChartTokens()`; in `PerformanceChart` the `colors` memo (line 367) adds the theme version to its dependencies (D11) |

Everything else in `src/` uses tokens (`AssetLogo`, `MoonMark`, `EventMarkers`, Radix portals)
or is one of the three decorative exceptions listed in the non-goals.

### 4.5 Preference, storage and wire types (D8, rule 2, rule 3, rule 6)

Migration `022_profile_theme` (append-only, next number):

```sql
ALTER TABLE user_profile ADD COLUMN theme TEXT NOT NULL DEFAULT 'system';
```

Rust (`src-tauri/src/models/user.rs`):

- `UserProfile.theme: String` (`#[serde(rename = "theme")]` not needed, single word) read in
  `get_user_profile` (`services/auth.rs`, add the column to the SELECT and the struct).
- `UpdateUserProfile.theme: Option<String>`; `validate()` rejects anything but
  `system` / `light` / `dark` with `AppError::Validation("validation.themeInvalid")`;
  `apply_profile_update` adds the `theme = ?` clause. `InsertUserProfile` is unchanged (the
  column default covers onboarding and `seed_demo`).
- `validation.themeInvalid` lands in `common.json` of all ten locales (rule 8).

Wire contract (`shared/schema.ts`, hand-mirrored; `generated-types.ts` regenerated):

```ts
export type ThemePreference = 'system' | 'light' | 'dark';
export interface UserProfile { …; theme: ThemePreference; … }
```

`ProfileUpdate` (`use-settings-mutations.ts`) adds `'theme'`, but the Appearance card does
not use `updateProfile` (its `onSuccess` always toasts "Profil uložen"): it has its own
mutation, as `LanguageCard` has for the currency (§4.7). `authApi.updateProfile` is unchanged
(it already takes `Partial<UserProfile>`). Every `UpdateUserProfile { … }` struct literal in
`src-tauri` (13 today, mostly tests) gains `theme: None`; the in-memory test schemas that create
`user_profile` gain the column.

Device mirror: `localStorage['moony-theme']` holds the preference (`system` / `light` /
`dark`), written through `src/utils/device-storage.ts` helpers. `DangerZoneCard`'s
`localStorage.clear()` drops it; the next start resolves System until the profile is read.

### 4.6 Runtime (D9, D10, D11)

```
index.html <head>:  <script src="/theme-boot.js"></script>   (classic, blocking, from public/)
  reads moony-theme, resolves 'system' with matchMedia, stamps <html data-theme="light|dark">
  → the stylesheet's first application already picks the right block
main.tsx bootstrap: … i18nReady → render <App>
App: QueryClientProvider → AuthProvider → ThemeProvider → I18nProvider → …
ThemeProvider (src/theme/ThemeProvider.tsx):
  state pref (from the mirror, default 'system'); systemDark from matchMedia; resolved = resolveTheme(pref, systemDark)
  effect [pref]: window.setTheme(pref === 'system' ? null : pref) (Tauri only, errors ignored),
          then re-read matchMedia (clearing the override lets the webview follow the OS again)
  effect [pref]: while pref === 'system' follow matchMedia('(prefers-color-scheme: dark)') changes
          and Tauri's onThemeChanged (both just re-read the media query)
  effect [resolved]: applyTheme(resolved) — data-theme, resetTokenCache(), themeVersion++,
          WebviewWindow.setBackgroundColor(canvas of the theme) (nicety, errors ignored)
  effect [user?.theme]: when the profile's theme arrives and differs from pref → setPref(user.theme)
          (profile wins; depends on user.theme only, like I18nProvider, so it cannot revert an
          optimistic click before the save lands)
  effect (once): getCurrentWindow().show() after the first applyTheme
  setPreference(p): setPref(p), write the mirror (the Appearance card persists to the profile)
useTheme(): { preference, resolved, setPreference }
useChartTokens(): useMemo(chartTokens, [themeVersion])
```

`setTheme(null)` under System matters: a forced `light` or `dark` pins the webview's
`prefers-color-scheme`, so an OS flip would never reach the media query (Tauri documents the
override as app-wide on macOS and Linux).

`src/utils/theme.ts` holds the pure parts (`THEME_KEY`, `isThemePreference`, `resolveTheme`,
`readStoredPreference`) so they are unit-tested; `public/theme-boot.js` repeats the three-line
read in plain ES5 (it cannot import) and is tested by running its source in `node:vm` against
stubbed globals (§5). ESLint lints `public/` today; the file gets an entry in the config's
`ignores` (it is not application code).

Tauri:

- `src-tauri/tauri.conf.json`: the main window gets `"visible": false`. The dev launch
  configs that pass a `--config` windows array (`moony-demo`, `moony-firstrun`) replace the
  whole window object and may omit it; a flash in dev is acceptable.
- `src-tauri/capabilities/default.json` adds `core:window:allow-show`,
  `core:window:allow-set-theme`, `core:window:allow-set-background-color` and
  `core:webview:allow-set-webview-background-color` (`WebviewWindow.setBackgroundColor` calls
  both commands).
- `src-tauri/src/lib.rs` setup: spawn a thread that sleeps 3 s and calls `show()` on the
  main window if `is_visible()` is still false (frontend crash safety).

Dev browser bridge: the window calls are relayed to the Tauri window like any other
`invoke`; in a plain browser they change the real window's theme, which is harmless.

### 4.7 Settings UI and strings (D14)

`src/components/settings/AppearanceCard.tsx`, rendered in `GeneralSection` after
`LanguageCard`:

- `SettingsCard` title `settings:appearance.title` ("Appearance" / "Vzhled"), description
  `settings:appearance.description` ("System follows the macOS, Windows or Linux setting." /
  "Systém sleduje nastavení macOS, Windows nebo Linuxu.").
- One `SettingsRow` labelled `settings:appearance.mode` ("Light or dark mode" / "Světlý nebo
  tmavý režim") with the `Segmented` control: `settings:appearance.system` / `.light` / `.dark`
  ("System / Light / Dark", "Systém / Světlý / Tmavý"), `aria-label` = the row label.
- On change: `setPreference(p)` (instant), then the card's own `useMutation` calls
  `authApi.updateProfile({ theme: p })`, on success invalidates `['user-profile']` with no toast
  (the change is visible), on error shows the usual error toast (`translateApiError`) and calls
  `setPreference` with the previous value.

All six keys plus `validation.themeInvalid` go to every locale folder (`en`, `cs`, `de`, `es`,
`it`, `nb`, `pl`, `sv`, `ja`, `zh`); `src/i18n/locales.test.ts` guards parity.

### 4.8 Design-system docs and style guide (D15)

- `tokens.css`: header rewritten (§4.1), the nine missing tokens added, the dark block appended.
- `README.md`: §2 — replace "Dark theme: not in v1. The app ships light only." with the dark
  token table, the validation table of §4.2 and the sentence on `data-theme`; §4 — the dark
  elevation sentence; §6 — Switch row notes the dark off-thumb; §8 — the chart sentence; §11 —
  decision 1 becomes "Dark theme was not part of v1 (2026-10-03). **Amended 2026-10-16:** the
  dark theme is in, as a second value block on the same token names, with the preference in
  Settings → General (spec `docs/specs/2026-10-16-dark-mode-design.md`)"; a new decision lists
  D2–D3 (depth from surfaces, inverted contrast material); §12 — tokens paragraph gains the
  `ThemeProvider` / `useChartTokens` sentence and the file table mentions the switch in the style
  guide.
- `styleguide.html`: a Světlý/Tmavý segmented control in the `.toc` title block (and the `D`
  key), stamping `data-theme` on `<html>`; the lead of "Barvy" no longer says the dark theme is
  an open question; the "Otevřené otázky" section drops the dark-theme item; the swatch grids
  show both values (light swatch, dark swatch) per token.
- `docs/architecture/overview.md`: the provider nesting line gains `ThemeProvider`.
- `docs/DEPRECATED.md`: unchanged (`ThemeToggle.tsx` stays listed; it is not resurrected).

## 5. Testing

Pure logic (Vitest):

- `src/utils/theme.test.ts`: `resolveTheme` for the six (preference × system) cases;
  `isThemePreference` rejects unknown strings; `readStoredPreference` falls back to `system`
  on a missing key, an unknown value and a throwing storage.
- `public/theme-boot.test.ts`: the script's source run in `node:vm` with stubbed
  `localStorage`, `matchMedia` and `document.documentElement` (Vitest stays on the `node`
  environment; no jsdom dependency is added): stamps `dark` for `dark`, `light` for `light`,
  follows `matchMedia` for `system` and for a missing key, and never throws when storage throws.
- `src/lib/tokens.test.ts`: with `document` and `getComputedStyle` stubbed through
  `vi.stubGlobal`, `resetTokenCache()` makes `token()` re-read a changed custom property.
- `src/i18n/locales.test.ts` (existing): parity of the new keys in all ten locales.
- A new style-sheet test, `src/index.test.ts` (there is no drift check today): `src/index.css`
  read as text, the dark block declares exactly the color, elevation and focus names the
  `:root` block declares (no missing, no extra; radius, type, spacing, layout and motion are
  excluded by prefix), and every `--name: value;` of the two blocks also appears, with the same
  value, in `docs/design-system/tokens.css`.

Rust (`cargo test`):

- `models/user.rs`: `UpdateUserProfile::validate` accepts the three values and rejects others
  with `validation.themeInvalid`.
- `services/auth.rs`: the in-memory test schema gains the `theme` column; `update_user_profile`
  writes it and `get_user_profile` reads it back; a profile created without it reads `system`.
- `db/migrations.rs`: the existing migration-list and idempotency tests cover `022`.

Palette (recorded in the README, repeatable):

```
node <dataviz skill>/scripts/validate_palette.js "#d8dbf0,#a2a7ec,#7b7ea5,#4f508e" --mode dark --surface "#292826"
node <dataviz skill>/scripts/validate_palette.js "#d8dbf0,#a2a7ec,#7b7ea5,#4f508e" --mode dark --surface "#292826" --ordinal
node <dataviz skill>/scripts/validate_palette.js "#45c6a6,#ef7f5e" --mode dark --surface "#292826"
```

Manual verification in the running app (demo profile, port of the worktree):

- Lock screen in dark before unlock (mirror = dark); unlock; Settings → General → Vzhled;
  switch Tmavý → Světlý → Systém and watch the sidebar, hero, charts and tooltips repaint.
- Restart with Tmavý chosen: no light frame before the lock screen (`visible: false` + show).
- A date input opens a dark picker; a select opens a dark list (`color-scheme`).
- Light theme: compare the six screenshots of `docs/screenshots/` against the new build by
  eye; only the avatar highlight (D13a) and the buy/sell label of the other-asset transactions
  modal (D13d) may differ.

Dark screenshots (`scripts/screenshots.mjs`): a new `--theme system|light|dark` option
(default `system`) writes `moony-theme` into `localStorage` next to the language in the
init step (line 322) before the first navigation, so the boot script resolves it. The dark set
is the `readme` set plus the import dialog (`--fixture`, the modal over the scrim), written to
`docs/screenshots/dark/` with the same file names and committed; the four the task names
(Přehled, account detail, Cashflow, modal) are shown in the review.

## 6. Phases

Each phase leaves the branch releasable.

1. **Tokens and literals.** Reconcile `tokens.css` with `index.css`, add the dark block to
   both, tokenize the literals (§4.4, including `charts.js` and `styleguide.html`), add the
   style-guide switch, `.cat-chip`, the switch rule. Verify with `data-theme="dark"` set by hand
   on `<html>` in the dev tools: every page dark, light unchanged.
2. **Runtime.** `theme.ts` + tests, `theme-boot.js` + test, `ThemeProvider`, `useChartTokens`
   in the five charts, `resetTokenCache`, Tauri window config, capabilities, Rust fallback show.
3. **Preference.** Migration 022, Rust model and service, wire types, `AppearanceCard`, ten
   locales, the profile sync in `ThemeProvider`.
4. **Docs and verification.** README, `tokens.css` header, overview.md, screenshots in the
   running app, the checks of §5, commit.
