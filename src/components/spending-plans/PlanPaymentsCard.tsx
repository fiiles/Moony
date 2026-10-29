import { Fragment, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FolderInput, ListPlus, Receipt, X } from 'lucide-react';
import type { BankTransaction, TransactionCategory } from '@shared/schema';
import { signedTotals } from '@/utils/spending-plans';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';
import { utcDayFloor } from '@/utils/chart-axis';
import { useCategoryName } from '@/hooks/use-categories';
import { useAssignWithUndo } from '@/hooks/use-spending-plan-mutations';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { CategoryIcon } from '@/components/common/CategoryIcon';
import { EmptyState } from '@/components/common/EmptyState';
import { FilterBadge } from '@/components/common/FilterBadge';
import { LedgerCounterpartyCell } from '@/components/bank-accounts/LedgerCounterpartyCell';
import { PlanPicker } from '@/components/spending-plans/PlanPicker';

const DAY = 86400;

interface PlanPaymentsCardProps {
  planId: string;
  planYear: number;
  /** A closed plan takes no new payments: "Přidat platby" waits for it to be reopened. */
  closed: boolean;
  /** Every payment of the plan, newest first. */
  transactions: BankTransaction[];
  /** The payments the filters keep, newest first. */
  shown: BankTransaction[];
  accountName: (accountId: string) => string;
  categoryById: Map<string, TransactionCategory>;
  /** What the filters keep ("červenec", "Ubytování"); empty without a filter. */
  filterLabels: string[];
  onClearFilters: () => void;
  onAddPayments: () => void;
}

/**
 * "Platby" (spec §9, O15): the plan's payments in the account ledger's look — grouped by day,
 * the counterparty with its round icon and "Účet · detail" under it, the category as an ink
 * icon and name, the native signed amount (credits in `gain`). Checkboxes select payments for
 * "Přesunout do…" (another plan) or "Odebrat z plánu".
 */
