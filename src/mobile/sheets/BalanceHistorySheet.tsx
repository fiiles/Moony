import { useTranslation } from 'react-i18next';
import type { BankBalanceRecord } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { Sheet } from '@/mobile/components/Sheet';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

interface BalanceHistorySheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  records: readonly BankBalanceRecord[];
  /** The account currency. */
  currency: string;
}

/**
 * "Historie zůstatku" from the detail's `···` (spec §8.4): the dated balances newest first — the
 * day, where it came from, the balance and its change against the record before. Read-only on the
 * phone: deleting a record stays on the desktop.
 */
export function BalanceHistorySheet({
  open,
  onOpenChange,
  records,
  currency,
}: BalanceHistorySheetProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const newestFirst = [...records].sort((a, b) => b.asOf - a.asOf);
  const newest = newestFirst[0];

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('balanceHistory.title')}
      description={
        newest
          ? t('balanceHistory.sub', { count: records.length, date: fmt.day(newest.asOf) })
          : t('balanceHistory.empty')
      }
      footer={
        <Button
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.close')}
        </Button>
      }
    >
      {newestFirst.length > 0 && (
        <EntityRows className="mb-0">
          {newestFirst.map((record, i) => {
            const previous = newestFirst[i + 1];
            const amount = Number(record.amount) || 0;
            return (
              <EntityRow
                key={record.id}
                title={fmt.day(record.asOf)}
                sub={t(
                  record.source === 'statement'
                    ? 'balanceHistory.sourceStatement'
                    : 'balanceHistory.sourceManual'
                )}
                value={moneyText(amount, currency, { decimals: 0 })}
                valueSub={
                  previous
                    ? moneyText(amount - (Number(previous.amount) || 0), currency, {
                        signed: true,
                        decimals: 0,
                      })
                    : undefined
                }
              />
            );
          })}
        </EntityRows>
      )}
    </Sheet>
  );
}
