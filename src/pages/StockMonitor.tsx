import { useMemo, useState } from 'react';
import { Link, useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Eye, Plus, RefreshCw } from 'lucide-react';
import type { WatchedStockRow } from '@shared/schema';
import { currencyDecimals } from '@shared/currencies';
import { exchangeName } from '@/utils/exchange-names';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { usePortfolioFollowCandidates, useWatchedStocks } from '@/hooks/use-stock-monitor';
import { useStockMonitorMutations } from '@/hooks/use-stock-monitor-mutations';
import {
  dayChange,
  formatNativePrice,
  latestFetchedAt,
  targetDistance,
} from '@/utils/stock-monitor';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Segmented } from '@/components/ui/segmented';
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
import { AssetLogo } from '@/components/common/AssetLogo';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StockSearchCombobox } from '@/components/stock-monitor/StockSearchCombobox';
import { RangeWithLabels } from '@/components/stock-monitor/RangeBar';
import {
  TargetPriceDialog,
  type TargetPriceTarget,
} from '@/components/stock-monitor/TargetPriceDialog';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { cn } from '@/lib/utils';

type SortKey = 'name' | 'change' | 'target';

const num = (v: string | null) => {
  const n = parseFloat(v ?? '');
  return isFinite(n) ? n : null;
};
const displayName = (row: WatchedStockRow) => row.shortName ?? row.longName ?? row.ticker;

/**
 * Watchlist (design system §7 Watchlist, prototype `stock-monitor.html`):
 * native-currency prices, day change against the previous close, the 52-week
 * range with the current price and the target tick, target state, three stats
 * and a wide search that starts following a stock.
 */
