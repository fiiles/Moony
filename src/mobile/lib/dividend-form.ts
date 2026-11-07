import type { InsertStockDividend, StockDividend } from '@shared/schema';
import { isoDateFromUtcTimestamp } from '@/utils/period';
import { parseAmountInput } from '@/mobile/lib/amount-input';
import { isoToUtcDay } from '@/mobile/lib/property-detail';

/** What "Zapsat dividendu" is typing: the texts as typed, the ex-date as the ISO day the field speaks. */
export interface DividendForm {
  dateIso: string;
  amountText: string;
  note: string;
}

const NOTE_MAX = 500;

/** A new dividend starts on `today` with empty fields; an edit takes the stored values. */
export function dividendForm(dividend: StockDividend | null, today: string): DividendForm {
  if (!dividend) return { dateIso: today, amountText: '', note: '' };
  return {
    dateIso: isoDateFromUtcTimestamp(dividend.exDate),
    amountText: String(Number(dividend.grossAmount)),
    note: dividend.note ?? '',
  };
}

export interface DividendErrors {
  amount?: 'required' | 'invalid' | 'notPositive';
  date?: 'required';
  note?: 'tooLong';
}

/** The gross must be a number above zero, the ex-date is needed, the trimmed note has 500 characters at most. */
export function dividendErrors(form: DividendForm): DividendErrors {
  const errors: DividendErrors = {};
  if (form.amountText.trim() === '') {
    errors.amount = 'required';
  } else {
    const amount = parseAmountInput(form.amountText);
    if (amount === null) errors.amount = 'invalid';
    else if (!(Number(amount) > 0)) errors.amount = 'notPositive';
  }
  if (form.dateIso === '') errors.date = 'required';
  if (form.note.trim().length > NOTE_MAX) errors.note = 'tooLong';
  return errors;
}

/** The payload the desktop's `StockDividendModal` writes; `null` while the form has an error. */
export function dividendInput(form: DividendForm, currency: string): InsertStockDividend | null {
  if (Object.keys(dividendErrors(form)).length > 0) return null;
  const grossAmount = parseAmountInput(form.amountText);
  if (grossAmount === null) return null;
  return {
    exDate: isoToUtcDay(form.dateIso),
    grossAmount,
    currency,
    note: form.note.trim() || null,
  };
}