export function PlanPaymentsCard({
  planId,
  planYear,
  closed,
  transactions,
  shown,
  accountName,
  categoryById,
  filterLabels,
  onClearFilters,
  onAddPayments,
}: PlanPaymentsCardProps) {
  const { t } = useTranslation('spending_plans');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const { assign, isPending } = useAssignWithUndo();
  const [selected, setSelected] = useState<Set<string>>(() => new Set());

  // Only what the filters show can be selected; a hidden row is never moved by surprise
  const chosen = useMemo(() => shown.filter((tx) => selected.has(tx.id)), [shown, selected]);
  const allOn = shown.length > 0 && chosen.length === shown.length;
  const filtered = filterLabels.length > 0;

  const toggle = (id: string, on: boolean) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });
  const clearSelection = () => setSelected(new Set());

  const today = utcDayFloor(Date.now() / 1000);
  const thisYear = new Date().getUTCFullYear();
  const dayLabel = (ts: number) => {
    const day = utcDayFloor(ts);
    if (day === today) return tc('time.today');
    if (day === today - DAY) return tc('time.yesterday');
    return fmt.day(
      ts,
      new Date(ts * 1000).getUTCFullYear() === thisYear
        ? { day: 'numeric', month: 'short' }
        : { day: 'numeric', month: 'short', year: 'numeric' }
    );
  };
  const totals = signedTotals(chosen)
    .map((s) => fmt.money(s.amount, s.currency, { signed: true }))
    .join(' · ');

  let lastGroup = '';

  return (
    <Card variant="table" className={cn(filtered && 'border-line-hover')}>
      <CardHeader>
        <div className="min-w-0">
          <CardTitle className="flex flex-wrap items-center gap-2">
            {t('detail.payments.title')}
            <FilterBadge
              labels={filterLabels}
              onClear={onClearFilters}
              title={t('detail.filterTitle')}
            />
          </CardTitle>
          {transactions.length > 0 && (
            <CardDescription>
              {filtered
                ? t('detail.payments.filtered', {
                    shown: shown.length,
                    count: transactions.length,
                  })
                : t('detail.payments.sub')}
            </CardDescription>
          )}
        </div>
        {transactions.length > 0 && (
          // A disabled button shows no hover, so the hint sits on its wrapper
          <span title={closed ? t('detail.payments.closedHint') : undefined}>
            <Button variant="outline" size="sm" disabled={closed} onClick={onAddPayments}>
              <ListPlus />
              {t('actions.addPayments')}
            </Button>
          </span>
        )}
      </CardHeader>

      {transactions.length === 0 ? (
        <EmptyState
          bare
          icon={<Receipt />}
          title={t('detail.payments.emptyTitle')}
          description={t('detail.payments.emptyText')}
          action={
            <span title={closed ? t('detail.payments.closedHint') : undefined}>
              <Button size="sm" disabled={closed} onClick={onAddPayments}>
                <ListPlus />
                {t('actions.addPayments')}
              </Button>
            </span>
          }
        />
      ) : (
        <>
          {chosen.length > 0 && (
            <div className="flex min-h-[52px] items-center gap-2 border-t border-line bg-well px-[14px] py-2 text-table">
              <b className="text-ink">{t('selection.count', { count: chosen.length })}</b>
              <span className="mr-auto truncate text-ink-3 num">· {totals}</span>
              <PlanPicker
                trigger={
                  <Button variant="outline" size="sm" disabled={isPending}>
                    <FolderInput />
                    {t('actions.moveTo')}
                  </Button>
                }
                transactionIds={chosen.map((tx) => tx.id)}
                currentPlanIds={chosen.map((tx) => tx.spendingPlanId)}
                excludePlanId={planId}
                defaultYear={planYear}
                align="end"
                onAssigned={clearSelection}
              />
              <Button
                variant="outline"
                size="sm"
                disabled={isPending}
                onClick={() =>
                  void assign(chosen, null).then((ok) => {
                    if (ok) clearSelection();
                  })
                }
              >
                <X />
                {t('actions.removeFromPlan')}
              </Button>
              <Button variant="ghost" size="sm" onClick={clearSelection}>
                {t('actions.clearSelection')}
              </Button>
            </div>
          )}
          <Table className="table-fixed">
            <TableHeader>
              <TableRow>
                <TableHead className="w-10 pr-0">
                  <Checkbox
                    checked={allOn ? true : chosen.length > 0 ? 'indeterminate' : false}
                    disabled={shown.length === 0}
                    aria-label={t('detail.payments.selectAll')}
                    onCheckedChange={(v) =>
                      setSelected(v === true ? new Set(shown.map((tx) => tx.id)) : new Set())
                    }
                  />
                </TableHead>
                <TableHead className="w-[52%]">{t('detail.payments.counterparty')}</TableHead>
                <TableHead>{t('detail.payments.category')}</TableHead>
                <TableHead className="w-[24%] text-right">{t('detail.payments.amount')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {shown.length === 0 ? (
                <TableRow className="hover:bg-transparent">
                  <TableCell colSpan={4} className="py-10 text-center text-table text-ink-3">
                    {t('detail.payments.emptyFiltered')}
                  </TableCell>
                </TableRow>
              ) : (
                shown.map((tx) => {
                  const group = dayLabel(tx.bookingDate);
                  const showGroup = group !== lastGroup;
                  lastGroup = group;
                  const credit = tx.type === 'credit';
                  const amount = Math.abs(parseFloat(tx.amount) || 0);
                  const counterparty = counterpartyLabel(tx);
                  const sub = [accountName(tx.bankAccountId), ...transactionDetails(tx)].join(
                    ' · '
                  );
                  const category = tx.categoryId ? categoryById.get(tx.categoryId) : undefined;
                  const on = selected.has(tx.id);
                  return (
                    <Fragment key={tx.id}>
                      {showGroup && (
                        <TableRow className="h-auto hover:bg-transparent">
                          <TableCell
                            colSpan={4}
                            className="bg-paper px-[14px] pb-1.5 pt-[9px] text-micro font-700 uppercase tracking-[0.06em] text-ink-4"
                          >
                            {group}
                          </TableCell>
                        </TableRow>
                      )}
                      <TableRow className="h-[52px]" data-state={on ? 'selected' : undefined}>
                        <TableCell className="pr-0">
                          <Checkbox
                            checked={on}
                            aria-label={t('detail.payments.select', {
                              name: counterparty.title ?? '—',
                            })}
                            onCheckedChange={(v) => toggle(tx.id, v === true)}
                          />
                        </TableCell>
                        <TableCell>
                          <LedgerCounterpartyCell
                            credit={credit}
                            title={counterparty.title}
                            subtitle={sub}
                          >
                            {tx.status === 'pending' && (
                              <Badge className="shrink-0">{tb('readiness.pending')}</Badge>
                            )}
                          </LedgerCounterpartyCell>
                        </TableCell>
                        <TableCell>
                          {category ? (
                            <span className="flex min-w-0 items-center gap-1.5 text-caption font-550 text-ink-2">
                              <CategoryIcon
                                iconName={category.icon || 'tag'}
                                className="size-3.5 shrink-0"
                              />
                              <span className="truncate">{categoryName(category)}</span>
                            </span>
                          ) : (
                            <span className="text-caption text-ink-4">
                              {t('detail.categories.uncategorized')}
                            </span>
                          )}
                        </TableCell>
                        <TableCell
                          className={cn(
                            'text-right font-650 num',
                            credit ? 'text-gain' : 'text-ink'
                          )}
                        >
                          {fmt.money(credit ? amount : -amount, tx.currency, { signed: true })}
                        </TableCell>
                      </TableRow>
                    </Fragment>
                  );
                })
              )}
            </TableBody>
          </Table>
        </>
      )}
    </Card>
  );
}
