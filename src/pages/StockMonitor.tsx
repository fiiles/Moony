import { useMemo, useState, type ReactNode } from 'react';
import { Link, useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Ellipsis,
  Eye,
  Plus,
  RefreshCw,
  Search,
} from 'lucide-react';
import type { WatchedStockRow } from '@shared/schema';
import { currencyDecimals } from '@shared/currencies';
import { exchangeName } from '@/utils/exchange-names';
import { useFormat } from '@/lib/use-format';
import { useCurrency } from '@/lib/currency';
import { cn } from '@/lib/utils';
import { useShellPage } from '@/components/shell/shell-context';
import { usePortfolioFollowCandidates, useWatchedStocks } from '@/hooks/use-stock-monitor';
import { useStockMonitorMutations } from '@/hooks/use-stock-monitor-mutations';
import {
  dayChange,
  filterWatchedRows,
  formatNativePrice,
  latestFetchedAt,
  sortWatchedRows,
  targetReached,
  watchedDisplayName as displayName,
  type WatchSortColumn,
} from '@/utils/stock-monitor';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Input, InputWrap } from '@/components/ui/input';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
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
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { McpTip } from '@/components/common/McpTip';
import { AssetLogo } from '@/components/common/AssetLogo';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StockSearchCombobox } from '@/components/stock-monitor/StockSearchCombobox';
import { RangeWithLabels } from '@/components/stock-monitor/RangeBar';
import {
  TargetPriceDialog,
  type TargetPriceTarget,
} from '@/components/stock-monitor/TargetPriceDialog';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { ChangeBadge, ChangeNote } from '@/components/common/ChangeBadge';

const num = (v: string | null) => {
  const n = parseFloat(v ?? '');
  return isFinite(n) ? n : null;
};

/**
 * Watchlist (design system §7 Watchlist, prototype `stock-monitor.html`):
 * native-currency prices, day change against the previous close, the 52-week
 * range with the current price and the target tick, target state, three stats
 * and a search in the head that starts following a stock. The table card
 * filters the watched stocks and sorts by its column heads, like the stocks list.
 */
