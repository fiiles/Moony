import { useMemo, useState, type ReactNode } from 'react';
import { useLocation } from 'wouter';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Ellipsis,
  Landmark,
  Pencil,
  Plus,
  Search,
  Trash2,
  Upload,
} from 'lucide-react';
import type { BankAccountWithInstitution, InsertBankAccount } from '@shared/schema';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { useBankAccountActivity } from '@/hooks/use-bank-account-activity';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { utcDayFloor } from '@/utils/chart-axis';
import { PageHead } from '@/components/shell/PageHead';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Card, CardHeader, CardTitle } from '@/components/ui/card';
import { Input, InputWrap } from '@/components/ui/input';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { BankAccountFormDialog } from '@/components/bank-accounts/BankAccountFormDialog';
import { NativeBalance } from '@/components/bank-accounts/NativeBalance';
import { cn } from '@/lib/utils';

type SortColumn = 'name' | 'balance' | 'change' | 'last';

const SERIES = [
  'bg-s1 text-ink-inverse',
  'bg-s2 text-ink-inverse',
  'bg-s3 text-ink-inverse',
  'bg-s4 text-ink',
];

function initials(account: BankAccountWithInstitution): string {
  const source = account.institution?.name || account.name;
  const words = source.split(/\s+/).filter(Boolean);
  return (words.length > 1 ? words[0][0] + words[1][0] : source.slice(0, 2)).toUpperCase();
}

/**
 * Bank accounts list (design system §7 List, prototype bank-accounts.html):
 * three stats — total on accounts, 30-day change, uncategorized transactions —
 * and the E3 table with institution · type, currency, balance (native as a
 * sub-line), 30-day change and the latest movement per account.
 */
