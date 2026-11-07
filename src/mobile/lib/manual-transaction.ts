import type { InsertBankTransaction } from '@shared/schema';
import { parseAmountInput } from '@/mobile/lib/amount-input';
import { isoToUtcDay } from '@/mobile/lib/property-detail';

/** What "Přidat transakci" is typing: the texts as typed, the day as the ISO day the field speaks. */
export interface ManualTxForm {
  type: 'debit' | 'credit';
  amountText: string;
  dateIso: string;
  counterparty: string;
  description: string;
  counterpartyIban: string;
  /** `null` = no category; the backend runs no rules for a manual entry. */
  categoryId: string | null;
}

/** A new entry: an outgoing payment for `today` (the desktop modal's default), no category. */
export function emptyManualTxForm(today: string): ManualTxForm {
  return {
    type: 'debit',
    amountText: '',
    dateIso: today,
    counterparty: '',
    description: '',
    counterpartyIban: '',
    categoryId: null,
  };
}

export interface ManualTxErrors {
  amount?: 'required' | 'invalid' | 'notPositive';
  date?: 'required';
}

/** What keeps the form from being saved: the amount must be a number above zero, the day is needed. */
export function manualTxErrors(form: ManualTxForm): ManualTxErrors {
  const errors: ManualTxErrors = {};
  if (form.amountText.trim() === '') {
    errors.amount = 'required';
  } else {
    const amount = parseAmountInput(form.amountText);
    if (amount === null) errors.amount = 'invalid';
    else if (!(Number(amount) > 0)) errors.amount = 'notPositive';
  }
  if (form.dateIso === '') errors.date = 'required';
  return errors;
}

const textOrNull = (text: string) => text.trim() || null;

/**
 * The payload the desktop's `AddTransactionModal` writes: the amount as decimal text in the
 * account's currency, the UTC-midnight day, empty texts as `null`, a booked status. `null` while
 * the form has an error.
 */
export function manualTxInput(
  form: ManualTxForm,
  accountId: string,
  currency: string
): InsertBankTransaction | null {
  if (Object.keys(manualTxErrors(form)).length > 0) return null;
  const amount = parseAmountInput(form.amountText);
  if (amount === null) return null;
  return {
    bankAccountId: accountId,
    type: form.type,
    amount,
    currency,
    bookingDate: isoToUtcDay(form.dateIso),
    description: textOrNull(form.description),
    counterpartyName: textOrNull(form.counterparty),
    counterpartyIban: textOrNull(form.counterpartyIban),
    categoryId: form.categoryId,
    status: 'booked',
  };
}
