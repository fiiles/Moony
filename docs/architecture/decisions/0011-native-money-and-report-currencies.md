# 0011. Native money and explicit report currencies

Date: 2026-10-08

## Status

Accepted. Supersedes ADR0001's mandatory CZK aggregation/presentation rule for actual cashflow, budgeting, their drilldowns and planned cashflow. ADR0001's decimal TEXT storage requirement remains in force. Explicitly CZK portfolio/projection cache contracts remain denominated in CZK until individually migrated.

The CZK cache clause is superseded by [0012](0012-explicit-currencies-and-eur-rate-reference.md): the caches no longer exist and portfolio values follow the same report-currency rules.

## Context

A native EUR payment shown to an EUR user must remain the same EUR payment. Converting historical payments into CZK and the historical total back at a current rate changes past spending when exchange rates move. A global application also needs same-currency reports to work without reference rates. Repeated explanations of this internal conversion burden everyday report screens without correcting their numbers.

## Decision

- Native amount, currency, date and instrument quantity are authoritative. User currency changes never rewrite transactions, contracts or budget limits.
- Reports declare their target currency. Actual cashflow, budgeting and drilldowns convert source amounts directly into it using a common historical quote date on or before booking, with at most ten days of lookback. Same-currency conversion is identity and requires no rates.
- Budget utilization is evaluated in the goal currency selected from the requested period, independent of profile currency. Rows label their native limit and spending; report headlines remain in the requested reporting currency. Missing conversion for budget spending leaves utilization unavailable.
- Planning is an estimate using a single current rate snapshot. Native contract amounts and annual/monthly frequencies are preserved independently of presentation.
- A reference currency is an implementation detail of the exchange-rate service. Cross conversion uses source/reference divided by target/reference at the same date, without intermediate currency rounding. Numeric output is rounded at the target currency's minor unit; detail contributions reconcile with totals.
- Missing conversion data is explicit and never an implicit 1:1 or a fallback into a different report currency. Normal report screens format their declared currency directly; they show a compact exception only when incompleteness affects the result.
- CZK may remain a clearly denominated, reproducible derived cache. It must not replace native facts or be silently reinterpreted as another currency. Existing portfolio/projection consumers remain subject to their explicit CZK contracts.

## Consequences

Report query keys include target currency and rate revision. Currency changes refetch monetary reports instead of changing only the symbol. Export, detail and screen use the same unit and conversion policy. Historical and current conversion are separate operations. Existing native storage makes a data migration unnecessary for this reporting change; older portfolio caches and investment accounting paths require their own targeted migrations if their public contracts change.
