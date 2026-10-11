import { useCallback, useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Plus, Search, Tag, TrendingUp } from 'lucide-react';
import type {
  StocksAnalysis as StocksAnalysisReport,
  StockPerformanceRequest,
  StockTag,
  StockTagGroup,
  TagMetricsReport,
} from '@shared/schema';
import { stockTagsApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useStockPerformance } from '@/hooks/use-stock-performance';
import {
  chosenTagIds,
  positionTotals,
  selectPositions,
  tagTotals,
  toAnalysisPosition,
} from '@/utils/stock-analysis';
import {
  ALL_FROM,
  dayFromIso,
  groupParts,
  presetRange,
  positionCurrency,
  seriesSpecs,
  compareGroups,
  utcDay,
  type DayRange,
  type PeriodPreset,
  type PositionLite,
} from '@/utils/stock-performance';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { ChipLabel } from '@/components/ui/chip';
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
import { EmptyState } from '@/components/common/EmptyState';
import { AssetLogo } from '@/components/common/AssetLogo';
import { ManageTagsDialog } from '@/components/stocks-analysis/ManageTagsDialog';
import { AssignTagsDialog } from '@/components/stocks-analysis/AssignTagsDialog';
import { PerformanceCard, PerformanceSkeleton } from '@/components/stocks-analysis/PerformanceCard';
import { ScopeCard, ScopeSkeleton, type ScopeGroup } from '@/components/stocks-analysis/ScopeCard';
import {
  amountOf,
  rankParts,
  sumKnown,
  type IndexId,
} from '@/components/stocks-analysis/performance-model';
import {
  loadGroupKey,
  loadPeriod,
  saveGroupKey,
  savePeriod,
  type PeriodChoice,
} from '@/components/stocks-analysis/performance-settings';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { ChangeBadge } from '@/components/common/ChangeBadge';

const UNGROUPED = '__ungrouped__';

/**
 * Stock analysis (design system §7 Analysis): the scope card with the chip filters per tag group,
 * the performance card (time-weighted and money-weighted return, change of value, the chart with
 * its drawdown, the board), the tag table and the holdings with their tags; manage and assign
 * modals. The chips narrow the parts, the board and the tables; the hero figures and the dashed
 * portfolio line stay whole-portfolio. This component only holds the state and wires the data.
 */
