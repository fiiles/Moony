import { useTranslation } from 'react-i18next';
import type { ReactNode } from 'react';
import type { TransactionCategory } from '@shared/schema';
import type { MoneyOptions } from '@/lib/format';
import { useFormat } from '@/lib/use-format';
import { EntityRow } from '@/mobile/components/EntityRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { CategoryRowModel } from '@/mobile/lib/cashflow-rows';
import { CategoryChip } from './CategoryChip';
import { ProgressBar } from './ProgressBar';

/** Whole amounts ("3 000 Kč"). */
const WHOLE: MoneyOptions = { decimals: 0 };

interface CategoryRowProps {
  row: CategoryRowModel;
  /** The translated name ("Potraviny", "Bez kategorie"). */
  title: string;
  /** The category's icon and hue; `null` for the uncategorized group. */
  category: TransactionCategory | null;
  /** The currency of the row's amount: the report's. */
  reportCurrency: string;
  /** The category's transactions in the period. */
  href: string;
  /** The bar under the sub-line (expenses and income); transfers have none. */
  withBar: boolean;
  onLongPress?: () => void;
  /** Beside the row, outside the part that slides (the long press's menu). */
  accessory?: ReactNode;
}

/**
 * A category of the Cashflow report as an entity row (spec §8.7 items 4–5, README §13): the
 * category chip, the name, the amount, and the sub-line "3 pohyby · 24 % výdajů". A category with
 * a budget limit shows the limit instead of the share ("3 pohyby · limit 3 000 Kč", then "blíží
 * se" or "překročeno o 250 Kč"; the amount turns `loss` when over) and its bar is the limit's
 * progress; without one the bar is the category's share of the movement. The limit's amounts are
 * in the budget's own currency, never compared with the report amount. A tap opens the
 * transactions.
 */
export function CategoryRow({
  row,
  title,
  category,
  reportCurrency,
  href,
  withBar,
  onLongPress,
  accessory,
}: CategoryRowProps) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { limit } = row;

  const sub: string[] = [t('cashflow.moves', { count: row.count })];
  if (limit) {
    sub.push(t('cashflow.limit', { amount: moneyText(limit.limit, limit.currency, WHOLE) }));
    if (limit.state === 'near') sub.push(t('cashflow.near'));
    if (limit.state === 'over') {
      sub.push(
        t('cashflow.over', { amount: moneyText(limit.spent - limit.limit, limit.currency, WHOLE) })
      );
    }
  } else if (row.flowKind !== 'transfer') {
    const share = fmt.percent(row.share, 0);
    sub.push(
      row.flowKind === 'income'
        ? t('cashflow.shareOfIncome', { share })
        : t('cashflow.shareOfExpenses', { share })
    );
  }

  const over = limit?.state === 'over';
  const bar = !withBar ? undefined : limit ? (
    <ProgressBar value={limit.percentage / 100} tone={over ? 'over' : 'limit'} />
  ) : (
    <ProgressBar value={row.share} tone="share" />
  );

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={title}
      sub={sub.join(' · ')}
      value={moneyText(row.amount, reportCurrency, WHOLE)}
      valueTone={over ? 'loss' : undefined}
      bar={bar}
      href={href}
      onLongPress={onLongPress}
      accessory={accessory}
    />
  );
}
