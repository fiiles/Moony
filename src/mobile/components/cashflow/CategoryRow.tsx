import { useTranslation } from 'react-i18next';
import type { TransactionCategory } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { EntityRow } from '@/mobile/components/EntityRow';
import type { CategoryRowModel } from '@/mobile/lib/cashflow-rows';
import { CategoryChip } from './CategoryChip';
import { ProgressBar } from './ProgressBar';

interface CategoryRowProps {
  row: CategoryRowModel;
  /** The translated name ("Výplata", "Nezařazeno"). */
  title: string;
  /** The category's icon and hue; `null` for the uncategorized group. */
  category: TransactionCategory | null;
  /** The row's amount, already worded in the report currency (signed for transfers). */
  value: string;
  /** The category's transactions in the period. */
  href: string;
  /** The share bar under the sub-line (income); transfers have none. */
  withBar: boolean;
}

/**
 * An income or transfer category of the Cashflow report as an entity row (spec §8.7 items 4–5,
 * README §13): the category chip, the name, the amount, and a sub-line that puts the share first
 * and the count last, so an ellipsis drops the count: "47 % příjmů · 3 transakce" (transfers only
 * count). Income has its share of the movement as a bar. A tap opens the transactions. The expense
 * rows, with their limits, are `ExpenseCategoryRow`.
 */
export function CategoryRow({ row, title, category, value, href, withBar }: CategoryRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const fmt = useFormat();
  const share = fmt.percent(row.share, 0);
  const count = tr('actuals.transactions', { count: row.count });
  const sub =
    row.flowKind === 'transfer'
      ? count
      : `${
          row.flowKind === 'income'
            ? t('cashflow.shareOfIncome', { share })
            : t('cashflow.shareOfExpenses', { share })
        } · ${count}`;

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={title}
      sub={sub}
      value={value}
      bar={withBar ? <ProgressBar value={row.share} tone="share" /> : undefined}
      href={href}
    />
  );
}
