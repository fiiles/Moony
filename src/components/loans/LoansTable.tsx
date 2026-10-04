import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Ellipsis,
  ExternalLink,
  Pencil,
  Trash2,
} from 'lucide-react';
import type { Loan } from '@shared/schema';
import type { LoanRow } from '@/utils/loans';
import { monthsAhead } from '@/utils/loans';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useLoanText } from '@/hooks/use-loan-text';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Progress } from '@/components/ui/progress';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { AssetLogo } from '@/components/common/AssetLogo';
import { LoanValuationNote } from '@/components/loans/LoanValuationNote';
import { cn } from '@/lib/utils';

type SortColumn = 'name' | 'outstanding' | 'principal' | 'rate' | 'payment' | 'payoff';

interface LoansTableProps {
  rows: LoanRow[];
  today: number;
  onEdit: (loan: Loan) => void;
  onDelete: (loan: Loan) => void;
}

/**
 * Loans table (prototype loans.html): outstanding with the repaid bar, principal,
 * rate with its fixation, payment, and the payoff month from the schedule with
 * "do roka" / "do 18 měsíců" badges; paid-off loans sit behind a toggle.
 */
export function LoansTable({ rows, today, onEdit, onDelete }: LoansTableProps) {
  const { t } = useTranslation('loans');
  const { formatCurrency } = useCurrency();
  const fmt = useFormat();
  const text = useLoanText();
  const [, setLocation] = useLocation();
  const [showMatured, setShowMatured] = useState(false);
  const [sortColumn, setSortColumn] = useState<SortColumn>('outstanding');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const maturedCount = rows.filter((r) => r.matured).length;

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      setSortDirection(column === 'name' || column === 'payoff' ? 'asc' : 'desc');
    }
  };

  const visible = useMemo(() => {
    const list = showMatured ? rows : rows.filter((r) => !r.matured);
    const far = Number.MAX_SAFE_INTEGER;
    return [...list].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = a.loan.name.localeCompare(b.loan.name, undefined, { sensitivity: 'base' });
          break;
        case 'outstanding':
          c = a.outstandingCzk - b.outstandingCzk;
          break;
        case 'principal':
          c = a.principalCzk - b.principalCzk;
          break;
        case 'rate':
          c = (Number(a.loan.interestRate) || 0) - (Number(b.loan.interestRate) || 0);
          break;
        case 'payment':
          c = a.paymentCzk - b.paymentCzk;
          break;
        case 'payoff':
          c = (a.payoffDay ?? far) - (b.payoffDay ?? far);
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [rows, showMatured, sortColumn, sortDirection]);

  const active = visible.filter((r) => !r.matured);
  const totalOutstanding = active.reduce((s, r) => s + r.outstandingCzk, 0);
  const totalPayments = active.reduce((s, r) => s + r.paymentCzk, 0);

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
    const activeCol = sortColumn === column;
    const Icon = !activeCol ? ArrowUpDown : sortDirection === 'asc' ? ArrowUp : ArrowDown;
    return (
      <TableHead className={cn(align === 'right' && 'text-right', className)}>
        <button
          type="button"
          onClick={() => handleSort(column)}
          aria-sort={activeCol ? (sortDirection === 'asc' ? 'ascending' : 'descending') : undefined}
          className={cn(
            'inline-flex items-center gap-1 rounded-r1 text-thead uppercase transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus',
            activeCol && 'text-ink'
          )}
        >
          {children}
          <Icon className={cn('size-3', !activeCol && 'opacity-60')} aria-hidden />
        </button>
      </TableHead>
    );
  };

  const sub = (content: ReactNode) => (
    <small className="mt-[3px] block text-micro font-500 text-ink-4">{content}</small>
  );

  const payoffCell = (r: LoanRow) => {
    if (r.matured) {
      const when = r.payoffDay ?? r.loan.endDate;
      return (
        <>
          <span className="text-ink-4">{t('table.matured')}</span>
          {when !== null && sub(text.monthYear(when))}
        </>
      );
    }
    if (r.payoffDay !== null) {
      const months = monthsAhead(today, r.payoffDay);
      return (
        <>
          {months <= 18 && (
            <Badge variant="dark" className="mr-1.5 align-[1px]">
              {months <= 12 ? t('table.soonYear') : t('table.soon')}
            </Badge>
          )}
          {text.monthYear(r.payoffDay)}
          {sub(t('table.in', { duration: text.months(months) }))}
        </>
      );
    }
    if (r.trajectory.end === 'endDate' && r.loan.endDate !== null) {
      const last = r.trajectory.schedule.rows[r.trajectory.schedule.rows.length - 1];
      return (
        <>
          {text.monthYear(r.loan.endDate)}
          {sub(
            t('table.balanceAtEnd', {
              amount: fmt.money(last?.balanceAfter ?? r.outstanding, r.loan.currency, {
                decimals: 0,
              }),
            })
          )}
        </>
      );
    }
    return (
      <>
        <span className="text-ink-4">—</span>
        {sub(
          r.mode === 'static'
            ? t('table.noPayment')
            : r.trajectory.end === 'capped'
              ? t('table.capped')
              : t('table.noPayoff')
        )}
      </>
    );
  };

  return (
    <Card variant="table">
      <CardHeader>
        <div>
          <CardTitle>{t('table.title')}</CardTitle>
          <CardDescription>
            {maturedCount > 0 && !showMatured ? t('table.subtitleHidden') : t('table.subtitle')}
          </CardDescription>
        </div>
        {maturedCount > 0 && (
          <label className="inline-flex cursor-pointer items-center gap-[9px] text-table font-500 text-ink-2">
            <Checkbox checked={showMatured} onCheckedChange={(v) => setShowMatured(v === true)} />
            {t('table.showMatured')}
          </label>
        )}
      </CardHeader>
      <Table>
        <TableHeader>
          <TableRow>
            <SortHead column="name" align="left" className="w-[26%]">
              {t('table.loan')}
            </SortHead>
            <SortHead column="outstanding">{t('table.outstanding')}</SortHead>
            <SortHead column="principal">{t('table.principal')}</SortHead>
            <SortHead column="rate">{t('table.rate')}</SortHead>
            <SortHead column="payment">{t('table.payment')}</SortHead>
            <SortHead column="payoff">{t('table.payoff')}</SortHead>
            <TableHead className="w-10">
              <span className="sr-only">{t('table.actions')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {visible.length === 0 ? (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={7} className="py-12">
                <div className="grid place-items-center text-center text-ink-3">
                  <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults.title')}</h3>
                  <p className="m-0 max-w-[320px] text-table">{t('table.noResults.description')}</p>
                </div>
              </TableCell>
            </TableRow>
          ) : (
            visible.map((r) => {
              const loan = r.loan;
              const needsNote = r.mode === 'static' || r.belowInterest;
              return (
                <TableRow
                  key={loan.id}
                  className={cn('h-[66px] cursor-pointer', r.matured && 'text-ink-4')}
                  onClick={() => setLocation(`/loans/${loan.id}`)}
                >
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px]">
                      <AssetLogo
                        ticker={loan.name}
                        type="stock"
                        variant={r.matured ? 'soft' : 'series'}
                      />
                      <div className="min-w-0">
                        <b
                          className={cn(
                            'block truncate text-table font-650',
                            r.matured ? 'text-ink-3' : 'text-ink'
                          )}
                        >
                          {loan.name}
                        </b>
                        {sub(
                          `${loan.currency || 'CZK'} · ${t('table.since', {
                            month: text.monthYear(loan.startDate),
                          })}`
                        )}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.matured ? (
                      <span className="text-ink-4">{t('table.matured')}</span>
                    ) : (
                      <>
                        <span className="inline-flex items-center justify-end gap-2">
                          {needsNote && <LoanValuationNote loan={loan} />}
                          <Progress value={r.repaidShare * 100} className="w-[60px]" />
                          <span className="font-650 text-ink">
                            {formatCurrency(r.outstandingCzk)}
                          </span>
                        </span>
                        {sub(t('table.repaid', { percent: fmt.percent(r.repaidShare, 0) }))}
                      </>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">{formatCurrency(r.principalCzk)}</TableCell>
                  <TableCell className="text-right num">
                    {fmt.percent((Number(loan.interestRate) || 0) / 100, 2)}
                    {sub(
                      r.fixationEnd !== null
                        ? t('table.fixedUntil', { month: text.monthYear(r.fixationEnd) })
                        : loan.interestRateValidityDate !== null
                          ? t('table.fixationEnded', {
                              month: text.monthYear(loan.interestRateValidityDate),
                            })
                          : t('table.fixedRate')
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.payment > 0 && !r.matured ? (
                      <>
                        {formatCurrency(r.paymentCzk)}
                        {sub(t('table.monthly'))}
                      </>
                    ) : (
                      <span className="text-ink-4">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">{payoffCell(r)}</TableCell>
                  <TableCell className="text-right">
                    <div data-row-actions onClick={(e) => e.stopPropagation()}>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label={t('table.actions')}>
                            <Ellipsis />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => setLocation(`/loans/${loan.id}`)}>
                            <ExternalLink />
                            {t('actions.open')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => onEdit(loan)}>
                            <Pencil />
                            {t('actions.edit')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem variant="danger" onSelect={() => onDelete(loan)}>
                            <Trash2 />
                            {t('actions.delete')}
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
      <div className="flex items-center justify-between gap-6 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
        <span>
          {t('table.footer.count', { count: active.length })} ·{' '}
          {t('table.footer.outstanding', { amount: formatCurrency(totalOutstanding) })} ·{' '}
          {t('table.footer.payments', { amount: formatCurrency(totalPayments) })}
        </span>
        <span>{t('table.footer.note')}</span>
      </div>
    </Card>
  );
}
