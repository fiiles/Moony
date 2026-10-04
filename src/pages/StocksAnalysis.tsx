import { useMemo, useRef, useState } from 'react';
import { Link } from 'wouter';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
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
import { useMediaQuery } from '@/hooks/use-media-query';
import { FILTER_ALL, chosenTagIds, selectPositions, tagTotals } from '@/utils/stock-analysis';
import { twrPercent, twrTickDigits } from '@/utils/twr';
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
const DAY = 86_400;
/**
 * From this viewport width the allocation ring sits beside its legend (`lg`: 184 px ring, 36 px
 * gap). The legend then keeps 230 px, as on the dashboard: (1360 − 264 sidebar − 96 padding − 18
 * gap) / 2 − 42 card padding − 220. Narrower, the ring (`md`) has its legend under it. The same
 * width decides how many lines the TWR legend keeps room for.
 */
const WIDE_QUERY = '(min-width: 1360px)';
/**
 * Narrow window: the ring card keeps the height of a full stacked ring, so a selection that drops
 * a legend row (or all of them) does not resize it. Body padding 12 + 20, ring 140, gap 16, five
 * legend rows of 32.6 (the most it shows) minus the last border.
 */
const RING_BODY_MIN_HEIGHT = 12 + 20 + 140 + 16 + 5 * 32.6 - 1;
const TWR_CHART_HEIGHT = 190;
/**
 * The TWR chart block: plot, date row (20), then the legend with its gap (10) and room for
 * `rows` lines (14 px, 6 px apart) — what the card keeps while the chart is empty. Keep in step
 * with MoonyLinesChart.
 */
const twrBlockHeight = (rows: number) => TWR_CHART_HEIGHT + 20 + 10 + rows * 14 + (rows - 1) * 6;

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
 * group, TWR per tag with the whole portfolio as the dashed reference, the tag
 * table and the holdings with their tags; manage and assign modals. The chips
 * filter everything below them (ring, TWR tags, tag table, holdings); the stats
 * and the reference line stay portfolio-wide.
 */
