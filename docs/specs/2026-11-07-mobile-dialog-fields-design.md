# Mobile dialog fields (audit C1)

## Why
About 20 desktop dialogs (buy/sell stock, crypto and other assets, account, loan and loan events, insurance, real estate, manual price, valuation, ...) open as phone sheets but keep desktop number fields. `<input type="number">` refuses a comma in WKWebView when the keypad language differs from the page's and shows no decimal pad. Phone edit sheets also prefill amounts with a decimal point in comma locales.

## Goals
- Every decimal `Input type="number"` in the phone shell is a text field with the decimal keypad that accepts "1,5" and "1 234,5", without rewriting the forms.
- Desktop rendering and behaviour are unchanged.
- One tested helper prefills phone amount fields with the locale decimal separator.

## Non-goals
No rewrite of the dialogs into phone forms (`AmountField`/`DateField`); no new defaults beyond what exists.

## Design
- Already in place (index.css, shell attribute rules): 16 px (`--m-fs-field`, Dynamic Type) floor for input/textarea/select in the shell, 48 pt height, no system chrome and left-aligned dates for fields in `[data-phone-sheet]`, comboboxes at 48 pt. Verified, not changed. Native `type="date"` stays: it opens the system picker and is already sized and aligned by those rules.
- `Input` (src/components/ui/input.tsx): when `isMobileShell` and `type="number"` and `inputMode` is unset, `decimal` or `text`, it renders `PhoneDecimalInput`: `type="text"`, `inputMode` decimal, min/max/step dropped. The field keeps the typed text; `onChange` hands the parent an event whose `target.value` is dot text (`decimalTextToDot`), so react-hook-form and `requiredNumber` (`Number("1.5")`) work as with a number input. A value changed from outside (reset, prefill) replaces the text, formatted with the UI language's separator. A numeric `0` default shows as an empty field (placeholder) instead of "0" to delete.
- Integer fields (`inputMode="numeric"`: splits, port, mapping) stay number inputs: digits only, no comma problem.
- **(call)** Signed fields (balance record, opening balance) get `inputMode="text"`: the decimal pad has no minus. Cost: the full keyboard shows for them.
- `amountInputText(value, locale)` and `decimalSeparator` in src/mobile/lib/amount-input.ts; used by CostSheet, DividendSheet (dividend-form), RevalueSheet (revalue), BudgetLimitSheet (limitDefaults), WatchTargetSheet, PlanFormSheet, ExtraPaymentSheet. Pure helpers get an optional `locale` (default `en`).
- **(call)** Smart defaults (§10.5): buy/sell dialogs already prefill date today and the current price; nothing added. Skipped: quantity prefill on sell (no clearly right value), account pre-selection.
- **(call)** Separator comes from the UI language, not the keypad's. Cost if wrong: prefilled text uses the app language's separator; any separator still parses.

## Risk
Parent handlers that read `e.target.valueAsNumber` or other DOM properties would break under the synthetic target; none exist (grep: only `.value`).
