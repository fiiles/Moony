import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { InvestmentTransaction } from '@shared/schema';
import { useInvestmentTransactionMutations } from '@/hooks/use-investment-mutations';
import { useFormat } from '@/lib/use-format';
import { EditTransactionModal } from '@/components/stocks/EditTransactionModal';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { Segment } from '@/mobile/components/Segment';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { PositionLedgerState } from '@/mobile/hooks/use-position-ledger';
import { nativeTotal, type TxFilter } from '@/mobile/lib/position-series';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { MoreRow } from './MoreRow';
import { TradeRow } from './TradeRow';

interface PositionLedgerProps {
  /** The filter, the fold and the flash of the rows (`usePositionLedger`). */
  ledger: PositionLedgerState;
  /** Every trade of the position, whatever the filter: the section's number. */
  count: number;
  /** The report currency the rows are valued in. */
  currency: string;
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
  ratesReady: boolean;
}

/**
 * "Transakce" of a position (spec §8.8, prototype frame 12): the section head with the number of
 * trades, the Vše · Nákupy · Prodeje segment and the trades newest first as rows — the first eight,
 * "Zobrazit všech N" for the rest. A row's menu opens the desktop's edit dialog or a confirm sheet
 * for the delete, which says what goes (the desktop's sentence, the amount through the eye).
 */
export function PositionLedger({
  ledger,
  count,
  currency,
  convertAt,
  ratesReady,
}: PositionLedgerProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { deleteTransaction } = useInvestmentTransactionMutations();
  const [editing, setEditing] = useState<InvestmentTransaction | null>(null);
  const [pendingDelete, setPendingDelete] = useState<InvestmentTransaction | null>(null);
  const { filter, setFilter, rows, shown, hidden, unfold, hot } = ledger;

  const options: { value: TxFilter; label: string }[] = [
    { value: 'all', label: ts('detail.ledger.filter.all') },
    { value: 'buy', label: ts('detail.ledger.filter.buys') },
    { value: 'sell', label: ts('detail.ledger.filter.sells') },
  ];
  const deleteDescription = pendingDelete
    ? ts('detail.confirmDeleteTransaction.description', {
        type: (pendingDelete.type === 'buy' ? tc('buttons.buy') : tc('buttons.sell')).toLowerCase(),
        quantity: fmt.number(parseFloat(pendingDelete.quantity) || 0, {
          maximumFractionDigits: 4,
        }),
        ticker: pendingDelete.ticker,
        date: fmt.day(pendingDelete.transactionDate),
        amount: moneyText(nativeTotal(pendingDelete), pendingDelete.currency || currency),
      })
    : '';

  return (
    <>
      <SectionHead title={ts('detail.tabs.transactions')} note={String(count)} />
      <Segment
        value={filter}
        options={options}
        onChange={setFilter}
        label={ts('detail.tabs.transactions')}
        className="mb-2"
      />
      {rows.length === 0 ? (
        <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
          {ts('detail.transactions.empty')}
        </p>
      ) : (
        <EntityRows>
          {shown.map((tx) => (
            <TradeRow
              key={tx.id}
              tx={tx}
              currency={currency}
              convertAt={convertAt}
              ratesReady={ratesReady}
              hot={hot.includes(tx.id)}
              onEdit={setEditing}
              onDelete={setPendingDelete}
            />
          ))}
          {hidden > 0 && (
            <MoreRow
              label={t('wealth.position.tx.showAll', { count: rows.length })}
              onClick={unfold}
            />
          )}
        </EntityRows>
      )}
      <EditTransactionModal
        transaction={editing}
        open={editing !== null}
        onOpenChange={(open) => {
          if (!open) setEditing(null);
        }}
      />
      <ConfirmSheet
        open={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title={ts('detail.confirmDeleteTransaction.title')}
        description={deleteDescription}
        confirmLabel={ts('detail.deleteTransaction')}
        isPending={deleteTransaction.isPending}
        onConfirm={() => {
          if (!pendingDelete) return;
          deleteTransaction.mutate(pendingDelete.id, { onSuccess: () => setPendingDelete(null) });
        }}
      />
    </>
  );
}