export default function StockMonitor() {
  const { t } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const locale = fmt.locale;
  const { currencyCode } = useCurrency();
  const [, setLocation] = useLocation();
  const { data: rows = [], isLoading } = useWatchedStocks();
  const { data: portfolioCandidates = [] } = usePortfolioFollowCandidates();
  const { unfollowMutation, refreshMutation, followPortfolioMutation, targetPriceMutation } =
    useStockMonitorMutations();

  const [search, setSearch] = useState('');
  const [sortColumn, setSortColumn] = useState<WatchSortColumn>('name');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('asc');
  const [target, setTarget] = useState<TargetPriceTarget | null>(null);
  const [pendingUnfollow, setPendingUnfollow] = useState<WatchedStockRow | null>(null);
  const [buy, setBuy] = useState<WatchedStockRow | null>(null);

  const fetchedAt = latestFetchedAt(rows);
  const fetchedAtLabel =
    fetchedAt == null
      ? null
      : new Date(fetchedAt * 1000).toDateString() === new Date().toDateString()
        ? fmt.time(fetchedAt)
        : fmt.dateTime(fetchedAt);
  useShellPage({
    status: isLoading
      ? undefined
      : fetchedAtLabel
        ? { text: t('status.updated', { time: fetchedAtLabel }), tone: 'fresh' }
        : { text: t('status.never'), tone: 'stale' },
  });

  const price = (v: string | null, currency: string | null) => {
    const n = num(v);
    return n === null ? '—' : formatNativePrice(n, currency, locale, currencyCode);
  };
  const plain = (n: number, currency: string | null) => {
    const d = currencyDecimals(currency);
    return fmt.number(n, { minimumFractionDigits: d, maximumFractionDigits: d });
  };

  const handleSort = (column: WatchSortColumn) => {
    if (sortColumn === column) setSortDirection((d) => (d === 'asc' ? 'desc' : 'asc'));
    else {
      setSortColumn(column);
      // The alphabet and the closest target start from the top; the rest from the largest.
      setSortDirection(column === 'name' || column === 'target' ? 'asc' : 'desc');
    }
  };

  const sorted = useMemo(
    () => sortWatchedRows(filterWatchedRows(rows, search), sortColumn, sortDirection, locale),
    [rows, search, sortColumn, sortDirection, locale]
  );

  const SortHead = ({
    column,
    children,
    align = 'right',
    className,
  }: {
    column: WatchSortColumn;
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
          aria-sort={active ? (sortDirection === 'asc' ? 'ascending' : 'descending') : undefined}
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

  // Stats
  const heldCount = rows.filter((r) => r.isHeld).length;
  const withTarget = rows
    .map((r) => ({ row: r, current: num(r.currentPrice), target: num(r.targetPrice) }))
    .filter(
      (x): x is { row: WatchedStockRow; current: number; target: number } =>
        x.current !== null && x.target !== null && x.target > 0
    );
  const reached = withTarget.filter((x) =>
    targetReached(x.current, x.target, x.row.targetDirection)
  );
  const pending = withTarget
    .filter((x) => !targetReached(x.current, x.target, x.row.targetDirection))
    .sort((a, b) => Math.abs(a.target / a.current - 1) - Math.abs(b.target / b.current - 1));
  const changes = rows
    .map((r) => ({ row: r, change: dayChange(r.currentPrice, r.previousClose) }))
    .filter(
      (x): x is { row: WatchedStockRow; change: NonNullable<typeof x.change> } => x.change !== null
    )
    .sort((a, b) => b.change.pct - a.change.pct);
  const best = changes[0];
  const worst = changes[changes.length - 1];
  const toTarget = (x: { row: WatchedStockRow; current: number; target: number }) =>
    t('stats.toTarget', { pct: fmt.percent(Math.abs(x.target / x.current - 1), 1) });

  const openTarget = (row: WatchedStockRow) =>
    setTarget({
      ticker: row.ticker,
      currency: row.currency,
      currentPrice: row.currentPrice,
      fiftyTwoWeekHigh: row.fiftyTwoWeekHigh,
      targetPrice: row.targetPrice,
      targetDirection: row.targetDirection,
    });

  const head = (
    <PageHead
      eyebrow={t('eyebrow')}
      title={t('title')}
      description={t('subtitle')}
      actions={
        <>
          <StockSearchCombobox />
          <Button
            variant="outline"
            onClick={() => refreshMutation.mutate()}
            loading={refreshMutation.isPending}
            data-testid="refresh-watched-prices"
          >
            <RefreshCw />
            {t('actions.refresh')}
          </Button>
          {portfolioCandidates.length > 0 && (
            <Button
              variant="outline"
              onClick={() => followPortfolioMutation.mutate()}
              loading={followPortfolioMutation.isPending}
              data-testid="follow-portfolio-stocks"
            >
              <Plus />
              {t('actions.followPortfolio', { count: portfolioCandidates.length })}
            </Button>
          )}
        </>
      }
    />
  );

  const dialogs = (
    <>
      <TargetPriceDialog
        target={target}
        onClose={() => setTarget(null)}
        onSave={(ticker, targetPrice, targetDirection) =>
          targetPriceMutation.mutateAsync({ ticker, targetPrice, targetDirection })
        }
        saving={targetPriceMutation.isPending}
      />
      <ConfirmDeleteDialog
        open={pendingUnfollow !== null}
        onOpenChange={(o) => !o && setPendingUnfollow(null)}
        title={t('unfollowConfirm.title', { ticker: pendingUnfollow?.ticker ?? '' })}
        description={t('unfollowConfirm.description')}
        confirmLabel={t('actions.unfollow')}
        isPending={unfollowMutation.isPending}
        onConfirm={() =>
          pendingUnfollow &&
          unfollowMutation.mutate(pendingUnfollow.ticker, {
            onSuccess: () => setPendingUnfollow(null),
          })
        }
      />
      <AddInvestmentModal
        open={buy !== null}
        onOpenChange={(open) => !open && setBuy(null)}
        initial={
          buy
            ? { ticker: buy.ticker, companyName: displayName(buy), currency: buy.currency }
            : undefined
        }
      />
    </>
  );

  if (isLoading) {
    return (
      <>
        {head}
        <Stats columns={3}>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      </>
    );
  }

  if (rows.length === 0) {
    return (
      <>
        {head}
        <EmptyState
          icon={<Eye />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <div className="flex flex-col items-center gap-4">
              {portfolioCandidates.length > 0 && (
                <Button
                  onClick={() => followPortfolioMutation.mutate()}
                  loading={followPortfolioMutation.isPending}
                >
                  <Plus />
                  {t('actions.followPortfolio', { count: portfolioCandidates.length })}
                </Button>
              )}
              <McpTip domain="watchlist" />
            </div>
          }
        />
        {dialogs}
      </>
    );
  }

  return (
    <>
      {head}

      <Stats columns={3}>
        <Stat
          label={t('stats.watched')}
          value={fmt.number(rows.length)}
          note={heldCount > 0 ? t('stats.heldNote', { count: heldCount }) : t('stats.heldNoteNone')}
        />
        <Stat
          label={t('stats.atTarget')}
          value={
            reached[0]
              ? displayName(reached[0].row)
              : pending[0]
                ? displayName(pending[0].row)
                : '—'
          }
          tone={reached[0] ? 'gain' : 'neutral'}
          noteTone={reached[0] ? 'gain' : 'neutral'}
          note={
            reached[0]
              ? [
                  t('stats.targetReached', {
                    price: price(reached[0].row.targetPrice, reached[0].row.currency),
                  }),
                  pending[0] ? `${pending[0].row.ticker} ${toTarget(pending[0])}` : null,
                ]
                  .filter(Boolean)
                  .join(' · ')
              : pending[0]
                ? toTarget(pending[0])
                : t('stats.noTargets')
          }
        />
        <Stat
          label={t('stats.topToday')}
          value={best ? displayName(best.row) : '—'}
          note={
            best ? (
              <span className="inline-flex flex-wrap items-center gap-x-1.5 gap-y-1">
                <ChangeBadge ratio={best.change.pct / 100} digits={2} />
                {worst && changes.length > 1 && (
                  <>
                    <span aria-hidden>·</span>
                    <ChangeNote
                      ratio={worst.change.pct / 100}
                      digits={2}
                      render={(change) =>
                        t('stats.worstNote', { name: displayName(worst.row), change })
                      }
                    />
                  </>
                )}
              </span>
            ) : (
              t('stats.noChanges')
            )
          }
        />
      </Stats>

      <Card variant="table">
        <CardHeader>
          <div>
            <CardTitle>{t('table.title')}</CardTitle>
            <CardDescription>{t('table.sub')}</CardDescription>
          </div>
          <InputWrap icon={<Search />} className="w-60">
            <Input
              className="h-[35px] text-table"
              placeholder={t('table.search')}
              aria-label={t('table.search')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              data-testid="input-search-watched"
            />
          </InputWrap>
        </CardHeader>
        <Table>
          <TableHeader>
            <TableRow>
              <SortHead column="name" align="left" className="w-[26%]">
                {t('table.stock')}
              </SortHead>
              {/* Prices are in each stock's own currency, so they don't sort against each other. */}
              <TableHead className="text-right">{t('table.price')}</TableHead>
              <SortHead column="change">{t('table.changeToday')}</SortHead>
              <SortHead column="range">{t('table.range')}</SortHead>
              <SortHead column="target">{t('table.target')}</SortHead>
              <SortHead column="held">{t('table.held')}</SortHead>
              <TableHead className="w-10" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {sorted.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={7} className="py-12">
                  <div className="grid place-items-center text-center text-ink-3">
                    <Search className="mb-3 size-7 text-ink-5" aria-hidden />
                    <h3 className="mb-1.5 text-h3 text-ink">{t('table.noResults.title')}</h3>
                    <p className="m-0 max-w-[320px] text-table">
                      {t('table.noResults.description')}
                    </p>
                  </div>
                </TableCell>
              </TableRow>
            )}
            {sorted.map((row) => {
              const change = dayChange(row.currentPrice, row.previousClose);
              const current = num(row.currentPrice);
              const low = num(row.fiftyTwoWeekLow);
              const high = num(row.fiftyTwoWeekHigh);
              const targetValue = num(row.targetPrice);
              const reachedTarget = targetReached(current, targetValue, row.targetDirection);
              return (
                <TableRow
                  key={row.id}
                  className="h-[60px] cursor-pointer"
                  onClick={() => setLocation(`/stock-monitor/${encodeURIComponent(row.ticker)}`)}
                  data-testid={`watched-row-${row.ticker}`}
                >
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px] text-ink">
                      <AssetLogo ticker={row.ticker} type="stock" />
                      <div className="min-w-0">
                        <b className="block truncate text-table font-650">{displayName(row)}</b>
                        <small className="mt-[3px] block text-micro font-500 text-ink-4">
                          {row.ticker}
                          {row.exchange ? ` · ${exchangeName(row.exchange)}` : ''}
                        </small>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    {price(row.currentPrice, row.currency)}
                  </TableCell>
                  <TableCell className="text-right num">
                    {change ? (
                      <>
                        <ChangeBadge ratio={change.pct / 100} digits={2} />
                        <small className="mt-[3px] block text-[10px] font-500 text-ink-4">
                          {formatNativePrice(
                            change.abs,
                            row.currency,
                            locale,
                            currencyCode,
                            undefined,
                            true
                          )}
                        </small>
                      </>
                    ) : (
                      <span className="text-ink-5">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    {low !== null && high !== null && current !== null ? (
                      <RangeWithLabels
                        low={low}
                        high={high}
                        current={current}
                        target={targetValue}
                        lowLabel={plain(low, row.currency)}
                        highLabel={plain(high, row.currency)}
                        label={`${t('table.range')} ${plain(low, row.currency)} – ${plain(high, row.currency)}`}
                      />
                    ) : (
                      <span className="text-ink-5">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right num">
                    {targetValue === null ? (
                      <button
                        type="button"
                        className="text-micro font-600 text-ink-4 underline-offset-[3px] hover:text-ink hover:underline"
                        onClick={(e) => {
                          e.stopPropagation();
                          openTarget(row);
                        }}
                      >
                        {t('table.setTarget')}
                      </button>
                    ) : reachedTarget ? (
                      <Badge variant="gain">
                        {t('table.targetReached', { price: price(row.targetPrice, row.currency) })}
                      </Badge>
                    ) : (
                      <>
                        <b className="font-650 text-ink">
                          <span aria-hidden>{row.targetDirection === 'below' ? '↓ ' : '↑ '}</span>
                          <span className="sr-only">
                            {t(
                              row.targetDirection === 'below'
                                ? 'table.waitsBelow'
                                : 'table.waitsAbove'
                            )}{' '}
                          </span>
                          {price(row.targetPrice, row.currency)}
                        </b>
                        {current !== null && (
                          <small className="mt-0.5 block text-[10px] font-500 text-ink-4">
                            {t('table.toTarget', {
                              pct: fmt.percent(Math.abs(targetValue / current - 1), 1),
                            })}
                          </small>
                        )}
                      </>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    {row.isHeld && row.heldInvestmentId ? (
                      <Link
                        href={`/stocks/${row.heldInvestmentId}`}
                        onClick={(e) => e.stopPropagation()}
                        title={t('actions.openInPortfolio')}
                      >
                        <Badge>{t('table.held')}</Badge>
                      </Link>
                    ) : (
                      <span className="text-ink-5">—</span>
                    )}
                  </TableCell>
                  <TableCell className="pr-3 text-right">
                    <div data-row-actions onClick={(e) => e.stopPropagation()}>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-label={tc('labels.moreActions')}
                          >
                            <Ellipsis />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => openTarget(row)}>
                            {targetValue === null
                              ? t('actions.setTarget')
                              : t('actions.editTarget')}
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            onSelect={() =>
                              setLocation(`/stock-monitor/${encodeURIComponent(row.ticker)}`)
                            }
                          >
                            {t('actions.notes')}
                          </DropdownMenuItem>
                          {!row.isHeld && (
                            <DropdownMenuItem onSelect={() => setBuy(row)}>
                              {t('actions.buy')}
                            </DropdownMenuItem>
                          )}
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            variant="danger"
                            onSelect={() => setPendingUnfollow(row)}
                          >
                            {t('actions.unfollow')}
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
        <div className="flex items-center justify-between gap-4 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
          <span>{t('table.footer', { count: sorted.length })}</span>
          <span>{t('table.source')}</span>
        </div>
      </Card>

      {dialogs}
    </>
  );
}
