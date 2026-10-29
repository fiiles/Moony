import { useMemo, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUpRight,
  Ellipsis,
  ExternalLink,
  Lock,
  Pencil,
  RotateCcw,
  Search,
  Trash2,
} from 'lucide-react';
import type { SpendingPlan } from '@shared/schema';
import {
  NEAR_THRESHOLD,
  overviewRows,
  type PlanRow,
  type PlanSegment,
} from '@/utils/spending-plans';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input, InputWrap } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { Segmented } from '@/components/ui/segmented';
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
import { PlanChip } from '@/components/spending-plans/PlanChip';
import { PlanMoney } from '@/components/spending-plans/PlanMoney';

interface PlansTableProps {
  year: number;
  rows: PlanRow[];
  /** Currency of the footer sums (the overview's report currency). */
  currency: string;
  onOpen: (plan: SpendingPlan) => void;
  onEdit: (plan: SpendingPlan) => void;
  onToggleClosed: (plan: SpendingPlan) => void;
  onDelete: (row: PlanRow) => void;
}

const COLUMNS = 7;

/**
 * "Plány N" (prototype overview): search, Aktivní · Uzavřené · Vše, one row per plan ordered by
 * risk, amounts in each plan's own currency, and a footer summing the shown rows in the report
 * currency (a plan without a rate is left out of the sums and the sums are marked).
 */
