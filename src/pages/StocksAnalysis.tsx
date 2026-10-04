import { useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Plus, Search, Tag, TrendingUp } from 'lucide-react';
import type {
  StockInvestmentWithTags,
  StockTag,
  StockTagGroup,
  TagMetrics,
  TwrSeries,
} from '@shared/schema';
import { investmentsApi, stockTagsApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Chip, ChipLabel } from '@/components/ui/chip';
import { Segmented } from '@/components/ui/segmented';
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
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { AssetLogo } from '@/components/common/AssetLogo';
import { AllocationRing } from '@/components/dashboard/AllocationRing';
import { MoonyLinesChart, type LineSeries } from '@/components/charts/MoonyLinesChart';
import { ManageTagsDialog } from '@/components/stocks-analysis/ManageTagsDialog';
import { AssignTagsDialog } from '@/components/stocks-analysis/AssignTagsDialog';
import { cn } from '@/lib/utils';

const PERIODS = ['3m', 'ytd', '1y'] as const;
type Period = (typeof PERIODS)[number];
const UNGROUPED = '__ungrouped__';
const ALL = 'all';
const DAY = 86_400;

/** UTC-midnight bounds of a TWR period ending today. */
function periodBounds(period: Period): { from: number; to: number } {
  const now = new Date();
  const y = now.getUTCFullYear();
  const m = now.getUTCMonth();
  const d = now.getUTCDate();
  const to = Date.UTC(y, m, d) / 1000;
  const from =
    period === 'ytd'
      ? Date.UTC(y, 0, 1) / 1000
      : period === '3m'
        ? Date.UTC(y, m - 3, d) / 1000
        : Date.UTC(y - 1, m, d) / 1000;
  return { from: Math.min(from, to - DAY), to };
}

interface FilterGroup {
  key: string;
  label: string;
  tags: StockTag[];
}

/**
 * Stock analysis (design system §7 Analysis, prototype `stocks-analysis.html`):
 * four stats, chip filters per tag group, allocation ring by the chosen
 * group, TWR per tag with the portfolio as a dashed reference, the tag table
 * and the holdings with their tags; manage and assign modals.
 */
