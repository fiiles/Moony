import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ListPlus, Search } from 'lucide-react';
import type { BankTransaction, PlanCandidateFilters, SpendingPlan } from '@shared/schema';
import {
  monthDate,
  monthOfDay,
  periodMonths,
  periodRange,
  signedTotals,
  type PlanPeriod,
} from '@/utils/spending-plans';
import { counterpartyLabel } from '@/utils/transaction-text';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { usePlanCandidates, useSpendingPlanIndex } from '@/hooks/use-spending-plans';
import { useAssignWithUndo } from '@/hooks/use-spending-plan-mutations';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { CategoryIcon } from '@/components/common/CategoryIcon';
import { PlanChip } from '@/components/spending-plans/PlanChip';

const ALL_ACCOUNTS = 'all';

interface AddPaymentsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The target plan; it is active (the detail offers the dialog for any plan, the backend checks). */
  plan: SpendingPlan;
}

/**
 * "Přidat platby do plánu" (spec §9, O16): payments from all accounts, searched by counterparty
 * or category, narrowed by account and period (the plan year by default) and, by default, to
 * payments without a plan. A row click toggles it; the selection survives a change of filters.
 * Payments in another plan move to this one (the footer says how many).
 */
export function AddPaymentsDialog({ open, onOpenChange, plan }: AddPaymentsDialogProps) {
  const { t } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { accounts } = useBankAccounts();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const index = useSpendingPlanIndex();
  const { assign, isPending } = useAssignWithUndo();

  const [query, setQuery] = useState('');
  const [accountId, setAccountId] = useState(ALL_ACCOUNTS);
  const [period, setPeriod] = useState<PlanPeriod>('year');
  const [onlyUnassigned, setOnlyUnassigned] = useState(true);
  // The chosen payments themselves, so the footer still counts one a filter hides now
  const [chosen, setChosen] = useState<Map<string, BankTransaction>>(() => new Map());

  // Opening the dialog starts over
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setQuery('');
      setAccountId(ALL_ACCOUNTS);
      setPeriod('year');
      setOnlyUnassigned(true);
      setChosen(new Map());
    }
  }

  const search = useDebouncedValue(query.trim(), 200);
  const filters = useMemo<PlanCandidateFilters>(
    () => ({
      planId: plan.id,
      ...periodRange(plan.year, period),
      accountId: accountId === ALL_ACCOUNTS ? null : accountId,
      onlyUnassigned,
      query: search || null,
      limit: null,
    }),
    [plan.id, plan.year, period, accountId, onlyUnassigned, search]
  );
  const candidates = usePlanCandidates(filters, open);
  const rows = candidates.data?.transactions ?? [];

  const accountNames = useMemo(() => new Map(accounts.map((a) => [a.id, a.name])), [accounts]);
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const planById = useMemo(
    () => new Map((index.data ?? []).map((s) => [s.plan.id, s.plan])),
    [index.data]
  );
  const months = periodMonths(plan.year, monthOfDay(Date.now() / 1000));

  const toggle = (tx: BankTransaction) =>
    setChosen((prev) => {
      const next = new Map(prev);
      if (next.has(tx.id)) next.delete(tx.id);
      else next.set(tx.id, tx);
      return next;
    });

  const picked = [...chosen.values()];
  const moving = picked.filter((tx) => tx.spendingPlanId !== null).length;
  const totals = signedTotals(picked)
    .map((s) => fmt.money(s.amount, s.currency, { signed: true }))
    .join(' · ');
  const thisYear = new Date().getUTCFullYear();
  const shortDay = (ts: number) =>
    fmt.day(ts, {
      day: 'numeric',
      month: 'numeric',
      ...(new Date(ts * 1000).getUTCFullYear() !== thisYear ? { year: '2-digit' } : {}),
    });

  const submit = async () => {
    if (await assign(picked, plan)) onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[90vh] max-w-[920px] flex-col">
        <DialogHeader>
          <DialogTitle>{t('addPayments.title', { name: plan.name })}</DialogTitle>
          <DialogDescription>{t('addPayments.description')}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-wrap items-center gap-2">
          <InputWrap icon={<Search />} className="w-[220px]">
            <Input
              type="search"
              className="h-[35px] text-table"
              placeholder={t('addPayments.search')}
              aria-label={t('addPayments.search')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </InputWrap>
          <Select value={accountId} onValueChange={setAccountId}>
            <SelectTrigger
              className="h-[35px] w-[200px] text-table"
              aria-label={t('addPayments.account')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL_ACCOUNTS}>{t('addPayments.allAccounts')}</SelectItem>
              {accounts.map((account) => (
                <SelectItem key={account.id} value={account.id}>
                  {account.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={period} onValueChange={setPeriod}>
            <SelectTrigger
              className="h-[35px] w-[170px] text-table"
              aria-label={t('addPayments.period')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="year">
                {t('addPayments.wholeYear', { year: plan.year })}
              </SelectItem>
              {months.map((month) => (
                <SelectItem key={month} value={month}>
                  {fmt.month(monthDate(month), { month: 'long', year: 'numeric' })}
                </SelectItem>
              ))}
              <SelectItem value="all">{t('addPayments.allPeriods')}</SelectItem>
            </SelectContent>
          </Select>
          <div className="ml-auto flex items-center gap-2">
            <Switch
              id="only-unassigned"
              checked={onlyUnassigned}
              onCheckedChange={setOnlyUnassigned}
            />
            <Label htmlFor="only-unassigned" className="text-table font-550 text-ink-2">
              {t('addPayments.onlyUnassigned')}
            </Label>
          </div>
        </div>

        <div className="-mx-[27px] min-h-[240px] flex-1 overflow-y-auto border-y border-line">
          <Table className="table-fixed border-t-0">
            <TableHeader className="sticky top-0 z-[1]">
              <TableRow>
                <TableHead className="w-10 pl-[27px] pr-0">
                  <span className="sr-only">{t('addPayments.select')}</span>
                </TableHead>
                <TableHead className="w-[76px]">{t('addPayments.date')}</TableHead>
                <TableHead className="w-[32%]">{t('addPayments.counterparty')}</TableHead>
                <TableHead>{t('addPayments.category')}</TableHead>
                <TableHead>{t('addPayments.plan')}</TableHead>
                <TableHead className="w-[17%] pr-[27px] text-right">
                  {t('addPayments.amount')}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.length === 0 ? (
                <TableRow className="hover:bg-transparent">
                  <TableCell colSpan={6} className="py-10 text-center text-table text-ink-3">
                    {candidates.isLoading ? '…' : t('addPayments.empty')}
                  </TableCell>
                </TableRow>
              ) : (
                rows.map((tx) => {
                  const on = chosen.has(tx.id);
                  const credit = tx.type === 'credit';
                  const amount = Math.abs(parseFloat(tx.amount) || 0);
                  const other = tx.spendingPlanId ? planById.get(tx.spendingPlanId) : undefined;
                  const category = tx.categoryId ? categoryById.get(tx.categoryId) : undefined;
                  const name = counterpartyLabel(tx).title;
                  const sub = [
                    accountNames.get(tx.bankAccountId),
                    other ? t('addPayments.inPlan', { name: other.name }) : null,
                  ]
                    .filter(Boolean)
                    .join(' · ');
                  return (
                    <TableRow
                      key={tx.id}
                      className="h-12 cursor-pointer"
                      data-state={on ? 'selected' : undefined}
                      onClick={() => toggle(tx)}
                    >
                      <TableCell className="pl-[27px] pr-0" onClick={(e) => e.stopPropagation()}>
                        <Checkbox
                          checked={on}
                          aria-label={t('addPayments.selectRow', { name: name ?? '—' })}
                          onCheckedChange={() => toggle(tx)}
                        />
                      </TableCell>
                      <TableCell className="num">{shortDay(tx.bookingDate)}</TableCell>
                      <TableCell>
                        <b
                          className="block truncate text-table font-650 text-ink"
                          title={name ?? undefined}
                        >
                          {name ?? '—'}
                        </b>
                        {sub && (
                          <small
                            className="block truncate text-micro font-500 text-ink-4"
                            title={sub}
                          >
                            {sub}
                          </small>
                        )}
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
                      <TableCell>
                        {other ? (
                          <span className="flex min-w-0 items-center gap-1.5 text-caption font-550 text-ink-2">
                            <PlanChip plan={other} size="sm" />
                            <span className="truncate">{other.name}</span>
                          </span>
                        ) : (
                          <span className="text-caption text-ink-4">{t('addPayments.noPlan')}</span>
                        )}
                      </TableCell>
                      <TableCell
                        className={cn(
                          'pr-[27px] text-right font-650 num',
                          credit ? 'text-gain' : 'text-ink'
                        )}
                      >
                        {fmt.money(credit ? amount : -amount, tx.currency, { signed: true })}
                      </TableCell>
                    </TableRow>
                  );
                })
              )}
            </TableBody>
          </Table>
        </div>
        <p className="-mt-1 text-micro font-500 text-ink-4">
          {candidates.data?.truncated
            ? t('addPayments.truncated', { count: rows.length })
            : [
                t('addPayments.count', { count: rows.length }),
                onlyUnassigned ? t('addPayments.hidden') : null,
              ]
                .filter(Boolean)
                .join(' · ')}
        </p>

        <DialogFooter className="items-center sm:justify-between">
          <span className="text-micro font-500 text-ink-4">
            {picked.length > 0
              ? [
                  t('selection.count', { count: picked.length }),
                  totals,
                  moving > 0 ? t('addPayments.moving', { count: moving }) : null,
                ]
                  .filter(Boolean)
                  .join(' · ')
              : t('addPayments.nothing')}
          </span>
          <div className="flex gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              type="button"
              disabled={picked.length === 0 || isPending}
              loading={isPending}
              onClick={() => void submit()}
            >
              <ListPlus />
              {picked.length > 0
                ? t('addPayments.submit', { count: picked.length })
                : t('addPayments.submitNone')}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