export default function StockMonitor() {
  const { t } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const locale = fmt.locale;
  const [, setLocation] = useLocation();
  const { data: rows = [], isLoading } = useWatchedStocks();
  const { data: portfolioCandidates = [] } = usePortfolioFollowCandidates();
  const { unfollowMutation, refreshMutation, followPortfolioMutation, targetPriceMutation } =
    useStockMonitorMutations();

  const [sort, setSort] = useState<SortKey>('name');
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
    return n === null ? '—' : formatNativePrice(n, currency, locale);
  };
  const plain = (n: number, currency: string | null) => {
    const d = currencyDecimals(currency);
    return fmt.number(n, { minimumFractionDigits: d, maximumFractionDigits: d });
  };

  const sorted = useMemo(() => {
    const list = [...rows];
    if (sort === 'name') {
      list.sort((a, b) => displayName(a).localeCompare(displayName(b), locale));
    } else if (sort === 'change') {
      list.sort(
        (a, b) =>
          (dayChange(b.currentPrice, b.previousClose)?.pct ?? -Infinity) -
          (dayChange(a.currentPrice, a.previousClose)?.pct ?? -Infinity)
      );
    } else {
      const dist = (r: WatchedStockRow) => {
        const d = targetDistance(r.currentPrice, r.targetPrice);
        return d === null ? Infinity : Math.abs(d);
      };
      list.sort((a, b) => dist(a) - dist(b));
    }
    return list;
  }, [rows, sort, locale]);

  // Stats
  const heldCount = rows.filter((r) => r.isHeld).length;
  const withTarget = rows
    .map((r) => ({ row: r, current: num(r.currentPrice), target: num(r.targetPrice) }))
    .filter(
      (x): x is { row: WatchedStockRow; current: number; target: number } =>
        x.current !== null && x.target !== null && x.target > 0
    );
  const reached = withTarget.filter((x) => x.current >= x.target);
  const pending = withTarget
    .filter((x) => x.current < x.target)
    .sort((a, b) => a.target / a.current - b.target / b.current);
  const changes = rows
    .map((r) => ({ row: r, change: dayChange(r.currentPrice, r.previousClose) }))
    .filter(
      (x): x is { row: WatchedStockRow; change: NonNullable<typeof x.change> } => x.change !== null
    )
    .sort((a, b) => b.change.pct - a.change.pct);
  const best = changes[0];
  const worst = changes[changes.length - 1];
  const pct = (v: number) => fmt.percent(v / 100, 1, { signed: true });
  const toTarget = (x: { row: WatchedStockRow; current: number; target: number }) =>
    t('stats.toTarget', { pct: fmt.percent(x.target / x.current - 1, 1) });

  const openTarget = (row: WatchedStockRow) =>
    setTarget({
      ticker: row.ticker,
      currency: row.currency,
      currentPrice: row.currentPrice,
      fiftyTwoWeekHigh: row.fiftyTwoWeekHigh,
      targetPrice: row.targetPrice,
    });

  const head = (
    <PageHead
      eyebrow={t('eyebrow')}
      title={t('title')}
      description={t('subtitle')}
      actions={
        <>
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
        onSave={(ticker, targetPrice) => targetPriceMutation.mutateAsync({ ticker, targetPrice })}
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
        <StockSearchCombobox className="mb-5" />
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
        <StockSearchCombobox className="mb-5" />
        <EmptyState
          icon={<Eye />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            portfolioCandidates.length > 0 ? (
              <Button
                onClick={() => followPortfolioMutation.mutate()}
                loading={followPortfolioMutation.isPending}
              >
                <Plus />
                {t('actions.followPortfolio', { count: portfolioCandidates.length })}
              </Button>
            ) : undefined
          }
        />
        {dialogs}
      </>
    );
  }

  return (
    <>
      {head}
      <StockSearchCombobox className="mb-5" />

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
          value={best ? `${displayName(best.row)} ${pct(best.change.pct)}` : '—'}
          tone={best ? (best.change.pct >= 0 ? 'gain' : 'loss') : 'neutral'}
          noteTone="neutral"
          note={
            worst && changes.length > 1
              ? t('stats.worstNote', {
                  name: displayName(worst.row),
                  change: pct(worst.change.pct),
                })
              : t('stats.noChanges')
          }
        />
      </Stats>

      <Card variant="table">
        <CardHeader>
          <div>
            <CardTitle>{t('table.title')}</CardTitle>
            <CardDescription>{t('table.sub')}</CardDescription>
          </div>
          <Segmented
            value={sort}
            onValueChange={setSort}
            options={[
              { value: 'name', label: t('table.sort.name') },
              { value: 'change', label: t('table.sort.change') },
              { value: 'target', label: t('table.sort.target') },
            ]}
          />
        </CardHeader>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[26%]">{t('table.stock')}</TableHead>
              <TableHead className="text-right">{t('table.price')}</TableHead>
              <TableHead className="text-right">{t('table.changeToday')}</TableHead>
              <TableHead className="text-right">{t('table.range')}</TableHead>
              <TableHead className="text-right">{t('table.target')}</TableHead>
              <TableHead className="text-right">{t('table.held')}</TableHead>
              <TableHead className="w-10" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {sorted.map((row) => {
              const change = dayChange(row.currentPrice, row.previousClose);
              const current = num(row.currentPrice);
              const low = num(row.fiftyTwoWeekLow);
              const high = num(row.fiftyTwoWeekHigh);
              const targetValue = num(row.targetPrice);
              const reachedTarget =
                current !== null && targetValue !== null && current >= targetValue;
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
                        <b className={cn('font-650', change.abs >= 0 ? 'text-gain' : 'text-loss')}>
                          {fmt.percent(change.pct / 100, 1, { signed: true })}
                        </b>
                        <small className="mt-0.5 block text-[10px] font-500 text-ink-4">
                          {formatNativePrice(change.abs, row.currency, locale, undefined, true)}
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
                        <b className="font-650 text-ink">{price(row.targetPrice, row.currency)}</b>
                        {current !== null && (
                          <small className="mt-0.5 block text-[10px] font-500 text-ink-4">
                            {t('table.toTarget', {
                              pct: fmt.percent(targetValue / current - 1, 1),
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
          <span>{t('table.footer', { count: rows.length })}</span>
          <span>{t('table.source')}</span>
        </div>
      </Card>

      {dialogs}
    </>
  );
}
