import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Ellipsis,
  ExternalLink,
  ListFilter,
  Lock,
  LockOpen,
  Pencil,
  Trash2,
} from 'lucide-react';
import type { CurrencyCode } from '@shared/currencies';
import type { InsuranceRow } from '@/utils/insurance';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useDurationText } from '@/hooks/use-duration-text';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
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
import { cn } from '@/lib/utils';

type SortColumn = 'name' | 'premium' | 'coverage' | 'nextPayment' | 'anniversary';

interface InsuranceTableProps {
  rows: InsuranceRow[];
  today: number;
  onEdit: (row: InsuranceRow) => void;
  onEnd: (row: InsuranceRow) => void;
  onReactivate: (row: InsuranceRow) => void;
  onDelete: (row: InsuranceRow) => void;
}

const SOON_MONTHS = 3;

/**
 * Policies table (prototype insurance.html): premium with its yearly
 * equivalent, coverage with the top limits, next payment, anniversary or
 * contract end with a badge when it is close; ended policies behind a toggle,
 * a type filter in the toolbar.
 */
export function InsuranceTable({
  rows,
  today,
  onEdit,
  onEnd,
  onReactivate,
  onDelete,
}: InsuranceTableProps) {
  const { t } = useTranslation('insurance');
  const { formatCurrency, convert, currencyCode } = useCurrency();
  const fmt = useFormat();
  const duration = useDurationText('insurance');
  const [, setLocation] = useLocation();
  const [showEnded, setShowEnded] = useState(false);
  const [type, setType] = useState('all');
  const [sortColumn, setSortColumn] = useState<SortColumn>('premium');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const endedCount = rows.filter((r) => r.ended).length;
  const types = useMemo(() => [...new Set(rows.map((r) => r.policy.type))].sort(), [rows]);
  const typeLabel = (value: string) => t(`typeShort.${value}`, { defaultValue: value });
  const compact = (czk: number) =>
    fmt.money(convert(czk, 'CZK', currencyCode as CurrencyCode), currencyCode, {
      compact: true,
      decimals: 2,
    });

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      setSortDirection(column === 'premium' || column === 'coverage' ? 'desc' : 'asc');
    }
  };

  const visible = useMemo(() => {
    const far = Number.MAX_SAFE_INTEGER;
    const list = rows.filter(
      (r) => (showEnded || !r.ended) && (type === 'all' || r.policy.type === type)
    );
    return [...list].sort((a, b) => {
      let c = 0;
      switch (sortColumn) {
        case 'name':
          c = a.policy.policyName.localeCompare(b.policy.policyName, undefined, {
            sensitivity: 'base',
          });
          break;
        case 'premium':
          c = (a.yearlyCzk || a.oneTimeCzk) - (b.yearlyCzk || b.oneTimeCzk);
          break;
        case 'coverage':
          c = a.coverageCzk - b.coverageCzk;
          break;
        case 'nextPayment':
          c = (a.nextPayment ?? far) - (b.nextPayment ?? far);
          break;
        case 'anniversary':
          c = (a.anniversary ?? a.endDay ?? far) - (b.anniversary ?? b.endDay ?? far);
          break;
      }
      return sortDirection === 'asc' ? c : -c;
    });
  }, [rows, showEnded, type, sortColumn, sortDirection]);

  const active = visible.filter((r) => !r.ended);

  const SortHead = ({
    column,
    children,
    align = 'left',
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
    <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">{content}</small>
  );

  const anniversaryCell = (r: InsuranceRow) => {
    if (r.ended) {
      return (
        <>
          <Badge className="mr-1.5 align-[1px]">{t('table.ended')}</Badge>
          {r.endDay !== null ? fmt.day(r.endDay) : '—'}
          {sub(t('table.endedOn'))}
        </>
      );
    }
    const isEnd = r.endDay !== null && (r.anniversary === null || r.endDay <= r.anniversary);
    const when = isEnd ? r.endDay : r.anniversary;
    if (when === null) return <span className="text-ink-4">—</span>;
    const months = Math.round((when - today) / (30.44 * 86_400));
    return (
      <>
        {months <= SOON_MONTHS && (
          <Badge variant="dark" className="mr-1.5 align-[1px]">
            {t('table.in', { duration: duration.ahead(today, when) })}
          </Badge>
        )}
        {fmt.day(when)}
        {sub(
          isEnd
            ? t('table.endOn')
            : months > SOON_MONTHS
              ? t('table.in', { duration: duration.ahead(today, when) })
              : t('table.anniversaryShort')
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
            {endedCount > 0 && !showEnded ? t('table.subtitleHidden') : t('table.subtitle')}
          </CardDescription>
        </div>
        <div className="flex items-center gap-4">
          {endedCount > 0 && (
            <label className="inline-flex cursor-pointer items-center gap-[9px] text-table font-500 text-ink-2">
              <Checkbox checked={showEnded} onCheckedChange={(v) => setShowEnded(v === true)} />
              {t('table.showEnded')}
            </label>
          )}
          {types.length > 1 && (
            <Select value={type} onValueChange={setType}>
              <SelectTrigger className="h-[35px] w-[160px] text-table">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('table.allTypes')}</SelectItem>
                {types.map((value) => (
                  <SelectItem key={value} value={value}>
                    {typeLabel(value)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </div>
      </CardHeader>
      <Table>
        <TableHeader>
          <TableRow>
            <SortHead column="name" className="w-[24%]">
              {t('table.policy')}
            </SortHead>
            <TableHead>{t('table.type')}</TableHead>
            <TableHead>{t('table.provider')}</TableHead>
            <SortHead column="premium" align="right">
              {t('table.premium')}
            </SortHead>
            <SortHead column="coverage" align="right">
              {t('table.coverage')}
            </SortHead>
            <SortHead column="nextPayment" align="right">
              {t('table.nextPayment')}
            </SortHead>
            <SortHead column="anniversary" align="right">
              {t('table.anniversary')}
            </SortHead>
            <TableHead className="w-10">
              <span className="sr-only">{t('table.actions')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {visible.length === 0 ? (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={8} className="py-12">
                <div className="grid place-items-center text-center text-ink-3">
                  <ListFilter className="mb-3 size-7 text-ink-5" aria-hidden />
                  <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults.title')}</h3>
                  <p className="m-0 max-w-[320px] text-table">{t('table.noResults.description')}</p>
                </div>
              </TableCell>
            </TableRow>
          ) : (
            visible.map((r) => {
              const p = r.policy;
              const limits = [...(p.limits ?? [])].sort((a, b) => b.amount - a.amount);
              return (
                <TableRow
                  key={p.id}
                  className={cn('h-[62px] cursor-pointer', r.ended && 'text-ink-4')}
                  onClick={() => setLocation(`/insurance/${p.id}`)}
                >
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px]">
                      <AssetLogo
                        ticker={p.provider}
                        type="stock"
                        variant={r.ended ? 'soft' : 'series'}
                      />
                      <div className="min-w-0">
                        <b
                          className={cn(
                            'block truncate text-table font-650',
                            r.ended ? 'text-ink-3' : 'text-ink'
                          )}
                        >
                          {p.policyName}
                        </b>
                        {p.policyNumber &&
                          sub(t('detail.policyNumber', { number: p.policyNumber }))}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge>{typeLabel(p.type)}</Badge>
                  </TableCell>
                  <TableCell className={cn(!r.ended && 'text-ink-2')}>{p.provider}</TableCell>
                  <TableCell className="text-right num">
                    <span className={cn('font-650', !r.ended && 'text-ink')}>
                      {formatCurrency(r.premiumCzk)}
                    </span>
                    {sub(
                      r.frequency === 'one_time'
                        ? t('frequencyShort.one_time')
                        : `${t(`frequencyShort.${r.frequency ?? 'monthly'}`)}${
                            r.frequency !== 'annually' && r.yearlyCzk > 0
                              ? ` · ${t('table.perYear', { amount: formatCurrency(r.yearlyCzk) })}`
                              : ''
                          }`
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.coverageCzk > 0 ? (
                      <>
                        {compact(r.coverageCzk)}
                        {sub(
                          limits
                            .slice(0, 2)
                            .map((l) => `${l.title} ${compact(l.amount)}`)
                            .join(' · ') +
                            (limits.length > 2
                              ? ` · ${t('table.moreLimits', { count: limits.length - 2 })}`
                              : '')
                        )}
                      </>
                    ) : (
                      <span className="text-ink-4">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {r.nextPayment !== null ? (
                      <>
                        {fmt.day(r.nextPayment)}
                        {sub(formatCurrency(r.premiumCzk))}
                      </>
                    ) : (
                      <span className="text-ink-4">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">{anniversaryCell(r)}</TableCell>
                  <TableCell className="text-right">
                    <div data-row-actions onClick={(e) => e.stopPropagation()}>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label={t('table.actions')}>
                            <Ellipsis />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => setLocation(`/insurance/${p.id}`)}>
                            <ExternalLink />
                            {t('actions.open')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => onEdit(r)}>
                            <Pencil />
                            {t('actions.edit')}
                          </DropdownMenuItem>
                          {r.ended ? (
                            <DropdownMenuItem onSelect={() => onReactivate(r)}>
                              <LockOpen />
                              {t('actions.reactivate')}
                            </DropdownMenuItem>
                          ) : (
                            <DropdownMenuItem onSelect={() => onEnd(r)}>
                              <Lock />
                              {t('actions.end')}
                            </DropdownMenuItem>
                          )}
                          <DropdownMenuSeparator />
                          <DropdownMenuItem variant="danger" onSelect={() => onDelete(r)}>
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
          {t('policies', { count: active.length })} ·{' '}
          {t('table.footer.yearly', {
            amount: formatCurrency(active.reduce((s, r) => s + r.yearlyCzk, 0)),
          })}{' '}
          ·{' '}
          {t('table.footer.coverage', {
            amount: compact(active.reduce((s, r) => s + r.coverageCzk, 0)),
          })}
        </span>
        <span>{t('table.footer.note')}</span>
      </div>
    </Card>
  );
}
