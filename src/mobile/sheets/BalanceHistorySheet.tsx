import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Trash2 } from 'lucide-react';
import type { BankBalanceRecord } from '@shared/schema';
import { useBalanceRecordMutations } from '@/hooks/use-balance-records';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { Sheet } from '@/mobile/components/Sheet';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';

interface BalanceHistorySheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The account the records belong to (the delete refreshes it). */
  accountId: string;
  records: readonly BankBalanceRecord[];
  /** The account currency. */
  currency: string;
}

/**
 * "Historie zůstatku" from the detail's `···` (spec §8.4, the desktop's `BalanceHistoryDialog`):
 * the dated balances newest first — the day, where it came from, the balance and its change against
 * the record before. A record with a wrong day or amount is deleted by a swipe to the left or from
 * the long press's menu, after the desktop's question in a confirm sheet over this one; the record
 * goes, and with it what the hero, "Změna letos" and the average read from it.
 */
export function BalanceHistorySheet({
  open,
  onOpenChange,
  accountId,
  records,
  currency,
}: BalanceHistorySheetProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { deleteRecord } = useBalanceRecordMutations(accountId);
  // The record to delete stays while its sheet slides away, so the sentence does not blank out
  const [pending, setPending] = useState<BankBalanceRecord | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const newestFirst = [...records].sort((a, b) => b.asOf - a.asOf);
  const newest = newestFirst[0];
  const askDelete = (record: BankBalanceRecord) => {
    setPending(record);
    setConfirmOpen(true);
  };

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
              <RecordRow
                key={record.id}
                label={tc('labels.moreActions')}
                onDelete={() => askDelete(record)}
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
      <ConfirmSheet
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t('balanceHistory.deleteTitle')}
        description={
          pending
            ? t('balanceHistory.deleteDescription', {
                amount: moneyText(Number(pending.amount) || 0, currency, { decimals: 0 }),
                date: fmt.day(pending.asOf),
              })
            : ''
        }
        confirmLabel={t('balanceHistory.delete')}
        isPending={deleteRecord.isPending}
        onConfirm={() =>
          pending && deleteRecord.mutate(pending.id, { onSuccess: () => setConfirmOpen(false) })
        }
      />
    </Sheet>
  );
}

interface RecordRowProps {
  /** The menu trigger's name. */
  label: string;
  title: string;
  sub: string;
  value: string;
  valueSub?: string;
  onDelete: () => void;
}

/** One record: swipe left (a `loss` panel) or the menu (a long press or its visible `···`) asks to delete it. */
function RecordRow({ label, title, sub, value, valueSub, onDelete }: RecordRowProps) {
  const { t } = useTranslation('bank_accounts');
  return (
    <EntityRow
      title={title}
      sub={sub}
      value={value}
      valueSub={valueSub}
      onSwipeLeft={onDelete}
      swipeLeftLabel={t('balanceHistory.delete')}
      swipeLeftIcon={Trash2}
      swipeLeftDestructive
      menu={{
        label: `${label}: ${title}`,
        // Smazat is nowhere else: the row has no tap
        shown: true,
        children: (
          <RowMenuItem variant="danger" onSelect={onDelete}>
            <Trash2 aria-hidden strokeWidth={1.75} />
            {t('balanceHistory.delete')}
          </RowMenuItem>
        ),
      }}
    />
  );
}
