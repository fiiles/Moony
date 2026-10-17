import { useTranslation } from 'react-i18next';
import type { BankTransaction } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

/**
 * The sentence of the delete confirmation, as the desktop words it: what goes, named by its
 * counterparty (or description), day and signed amount in its own currency (masked while amounts
 * are hidden: the date and the counterparty still say which row it is).
 */
export function useDeleteDescription(): (tx: BankTransaction) => string {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const moneyText = useMoneyText();

  return (tx) => {
    const value = Math.abs(Number(tx.amount) || 0);
    const signedValue = tx.type === 'credit' ? value : -value;
    const amount = moneyText(signedValue, tx.currency, { signed: true });
    const name = tx.counterpartyName || tx.description;
    const date = fmt.day(tx.bookingDate);
    return name
      ? t('confirmDeleteTransaction.description', { name, date, amount })
      : t('confirmDeleteTransaction.descriptionNoName', { date, amount });
  };
}
