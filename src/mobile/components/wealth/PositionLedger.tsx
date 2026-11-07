import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { StockDividend } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { Segment } from '@/mobile/components/Segment';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { PositionLedgerState } from '@/mobile/hooks/use-position-ledger';
import { nativeTotal, type LedgerFilter, type LedgerTx } from '@/mobile/lib/position-series';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { MoreRow } from './MoreRow';
import { DividendRow } from './DividendRow';
import { TradeRow } from './TradeRow';

interface PositionLedgerProps<T extends LedgerTx> {
  /** The filter, the fold and the flash of the rows (`usePositionLedger`). */
  ledger: PositionLedgerState<T>;
  /** Every trade and dividend of the position, whatever the filter: the section's number. */
  count: number;
  /** The report currency the rows are valued in. */
  currency: string;
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
  ratesReady: boolean;
  /** What the quantity is counted in ("ks" for shares). */
  unit: string;
  /** Decimals the quantity is written with (default 4; a coin is held to 8). */
  quantityDigits?: number;
  /** The delete of a trade (the position's own mutation); the confirm sheet runs it. */
  deleteTransaction: {
    mutate: (id: string, opts: { onSuccess: () => void }) => void;
    isPending: boolean;
  };
  /** Opens the edit of a trade (the page owns the dialog); without it the rows have no Upravit. */
  onEdit?: (tx: T) => void;
  /** A stock's received dividends (the ledger's own, `usePositionLedger`): adds the fourth segment option. */
  dividends?: readonly StockDividend[];
  /** Opens the edit of a manual dividend; without it the dividend's menu has only Smazat. */
  onEditDividend?: (dividend: StockDividend) => void;
  /** The delete of a dividend (the page's `useStockDividendMutations`); the confirm sheet runs it. */
  deleteDividend?: {
    mutate: (id: string, opts: { onSuccess: () => void }) => void;
    isPending: boolean;
  };
}

/**
 * "Transakce" of a position (spec §8.8, prototype frame 12): the section head with the number of
 * trades and dividends, the Vše · Nákupy · Prodeje (· Dividendy for a stock) segment and the trades
 * and dividends newest first as rows — the first eight,
 * "Zobrazit všech N" for the rest. A row's menu offers the page's edit (when it has one) or a
 * confirm sheet for the delete, which says what goes (the desktop's sentence).
 */
export function PositionLedger<T extends LedgerTx>({
  ledger,
  count,
  currency,
  convertAt,
  ratesReady,
  unit,
  quantityDigits = 4,
  deleteTransaction,
  onEdit,
  dividends,
  onEditDividend,
  deleteDividend,
}: PositionLedgerProps<T>) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [pendingDelete, setPendingDelete] = useState<T | null>(null);
  const [pendingDividendDelete, setPendingDividendDelete] = useState<StockDividend | null>(null);
  const { filter, setFilter, rows, shown, hidden, unfold, hot } = ledger;

  const options: { value: LedgerFilter; label: string }[] = [
    { value: 'all', label: ts('detail.ledger.filter.all') },
    { value: 'buy', label: ts('detail.ledger.filter.buys') },
    { value: 'sell', label: ts('detail.ledger.filter.sells') },
    ...(dividends
      ? [{ value: 'dividend' as const, label: ts('detail.ledger.filter.dividends') }]
      : []),
  ];
  const dividendDescription = pendingDividendDelete
    ? ts('detail.dividendRecords.confirmDelete.description', {
        date: fmt.day(pendingDividendDelete.exDate),
        amount: moneyText(
          Number(pendingDividendDelete.grossAmount),
          pendingDividendDelete.currency
        ),
      })
    : '';
  const deleteDescription = pendingDelete
    ? ts('detail.confirmDeleteTransaction.description', {
        type: (pendingDelete.type === 'buy' ? tc('buttons.buy') : tc('buttons.sell')).toLowerCase(),
        quantity: fmt.number(parseFloat(pendingDelete.quantity) || 0, {
          maximumFractionDigits: quantityDigits,
        }),
        ticker: pendingDelete.ticker,
        date: fmt.day(pendingDelete.transactionDate),
        amount: moneyText(nativeTotal(pendingDelete), pendingDelete.currency || currency),
      })
    : '';

  return (
    <>
      <SectionHead
        title={ts('detail.tabs.transactions')}
        note={String(count + (dividends?.length ?? 0))}
      />
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
          {shown.map((entry) =>
            entry.kind === 'dividend' ? (
              <DividendRow
                key={entry.id}
                dividend={entry.dividend}
                currency={currency}
                convertAt={convertAt}
                ratesReady={ratesReady}
                hot={hot.includes(entry.id)}
                onEdit={onEditDividend}
                onDelete={setPendingDividendDelete}
              />
            ) : (
              <TradeRow
                key={entry.id}
                tx={entry.tx}
                unit={unit}
                quantityDigits={quantityDigits}
                currency={currency}
                convertAt={convertAt}
                ratesReady={ratesReady}
                hot={hot.includes(entry.id)}
                onEdit={onEdit}
                onDelete={setPendingDelete}
              />
            )
          )}
          {hidden > 0 && (
            <MoreRow
              label={t('wealth.position.tx.showAll', { count: rows.length })}
              onClick={unfold}
            />
          )}
        </EntityRows>
      )}
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
      {deleteDividend && (
        <ConfirmSheet
          open={pendingDividendDelete !== null}
          onOpenChange={(open) => {
            if (!open) setPendingDividendDelete(null);
          }}
          title={ts('detail.dividendRecords.confirmDelete.title')}
          description={dividendDescription}
          confirmLabel={ts('detail.dividendRecords.confirmDelete.confirm')}
          isPending={deleteDividend.isPending}
          onConfirm={() => {
            if (!pendingDividendDelete) return;
            deleteDividend.mutate(pendingDividendDelete.id, {
              onSuccess: () => setPendingDividendDelete(null),
            });
          }}
        />
      )}
    </>
  );
}