export default function StocksAnalysis() {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { formatCurrency, formatCurrencyShort } = useCurrency();
  const fmt = useFormat();

  const [filters, setFilters] = useState<Record<string, string>>({});
  const [query, setQuery] = useState('');
  const [period, setPeriod] = useState<Period>('ytd');
  const [chartGroupChoice, setChartGroupChoice] = useState<string | null>(null);
  const [manageOpen, setManageOpen] = useState(false);
  const [assign, setAssign] = useState<{ open: boolean; stockIds: string[] }>({
    open: false,
    stockIds: [],
  });

  const { data: tags = [], isLoading: tagsLoading } = useQuery<StockTag[]>({
    queryKey: ['stock-tags'],
    queryFn: stockTagsApi.getAll,
  });
  const { data: groups = [], isLoading: groupsLoading } = useQuery<StockTagGroup[]>({
    queryKey: ['stock-tag-groups'],
    queryFn: stockTagsApi.getAllGroups,
  });
  const { data: stocks = [], isLoading: stocksLoading } = useQuery<StockInvestmentWithTags[]>({
    queryKey: ['stocks-analysis'],
    queryFn: stockTagsApi.getAnalysis,
  });
  const { data: metrics = [], isLoading: metricsLoading } = useQuery<TagMetrics[]>({
    queryKey: ['tag-metrics', []],
    queryFn: () => stockTagsApi.getTagMetrics([]),
  });

  // Tag groups for the filters and the charts; ungrouped tags form a group of their own
  const filterGroups = useMemo<FilterGroup[]>(() => {
    const sorted = [...groups].sort((a, b) => a.createdAt - b.createdAt);
    const list: FilterGroup[] = sorted
      .map((g) => ({ key: g.id, label: g.name, tags: tags.filter((x) => x.groupId === g.id) }))
      .filter((g) => g.tags.length > 0);
    const ungrouped = tags.filter((x) => !x.groupId || !groups.some((g) => g.id === x.groupId));
    if (ungrouped.length > 0) {
      list.push({ key: UNGROUPED, label: t('stocksAnalysis.filters.ungrouped'), tags: ungrouped });
    }
    return list;
  }, [groups, tags, t]);

  const chartGroup =
    filterGroups.find((g) => g.key === chartGroupChoice) ?? filterGroups[0] ?? null;
  const metricOf = (tagId: string) => metrics.find((m) => m.tag.id === tagId);

  // Up to four tags of the chart group, largest first, for the ring and the TWR lines
  const chartTags = useMemo(() => {
    if (!chartGroup) return [];
    return [...chartGroup.tags]
      .sort((a, b) => (metricOf(b.id)?.totalValue ?? 0) - (metricOf(a.id)?.totalValue ?? 0))
      .slice(0, 4);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chartGroup, metrics]);

  const bounds = useMemo(() => periodBounds(period), [period]);
  const chartTagIds = chartTags.map((x) => x.id);
  const { data: twr = [], isLoading: twrLoading } = useQuery<TwrSeries[]>({
    queryKey: ['stock-twr', chartTagIds, true, false, bounds.from, bounds.to],
    queryFn: () => investmentsApi.getStockTwr(chartTagIds, true, false, bounds.from, bounds.to),
    enabled: stocks.length > 0,
  });

  // TWR as an index (100 = start of the period) on a shared date grid
  const twrChart = useMemo(() => {
    if (twr.length === 0) return null;
    const dates = Array.from(new Set(twr.flatMap((s) => s.data.map((p) => p.date)))).sort();
    const series: LineSeries[] = [];
    for (const tag of chartTags) {
      const s = twr.find((x) => x.tag?.id === tag.id);
      if (!s) continue;
      const byDate = new Map(s.data.map((p) => [p.date, p.twr]));
      series.push({
        id: tag.id,
        name: tag.name,
        values: dates.map((d) => (byDate.has(d) ? 100 + (byDate.get(d) ?? 0) : null)),
      });
    }
    const portfolio = twr.find((x) => !x.tag && !x.isUntagged);
    if (portfolio) {
      const byDate = new Map(portfolio.data.map((p) => [p.date, p.twr]));
      series.push({
        id: 'portfolio',
        name: t('stocksAnalysis.twr.portfolio'),
        values: dates.map((d) => (byDate.has(d) ? 100 + (byDate.get(d) ?? 0) : null)),
        dashed: true,
      });
    }
    const dateTs = (d: string) =>
      Date.UTC(+d.slice(0, 4), +d.slice(5, 7) - 1, +d.slice(8, 10)) / 1000;
    const labelCount = Math.min(6, dates.length);
    const axisLabels = Array.from({ length: labelCount }, (_, i) => {
      if (i === labelCount - 1) return tc('charts.today');
      const idx = Math.round((i * (dates.length - 1)) / Math.max(1, labelCount - 1));
      return fmt.day(dateTs(dates[idx]), { month: 'short' });
    });
    return { series, dates, axisLabels, dateTs };
  }, [twr, chartTags, t, tc, fmt]);

  // Derived figures
  const totalValue = stocks.reduce((s, x) => s + x.currentValue, 0);
  const taggedStocks = stocks.filter((x) => x.tags.length > 0);
  const untaggedStocks = stocks.filter((x) => x.tags.length === 0);
  const totalDividends = stocks.reduce((s, x) => s + x.dividendYield, 0);
  const taggedDividends = taggedStocks.reduce((s, x) => s + x.dividendYield, 0);
  const best = metrics
    .filter((m) => m.holdingsCount > 0)
    .sort((a, b) => b.gainLossPercent - a.gainLossPercent)[0];

  // Ring: each position's value split across its tags in the chart group; the rest is untagged
  const ringSegments = useMemo(() => {
    if (!chartGroup) return { segments: [], untagged: totalValue };
    const byTag = new Map<string, number>(chartGroup.tags.map((x) => [x.id, 0]));
    let untagged = 0;
    for (const s of stocks) {
      const own = s.tags.filter((x) => byTag.has(x.id));
      if (own.length === 0) untagged += s.currentValue;
      else
        for (const x of own) byTag.set(x.id, (byTag.get(x.id) ?? 0) + s.currentValue / own.length);
    }
    return {
      segments: chartGroup.tags.map((x) => ({
        key: x.id,
        label: x.name,
        value: byTag.get(x.id) ?? 0,
      })),
      untagged,
    };
  }, [chartGroup, stocks, totalValue]);

  // Holdings filtered by the chips and the search
  const filtered = useMemo(() => {
    const q = query.trim().toLocaleLowerCase(fmt.locale);
    return stocks.filter((s) => {
      for (const g of filterGroups) {
        const chosen = filters[g.key];
        if (chosen && chosen !== ALL && !s.tags.some((x) => x.id === chosen)) return false;
      }
      if (q && !`${s.companyName} ${s.ticker}`.toLocaleLowerCase(fmt.locale).includes(q))
        return false;
      return true;
    });
  }, [stocks, filterGroups, filters, query, fmt.locale]);
  const filteredValue = filtered.reduce((s, x) => s + x.currentValue, 0);

  const money = (v: number) => formatCurrency(v);
  const gainText = (pct: number) => (
    <b className={cn('font-650', pct >= 0 ? 'text-gain' : 'text-loss')}>
      {fmt.percent(pct / 100, 1, { signed: true })}
    </b>
  );
  const openAssign = (stockIds: string[]) => setAssign({ open: true, stockIds });

  const head = (
    <PageHead
      eyebrow={t('stocksAnalysis.eyebrow')}
      title={t('stocksAnalysis.title')}
      description={t('stocksAnalysis.subtitle')}
      actions={
        <>
          <Button variant="outline" onClick={() => setManageOpen(true)}>
            <Tag />
            {t('stocksAnalysis.actions.manage')}
          </Button>
          <Button onClick={() => openAssign([])} disabled={stocks.length === 0}>
            <Plus />
            {t('stocksAnalysis.actions.assign')}
          </Button>
        </>
      }
    />
  );

  const dialogs = (
    <>
      <ManageTagsDialog
        open={manageOpen}
        onOpenChange={setManageOpen}
        tags={tags}
        groups={groups}
      />
      <AssignTagsDialog
        open={assign.open}
        onOpenChange={(open) => setAssign((a) => ({ ...a, open }))}
        stocks={stocks}
        tags={tags}
        initialStockIds={assign.stockIds}
      />
    </>
  );

  if (tagsLoading || groupsLoading || stocksLoading || metricsLoading) {
    return (
      <>
        {head}
        <Stats>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      </>
    );
  }

  if (stocks.length === 0) {
    return (
      <>
        {head}
        <EmptyState
          icon={<TrendingUp />}
          title={t('stocksAnalysis.empty.title')}
          description={t('stocksAnalysis.empty.description')}
          action={
            <Button asChild>
              <Link href="/stocks">{t('stocksAnalysis.empty.action')}</Link>
            </Button>
          }
        />
        {dialogs}
      </>
    );
  }

  return (
    <>
      {head}

      <Stats>
        <Stat
          label={t('stocksAnalysis.stats.value')}
          value={money(totalValue)}
          note={`${t('stocksAnalysis.stats.positions', { count: stocks.length })} · ${t('stocksAnalysis.stats.tagged', { count: taggedStocks.length })}`}
        />
        <Stat
          label={t('stocksAnalysis.stats.bestTag')}
          value={best ? best.tag.name : '—'}
          tone={best ? (best.gainLossPercent >= 0 ? 'gain' : 'loss') : 'neutral'}
          note={
            best
              ? `${fmt.percent(best.gainLossPercent / 100, 1, { signed: true })} · ${t('stocksAnalysis.stats.positions', { count: best.holdingsCount })}`
              : t('stocksAnalysis.stats.noTags')
          }
        />
        <Stat
          label={t('stocksAnalysis.stats.dividends')}
          value={money(taggedDividends)}
          note={
            totalDividends > 0
              ? t('stocksAnalysis.stats.dividendsNote', {
                  percent: fmt.percent(taggedDividends / totalDividends, 0),
                })
              : t('stocksAnalysis.stats.noDividends')
          }
        />
        <Stat
          label={t('stocksAnalysis.stats.untagged')}
          value={fmt.number(untaggedStocks.length)}
          note={
            untaggedStocks.length > 0 ? (
              <button
                type="button"
                className="font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
                onClick={() => openAssign(untaggedStocks.map((s) => s.id))}
              >
                {t('stocksAnalysis.stats.assignLink')} →
              </button>
            ) : (
              t('stocksAnalysis.stats.allTagged')
            )
          }
        />
      </Stats>

      {filterGroups.length > 0 && (
        <div className="mb-[18px] flex flex-wrap gap-5">
          {filterGroups.map((g) => {
            const chosen = filters[g.key] ?? ALL;
            return (
              <div key={g.key} className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1 text-eyebrow uppercase text-ink-4">{g.label}</span>
                <Chip
                  active={chosen === ALL}
                  onClick={() => setFilters((f) => ({ ...f, [g.key]: ALL }))}
                >
                  {t('stocksAnalysis.filters.all')}
                </Chip>
                {g.tags.map((x) => (
                  <Chip
                    key={x.id}
                    active={chosen === x.id}
                    onClick={() => setFilters((f) => ({ ...f, [g.key]: x.id }))}
                  >
                    {x.name}
                  </Chip>
                ))}
              </div>
            );
          })}
        </div>
      )}

      <div className="mb-[18px] grid grid-cols-2 items-start gap-[18px]">
        <Card>
          <CardHeader className="pb-1">
            <div>
              <CardTitle className="text-[16px]">{t('stocksAnalysis.allocation.title')}</CardTitle>
              <CardDescription>
                {chartGroup
                  ? t('stocksAnalysis.allocation.sub', { group: chartGroup.label })
                  : t('stocksAnalysis.allocation.subNoGroup')}
              </CardDescription>
            </div>
            {filterGroups.length > 1 && (
              <Segmented
                value={chartGroup?.key ?? ''}
                onValueChange={setChartGroupChoice}
                options={filterGroups.map((g) => ({ value: g.key, label: g.label }))}
                aria-label={t('stocksAnalysis.allocation.groupSwitch')}
              />
            )}
          </CardHeader>
          <div className="px-[21px] pb-5 pt-1">
            <AllocationRing
              segments={ringSegments.segments}
              otherSegment={{
                key: 'untagged',
                label: t('stocksAnalysis.allocation.untagged'),
                value: ringSegments.untagged,
              }}
              centerLabel={t('stocksAnalysis.allocation.center')}
              centerValue={formatCurrencyShort(totalValue)}
              otherLabel={t('stocksAnalysis.allocation.other')}
              formatValue={money}
              formatPercent={(r) => fmt.percent(r, 0)}
            />
          </div>
        </Card>

        <Card>
          <CardHeader className="pb-0">
            <div>
              <CardTitle className="text-[16px]">{t('stocksAnalysis.twr.title')}</CardTitle>
              <CardDescription>{t('stocksAnalysis.twr.sub')}</CardDescription>
            </div>
            <Segmented
              value={period}
              onValueChange={setPeriod}
              options={PERIODS.map((p) => ({
                value: p,
                label: t(`stocksAnalysis.twr.period.${p}`),
              }))}
              aria-label={tc('periods.label')}
            />
          </CardHeader>
          <div className="px-[21px] pb-4">
            {twrChart && twrChart.series.length > 0 ? (
              <MoonyLinesChart
                className="-mx-1 mt-[14px]"
                series={twrChart.series}
                axisLabels={twrChart.axisLabels}
                height={190}
                formatValue={(v) => fmt.number(v, { maximumFractionDigits: 1 })}
                formatTick={(v) => fmt.number(v, { maximumFractionDigits: 0 })}
                tipLabel={(i) => fmt.day(twrChart.dateTs(twrChart.dates[i] ?? twrChart.dates[0]))}
              />
            ) : (
              <p className="grid h-[190px] place-items-center text-table text-ink-3">
                {twrLoading ? tc('status.loading') : t('stocksAnalysis.twr.empty')}
              </p>
            )}
          </div>
        </Card>
      </div>

      <Card variant="table" className="mb-[18px]">
        <CardHeader>
          <div>
            <CardTitle>{t('stocksAnalysis.tagsTable.title')}</CardTitle>
            <CardDescription>{t('stocksAnalysis.tagsTable.sub')}</CardDescription>
          </div>
        </CardHeader>
        {metrics.length === 0 ? (
          <EmptyState
            bare
            icon={<Tag />}
            title={t('stocksAnalysis.tagsTable.empty.title')}
            description={t('stocksAnalysis.tagsTable.empty.description')}
            action={
              <Button variant="outline" onClick={() => setManageOpen(true)}>
                <Tag />
                {t('stocksAnalysis.actions.manage')}
              </Button>
            }
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[30%]">{t('stocksAnalysis.tagsTable.tag')}</TableHead>
                <TableHead className="text-right">
                  {t('stocksAnalysis.tagsTable.positions')}
                </TableHead>
                <TableHead className="text-right">{t('stocksAnalysis.tagsTable.value')}</TableHead>
                <TableHead className="text-right">{t('stocksAnalysis.tagsTable.share')}</TableHead>
                <TableHead className="text-right">{t('stocksAnalysis.tagsTable.gain')}</TableHead>
                <TableHead className="text-right">
                  {t('stocksAnalysis.tagsTable.dividend')}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {metrics.map((m) => {
                const group = groups.find((g) => g.id === m.tag.groupId);
                const has = m.holdingsCount > 0;
                return (
                  <TableRow key={m.tag.id} className="h-12">
                    <TableCell>
                      <span className="inline-flex items-center gap-1.5">
                        <ChipLabel>{m.tag.name}</ChipLabel>
                        {group && (
                          <span className="text-[10px] font-500 text-ink-4">{group.name}</span>
                        )}
                      </span>
                    </TableCell>
                    <TableCell className="text-right num">{fmt.number(m.holdingsCount)}</TableCell>
                    <TableCell className="text-right font-650 text-ink num">
                      {has ? money(m.totalValue) : <span className="text-ink-5">—</span>}
                    </TableCell>
                    <TableCell className="text-right num">
                      {has ? fmt.percent(m.portfolioPercent / 100, 0) : '—'}
                    </TableCell>
                    <TableCell className="text-right num">
                      {has ? gainText(m.gainLossPercent) : '—'}
                    </TableCell>
                    <TableCell className="text-right num">
                      {m.estimatedYearlyDividend > 0 ? (
                        money(m.estimatedYearlyDividend)
                      ) : (
                        <span className="text-ink-5">—</span>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </Card>

      <Card variant="table">
        <CardHeader>
          <div>
            <CardTitle>{t('stocksAnalysis.holdings.title')}</CardTitle>
            <CardDescription>
              {t('stocksAnalysis.holdings.sub', {
                shown: filtered.length,
                total: stocks.length,
                amount: money(filteredValue),
                percent: fmt.percent(totalValue > 0 ? filteredValue / totalValue : 0, 0),
              })}
            </CardDescription>
          </div>
          <InputWrap icon={<Search />} className="w-[220px]">
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('stocksAnalysis.holdings.search')}
              aria-label={t('stocksAnalysis.holdings.search')}
            />
          </InputWrap>
        </CardHeader>
        {filtered.length === 0 ? (
          <EmptyState
            bare
            icon={<Tag />}
            title={t('stocksAnalysis.holdings.emptyFilter.title')}
            description={t('stocksAnalysis.holdings.emptyFilter.description')}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[34%]">{t('stocksAnalysis.holdings.stock')}</TableHead>
                <TableHead>{t('stocksAnalysis.holdings.tags')}</TableHead>
                <TableHead className="text-right">{t('stocksAnalysis.holdings.value')}</TableHead>
                <TableHead className="w-[12%] text-right">
                  {t('stocksAnalysis.holdings.gain')}
                </TableHead>
                <TableHead className="w-10" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {filtered.map((s) => (
                <TableRow key={s.id} className="h-[58px]">
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px] text-ink">
                      <AssetLogo ticker={s.ticker} type="stock" />
                      <div className="min-w-0">
                        <b className="block truncate text-table font-650">{s.companyName}</b>
                        <small className="mt-[3px] block text-micro font-500 text-ink-4">
                          {s.ticker}
                        </small>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex flex-wrap items-center gap-1.5">
                      {s.tags.length > 0 ? (
                        s.tags.map((x) => <Badge key={x.id}>{x.name}</Badge>)
                      ) : (
                        <>
                          <Badge variant="outline">{t('stocksAnalysis.holdings.noTag')}</Badge>
                          <button
                            type="button"
                            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
                            onClick={() => openAssign([s.id])}
                          >
                            {t('stocksAnalysis.holdings.assign')}
                          </button>
                        </>
                      )}
                    </div>
                  </TableCell>
                  <TableCell className="text-right font-650 text-ink num">
                    {money(s.currentValue)}
                  </TableCell>
                  <TableCell className="text-right num">{gainText(s.gainLossPercent)}</TableCell>
                  <TableCell className="pr-3 text-right">
                    <div data-row-actions>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={t('stocksAnalysis.holdings.editTags')}
                        onClick={() => openAssign([s.id])}
                      >
                        <Tag />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>

      {dialogs}
    </>
  );
}
