# Changelog

All notable changes to Moony are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- PRIVACY.md lists every outbound host and describes what Delete Account leaves behind.
- **Delete Account says what stays.** The confirmation dialog and the hint next to the button, in
  all ten languages, no longer promise that all data is removed: the database, the key files and
  the attachments go, while the copies Moony keeps next to the database (`moony.db.bak-…`,
  `pre-restore-…`) stay in the data folder, which *Data and backups → Open folder* opens.

## [0.10.0] - 2026-10-12

This is the beta for 1.0.0. It changes how amounts in other currencies are counted and runs
several data migrations on the first start (the database is copied next to itself first, see
below), so keep the `moony.db.bak-<time>` copy until you have checked your numbers.

### Added

- **Ten languages.** Besides English and Czech, the app speaks German, Spanish, Italian, Norwegian
  (bokmål), Polish, Swedish, Japanese and Simplified Chinese. The language picker in Settings is a
  dropdown; the translations are machine-made, so reports of awkward wording are welcome.
- **Main currency from 36 currencies.** The main currency can be any currency with an
  exchange-rate source: the 30 the ECB publishes plus AED, SAR, QAR, BHD, OMR (fixed pegs to the
  US dollar) and BGN (fixed peg to the euro), whose rates Moony derives from the dollar or euro rate
  of each day. The picker in Settings and the first-run wizard list only these; a profile that
  already uses another main currency keeps working for amounts in that currency and shows a
  prompt in Settings. Accounts, holdings and contracts can still use any of the 45 selectable
  currencies.
- **Missing and estimated rates are named.** An amount whose currency has no usable rate stays
  visible in its own currency, is left out of the totals and the page says which currency is
  missing, instead of being counted as if one unit were worth one crown. A cost converted at
  today's rate because its purchase day had no rate is marked as estimated. Rates older than 14
  days are reported as stale.
- **Remembered corrections reach other payments.** The category picker in the bank ledger and in
  the cashflow report's drilldown says how many other payments of the same payee the remembered
  rule covers, broken down by their current category, and categorizes them in the same step; the
  toast offers "Undo". The same offer appears when a learned rule's category is changed on
  the rules page.
- **Cashflow planning: categories as groups, rows in or out of the plan.** The planning page lists
  every category as a collapsible row with its total, count and origin (the investment section opens
  as a handful of rows instead of one line per dividend). Any row — automatic or manual — and any whole
  category can be left out of the plan from its "···" menu without deleting it: it stays visible,
  muted and labelled "Out of plan", and stops counting in the totals, stats and flow bar. A category left
  out also covers items that appear later. The menu of an automatic row also opens the existing
  "replace with my own amount" flow and the source module. The add dialog offers every category
  (income or expense) in one grouped list and no longer asks for an automatic source to replace. The
  MCP tool `cashflow_report` carries `included` per item and its totals follow the same rule.
- **Bank account detail rebuilt around dated balance records.** The balance history is a list of
  dated records (recorded by hand or confirmed from a statement); the account's balance mirrors the
  newest record. The page draws a balance line through the records, a 12-month flows chart, and
  the ledger opens on the newest month with movements. Balance history, import history and
  interest tiers moved to the ··· menu. The balance field left the edit form; new accounts take an
  optional opening balance with a date.
- **Stocks analysis rebuilt around performance.** One wide card shows the time-weighted return of
  the whole portfolio, the money-weighted return (which reflects when you deposited) with its gap
  to the time-weighted one, and the change of value split into net deposits and portfolio gain. The
  return chart is larger and has a drawdown panel under it; both share a date axis with labels at
  real dates (month starts, quarters, years). "Split by" splits the selected positions by a
  tag group or by the automatic groups Sector and Currency (offered only when they split your
  portfolio), or not at all with "Don't split": the chart then shows the whole portfolio as one
  solid line, and with a filter on the selected positions as one solid line against the dashed
  portfolio; the table under the chart is the legend and shows each part's return, difference to
  the portfolio, contribution in percentage points, maximum drawdown, share and value. Hovering
  the chart rewrites the figures and the table to that day. New cards show each position's
  contribution to the return (its "Other" row opens a list of the positions it folds) and
  deposits against portfolio gain per week, month, quarter or year; the tag and holdings tables
  gain the return for the period. "Which positions to analyse"
  replaces the chip filters with a summary and a rule sentence. The stats row and the allocation
  ring are gone.