export default function StocksAnalysis() {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { formatCurrency, formatCurrencyShort } = useCurrency();
  const fmt = useFormat();
  const wide = useMediaQuery(WIDE_QUERY);
  // Five series (four tags and the portfolio) need three lines of legend in a narrow card
  const legendRows = wide ? 2 : 3;

  const [filters, setFilters] = useState<Record<string, string>>({});
  const chipsRef = useRef<HTMLDivElement>(null);
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

  // The chips narrow the analysis: the selected positions feed the ring, the TWR tag lines, the
  // tag table and the holdings. The stats row and the TWR reference stay portfolio-wide.
  const chosenIds = useMemo(() => chosenTagIds(filterGroups, filters), [filterGroups, filters]);
  const filterActive = chosenIds.length > 0;
  const selected = useMemo(() => selectPositions(stocks, chosenIds), [stocks, chosenIds]);
  const selectedIds = useMemo(() => selected.map((s) => s.id), [selected]);
  const selectedValue = selected.reduce((sum, x) => sum + x.currentValue, 0);
  const selectionText = t('stocksAnalysis.selection', {
    shown: selected.length,
    total: stocks.length,
  });

  // Ring: each selected position's value split across its tags in the chart group; the rest is untagged
  const ringSegments = useMemo(() => {
    if (!chartGroup) return { segments: [], untagged: selectedValue };
    const byTag = new Map<string, number>(chartGroup.tags.map((x) => [x.id, 0]));
    let untagged = 0;
    for (const s of selected) {
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
  }, [chartGroup, selected, selectedValue]);

  // Up to four tags of the chart group that have a selected position, largest first (the ring's
  // order, so a tag keeps its colour in both charts), for the TWR lines
  const chartTags = useMemo(() => {
    if (!chartGroup) return [];
    const valueOf = new Map(ringSegments.segments.map((x) => [x.key, x.value]));
    const carried = new Set(selected.flatMap((s) => s.tags.map((x) => x.id)));
    return chartGroup.tags
      .filter((x) => carried.has(x.id))
      .sort((a, b) => (valueOf.get(b.id) ?? 0) - (valueOf.get(a.id) ?? 0))
      .slice(0, 4);
  }, [chartGroup, ringSegments, selected]);

  const bounds = useMemo(() => periodBounds(period), [period]);
  const chartTagIds = chartTags.map((x) => x.id);
  const {
    data: twr = [],
    isLoading: twrLoading,
    isPlaceholderData: twrStale,
  } = useQuery<TwrSeries[]>({
    queryKey: [
      'stock-twr',
      chartTagIds,
      true,
      false,
      filterActive ? selectedIds : null,
      bounds.from,
      bounds.to,
    ],
    queryFn: () =>
      investmentsApi.getStockTwr(
        chartTagIds,
        true,
        false,
        bounds.from,
        bounds.to,
        filterActive ? selectedIds : undefined
      ),
    enabled: stocks.length > 0,
    // Keep the chart (and the card's height) while the next period or selection loads
    placeholderData: keepPreviousData,
  });

  // TWR as an index (100 = start of the period) on a shared date grid; the whole portfolio is the
  // reference line and its return for the period goes to the card head. The tag lines come from
  // the payload itself, so while the previous one stays up (dimmed) it is never mixed with the
  // tags of the next selection.
  const twrChart = useMemo(() => {
    const dates = Array.from(new Set(twr.flatMap((s) => s.data.map((p) => p.date)))).sort();
    if (dates.length < 2) return null;
    const indexOn = (s: TwrSeries) => {
      const byDate = new Map(s.data.map((p) => [p.date, p.twr]));
      return dates.map((d) => (byDate.has(d) ? 100 + (byDate.get(d) ?? 0) : null));
    };
    const series: LineSeries[] = [];
    for (const s of twr) {
      if (s.tag) series.push({ id: s.tag.id, name: s.tag.name, values: indexOn(s) });
    }
    const portfolio = twr.find((x) => !x.tag && !x.isUntagged);
    let portfolioReturn: number | null = null;
    if (portfolio) {
      series.push({
        id: 'portfolio',
        name: t('stocksAnalysis.twr.portfolio'),
        values: indexOn(portfolio),
        reference: true,
      });
      const last = portfolio.data[portfolio.data.length - 1];
      if (last) portfolioReturn = twrPercent(100 + last.twr);
    }
    if (series.length === 0) return null;
    const dateTs = (d: string) =>
      Date.UTC(+d.slice(0, 4), +d.slice(5, 7) - 1, +d.slice(8, 10)) / 1000;
    const labelCount = Math.min(6, dates.length);
    const axisLabels = Array.from({ length: labelCount }, (_, i) => {
      if (i === labelCount - 1) return tc('charts.today');
      const idx = Math.round((i * (dates.length - 1)) / Math.max(1, labelCount - 1));
      return fmt.day(dateTs(dates[idx]), { month: 'short' });
    });
    return { series, dates, axisLabels, dateTs, portfolioReturn };
  }, [twr, t, tc, fmt]);

  // Derived figures (the stats stay portfolio-wide)
  const totalValue = stocks.reduce((s, x) => s + x.currentValue, 0);
  const taggedStocks = stocks.filter((x) => x.tags.length > 0);
  const untaggedStocks = stocks.filter((x) => x.tags.length === 0);
  const totalDividends = stocks.reduce((s, x) => s + x.dividendYield, 0);
  const taggedDividends = taggedStocks.reduce((s, x) => s + x.dividendYield, 0);
  const best = metrics
    .filter((m) => m.holdingsCount > 0)
    .sort((a, b) => b.gainLossPercent - a.gainLossPercent)[0];

  // Tag table: the totals of the selected positions per tag; with a filter, only tags that have one
  const tagRows = useMemo(
    () =>
      metrics
        .map((m) => ({ tag: m.tag, totals: tagTotals(selected, m.tag.id) }))
        .filter((row) => !filterActive || row.totals.count > 0),
    [metrics, selected, filterActive]
  );

  // Holdings: the selected positions, narrowed further by the search
  const filtered = useMemo(() => {
    const q = query.trim().toLocaleLowerCase(fmt.locale);
    if (!q) return selected;
    return selected.filter((s) =>
      `${s.companyName} ${s.ticker}`.toLocaleLowerCase(fmt.locale).includes(q)
    );
  }, [selected, query, fmt.locale]);
  const filteredValue = filtered.reduce((s, x) => s + x.currentValue, 0);

  const money = (v: number) => formatCurrency(v);
  const gainText = (pct: number) => (
    <b className={cn('font-650', pct >= 0 ? 'text-gain' : 'text-loss')}>
      {fmt.percent(pct / 100, 1, { signed: true })}
    </b>
  );
  const openAssign = (stockIds: string[]) => setAssign({ open: true, stockIds });

  const allocationSub = filterActive
    ? [chartGroup?.label, selectionText].filter(Boolean).join(' · ')
    : chartGroup
      ? t('stocksAnalysis.allocation.sub', { group: chartGroup.label })
      : t('stocksAnalysis.allocation.subNoGroup');
  const twrSub = filterActive
    ? `${selectionText} · ${t('stocksAnalysis.twr.referenceNote')}`
    : t('stocksAnalysis.twr.sub');
  const tagsSub = filterActive
    ? `${selectionText} · ${t('stocksAnalysis.tagsTable.selectedNote')}`
    : t('stocksAnalysis.tagsTable.sub');
  const portfolioReturn = twrChart?.portfolioReturn ?? null;

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
        <div ref={chipsRef} className="mb-[18px] flex flex-wrap gap-5">
          {filterGroups.map((g) => {
            // The chosen tag of the group; a choice that no longer exists reads as "all"
            const chosen = g.tags.find((x) => chosenIds.includes(x.id))?.id ?? FILTER_ALL;
            return (
              <div key={g.key} className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1 text-eyebrow uppercase text-ink-4">{g.label}</span>
                <Chip
                  active={chosen === FILTER_ALL}
                  onClick={() => setFilters((f) => ({ ...f, [g.key]: FILTER_ALL }))}
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
          {filterActive && (
            <button
              type="button"
              className="self-center text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
              onClick={() => {
                setFilters({});
                // The link goes away with the filter: keep keyboard focus among the chips
                chipsRef.current?.querySelector('button')?.focus();
              }}
            >
              {t('stocksAnalysis.filters.clear')}
            </button>
          )}
        </div>
      )}

      {/* The grid stretches, so both cards are as tall as the taller one */}
      <div className="mb-[18px] grid grid-cols-2 gap-[18px]">
        <Card className="flex flex-col">
          <CardHeader className="items-start pb-1">
            <div>
              <CardTitle className="text-[16px]">{t('stocksAnalysis.allocation.title')}</CardTitle>
              {/* Narrow: the subtitle wraps to two lines unfiltered, to one with the selection */}
              <CardDescription className={cn(!wide && 'min-h-[28px]')}>
                {allocationSub}
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
          {/* The ring centers in the height the TWR card sets */}
          <div
            className="flex flex-1 items-center px-[21px] pb-5 pt-3"
            style={wide ? undefined : { minHeight: RING_BODY_MIN_HEIGHT }}
          >
            {selected.length === 0 ? (
              <p className="w-full text-center text-table text-ink-3">
                {t('stocksAnalysis.holdings.emptyFilter.description')}
              </p>
            ) : (
              <AllocationRing
                className="w-full"
                size={wide ? 'lg' : 'md'}
                segments={ringSegments.segments}
                otherSegment={{
                  key: 'untagged',
                  label: t('stocksAnalysis.allocation.untagged'),
                  value: ringSegments.untagged,
                }}
                centerLabel={
                  filterActive
                    ? t('stocksAnalysis.allocation.centerSelected')
                    : t('stocksAnalysis.allocation.center')
                }
                centerValue={formatCurrencyShort(selectedValue)}
                otherLabel={t('stocksAnalysis.allocation.other')}
                formatValue={money}
                formatPercent={(r) => fmt.percent(r, 0)}
              />
            )}
          </div>
        </Card>

        <Card className={cn('transition-opacity duration-base', twrStale && 'opacity-50')}>
          <CardHeader className="items-start pb-0">
            <div>
              <CardTitle className="text-[16px]">{t('stocksAnalysis.twr.title')}</CardTitle>
              <CardDescription>{twrSub}</CardDescription>
              {/* The whole portfolio's return for the period, the line the tags are read against */}
              <p className="mt-3 flex min-h-[22px] items-baseline gap-2">
                {portfolioReturn !== null && (
                  <>
                    <span className="text-caption font-600 text-ink-3">
                      {t('stocksAnalysis.twr.portfolio')}
                    </span>
                    <b
                      className={cn(
                        'text-h2 num',
                        Math.abs(portfolioReturn) < 0.0005
                          ? 'text-ink'
                          : portfolioReturn > 0
                            ? 'text-gain'
                            : 'text-loss'
                      )}
                    >
                      {fmt.percent(portfolioReturn, 1, { signed: true })}
                    </b>
                  </>
                )}
              </p>
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
            {twrChart ? (
              <MoonyLinesChart
                className="-mx-1 mt-[14px]"
                series={twrChart.series}
                axisLabels={twrChart.axisLabels}
                height={TWR_CHART_HEIGHT}
                labels="legend"
                legendRows={legendRows}
                formatValue={(v) => fmt.percent(twrPercent(v), 1, { signed: true })}
                formatTick={(v) => fmt.percent(twrPercent(v), twrTickDigits(v), { signed: true })}
                tipLabel={(i) => fmt.day(twrChart.dateTs(twrChart.dates[i] ?? twrChart.dates[0]))}
              />
            ) : (
              <p
                className="mt-[14px] grid place-items-center text-table text-ink-3"
                style={{ height: twrBlockHeight(legendRows) }}
              >
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
            <CardDescription>{tagsSub}</CardDescription>
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
        ) : tagRows.length === 0 ? (
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
              {tagRows.map(({ tag, totals }) => {
                const group = groups.find((g) => g.id === tag.groupId);
                const has = totals.count > 0;
                return (
                  <TableRow key={tag.id} className="h-12">
                    <TableCell>
                      <span className="inline-flex items-center gap-1.5">
                        <ChipLabel>{tag.name}</ChipLabel>
                        {group && (
                          <span className="text-[10px] font-500 text-ink-4">{group.name}</span>
                        )}
                      </span>
                    </TableCell>
                    <TableCell className="text-right num">{fmt.number(totals.count)}</TableCell>
                    <TableCell className="text-right font-650 text-ink num">
                      {has ? money(totals.value) : <span className="text-ink-5">—</span>}
                    </TableCell>
                    <TableCell className="text-right num">
                      {has ? fmt.percent(totalValue > 0 ? totals.value / totalValue : 0, 0) : '—'}
                    </TableCell>
                    <TableCell className="text-right num">
                      {has ? gainText(totals.gainLossPercent) : '—'}
                    </TableCell>
                    <TableCell className="text-right num">
                      {totals.dividend > 0 ? (
                        money(totals.dividend)
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
