import type { InsertAssetValuation } from '@shared/schema';
import { parseAmountInput } from './amount-input';
import { isoToUtcDay } from './property-detail';

/**
 * The revaluation form as pure functions (spec §8.8, the phone counterpart of the desktop's
 * `RevalueDialog`): what a new estimate starts from, the valuation the form makes and the total it
 * stands for. One sheet serves the property (a whole value) and the other asset (a price per unit).
 */

export interface RevalueForm {
  /** As typed on the keypad. */
  valueText: string;
  /** The date field: `YYYY-MM-DD`, or empty. */
  dateIso: string;
  note: string;
}

export interface RevalueErrors {
  value?: 'required' | 'invalid';
  date?: 'required' | 'future';
}

/**
 * The form of a new estimate: today's estimate (`value`, in the asset's currency; per unit for an
 * asset held by the unit) to start from, today (`YYYY-MM-DD`), no note. While the amounts are hidden
 * the field starts empty: a prefilled amount would show what the eye hides.
 */
export function startRevalueForm(value: number, hidden: boolean, today: string): RevalueForm {
  return { valueText: hidden ? '' : String(value), dateIso: today, note: '' };
}

/**
 * The revaluation the form makes (the desktop `RevalueDialog`'s rules): an amount of zero or more,
 * a day that is not in the future, an optional note. The amount goes to the backend as decimal
 * text, the day as its UTC-midnight second.
 */
export function revalueData(
  form: RevalueForm,
  args: { assetId: string; currency: string; today: string }
): { ok: true; data: InsertAssetValuation } | { ok: false; errors: RevalueErrors } {
  const errors: RevalueErrors = {};
  const value = parseAmountInput(form.valueText);
  if (form.valueText.trim() === '') errors.value = 'required';
  else if (value === null || value.startsWith('-')) errors.value = 'invalid';
  if (form.dateIso === '') errors.date = 'required';
  else if (form.dateIso > args.today) errors.date = 'future';
  if (errors.value || errors.date || value === null) return { ok: false, errors };
  return {
    ok: true,
    data: {
      assetId: args.assetId,
      value,
      currency: args.currency,
      valuedAt: isoToUtcDay(form.dateIso),
      note: form.note.trim() || null,
    },
  };
}

/**
 * What the typed price makes of the units held (`quantity × price`, the desktop's total hint);
 * `null` while the field holds no amount of zero or more.
 */
export function revaluedTotal(valueText: string, quantity: number): number | null {
  const value = parseAmountInput(valueText);
  if (value === null || value.startsWith('-')) return null;
  const total = Number(value) * quantity;
  return Number.isFinite(total) ? total : null;
}