- **Custom periods in the stocks analysis.** Besides 3 months, This year, Year, 3 years and All there is
  Custom: type the dates or drag a range in the chart. The page remembers the period and the
  chosen split per device.
- **Compare with an index.** The stocks analysis can draw S&P 500 (through CSPX.L) or MSCI World
  (through IWDA.L) as a dotted line and name how far ahead or behind it you were. The prices are
  fetched from Yahoo when you switch the index on and are not stored; offline the page says the
  index could not be loaded.
- **First-run welcome.** The first run opens with a short brand animation that you can skip
  (reduced motion skips it) and that leads into the setup wizard. The language choice has a
  header of its own in the intro and in the wizard, the fourth step is called "What to track", and
  the currency fields look like the other selects while staying searchable. Closing the statement
  import in the last step no longer finishes the setup: the account you created stays, its fields
  become read-only, the import can be reopened for the same account without creating a duplicate,
  and a separate button enters the app.
- **Page transitions.** A page fades and slides in over 0.4 seconds when you move between pages in
  the main window; reduced motion turns it off.

### Fixed

- **Attached documents open again.** Insurance and real estate documents attached before 0.9.0
  pointed at the old data folder, so opening them did nothing. Moony now finds the file in the
  current data folder and says so when a file is really missing.
- **Lists inside dialogs scroll with the wheel in the app window.** A select opened inside a modal
  (the planning item's category, the settings category selects) no longer freezes under the dialog's
  scroll lock in the Tauri webview; the same guard the currency picker already had.

- **The category picker stays inside the window.** When a row sits near the top or bottom of the
  screen the picker is capped to the space on its side and its list scrolls, instead of running
  past the edge where its footer could not be reached. A toast's "Undo" can be clicked while
  a dialog is open and no longer closes the dialog.
- **Stock prices lost their third decimal.** Adding a position, buying or selling rounded the price
  per share to two decimals, so a London share quoted in pence (0.953 £) was recorded as 0.95 £;
  the buy dialog also pre-filled the rounded price. The price is now kept as typed.
- **Every enabled rule pack has its own tab** on the categorization rules page. Before, only the
  first enabled pack was visible as a tab and the others hid behind a selector inside it.

### Changed

- **No base currency.** CZK is no longer the unit Moony sums in. Net worth, history, positions,
  tags, stock analysis, returns (TWR) and the projection are computed from the native amounts of
  every account and holding and are shown in your main currency; an EUR profile holding only EUR
  assets sees the same numbers whatever the CZK rate does.
- **Gains in your currency.** The gain of a foreign stock or coin is the value today minus what you
  paid, each purchase converted at its own day's rate, in your main currency. An EUR profile no
  longer sees the CZK/EUR move inside the gain of a USD stock; the time-weighted return is
  computed in your main currency the same way. The dividend per share keeps its own currency; the
  yearly dividend of a position is shown in your main currency.
- **Portfolio history comes from the stored breakdowns.** The history chart is computed from the
  per-currency breakdowns stored for each day, converted in your main currency. Past days can shift
  slightly where the old CZK totals disagreed with their own breakdowns.
- **Pages convert historical values at their own day.** The debt trajectory, the real estate value
  trace and purchase price, other-asset cost and recent moves use the rate of the day of each
  value instead of today's; every page refreshes when rates are refreshed, without unlocking.
- **Exchange rates are stored against the euro.** Rates are kept as published by the ECB, with
  CZK as an ordinary quote, so the app no longer depends on the CZK feed. Rate history is now also
  downloaded for every currency you hold (balances, properties, loans, bonds, other assets,
  insurance, budgets), not only for transaction currencies.
- **Symbols.** Only the main currency is shown with its compact symbol ("$", "€"); every other
  currency is shown with its ISO code, so "$" and "kr" can no longer mean several currencies.
- **Calculator defaults** (annuity, real estate, the what-if step) scale to your main currency or
  start empty instead of using crown-sized numbers.
- **Data migrations 011 and 012.** On the first start after the update Moony rewrites stored
  exchange rates to euro quotes and removes the CZK copies of net worth history and per-ticker
  values (native breakdowns stay; an old history day that had no breakdown keeps its CZK figure
  as a CZK amount). Before it runs, the database is copied next to itself as
  `moony.db.bak-<time>` (the newest three are kept), so keep that copy until you have checked
  your history. After the update an older Moony version can no longer open the database.
- **MCP server (breaking for MCP clients).** Aggregates are computed by the same services as
  the app, in your main currency (`mainCurrency`), as decimal strings at the currency's own
  precision. An amount that cannot be converted is `null` and its currency is listed in
  `missingCurrencies`; `fxEstimated` is true when a cost used today's rate. `investments_list` and
  `crypto_list` return an object (`{mainCurrency, investments | crypto, summary, missingCurrencies,
  fxEstimated}`) instead of an array and add `value`, `costBasis`, `unrealizedGain`,
  `unrealizedGainPct` and `realizedGain` per row, plus `annualDividend` for dividend-paying
  stocks. `stock_value_history` and
  `crypto_value_history` rows no longer carry `id` and add `costBasis`. `stocks_analysis`,
  `tag_metrics`, `portfolio_get_metrics` and `portfolio_get_history` report gains in the main
  currency with the cost at each purchase day's rate. `exchange_rates_list` quotes against an
  explicit `baseCurrency` (EUR; was CZK), marks derived rates and lists `supportedCurrencies`.
- **Cashflow by month, quarter or year.** The Cashflow report now uses the same period switch as
  Budgets instead of 6 months / 12 months / year to date, and it no longer compares with a previous
  range. The chart shows twelve months and highlights the chosen period.
- **Income by category.** Income is grouped by category instead of by counterparty, so a friend paying
  you back no longer shows up as an income source. Uncategorized credits have their own row.
- **Investments, savings and transfers have their own card.** They count as neither income nor
  expenses and are listed by category.
- **Fix categories from the report.** Clicking any row opens its transactions, and each one can be
  recategorized right there; the report updates while the list stays put.
- **Three category roles.** A category now counts as income, expense or money transfer. The former
  investment, financing and adjustment roles became money transfer; no total changes.
- **Salary and new income categories.** "Income" is renamed "Salary" (unless you renamed it), and
  "Investment income" and "Other income" are added.
- **Savings no longer count as spending.** The Savings category is now a money transfer, unless you
  had chosen another role. Past months change accordingly: spending drops and the savings rate rises.
  A budget limit on Savings stops tracking anything; remove it on the Budgets page.
- **Automatic categorization knows the new income categories.** Interest keywords now go to
  Investment income, and pensions, benefits and tax refunds go to Other income instead of Salary. The
  Czech "vratka" and "dobropis" keywords no longer file a refund as income: a refund belongs in the
  category of the original purchase, so it stays uncategorized for you to assign.
- **A correction under a custom rule rewrites that rule.** When one of your own rules decides a
  payee's category, remembering a correction changes the rule's category (it has precedence over
  learned choices) and the picker says so, with a link to the rule. A payment that cannot be
  remembered, or a transfer between your own accounts, now says why instead of silently learning
  nothing.
- **One tag per group.** A tag group now splits your portfolio into parts, so a position has at
  most one tag from each group; tags without a group stay free labels that can overlap and
  filter but never split. The assign dialog offers one choice per group, and moving a tag into a
  group is refused when a position would then carry two tags of it (the dialog names them). Before
  this rule a position could carry several tags of one group and the same tag showed different
  values in the ring, the table and the chart. Migration 015 resolves such duplicates from
  earlier versions on the first start after the update: the tag assigned first stays, the other
  assignments of that group are removed (tags themselves are kept); the database is copied next
  to itself first, as for any migration. For overlapping labels use tags without a group, or
  two groups.
- **Tags are assigned one position at a time.** The assign dialog lists your positions with a search
  box: click a position and pick its tags (one choice per group, any number of tags without a
  group). The dialog counts the changed positions, and one "Save tags" writes the changes of all
  of them together.
- **MCP imports keep the same rules as the app.** Writes through the MCP server are validated
  like the app's own forms: a sale above the holding (counted in date order over the whole
  batch), an unknown or conflicting coin ID, a non-finite number, an invalid date or a case
  variant of an enum is refused with the offending row named. Stock imports through MCP are
  recorded as import batches that can be undone. Duplicates are compared by value, currency and
  day. Exchange rates, prices and history are refreshed by a background job that survives a
  locked app and resumes after unlock (migration 016). Positions that were already oversold
  before this version cannot be edited, deleted or undone until the sale is corrected.