export function PlansTable({
  year,
  rows,
  currency,
  onOpen,
  onEdit,
  onToggleClosed,
  onDelete,
}: PlansTableProps) {
  const { t } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { convert, fxReady } = useCurrency();
  const [segment, setSegment] = useState<PlanSegment>('active');
  const [query, setQuery] = useState('');

  const counts = useMemo(
    () => ({
      active: rows.filter((r) => !r.closed).length,
      closed: rows.filter((r) => r.closed).length,
      all: rows.length,
    }),
    [rows]
  );
  const shown = useMemo(
    () => overviewRows(rows, segment, query, fmt.locale),
    [rows, segment, query, fmt.locale]
  );

  const footer = useMemo(() => {
    let planned = 0;
    let spent = 0;
    const missing = new Set<string>();
    for (const row of shown) {
      const from = row.summary.plan.currency;
      const p = convert(row.planned, from, currency);
      const s = row.spent === null ? null : convert(row.spent, from, currency);
      // While the quotes load a null is pending, not missing
      if (fxReady && (p === null || (row.spent !== null && s === null))) missing.add(from);
      planned += p ?? 0;
      spent += s ?? 0;
    }
    return { planned, spent, missing: [...missing] };
  }, [shown, convert, currency, fxReady]);

  // "12. 10." this year, "28. 12. 2025" otherwise (a payment may sit outside the plan year)
  const thisYear = new Date().getUTCFullYear();
  const shortDay = (day: number) =>
    fmt.day(day, {
      day: 'numeric',
      month: 'numeric',
      ...(new Date(day * 1000).getUTCFullYear() !== thisYear ? { year: 'numeric' } : {}),
    });

  const open = (plan: SpendingPlan) => () => onOpen(plan);
  const onRowKey = (plan: SpendingPlan) => (e: KeyboardEvent) => {
    if (e.key === 'Enter' && e.target === e.currentTarget) onOpen(plan);
  };

  const statusCell = (row: PlanRow) => {
    if (row.closed) return <Badge variant="outline">{t('status.closed')}</Badge>;
    switch (row.status) {
      case 'over':
        return (
          <Badge variant="loss">
            <ArrowUpRight />
            {t('status.over')}
          </Badge>
        );
      case 'near':
        return <Badge>{t('status.near')}</Badge>;
      default:
        return <span className="text-ink-4">{t(`status.${row.status}`)}</span>;
    }
  };

  const emptyBody = () => {
    const q = query.trim();
    const title = q
      ? t('table.noResults.searchTitle')
      : segment === 'active'
        ? t('table.noResults.allClosed', { year })
        : t('table.noResults.noClosed');
    const description = q
      ? t(
          segment === 'active'
            ? 'table.noResults.searchActive'
            : segment === 'closed'
              ? 'table.noResults.searchClosed'
              : 'table.noResults.searchAll',
          { query: q, year }
        )
      : null;
    return (
      <TableRow className="hover:bg-transparent">
        <TableCell colSpan={COLUMNS} className="py-12">
          <div className="grid place-items-center text-center text-ink-3">
            <Search className="mb-3 size-7 text-ink-5" aria-hidden />
            <h3 className="mb-1.5 text-h3 text-ink">{title}</h3>
            {description && <p className="m-0 max-w-[360px] text-table">{description}</p>}
            {segment !== 'all' && (
              <Button
                variant="outline"
                size="sm"
                className="mt-4"
                onClick={() => setSegment('all')}
              >
                {t('table.noResults.showAll')}
              </Button>
            )}
          </div>
        </TableCell>
      </TableRow>
    );
  };

  return (
    <Card variant="table">
      <CardHeader>
        <div className="min-w-0">
          <CardTitle>{t('table.title', { year })}</CardTitle>
          <CardDescription>{t('table.subtitle')}</CardDescription>
        </div>
        <div className="flex items-center gap-3">
          <InputWrap icon={<Search />} className="w-56">
            <Input
              type="search"
              className="h-[35px] text-table"
              placeholder={t('table.search')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </InputWrap>
          <Segmented
            value={segment}
            onValueChange={setSegment}
            aria-label={t('table.segmentLabel')}
            options={[
              { value: 'active', label: t('table.segment.active', { n: counts.active }) },
              { value: 'closed', label: t('table.segment.closed', { n: counts.closed }) },
              { value: 'all', label: t('table.segment.all', { n: counts.all }) },
            ]}
          />
        </div>
      </CardHeader>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-[30%]">{t('table.plan')}</TableHead>
            <TableHead className="text-right">{t('table.planned')}</TableHead>
            <TableHead className="text-right">{t('table.spent')}</TableHead>
            <TableHead className="w-[15%]">{t('table.usage')}</TableHead>
            <TableHead className="text-right">{t('table.left')}</TableHead>
            <TableHead>{t('table.status')}</TableHead>
            <TableHead className="w-10">
              <span className="sr-only">{t('actions.menu')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {shown.length === 0
            ? emptyBody()
            : shown.map((row) => {
                const { plan, paymentCount, lastPaymentDate, fxQuality } = row.summary;
                const muted = row.closed;
                return (
                  <TableRow
                    key={plan.id}
                    tabIndex={0}
                    className={cn('h-[62px] cursor-pointer', muted && 'text-ink-4')}
                    onClick={open(plan)}
                    onKeyDown={onRowKey(plan)}
                  >
                    <TableCell>
                      <div className="flex min-w-0 items-center gap-[10px]">
                        <PlanChip plan={plan} />
                        <div className="min-w-0">
                          <b
                            className={cn(
                              'block truncate text-table font-650',
                              muted ? 'text-ink-3' : 'text-ink'
                            )}
                          >
                            {plan.name}
                          </b>
                          <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                            {paymentCount > 0 && lastPaymentDate !== null
                              ? t('table.payments', {
                                  count: paymentCount,
                                  date: shortDay(lastPaymentDate),
                                })
                              : t('table.noPayments')}
                          </small>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="text-right num">
                      {fmt.money(row.planned, plan.currency)}
                    </TableCell>
                    <TableCell className={cn('text-right font-650 num', !muted && 'text-ink')}>
                      <PlanMoney
                        amount={row.summary.spent}
                        currency={plan.currency}
                        quality={fxQuality}
                      />
                    </TableCell>
                    <TableCell>
                      {row.ratio === null ? (
                        <span className="text-ink-4">—</span>
                      ) : (
                        <div className="max-w-[140px]">
                          <span
                            className={cn(
                              'mb-1 block text-micro font-650 num',
                              row.status === 'over' ? 'text-loss' : 'text-ink-3'
                            )}
                          >
                            {fmt.percent(row.ratio, 0)}
                          </span>
                          <Progress value={row.ratio * 100} over={row.status === 'over'} />
                        </div>
                      )}
                    </TableCell>
                    <TableCell
                      className={cn(
                        'text-right num',
                        row.left !== null && row.left < 0 && 'text-loss'
                      )}
                    >
                      {row.left === null ? (
                        <span className="text-ink-4">—</span>
                      ) : (
                        fmt.money(row.left, plan.currency, { signed: row.left < 0 })
                      )}
                    </TableCell>
                    <TableCell>{statusCell(row)}</TableCell>
                    <TableCell className="text-right">
                      <div data-row-actions onClick={(e) => e.stopPropagation()}>
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              aria-label={t('actions.menu')}
                              onKeyDown={(e) => e.stopPropagation()}
                            >
                              <Ellipsis />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end">
                            <DropdownMenuItem onSelect={() => onOpen(plan)}>
                              <ExternalLink />
                              {t('actions.open')}
                            </DropdownMenuItem>
                            <DropdownMenuItem onSelect={() => onEdit(plan)}>
                              <Pencil />
                              {t('actions.edit')}
                            </DropdownMenuItem>
                            <DropdownMenuItem onSelect={() => onToggleClosed(plan)}>
                              {row.closed ? <RotateCcw /> : <Lock />}
                              {row.closed ? t('actions.reopen') : t('actions.close')}
                            </DropdownMenuItem>
                            <DropdownMenuSeparator />
                            <DropdownMenuItem variant="danger" onSelect={() => onDelete(row)}>
                              <Trash2 />
                              {t('actions.delete')}
                            </DropdownMenuItem>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
        </TableBody>
      </Table>
      {shown.length > 0 && (
        <div className="flex items-center justify-between gap-6 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
          <span
            className={cn(footer.missing.length > 0 && 'text-loss')}
            title={
              footer.missing.length > 0
                ? tc('fxQuality.missing', { currency, currencies: footer.missing.join(', ') })
                : undefined
            }
          >
            {t('table.footer', {
              count: shown.length,
              planned: fmt.money(footer.planned, currency),
              spent: fmt.money(footer.spent, currency),
            })}
          </span>
          <span>{t('table.footerNote', { percent: fmt.percent(NEAR_THRESHOLD, 0) })}</span>
        </div>
      )}
    </Card>
  );
}