export default function BankAccounts() {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const { accounts, isLoading } = useBankAccounts();
  const { createAccount, updateAccount, deleteAccount } = useBankAccountMutations();
  const { formatCurrencyRaw, convert, currencyCode } = useCurrency();
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const activity = useBankAccountActivity(accounts);

  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [editing, setEditing] = useState<BankAccountWithInstitution | null>(null);
  const [pendingDelete, setPendingDelete] = useState<BankAccountWithInstitution | null>(null);
  const [search, setSearch] = useState('');
  const [sortColumn, setSortColumn] = useState<SortColumn>('balance');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const toDisplay = (amount: number, currency: string) =>
    convert(amount, (currency || 'CZK') as CurrencyCode, currencyCode);

  // Totals and the 30-day money flow across the accounts counted in net worth,
  // in the display currency. Excluded accounts stay out of them like they do on
  // the dashboard (so the total is its cash) and are reported apart.
  const totals = useMemo(() => {
    let balance = 0;
    let excluded = 0;
    let income = 0;
    let expense = 0;
    let uncategorized = 0;
    let latest: number | null = null;
    const currencies = new Set<string>();
    for (const account of accounts) {
      currencies.add(account.currency || 'CZK');
      const accountBalance = toDisplay(parseFloat(account.balance || '0'), account.currency);
      const a = activity.get(account.id);
      // Transactions to categorize and the newest movement concern every account
      if (a) {
        uncategorized += a.last30.uncategorized;
        if (a.last && (latest === null || a.last.bookingDate > latest)) {
          latest = a.last.bookingDate;
        }
      }
      if (account.excludeFromBalance) {
        excluded += accountBalance;
        continue;
      }
      balance += accountBalance;
      if (a) {
        income += toDisplay(a.last30.income, account.currency);
        expense += toDisplay(a.last30.expense, account.currency);
      }
    }
    return {
      balance,
      excluded,
      income,
      expense,
      net: income - expense,
      uncategorized,
      currencies,
      latest,
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts, activity, currencyCode]);

  useShellPage({
    status:
      totals.latest !== null
        ? { text: t('status.lastMovement', { date: fmt.day(totals.latest) }), tone: 'neutral' }
        : undefined,
  });

  const rows = useMemo(() => {
    const term = search.trim().toLowerCase();
    const filtered = term
      ? accounts.filter(
          (a) =>
            a.name.toLowerCase().includes(term) ||
            (a.institution?.name ?? '').toLowerCase().includes(term)
        )
      : accounts;
    const change = (a: BankAccountWithInstitution) =>
      toDisplay(activity.get(a.id)?.last30.net ?? 0, a.currency);
    return [...filtered].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
          break;
        case 'balance':
          c =
            convertToCzK(parseFloat(a.balance), (a.currency || 'CZK') as CurrencyCode) -
            convertToCzK(parseFloat(b.balance), (b.currency || 'CZK') as CurrencyCode);
          break;
        case 'change':
          c = change(a) - change(b);
          break;
        case 'last':
          c =
            (activity.get(a.id)?.last?.bookingDate ?? 0) -
            (activity.get(b.id)?.last?.bookingDate ?? 0);
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts, activity, search, sortColumn, sortDirection, currencyCode]);

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      setSortDirection(column === 'name' ? 'asc' : 'desc');
    }
  };

  const SortHead = ({
    column,
    children,
    align = 'right',
    className,
  }: {
    column: SortColumn;
    children: ReactNode;
    align?: 'left' | 'right';
    className?: string;
  }) => {
    const active = sortColumn === column;
    const Icon = !active ? ArrowUpDown : sortDirection === 'asc' ? ArrowUp : ArrowDown;
    return (
      <TableHead className={cn(align === 'right' && 'text-right', className)}>
        <button
          type="button"
          onClick={() => handleSort(column)}
          className={cn(
            'inline-flex items-center gap-1 rounded-r1 text-thead uppercase transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus',
            active && 'text-ink'
          )}
        >
          {children}
          <Icon className={cn('size-3', !active && 'opacity-60')} aria-hidden />
        </button>
      </TableHead>
    );
  };

  const today = utcDayFloor(Date.now() / 1000);
  const dayLabel = (ts: number) => {
    const day = utcDayFloor(ts);
    if (day === today) return tc('time.today');
    if (day === today - 86400) return tc('time.yesterday');
    return fmt.day(ts, { day: 'numeric', month: 'short' });
  };

  const mostUncategorized = accounts.reduce<BankAccountWithInstitution | null>((best, a) => {
    const n = activity.get(a.id)?.last30.uncategorized ?? 0;
    return !best || n > (activity.get(best.id)?.last30.uncategorized ?? 0) ? a : best;
  }, null);

  const foreign = [...totals.currencies].filter((c) => c !== currencyCode);
  const excludedNote =
    totals.excluded !== 0
      ? t('metrics.excludedHint', { amount: formatCurrencyRaw(totals.excluded) })
      : null;
  const isEmpty = !isLoading && accounts.length === 0;

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          !isEmpty && (
            <>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="outline" disabled={accounts.length === 0}>
                    <Upload />
                    {t('csvImport.importCsv')}
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  {accounts.map((a) => (
                    <DropdownMenuItem
                      key={a.id}
                      onSelect={() => setLocation(`/bank-accounts/${a.id}?import=1`)}
                    >
                      <Landmark />
                      {a.name}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
              <Button onClick={() => setAddDialogOpen(true)}>
                <Plus />
                {t('addAccount')}
              </Button>
            </>
          )
        }
      />

      {isLoading ? (
        <Stats columns={3}>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      ) : isEmpty ? (
        <EmptyState
          icon={<Landmark />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <Button onClick={() => setAddDialogOpen(true)}>
              <Plus />
              {t('addAccount')}
            </Button>
          }
        />
      ) : (
        <>
          <Stats columns={3}>
            <Stat
              label={t('metrics.totalOnAccounts')}
              value={formatCurrencyRaw(totals.balance)}
              note={[
                `${t('metrics.accounts', { count: accounts.length })} · ${t('metrics.currencies', { count: totals.currencies.size })}`,
                excludedNote,
              ]
                .filter(Boolean)
                .join(' ')}
            />
            <Stat
              label={t('metrics.change30')}
              value={fmt.money(totals.net, currencyCode, { signed: true, decimals: 0 })}
              tone={totals.net > 0 ? 'gain' : totals.net < 0 ? 'loss' : 'neutral'}
              noteTone="neutral"
              note={
                totals.income === 0 && totals.expense === 0
                  ? t('metrics.noMovements30')
                  : totals.net < 0 && totals.income > 0
                    ? t('metrics.expensesOverIncome', {
                        percent: fmt.percent(totals.expense / totals.income - 1, 0),
                      })
                    : totals.net > 0 && totals.expense > 0
                      ? t('metrics.incomeOverExpenses', {
                          percent: fmt.percent(totals.income / totals.expense - 1, 0),
                        })
                      : t('metrics.balanced')
              }
            />
            <Stat
              label={t('metrics.uncategorized30')}
              value={fmt.number(totals.uncategorized)}
              note={
                totals.uncategorized > 0 && mostUncategorized ? (
                  <button
                    type="button"
                    onClick={() =>
                      setLocation(`/bank-accounts/${mostUncategorized.id}?category=uncategorized`)
                    }
                    className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
                  >
                    {t('metrics.categorize')} →
                  </button>
                ) : (
                  t('metrics.allCategorized')
                )
              }
            />
          </Stats>

          <Card variant="table">
            <CardHeader>
              <CardTitle>{t('table.title')}</CardTitle>
              <InputWrap icon={<Search />} className="w-60">
                <Input
                  className="h-[35px] text-table"
                  placeholder={t('table.search')}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
              </InputWrap>
            </CardHeader>
            <Table>
              <TableHeader>
                <TableRow>
                  <SortHead column="name" align="left" className="w-[34%]">
                    {t('table.account')}
                  </SortHead>
                  <TableHead>{t('fields.currency')}</TableHead>
                  <SortHead column="balance">{t('fields.balance')}</SortHead>
                  <SortHead column="change">{t('table.change30')}</SortHead>
                  <SortHead column="last">{t('table.lastMovement')}</SortHead>
                  <TableHead className="w-10">
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.length === 0 ? (
                  <TableRow className="hover:bg-transparent">
                    <TableCell colSpan={6} className="py-12">
                      <div className="grid place-items-center text-center text-ink-3">
                        <Search className="mb-3 size-7 text-ink-5" aria-hidden />
                        <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults')}</h3>
                      </div>
                    </TableCell>
                  </TableRow>
                ) : (
                  rows.map((account, index) => {
                    const a = activity.get(account.id);
                    const change = a ? a.last30.net : 0;
                    const rate =
                      account.effectiveInterestRate ??
                      (account.interestRate ? parseFloat(account.interestRate) : 0);
                    const sub = [
                      account.institution?.name,
                      t(`accountTypes.${account.accountType}`),
                      rate > 0 ? fmt.percent(rate / 100, 2) : null,
                    ]
                      .filter(Boolean)
                      .join(' · ');
                    return (
                      <TableRow
                        key={account.id}
                        className="h-[62px] cursor-pointer"
                        onClick={() => setLocation(`/bank-accounts/${account.id}`)}
                        tabIndex={0}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') setLocation(`/bank-accounts/${account.id}`);
                        }}
                      >
                        <TableCell>
                          <div className="flex min-w-0 items-center gap-[10px] text-ink">
                            <span
                              aria-hidden
                              className={cn(
                                'grid size-[30px] shrink-0 place-items-center rounded-full text-micro font-750 shadow-[inset_0_1px_rgba(255,255,255,0.13)]',
                                SERIES[index % SERIES.length]
                              )}
                            >
                              {initials(account)}
                            </span>
                            <div className="min-w-0">
                              <b className="block truncate text-table font-650">
                                {account.name}
                                {account.excludeFromBalance && (
                                  <Badge
                                    variant="outline"
                                    className="ml-2 align-middle"
                                    title={t('detail.includedInPortfolioHint')}
                                  >
                                    {t('table.excluded')}
                                  </Badge>
                                )}
                              </b>
                              <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                                {sub}
                              </small>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell>{account.currency || 'CZK'}</TableCell>
                        <TableCell className="text-right font-650 text-ink num">
                          <NativeBalance
                            amount={parseFloat(account.balance || '0')}
                            currency={account.currency || 'CZK'}
                            secondaryClassName="text-micro font-500 text-ink-4"
                          />
                        </TableCell>
                        <TableCell
                          className={cn(
                            'text-right num',
                            change > 0 && 'text-gain',
                            change < 0 && 'text-loss',
                            change === 0 && 'text-ink-4'
                          )}
                        >
                          {a?.isLoading ? (
                            <span className="skeleton inline-block h-3 w-16" />
                          ) : change === 0 ? (
                            t('table.noChange')
                          ) : (
                            fmt.money(change, account.currency || 'CZK', {
                              signed: true,
                              decimals: 0,
                            })
                          )}
                        </TableCell>
                        <TableCell className="text-right num">
                          {a?.last ? (
                            <>
                              <span>{dayLabel(a.last.bookingDate)}</span>
                              <small className="mt-0.5 block truncate text-micro font-500 text-ink-4">
                                {(
                                  a.last.counterpartyName ||
                                  a.last.description ||
                                  t('table.movement')
                                ).slice(0, 28)}
                                {' · '}
                                {fmt.money(
                                  (a.last.type === 'credit' ? 1 : -1) *
                                    Math.abs(Number(a.last.amount) || 0),
                                  a.last.currency || account.currency || 'CZK',
                                  { signed: true, decimals: 0 }
                                )}
                              </small>
                            </>
                          ) : (
                            <span className="text-ink-4">—</span>
                          )}
                        </TableCell>
                        <TableCell className="text-right">
                          <div data-row-actions onClick={(e) => e.stopPropagation()}>
                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  aria-label={tc('labels.actions')}
                                >
                                  <Ellipsis />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                <DropdownMenuItem onSelect={() => setEditing(account)}>
                                  <Pencil />
                                  {t('editAccount')}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  onSelect={() =>
                                    setLocation(`/bank-accounts/${account.id}?import=1`)
                                  }
                                >
                                  <Upload />
                                  {t('csvImport.importCsv')}
                                </DropdownMenuItem>
                                <DropdownMenuSeparator />
                                <DropdownMenuItem
                                  variant="danger"
                                  onSelect={() => setPendingDelete(account)}
                                >
                                  <Trash2 />
                                  {t('deleteAccount')}
                                </DropdownMenuItem>
                              </DropdownMenuContent>
                            </DropdownMenu>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
            <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
              <span>
                {t('metrics.accounts', { count: rows.length })} · {t('table.total')}{' '}
                <span className="num">{formatCurrencyRaw(totals.balance)}</span>
                {excludedNote && ` ${excludedNote}`}
              </span>
              <span className="num">
                {foreign.length > 0
                  ? foreign
                      .map((c) =>
                        t('table.rate', {
                          currency: c,
                          rate: fmt.money(
                            convert(1, c as CurrencyCode, currencyCode),
                            currencyCode,
                            {
                              decimals: 2,
                            }
                          ),
                        })
                      )
                      .join(' · ')
                  : t('table.singleCurrency', { currency: currencyCode })}
              </span>
            </div>
          </Card>
        </>
      )}

      <BankAccountFormDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        onSubmit={(data, zones) => {
          createAccount.mutate(
            { data: data as InsertBankAccount, zones },
            {
              onSuccess: () => {
                setAddDialogOpen(false);
                queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
              },
            }
          );
        }}
        isLoading={createAccount.isPending}
      />
      <BankAccountFormDialog
        open={editing !== null}
        onOpenChange={(open) => {
          if (!open) setEditing(null);
        }}
        account={editing ?? undefined}
        onSubmit={(data) => {
          const { id, ...updateData } = data as InsertBankAccount & { id: string };
          updateAccount.mutate(
            { id, data: updateData },
            {
              onSuccess: () => {
                setEditing(null);
                queryClient.invalidateQueries({ queryKey: ['bank-accounts'] });
                queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
              },
            }
          );
        }}
        isLoading={updateAccount.isPending}
      />
      <ConfirmDeleteDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.descriptionNamed', { name: pendingDelete?.name ?? '' })}
        onConfirm={() =>
          pendingDelete &&
          deleteAccount.mutate(pendingDelete.id, { onSuccess: () => setPendingDelete(null) })
        }
        isPending={deleteAccount.isPending}
        confirmLabel={t('deleteAccount')}
      />
    </>
  );
}