- **Dependencies.** Tauri 2.12.3 and its plugins, Yahoo Finance client 5, zip 9 for backups and
  the usual minor updates; two unused front-end packages were removed.

## [0.9.1] - 2026-10-05

### Added

- **Stock import from brokers.** A wizard (file, columns, review, done) replaces the old import
  table. It reads exports from XTB, Trading 212, Degiro and Interactive Brokers, and your own
  table made in Excel (Czech or English headers, any delimiter and encoding); other layouts are
  mapped column by column and can be remembered. Before anything is written it confirms the
  securities on Yahoo Finance, shows what happens to every row and catches duplicates (by the
  broker's trade ID, the same values, or a trade you entered by hand with a rounded price) and
  sells above the holding. Every import is recorded and can be undone. Fees are not imported.
- **Year to date.** The dashboard, the list trend cards and the stock, crypto and bank account
  charts offer the horizons 30 days · 3 months · This year · Year · All, bounded on UTC days.
- **Cash over time.** Bank accounts get a trend card with the recorded cash history, the same
  data that feeds net worth.
- **Buys and sells on the stocks trend.** The portfolio chart on Stocks marks each trading day
  (tickers and amount in the tooltip), as Crypto already did.
- **New loans on the debt trajectory.** Each loan start is marked where the debt steps up (paid-off
  loans included), and the step is drawn as a step instead of a ramp.
- **Purchase date for real estate.** An optional purchase date starts the value trace with the
  purchase and is part of the real estate CSV export; the first estimate of a new property or
  other asset is recorded when it is created.
- **Company data on the position detail.** Sector, industry, P/E, market cap and the other key
  figures of a holding are downloaded (at most once a day) and shown on the stock detail.
- **Upcoming items on the overview.** A "What's coming up" card on the dashboard is an agenda of what
  is coming: each item has a date chip with the day and how far away it is (dark only for a deadline
  at most a week off), in two columns, six at first and "Show all" for the rest. It covers
  insurance anniversaries with the last day to give notice, contract ends and larger insurance
  payments, fixed-rate ends, loan payoff, bond maturity and coupons, term-account and
  promotional-rate ends, and watchlist targets the price has reached. Information (insurance
  payments, bond coupons, loan payoff) appears only inside its reminder window. Data upkeep (a stale
  backup, bank balances, property valuations, loan balances to compare with a statement) is folded
  into one quiet line under it. The "···" menu of every item offers "Hide" (until the data changes,
  for upkeep), "Remind me in a week" (not for an item at most a week off; "Remind me in a month" for upkeep)
  and "Don't remind me about …" for a whole group, each confirmed by a toast with "Undo". Settings → General
  has a card "Overview reminders" to turn a group back on. The top bar shows only a deadline
  (notice, contract or fixed-rate end, maturity, account or promotional-rate end) at most 14 days
  away, as "Savings account · in 10 days".
- **Rate valid until on bank accounts.** An account can carry the date its promotional rate ends
  ("Rate valid until"); it shows up among the upcoming items.
- **Watchlist targets that wait for a dip or a rise.** A target now remembers whether it waits for
  the price to fall (a buy) or to rise (a sell), is pre-selected from the typed price and can be
  switched in the target dialog.

### Changed

- **Charts.** The net worth chart is taller (220 px) and every list-page trend card shares one
  card and a 170 px chart (Stocks, Crypto, Real estate, Other assets, Bank accounts, Loans, Bonds,
  Insurance).
- **Stocks analysis.** The tag chips filter everything below them (allocation, TWR per tag, tag
  and holdings tables); the two cards are equally tall; TWR is shown in percent with the whole
  portfolio as a dark dashed reference and its return in the card head.
- **Insurance and real estate tables** have fewer, denser columns (type and insurer, type and
  address, the purchase price under the market value) and never scroll horizontally.
- **Recent moves** on the dashboard no longer has a "Show all" link that opened bank accounts.
- **Import CSV** is a button in the head of the Stocks page instead of the footer of the add
  dialog.
- **Bank accounts total** leaves out accounts excluded from net worth, as the dashboard does; the
  excluded amount is shown next to it.
- **Amounts follow the main currency.** Gain, yield, appreciation, rent and savings totals on
  Other assets, Real estate, Cashflow, Cashflow planning and Projection were shown in CZK
  whatever main currency you chose; they now follow it. The cashflow CSV export writes its
  amounts in the main currency and its headers end in the currency code (for example
  `income_eur`). The projection's monthly contributions are entered in the main currency, and
  new other-asset forms default to it.
- **MCP server (breaking for MCP clients).** The aggregate tools report amounts in your main
  currency and name it in a top-level `mainCurrency` field. The `*_czk` / `*Czk` keys lost the
  suffix (for example `net_worth_czk` is now `net_worth`, `valueCzk` is now `value`). History
  tools convert each day at that day's exchange rate. `tag_metrics`, `portfolio_get_history`,
  `stock_value_history` and `crypto_value_history` now return an object (`{mainCurrency, tags}`
  or `{mainCurrency, history}`) instead of a bare array. Create tools default a missing
  currency to the main currency.
- **Real estate investment calculator.** The rental property calculator is now called the real
  estate investment calculator ("Investice do nemovitosti" in Czech), since it values the whole
  investment: rent and cashflow, loan repayment, price growth and the gain from a future sale.

### Fixed

- **A watchlist target below the current price showed as reached.** A target set under the price
  is now a target for a dip and is reported only once the price falls to it.
- **London-listed stocks were valued in the wrong currency.** A quote was labelled with the
  currency its ticker suffix suggests, so a London share quoted in pence (BARC.L, LLOY.L) was a
  hundred times too high and a dollar ETF listed in London (CSPX.L, VWRA.L, IWDA.L, EIMI.L) was
  valued as pounds. Prices, the previous close, dividends, the charts and the 52-week range now
  use the currency Yahoo reports, and the value history of the affected holdings is rebuilt once
  in the background. If you entered the purchase prices of such a holding in pence to match the
  old display, edit them to pounds.
- **Chart tooltips froze after a buy or sell mark.** On every chart with marks the price tooltip
  kept showing the first day while the cursor moved; it now follows the cursor, and a mark's own
  tooltip appears on hover and stays inside the card at the edges.
- **Real estate and other assets value charts showed a single point** and asked the database for
  the history about 34 times a second; they now load the history once per day and horizon.
- **A first revaluation lost the original estimate.** Every property and other asset now has a
  valuation log from its creation (existing ones are seeded once with their current value), so
  the trace shows each estimate; same-day revaluations no longer scramble the line.
- **Company data on the stock detail was always empty**; it was only downloaded for watched
  stocks and never read for holdings.
- **The 52-week range overflowed its card** on the watchlist detail.
- **Stocks analysis switches did nothing visible**: the tag chips only filtered the holdings table
  at the bottom of the page; the whole-portfolio TWR line was drawn in nearly the colour of a tag.
- **Net worth history after saving a property.** Adding or editing a property now records
  today's snapshot right away, as every other change does, instead of waiting for the next one.
- **Insurance limits in another currency** were labelled as Czech crowns in the policies table
  and ranked by their raw amount (a yen limit above a euro one) there and on the policy detail.
- **MCP cashflow and budget reports.** The `cashflow_report` and `budgeting_report` tools
  returned empty lists because they read money stored as text as numbers.
- **Loading a property into the investment calculator brings its loans.** The loan amount,
  term and interest rate now come from the loans linked to the property (summed, rate and term
  weighted by principal); a property without a linked loan sets the loan to 0 instead of
  keeping the example mortgage. Prices, rent, costs and loans are converted to the main
  currency.
- **Calculator numbers in English.** The annuity and real estate calculators read
  "5,400,000" as 5.4, so amounts loaded from a loan or a property were wrong in the English
  interface; thousands separators of the interface language are now understood.
- **Real estate calculator chart amounts.** The sale milestone and the chart tooltips converted
  the calculator's amounts from CZK a second time when the main currency was not CZK.

### Security

- **Tauri 2.12.** The Tauri version in 0.9.0 (2.11.5) had an IPC access-control flaw in its fetch
  command ([GHSA-w28w-mhc8-qvjv](https://github.com/advisories/GHSA-w28w-mhc8-qvjv)); 0.9.1 ships
  Tauri 2.12 with the fix, together with updated Tauri plugins, Rust crates and npm packages.

## [0.9.0] - 2026-10-04

First public release of Moony, published as a pre-1.0 version for a round of testing on all
three platforms before 1.0.0. Moony started as a private project; this version is a fresh start
with a new repository, a new application identifier and a single database schema, so it does
not open databases written by earlier private builds (the owner's data is converted once by a
local tool). Everything below is new to the public.

### What Moony does

- **Net worth in one place.** Bank accounts, stocks, crypto, bonds, real estate, loans,
  insurance and other assets, with a net-worth history chart, asset allocation and a
  *Getting started* checklist on the dashboard.
- **Local and encrypted.** All records live in one SQLCipher (AES-256) database in your user
  profile, protected by your password and a 24-character recovery key. No account, no cloud,
  no sync. Market data is the only thing fetched from the internet (Yahoo Finance, CoinGecko,
  ECB and Frankfurter exchange rates); see `PRIVACY.md` for the exact list. Moony sends no
  telemetry: no analytics, no usage statistics and no crash reports.
- **Bank statements.** CSV import with bank presets detected from the header row (Revolut,
  Wise, N26, Fio, Česká spořitelna, Komerční banka, Air Bank, ČSOB, mBank, Raiffeisenbank,
  Moneta, Chase, Sparkasse, DKB, ING Germany and a generic UK debit/credit layout), multilingual
  column detection with a parsed preview, duplicate skipping and one-click undo of an import.
- **Categorization.** Your own rules, rules learned from your choices, and rule packs for many
  countries; a rules page with hit counts, automation rate and a live match preview.
- **Investments.** Stock and crypto positions with live prices (no API key needed for stocks),
  cost basis from transactions, dividends, tags and tag groups, a stock analysis page, a stock
  monitor watchlist that never counts toward net worth, and per-ticker value history rebuilt in
  the background.
- **Loans, real estate, insurance.** Amortized loan balances with a repayment schedule and
  loan events (extra payments, rate changes, balance checks), dated property valuations, photo
  and document attachments, recurring costs, linked loans and insurance policies.
- **Planning.** Budgets by category, actual cashflow from bank transactions, cashflow planning,
  a net-worth projection with explicit assumptions, and annuity and rental-property calculators.
- **International.** English and Czech interface; money, numbers and dates follow the UI
  language; every ECB currency plus common others, each account in its own currency and totals
  in the one you choose.
- **Data and backups.** One-click backup to a ZIP, restore that keeps your current data in a
  `pre-restore-…` folder, full JSON export and a database integrity check, all in Settings.
- **AI assistant (optional).** A built-in MCP server on `127.0.0.1`, off by default, token
  protected, with read tools and a small set of write tools.
- **Updates.** A one-click in-app update checked after unlock, verified against the project's
  updater key; the check can be switched off.
- **Security.** Webview access to the data folder is denied except for photos; strict
  content-security policy; owner-only key files; scrypt N = 2¹⁷ key derivation; constant-time
  token comparison; validated backup archives.

### Known limitations

- Installers are not signed by Apple or Microsoft; the README explains the one-time Gatekeeper
  and SmartScreen steps. The in-app updater still verifies its own signature.
- Attachments (photos and documents) are stored as plain files next to the encrypted database.
- The updater only offers versions higher than the installed one, so a machine running an
  older 1.x build from the previous private repository must install 0.9.0 by hand.

[Unreleased]: https://github.com/fiiles/Moony/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/fiiles/Moony/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/fiiles/Moony/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/fiiles/Moony/releases/tag/v0.9.0
