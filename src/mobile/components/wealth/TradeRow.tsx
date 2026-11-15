import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Pencil, TrendingDown, TrendingUp, Trash2 } from 'lucide-react';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { Money } from '@/mobile/components/Money';
import { useApprox } from '@/mobile/hooks/use-approx';
import { exactMoneyOptions } from '@/mobile/lib/money-rule';
import { nativeTotal, type LedgerTx } from '@/mobile/lib/position-series';
import { SubFacts } from '@/mobile/components/SubFacts';

interface TradeRowProps<T extends LedgerTx> {
  tx: T;
  /** What the quantity is counted in ("ks" for shares). */
  unit: string;
  /** Decimals the quantity is written with (default 4; a coin is held to 8). */
  quantityDigits?: number;
  /** The report currency the value is converted into. */
  currency: string;
  /** Converts at the rates of the trade's own day; `null` without a rate and until `ratesReady`. */
  convertAt: (amount: number, from: string, to: string, timestamp: number) => number | null;
  ratesReady: boolean;
  /** The row a tap on its mark on the chart points at: it takes `well` and fades back. */
  hot: boolean;
  /** Without it the row's menu has no Upravit (a coin's trade cannot be edited). */
  onEdit?: (tx: T) => void;
  onDelete: (tx: T) => void;
}

/**
 * A buy or a sell of the position (spec §8.8, prototype frame 12): the entity row with a ▲ / ▽
 * chip (green for a sell), "Nákup · 12 ks" over the day and the price in the trade's currency, and
 * the total in the trade's own currency, exactly — "− 1 350,50 USD" for a buy, "+ 520 USD" for a
 * sell — over its conversion into the main currency at the rates of the trade's day ("≈ 32 750
 * Kč"; README §13, "Money and currencies", audit A6). A trade in the main currency has no second
 * line; without a rate for that day the second line says so. A tap or a long press opens the row's menu (Upravit when the row can be
 * edited, Smazat), anchored like the agenda row's.
 */
export function TradeRow<T extends LedgerTx>({
  tx,
  unit,
  quantityDigits = 4,
  currency,
  convertAt,
  ratesReady,
  hot,
  onEdit,
  onDelete,
}: TradeRowProps<T>) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const approx = useApprox();
  const [menuOpen, setMenuOpen] = useState(false);

  const sell = tx.type === 'sell';
  const quantity = parseFloat(tx.quantity) || 0;
  const txCurrency = tx.currency || currency;
  const total = nativeTotal(tx);
  const converted = convertAt(total, txCurrency, currency, tx.transactionDate);
  const sign = sell ? '+ ' : '− ';
  const Glyph = sell ? TrendingDown : TrendingUp;

  return (
    <EntityRow
      id={`tx-${tx.id}`}
      // The tint goes on at once and fades back over 1.2 s when the row stops being hot
      className={cn(
        'transition-colors ease-out motion-reduce:transition-none',
        hot ? 'bg-well duration-0' : '[transition-duration:1200ms]'
      )}
      chip={
        <RowChip tone={sell ? 'gain' : 'default'}>
          <Glyph strokeWidth={1.75} />
        </RowChip>
      }
      // The quantity is the title, a short figure that stays whole beside an exact foreign total
      // (C1); what kind of trade and when are the sub, the word giving way before the day. The
      // price per unit is the trade's form's (and the position's Cena)
      title={t('wealth.position.tx.quantity', {
        quantity: fmt.number(quantity, { maximumFractionDigits: quantityDigits }),
        unit,
      })}
      sub={
        <SubFacts
          lead={t(`wealth.position.tx.${sell ? 'sell' : 'buy'}`)}
          tail={fmt.day(tx.transactionDate)}
        />
      }
      value={
        <Money
          value={total}
          currency={txCurrency}
          options={exactMoneyOptions(total, txCurrency)}
          sign={sign}
        />
      }
      valueTone={sell ? 'gain' : undefined}
      valueSub={approx(total, txCurrency, { converted, pending: !ratesReady })}
      onClick={() => setMenuOpen(true)}
      menu={{
        label: t('wealth.position.tx.menu'),
        // The row's tap opens the menu; the visible `···` says it has one
        shown: true,
        open: menuOpen,
        onOpenChange: setMenuOpen,
        children: (
          <>
            {onEdit && (
              <RowMenuItem onSelect={() => onEdit(tx)}>
                <Pencil strokeWidth={1.75} aria-hidden />
                {tc('buttons.edit')}
              </RowMenuItem>
            )}
            <RowMenuItem variant="danger" onSelect={() => onDelete(tx)}>
              <Trash2 strokeWidth={1.75} aria-hidden />
              {ts('detail.deleteTransaction')}
            </RowMenuItem>
          </>
        ),
      }}
    />
  );
}
