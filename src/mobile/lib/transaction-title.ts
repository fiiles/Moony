import { completeAccountIdentifier } from '@/utils/iban-utils';

/** What a payment carries that can name it. */
export interface TransactionNaming {
  counterpartyName: string | null;
  counterpartyIban: string | null;
  counterpartyBankCode: string | null;
  description: string | null;
}

const clean = (value: string | null | undefined) => value?.trim() || null;

/**
 * The one title of a payment on every list of the phone (README §13, "Transactions", audit
 * 2026-11-15 C4): the named counterparty, else the payment's description, else the counterparty's
 * account number; null when the payment says none of it (the caller words a fallback). The bank's
 * remittance note never titles a row: it is raw statement text, shown in full only in the
 * transaction sheet.
 */
export function transactionTitle(tx: TransactionNaming): string | null {
  return (
    clean(tx.counterpartyName) ??
    clean(tx.description) ??
    completeAccountIdentifier(tx.counterpartyIban, null, tx.counterpartyBankCode)
  );
}

/**
 * The sub-line facts of a payment row, always in one order — the day, the account, the category —
 * with what the screen already says left out (`null`): the ledger has day heads and is the
 * account, the category detail is the category.
 */
export function transactionSubFacts(parts: {
  day: string | null;
  account: string | null;
  category: string | null;
}): string[] {
  return [parts.day, parts.account, parts.category].filter((part): part is string => !!part);
}