export default function StocksAnalysis() {
  const { t } = useTranslation('reports');
  const { currencyCode, fxRevision } = useCurrency();
  const fmt = useFormat();

  const [filters, setFilters] = useState<Record<string, string>>({});
  const [query, setQuery] = useState('');
  // Period, custom range and split are remembered per device; the index is not
  const [view, setView] = useState<PeriodChoice>(loadPeriod);
  const [groupChoice, setGroupChoice] = useState<string | null>(loadGroupKey);
  const [index, setIndex] = useState<IndexId | null>(null);
  // First recorded day, learned from "Vše": the lower bound of the custom range
  const [firstDate, setFirstDate] = useState<number | null>(null);
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
  const { data: analysis, isLoading: stocksLoading } = useQuery<StocksAnalysisReport>({
    queryKey: ['stocks-analysis', currencyCode, fxRevision],
    queryFn: () => stockTagsApi.getAnalysis(currencyCode),
  });
  const { data: metricsReport, isLoading: metricsLoading } = useQuery<TagMetricsReport>({
    queryKey: ['tag-metrics', currencyCode, fxRevision],
    queryFn: () => stockTagsApi.getTagMetrics([], currencyCode),
  });
  // Amounts in the report currency of the response (ADR 0011); null = not convertible
  const reportCurrency = analysis?.currency ?? currencyCode;
  const stocks = useMemo(() => (analysis?.items ?? []).map(toAnalysisPosition), [analysis]);
  const metrics = useMemo(() => metricsReport?.tags ?? [], [metricsReport]);

  // Tag groups for the filters; ungrouped tags form a row of their own
  const filterGroups = useMemo<ScopeGroup[]>(() => {
    const sorted = [...groups].sort((a, b) => a.createdAt - b.createdAt);
    const list: ScopeGroup[] = sorted
      .map((g) => ({ key: g.id, label: g.name, tags: tags.filter((x) => x.groupId === g.id) }))
      .filter((g) => g.tags.length > 0);
    const ungrouped = tags.filter((x) => !x.groupId || !groups.some((g) => g.id === x.groupId));
    if (ungrouped.length > 0) {
      list.push({
        key: UNGROUPED,
        label: t('stocksAnalysis.scope.ungrouped'),
        tags: ungrouped,
        free: true,
      });
    }
    return list;
  }, [groups, tags, t]);

  // The chips narrow the analysis: the selected positions feed the parts, the board, the tag
  // table and the holdings. The hero figures and the portfolio line stay whole-portfolio.
  const chosenIds = useMemo(() => chosenTagIds(filterGroups, filters), [filterGroups, filters]);
  const filterActive = chosenIds.length > 0;
  const selected = useMemo(() => selectPositions(stocks, chosenIds), [stocks, chosenIds]);
  const selectedValue = selected.reduce((sum, x) => sum + (x.currentValue ?? 0), 0);
  const selectionText = t('stocksAnalysis.selection', {
    shown: selected.length,
    total: stocks.length,
  });

  // --- performance -----------------------------------------------------------------------------

  const today = utcDay(Math.floor(Date.now() / 1000));
  const { period, custom } = view;
  const range = useMemo<DayRange>(
    () =>
      period === 'custom' && custom
        ? custom
        : presetRange(period === 'custom' ? 'ytd' : period, today),
    [period, custom, today]
  );
  const endsToday = range.to >= today;

  const lite = useMemo<PositionLite[]>(
    () =>
      stocks.map((s) => ({
        id: s.id,
        sector: s.sector,
        currency: positionCurrency(s),
        tags: s.tags,
      })),
    [stocks]
  );
  const selectedLite = useMemo(() => selectPositions(lite, chosenIds), [lite, chosenIds]);

  // Ways to split: tag groups, then the automatic Sektor and Měna; default is the first
  const splitOptions = useMemo(() => compareGroups(groups, tags, lite), [groups, tags, lite]);
  const group = splitOptions.find((g) => g.key === groupChoice) ?? splitOptions[0] ?? null;

  const request = useMemo<StockPerformanceRequest>(
    () => ({
      fromTs: range.from,
      toTs: range.to,
      reportCurrency: currencyCode,
      series: seriesSpecs(selectedLite, tags, group, currencyCode).map(
        ({ key, investmentIds, currency }) =>
          currency ? { key, investmentIds, currency } : { key, investmentIds }
      ),
    }),
    [range, currencyCode, selectedLite, tags, group]
  );
  const performance = useStockPerformance(stocks.length > 0 ? request : null);

  // Values as of the end of the period: today's analysis values when it ends today, else the
  // values of its last day from the performance response
  const stockById = useMemo(() => new Map(stocks.map((s) => [s.id, s])), [stocks]);
  const performanceById = useMemo(
    () => new Map((performance.data?.positions ?? []).map((p) => [p.investmentId, p])),
    [performance.data]
  );
  const endValueOf = useCallback(
    (id: string): number | null =>
      endsToday
        ? (stockById.get(id)?.currentValue ?? null)
        : amountOf(performanceById.get(id)?.endValue),
    [endsToday, stockById, performanceById]
  );
  const endTotal = useMemo(
    () => sumKnown(stocks.map((s) => endValueOf(s.id))),
    [stocks, endValueOf]
  );
  const endSelected = useMemo(
    () => sumKnown(selected.map((s) => endValueOf(s.id))),
    [selected, endValueOf]
  );
  // The parts of the chosen group in the selection, by value, the first four drawn
  const parts = useMemo(
    () => (group ? rankParts(groupParts(selectedLite, group, tags), endValueOf) : []),
    [group, selectedLite, tags, endValueOf]
  );

  /** Commit a period (and the last custom range): remember it, and learn the first day from "Vše". */
  const commitPeriod = (choice: PeriodChoice) => {
    if (period === 'all' && performance.data) {
      const first = dayFromIso(performance.data.dates[0] ?? '');
      if (first !== null) setFirstDate(first);
    }
    setView(choice);
    savePeriod(choice);
  };
  const changePeriod = (next: PeriodPreset) => {
    if (next !== 'custom') return commitPeriod({ period: next, custom });
    // "Vlastní" starts from what is on screen unless a custom range was used before
    const first = performance.data ? dayFromIso(performance.data.dates[0] ?? '') : null;
    const shown = { from: Math.max(range.from, first ?? range.from), to: range.to };
    commitPeriod({ period: 'custom', custom: custom ?? shown });
  };
  const changeGroup = (key: string) => {
    setGroupChoice(key);
    saveGroupKey(key);
  };
  const filterByTag = (tag: StockTag) => {
    const key = filterGroups.find((g) => g.tags.some((x) => x.id === tag.id))?.key;
    if (key) setFilters((f) => ({ ...f, [key]: tag.id }));
  };

  // --- tables ----------------------------------------------------------------------------------

  // Derived figures (portfolio-wide)
  const portfolio = positionTotals(stocks);
  const totalValue = portfolio.value;
  const untaggedStocks = stocks.filter((x) => x.tags.length === 0);

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
  const filteredValue = filtered.reduce((s, x) => s + (x.currentValue ?? 0), 0);

  const money = (v: number | null) =>
    v === null ? '—' : fmt.money(v, reportCurrency, { decimals: 0 });
  const gainText = (pct: number | null) => (pct === null ? '—' : <ChangeBadge ratio={pct / 100} />);
  const openAssign = (stockIds: string[]) => setAssign({ open: true, stockIds });

  const tagsSub = filterActive
    ? `${selectionText} · ${t('stocksAnalysis.tagsTable.selectedNote')}`
    : t('stocksAnalysis.tagsTable.sub');

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
        positions={stocks}
      />
      <AssignTagsDialog
        open={assign.open}
        onOpenChange={(open) => setAssign((a) => ({ ...a, open }))}
        stocks={stocks}
        tags={tags}
        groups={groups}
        initialStockIds={assign.stockIds}
      />
    </>
  );

  if (tagsLoading || groupsLoading || stocksLoading || metricsLoading) {
    return (
      <>
        {head}
        <ScopeSkeleton />
        <PerformanceSkeleton />
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
      <ConversionQualityNote quality={analysis?.fxQuality} currency={reportCurrency} />

      <ScopeCard
        groups={filterGroups}
        chosenIds={chosenIds}
        onChoose={(groupKey, tagId) => setFilters((f) => ({ ...f, [groupKey]: tagId }))}
        onClear={() => setFilters({})}
        shown={selected.length}
        total={stocks.length}
        shownValue={selectedValue}
        totalValue={totalValue}
        currency={reportCurrency}
        untagged={untaggedStocks.length}
        onAssignUntagged={() => openAssign(untaggedStocks.map((s) => s.id))}
      />

      <PerformanceCard
        data={performance.data}
        loading={performance.isLoading}
        stale={performance.isPlaceholderData}
        failed={performance.isError}
        onRetry={() => void performance.refetch()}
        today={today}
        endsToday={endsToday}
        period={period}
        custom={custom}
        minDate={firstDate ?? ALL_FROM}
        onPeriodChange={changePeriod}
        onCustomChange={(next) => commitPeriod({ period: 'custom', custom: next })}
        splitOptions={splitOptions}
        group={group}
        onGroupChange={changeGroup}
        parts={parts}
        positionCount={stocks.length}
        filterActive={filterActive}
        totalValue={endTotal}
        selectedValue={endSelected}
        index={index}
        onIndexChange={setIndex}
        onFilterTag={filterByTag}
      />

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
                      {has && totals.value !== null
                        ? fmt.percent(
                            totalValue && totalValue > 0 ? totals.value / totalValue : 0,
                            0
                          )
                        : '—'}
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
                percent: fmt.percent(
                  totalValue && totalValue > 0 ? filteredValue / totalValue : 0,
                  0
                ),
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
