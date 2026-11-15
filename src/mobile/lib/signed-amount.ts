import type { BankTransaction } from '@shared/schema';
import type { Formatters } from '@/lib/format';
import { exactMoneyOptions } from './money-rule';

/**
 * A movement's amount as text, signed, exact and in its own currency: "+ 1 249 Kč" for a credit,
 * "− 2,06 €" or "− 80 €" for a debit (the sign comes from the type, the number is the absolute
 * amount at the currency's minor units, none on a whole amount: README §13, "Money and
 * currencies"). Pass the phone's `useMoneyText()` as `money`. Shared by every list of payments and
 * the transaction sheet.
 */
export function signedAmountText(
  money: Formatters['money'],
  tx: Pick<BankTransaction, 'type' | 'amount' | 'currency'>
): string {
  const sign = tx.type === 'credit' ? '+ ' : '− ';
  const amount = Math.abs(Number(tx.amount) || 0);
  return `${sign}${money(amount, tx.currency, exactMoneyOptions(amount, tx.currency))}`;
}
