import { useTranslation } from 'react-i18next';
import { Info, Tag } from 'lucide-react';
import { useCategories } from '@/hooks/use-categories';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { CategoryChip } from './CategoryChip';
import { useApprox } from '@/mobile/hooks/use-approx';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { signedAmountText } from '@/mobile/lib/signed-amount';
import type { DrilldownRow as DrilldownRowData } from '@/mobile/lib/cashflow-drilldown';

interface DrilldownRowProps {
  row: DrilldownRowData;
  /** Open the payment's sheet (a tap, a swipe to the right, the menu). */
  onOpen: (row: DrilldownRowData) => void;
  /** Open the category picker for this payment (a swipe to the left, the menu). */
  onCategorize: (row: DrilldownRowData) => void;
}

/**
 * One payment as an entity row, as every payment row (README §13, "Transactions"): the chip of the
 * category it has now (a change made here shows at once), its title, the exact amount
 * in its own currency ("− 129,90 Kč", as every list of payments writes it) and, when that is not
 * the report currency, "≈" its worth in it (README §13, "Money and currencies"; "bez kurzu pro
 * EUR" when it could not be converted). A moved row is dimmed and says
 * so. As every payment (README §13, Gestures): a tap and a swipe to the right open the transaction
 * sheet, a swipe to the left the category picker; the long press's menu has both.
 */
export function DrilldownTransactionRow({ row, onOpen, onCategorize }: DrilldownRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const moneyText = useMoneyText();
  const approx = useApprox();
  const { categories } = useCategories();
  const { tx, credit } = row;
  const category = categories.find((candidate) => candidate.id === row.categoryId);
  // The marker leads: the sub-line ends in an ellipsis when it is too long, and "přesunuto" is
  // the one sign of a moved row that is not its dimming
  const sub = (row.moved ? [tr('readiness.moved'), ...row.sub] : row.sub).join(' · ');
  // The report's own conversion, at the payment's day; the sign is on the amount above it
  const valueSub = approx(Number(tx.amount) || 0, tx.currency, {
    converted: row.reportAmount,
    pending: false,
  });

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={row.title}
      sub={sub}
      value={signedAmountText(moneyText, {
        type: credit ? 'credit' : 'debit',
        amount: tx.amount,
        currency: tx.currency,
      })}
      valueTone={credit ? 'gain' : undefined}
      valueSub={valueSub}
      muted={row.moved}
      onClick={() => onOpen(row)}
      onSwipeLeft={() => onCategorize(row)}
      swipeLeftLabel={t('account.ledger.categorize')}
      swipeLeftIcon={Tag}
      onSwipeRight={() => onOpen(row)}
      swipeRightLabel={t('account.ledger.details')}
      swipeRightIcon={Info}
      menu={{
        label: t('cashflow.rowMenu', { title: row.title }),
        children: (
          <>
            <RowMenuItem onSelect={() => onOpen(row)}>
              <Info aria-hidden strokeWidth={1.75} />
              {t('account.rowMenu.details')}
            </RowMenuItem>
            <RowMenuItem onSelect={() => onCategorize(row)}>
              <Tag aria-hidden strokeWidth={1.75} />
              {t('account.rowMenu.category')}
            </RowMenuItem>
          </>
        ),
      }}
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
