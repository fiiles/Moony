import { Fragment, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
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
 * category chip, the name, the amount, and a sub-line that puts what matters first and the count
 * last, so an ellipsis drops the count: "47 % výdajů · 3 pohyby", or for a category with a budget
 * limit its progress, "3 206 / 3 000 Kč" (red when over), then "blíží se limitu" or "nad
 * limitem", then the count. The amount turns `loss` when over and the bar is the limit's
 * progress; without a limit the bar is the category's share of the movement. The limit's amounts
 * are in the budget's own currency, never compared with the report amount. A tap opens the
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
  const over = limit?.state === 'over';

  const parts: ReactNode[] = [];
  if (limit) {
    // The spent number is bare (the limit carries the currency)
    const spent = fmt.number(limit.spent, { maximumFractionDigits: 0 });
    const progress = t('cashflow.limitProgress', {
      spent,
      limit: moneyText(limit.limit, limit.currency, WHOLE),
    });
    parts.push(over ? <span className="text-loss">{progress}</span> : progress);
    if (limit.state === 'near') parts.push(t('cashflow.near'));
    if (over) parts.push(t('cashflow.overLimit'));
  } else if (row.flowKind !== 'transfer') {
    const share = fmt.percent(row.share, 0);
    parts.push(
      row.flowKind === 'income'
        ? t('cashflow.shareOfIncome', { share })
        : t('cashflow.shareOfExpenses', { share })
    );
  }
  parts.push(t('cashflow.moves', { count: row.count }));
  const sub = parts.map((part, index) => (
    <Fragment key={index}>
      {index > 0 && ' · '}
      {part}
    </Fragment>
  ));

  const bar = !withBar ? undefined : limit ? (
    <ProgressBar value={limit.percentage / 100} tone={over ? 'over' : 'limit'} />
  ) : (
    <ProgressBar value={row.share} tone="share" />
  );

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={title}
      sub={sub}
      value={moneyText(row.amount, reportCurrency, WHOLE)}
      valueTone={over ? 'loss' : undefined}
      bar={bar}
      href={href}
      onLongPress={onLongPress}
      accessory={accessory}
    />
  );
}
