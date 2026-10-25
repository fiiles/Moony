import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Landmark, Tag } from 'lucide-react';
import type { MoneyOptions } from '@/lib/format';
import { RowMenu, RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { Money } from '@/mobile/components/Money';
import { initials } from '@/mobile/lib/initials';
import type { DrilldownRow as DrilldownRowData } from '@/mobile/lib/cashflow-drilldown';

/** An amount with its sign ("− 250,00 Kč", "+ 32 000 Kč"). */
const SIGNED: MoneyOptions = { signed: true };

interface DrilldownRowProps {
  row: DrilldownRowData;
  /** Open the payment's sheet (a tap). */
  onOpen: (row: DrilldownRowData) => void;
  /** Open the category picker for this payment (a swipe to the left, the menu). */
  onCategorize: (row: DrilldownRowData) => void;
}

/**
 * One payment as an entity row: initials chip (green for a credit), who and what, the amount in
 * its own currency with its worth in the report currency under it. A moved row is dimmed and says
 * so. A tap opens the transaction sheet; a swipe to the left and the long press's menu open the
 * category picker.
 */
export function DrilldownTransactionRow({ row, onOpen, onCategorize }: DrilldownRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const [menuOpen, setMenuOpen] = useState(false);
  const { tx, credit } = row;
  // The marker leads: the sub-line ends in an ellipsis when it is too long, and "přesunuto" is
  // the one sign of a moved row that is not its dimming
  const sub = (row.moved ? [tr('readiness.moved'), ...row.sub] : row.sub).join(' · ');
  // The converted amount carries the row's own sign (`reportAmount`), not the report line's
  const valueSub =
    row.reportAmount === null ? (
      tr('readiness.fxUnavailable')
    ) : tx.currency !== tx.reportCurrency ? (
      <Money value={row.reportAmount} currency={tx.reportCurrency} options={SIGNED} />
    ) : undefined;

  return (
    <EntityRow
      chip={
        <RowChip tone={credit ? 'gain' : 'default'}>
          {initials(row.title) || <Landmark strokeWidth={1.75} />}
        </RowChip>
      }
      title={row.title}
      sub={sub}
      value={
        <Money
          value={Number(tx.amount) * (credit ? 1 : -1)}
          currency={tx.currency}
          options={SIGNED}
        />
      }
      valueTone={credit ? 'gain' : undefined}
      valueSub={valueSub}
      muted={row.moved}
      onClick={() => onOpen(row)}
      onSwipeLeft={() => onCategorize(row)}
      swipeLeftLabel={t('upkeep.actions.categorize')}
      swipeLeftIcon={Tag}
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <RowMenu
          open={menuOpen}
          onOpenChange={setMenuOpen}
          label={t('cashflow.rowMenu', { title: row.title })}
        >
          <RowMenuItem onSelect={() => onCategorize(row)}>
            <Tag aria-hidden strokeWidth={1.75} />
            {t('cashflow.picker.change')}
          </RowMenuItem>
        </RowMenu>
      }
    />
  );
}

/** While the list loads for the first time: three rows in outline. */
export function DrilldownSkeleton() {
  return (
    <div aria-hidden>
      <EntityRows>
        {[0, 1, 2].map((i) => (
          <li
            key={i}
            className="flex min-h-[60px] list-none items-center gap-3 border-t border-line-soft px-3.5 first:border-t-0"
          >
            <div className="skeleton size-9 flex-none rounded-full" />
            <div className="min-w-0 flex-1">
              <div className="skeleton h-3.5 w-2/5" />
              <div className="skeleton mt-2 h-3 w-3/5" />
            </div>
          </li>
        ))}
      </EntityRows>
    </div>
  );
}
