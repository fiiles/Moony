# Plan: mobile dialog fields

1. `amountInputText`, `decimalSeparator`, `decimalTextToDot` in src/mobile/lib/amount-input.ts + tests (done first, TDD).
2. `PhoneDecimalInput` inside src/components/ui/input.tsx (component, no unit test per policy).
3. `inputMode="text"` on BalanceRecordDialog and BankAccountFormDialog balance.
4. Locale prefill in revalue.ts, dividend-form.ts, budget-limit.ts (+ tests) and the seven sheets.
5. Gates: typecheck, lint, format:check, full vitest.
