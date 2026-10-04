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
 * Layout of the fixed table (long text ends with an ellipsis instead of widening the table). The
 * content column is 720 px wide at the 1080 px window minimum and 1040 px at the default 1400 px;
 * the policy column has no width and takes what the others leave. Each column is as wide as what
 * it has to show (measured in WebKit, whose text runs about 3 % wider than Chrome's):
 * - Narrow layout (window under 1280 px, 920 px of content), main values only: premium 112 px,
 *   coverage 124 and anniversary 136 (their headers need up to 103, 108 and 129 px) leave the
 *   policy about 158 px at 720 px. The yearly total, the limit title and the badge beside the date
 *   wait for the wide layout; the full text stays in `title`.
 * - Wide layout (`xl:`): the yearly total fits in 176 px, the badge and date in 192 px, and the
 *   coverage takes 20 % so the limit title gets room as the window grows.
 * - "Next payment" needs 135 px for its header at every width, so it gets 144; the dates are short.
 * - The actions column is the 32 px menu button between two 6 px paddings.
 */
const COLUMN = {
  premium: 'w-[112px] xl:w-[176px]',
  coverage: 'w-[124px] xl:w-[20%]',
  nextPayment: 'w-[144px]',
  anniversary: 'w-[136px] xl:w-[192px]',
  actions: 'w-11 px-1.5',
} as const;

/**
 * Policies table (prototype insurance.html): premium with its yearly
 * equivalent, coverage with the largest limit, next payment, anniversary or
 * contract end with a badge when it is close; ended policies behind a toggle,
 * a type filter in the toolbar. Fixed layout: free text (policy and limit
 * titles, insurer) ends with an ellipsis and carries its full text in `title`.
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
  /** A CZK total in the display currency. */
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
            'inline-flex max-w-full items-center gap-1 rounded-r1 text-thead uppercase transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus',
            activeCol && 'text-ink'
          )}
        >
          {/* leading-4: the ellipsis clip must not cut the accents of Czech capitals (Í, Š, Ř) */}
          <span className="truncate leading-4">{children}</span>
          <Icon className={cn('size-3 shrink-0', !activeCol && 'opacity-60')} aria-hidden />
        </button>
      </TableHead>
    );
  };

  /**
   * The second line of a cell: 10 px, one line, cut with an ellipsis, full text on hover. The lead
   * stands in front of the text in the narrow layout only (where the badge it repeats is hidden).
   */
  const sub = (text: string, narrowLead?: string) => (
    <small
      title={narrowLead ? `${narrowLead} · ${text}` : text}
      className="mt-[3px] block truncate text-micro font-500 text-ink-4"
    >
      {narrowLead && <span className="xl:hidden">{narrowLead} · </span>}
      {text}
    </small>
  );

  /** A date; the badge to its left (how soon it is, or that the policy ended) needs the wide layout. */
  const dateLine = (date: string, badge?: { text: string; variant?: 'dark' }) => (
    <div
      title={badge ? `${badge.text} ${date}` : date}
      className="flex h-4 items-center justify-end gap-1.5"
    >
      {badge && (
        <Badge variant={badge.variant} className="hidden xl:inline-flex">
          {badge.text}
        </Badge>
      )}
      <span>{date}</span>
    </div>
  );

  /** Frequency, then the yearly total (wide layout only; the hover text always has both). */
  const premiumSub = (r: InsuranceRow) => {
    if (r.frequency === 'one_time') return sub(t('frequencyShort.one_time'));
    const frequency = t(`frequencyShort.${r.frequency ?? 'monthly'}`);
    if (r.frequency === 'annually' || r.yearlyCzk <= 0) return sub(frequency);
    const yearly = t('table.perYear', { amount: formatCurrency(r.yearlyCzk) });
    return (
      <small
        title={`${frequency} · ${yearly}`}
        className="mt-[3px] block truncate text-micro font-500 text-ink-4"
      >
        {frequency}
        <span className="hidden xl:inline"> · {yearly}</span>
      </small>
    );
  };

  /** Total in the display currency; under it the largest limit in its own currency and how many more. */
  const coverageCell = (r: InsuranceRow) => {
    const top = r.topLimit;
    if (r.coverageCzk <= 0 || !top) return <span className="text-ink-4">—</span>;
    const title = top.title.trim();
    const amount = fmt.money(top.amount, top.currency || 'CZK', { compact: true, decimals: 2 });
    const more = r.limitCount > 1 ? ` · ${t('table.moreLimits', { count: r.limitCount - 1 })}` : '';
    const rest = `${amount}${more}`;
    return (
      <>
        {compact(r.coverageCzk)}
        {/* the free-text title (wide layout) gives way first; the amount and the count stay whole */}
        <small
          title={title ? `${title} ${rest}` : rest}
          className="mt-[3px] flex justify-end gap-[3px] text-micro font-500 text-ink-4"
        >
          {title && <span className="hidden min-w-0 truncate xl:block">{title}</span>}
          <span className="max-w-full shrink-0 truncate">{rest}</span>
        </small>
      </>
    );
  };

  const anniversaryCell = (r: InsuranceRow) => {
    if (r.ended) {
      return (
        <>
          {dateLine(r.endDay !== null ? fmt.day(r.endDay) : '—', { text: t('table.ended') })}
          {sub(t('table.endedOn'))}
        </>
      );
    }
    const isEnd = r.endDay !== null && (r.anniversary === null || r.endDay <= r.anniversary);
    const when = isEnd ? r.endDay : r.anniversary;
    if (when === null) return <span className="text-ink-4">—</span>;
    const months = Math.round((when - today) / (30.44 * 86_400));
    const soon = months <= SOON_MONTHS;
    const ahead = t('table.in', { duration: duration.ahead(today, when) });
    const kind = isEnd ? t('table.endOn') : t('table.anniversaryShort');
    return (
      <>
        {dateLine(fmt.day(when), soon ? { text: ahead, variant: 'dark' } : undefined)}
        {sub(soon || isEnd ? kind : ahead, soon ? ahead : undefined)}
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
      <Table className="table-fixed">
        <TableHeader>
          <TableRow>
            <SortHead column="name">{t('table.policy')}</SortHead>
            <SortHead column="premium" align="right" className={COLUMN.premium}>
              {t('table.premium')}
            </SortHead>
            <SortHead column="coverage" align="right" className={COLUMN.coverage}>
              {t('table.coverage')}
            </SortHead>
            <SortHead column="nextPayment" align="right" className={COLUMN.nextPayment}>
              {t('table.nextPayment')}
            </SortHead>
            <SortHead column="anniversary" align="right" className={COLUMN.anniversary}>
              {t('table.anniversary')}
            </SortHead>
            <TableHead className={COLUMN.actions}>
              <span className="sr-only">{t('table.actions')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {visible.length === 0 ? (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={6} className="py-12">
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
              return (
                <TableRow
                  key={p.id}
                  className={cn('cursor-pointer', r.ended && 'text-ink-4')}
                  onClick={() => setLocation(`/insurance/${p.id}`)}
                >
                  <TableCell className="overflow-hidden">
                    <div className="flex min-w-0 items-center gap-[10px]">
                      <AssetLogo
                        ticker={p.provider}
                        type="stock"
                        variant={r.ended ? 'soft' : 'series'}
                      />
                      <div className="min-w-0">
                        <b
                          title={p.policyName}
                          className={cn(
                            'block truncate text-table font-650',
                            r.ended ? 'text-ink-3' : 'text-ink'
                          )}
                        >
                          {p.policyName}
                        </b>
                        {sub([typeLabel(p.type), p.provider].filter(Boolean).join(' · '))}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="truncate text-right num">
                    <span className={cn('font-650', !r.ended && 'text-ink')}>
                      {formatCurrency(r.premiumCzk)}
                    </span>
                    {premiumSub(r)}
                  </TableCell>
                  <TableCell className="truncate text-right num">{coverageCell(r)}</TableCell>
                  <TableCell className="truncate text-right num">
                    {r.nextPayment !== null ? (
                      <>
                        {fmt.day(r.nextPayment)}
                        {sub(formatCurrency(r.premiumCzk))}
                      </>
                    ) : (
                      <span className="text-ink-4">—</span>
                    )}
                  </TableCell>
                  <TableCell className="truncate text-right num">{anniversaryCell(r)}</TableCell>
                  <TableCell className="px-1.5 text-right">
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
