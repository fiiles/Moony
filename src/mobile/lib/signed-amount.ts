import type { BankTransaction } from '@shared/schema';
import type { Formatters } from '@/lib/format';

/**
 * A movement's amount as text, signed and in its own currency: "+ 1 249 Kč" for a credit, "− 80 €"
 * for a debit (the sign comes from the type, the number is the absolute amount). Pass the phone's
 * `useMoneyText()` as `money`: while amounts are hidden the number is masked and the sign stays
 * ("+ •••• Kč"). Shared by the ledger row and the transaction sheet.
 */
export function signedAmountText(
  money: Formatters['money'],
  tx: Pick<BankTransaction, 'type' | 'amount' | 'currency'>
): string {
  const sign = tx.type === 'credit' ? '+ ' : '− ';
  return `${sign}${money(Math.abs(Number(tx.amount) || 0), tx.currency)}`;
}
