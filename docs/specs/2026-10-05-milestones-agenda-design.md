# Milestones as an agenda — design

Date: 2026-10-05 · Status: approved by the owner on 2026-10-05 · Revises:
`2026-10-05-milestones-design.md` (§2 grouping, §3 `canDismiss`, §7 card and indicator).

## 1. Why

The first version grouped items into "Teď jednat" and "Brzy" and counted "N k vyřízení" in the top
bar on every page. Moony is opened every few days, and three different things shared one list:

- **Deadlines** (notice deadline, fixed-rate end, promotional rate end, maturity) — dated, happen
  once, disappear after the date. The reason the feature exists.
- **Information** (insurance payment, coupon, last loan payment) — nothing to do.
- **Data upkeep** (backup, balances, valuation, loan balance check, missing rate after a fixed
  rate) — undated, never disappear by themselves. They could only be snoozed for a week, so they
  came back every week and kept the top-bar count up. That is the nagging the owner reported.

The owner also found the full-width card mostly empty space.

## 2. Decisions

1. **The card is an agenda, not a to-do list.** One chronological list, no groups. Each row
   starts with a date chip (day and month, the relative distance under it); the title and one
   sub-line follow; a "···" menu ends the row. The date is the day that matters: the notice
   deadline of an insurance anniversary while it can still be met, otherwise the event day; a
   crossed watchlist target shows "Dnes · cíl". Rows are laid out in two columns, filled top to
   bottom and then the second column. At most 6 rows; "Zobrazit vše (N)" expands.
2. **The chip turns dark material only for a deadline at most 7 days away**, so urgency is rare
   and visible.
3. **Data upkeep is folded into one quiet line under the agenda**: "Údržba dat · Byt 3+kk
   Vinohrady přecenit · Hypotéka — Vinohrady ověřit zůstatek · Zobrazit 2". It expands into
   rows with an icon instead of a date chip. Upkeep is never counted and never in the top bar.
   Upkeep kinds: `backup_stale`, `balances_stale`, `valuation_stale`, `loan_balance_check`,
   `loan_fixation_expired`.
4. **Every item can be ignored**, from the row's "···" menu:
   - **Skrýt** — this occurrence never shows again. Upkeep keys now carry the date of the data
     they are about (`backup_stale:<last backup day | never>`, `balances_stale:<oldest update
     day>`, `valuation_stale:<id>:<last valuation day>`, `loan_balance_check:<id>:<anchor
     day>`), so hiding an upkeep item lasts until the data changes, not forever.
   - **Připomenout za týden** (upkeep: **za měsíc**) — snooze 7 days, upkeep 30 days.
   - **Nepřipomínat …** — mutes a whole group of kinds. Groups (id → kinds): `insuranceDates`
     (anniversary, end), `insurancePayments` (payment), `loans` (fixation end, payoff), `bonds`
     (maturity, coupon), `accounts` (termination, promotional rate end), `targets` (watch
     target), `backup`, `balances`, `valuations`, `loanChecks` (balance check, fixation
     expired). Muted kinds are stored in `app_config` (`milestones.mutedKinds`, a JSON array)
     and filtered out by `list_milestones`. Settings → Obecné gets a card "Připomínky na
     přehledu" with one switch per group, so a muted group can be turned back on. The agenda
     card head links to it ("Upravit připomínky →").
   - `canDismiss` is removed: every item can be hidden.
5. **The top bar shows only what can be missed**: the nearest deadline (decision 1 kinds while
   acting is still possible) at most 14 days away, as "Spořicí účet · za 10 dní" (+N when there
   are more). Information, upkeep and targets never appear there. A click opens a popover with
   those deadlines and a link to the overview. Most weeks the top bar shows nothing.
6. The reminder windows of the first spec stay (insurance 10 weeks before the anniversary,
   fixed-rate end 6 months, …): the card warns early, the top bar only when it is close.

## 3. Copy (cs)

Sub-lines lose the date that the chip already shows: "Poslední den výpovědi · výročí 10. 12.",
"Smlouva končí", "Platba 3 600 Kč", "Konec fixace · čas porovnat nabídky", "Poslední splátka",
"Splatnost · vrátí se 104 500 Kč", "Kupón 4 500 Kč", "Účet končí", "Zvýhodněná sazba končí".
Menu: "Skrýt", "Připomenout za týden" / "Připomenout za měsíc", "Nepřipomínat platby
pojistného". Toasts: "Skryto", "Odloženo o týden" / "Odloženo o měsíc", "Připomínky vypnuty ·
Platby pojistného", each with "Vrátit". Relative labels are lower case ("za 10 dní"): they
follow a date or a title.

## 4. Out of scope

OS notifications, calendar export, per-item notes, changing the reminder windows in settings.
