import { useState, type ComponentProps } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { List, Pencil, Plus } from 'lucide-react';
import { CategoryRow } from './CategoryRow';
import { RowMenu, RowMenuItem } from '@/mobile/components/RowMenu';

interface ExpenseCategoryRowProps extends Omit<
  ComponentProps<typeof CategoryRow>,
  'onLongPress' | 'accessory'
> {
  /** The category has a budget in the period: its item reads "Upravit limit", else "Nastavit limit". */
  hasLimit: boolean;
  /** Open the limit sheet for the category; `null` while it cannot be (the budget's goal is not loaded yet). */
  onLimit: (() => void) | null;
}

/**
 * An expense category of the Cashflow report with the long press's menu (the desktop Budgets
 * table's row menu, ruling R15): "Zobrazit transakce" (the drilldown the tap opens, off when the
 * row has no payments) and "Upravit limit" / "Nastavit limit". It has no swipes: the report row has
 * no natural action to slide. The `···` trigger is the row's `accessory` for keyboard and screen
 * reader (`RowMenu`).
 */
export function ExpenseCategoryRow({ hasLimit, onLimit, ...rowProps }: ExpenseCategoryRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('budgeting');
  const [, setLocation] = useLocation();
  const [menuOpen, setMenuOpen] = useState(false);
  const { title, href, row } = rowProps;

  return (
    <CategoryRow
      {...rowProps}
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <RowMenu
          open={menuOpen}
          onOpenChange={setMenuOpen}
          label={t('cashflow.rowMenu', { title })}
        >
          <RowMenuItem disabled={row.count === 0} onSelect={() => setLocation(href)}>
            <List aria-hidden strokeWidth={1.75} />
            {tb('actions.showTransactions')}
          </RowMenuItem>
          <RowMenuItem disabled={onLimit === null} onSelect={() => onLimit?.()}>
            {hasLimit ? (
              <Pencil aria-hidden strokeWidth={1.75} />
            ) : (
              <Plus aria-hidden strokeWidth={1.75} />
            )}
            {tb(hasLimit ? 'actions.editLimit' : 'actions.setLimit')}
          </RowMenuItem>
        </RowMenu>
      }
    />
  );
}
