import { useCallback, useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Plus, Tag, TrendingUp } from 'lucide-react';
import type {
  StocksAnalysis as StocksAnalysisReport,
  StockPerformanceRequest,
  StockTag,
  StockTagGroup,
} from '@shared/schema';
import { stockTagsApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useStockPerformance } from '@/hooks/use-stock-performance';
import {
  chosenTagIds,
  heldPositions,
  positionTotals,
  selectPositions,
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
import { EmptyState } from '@/components/common/EmptyState';
import { ManageTagsDialog } from '@/components/stocks-analysis/ManageTagsDialog';
import { AssignTagsDialog } from '@/components/stocks-analysis/AssignTagsDialog';
import { ContributionCard } from '@/components/stocks-analysis/ContributionCard';
import { FlowsCard } from '@/components/stocks-analysis/FlowsCard';
import { HoldingsTable } from '@/components/stocks-analysis/HoldingsTable';
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
import { TagsTable } from '@/components/stocks-analysis/TagsTable';
import { useHeldWhile } from '@/components/stocks-analysis/use-held-while';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';

const UNGROUPED = '__ungrouped__';

// Stand-ins while a query has no data: one array for all renders, so what is derived from them
// (and held by `useHeldWhile`) keeps its identity
const NO_TAGS: StockTag[] = [];
const NO_GROUPS: StockTagGroup[] = [];

/**
 * Stock analysis (design system §7 Analysis): the scope card with the chip filters per tag group,
 * the performance card (time-weighted and money-weighted return, change of value, the chart with
 * its drawdown, the board), the contribution and the deposits-and-market cards side by side, the
 * tag table and the holdings with their tags; manage and assign modals. The chips narrow the
 * parts, the board, the contribution card and the tables; the hero figures, the dashed portfolio
 * line and the deposits card stay whole-portfolio. This component only holds the state and wires
 * the data.
 *
 * The analysis is of the held positions: sold-out ones (quantity 0) are in the report but not in
 * the performance engine, so they are left out of everything on the page except the two tag
 * dialogs, whose move conflicts the backend counts over every position.
 */
export default function StocksAnalysis() {
  const { t } = useTranslation('reports');
  const { currencyCode, fxRevision } = useCurrency();

  const [filters, setFilters] = useState<Record<string, string>>({});
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

  const { data: tags = NO_TAGS, isLoading: tagsLoading } = useQuery<StockTag[]>({
    queryKey: ['stock-tags'],
    queryFn: stockTagsApi.getAll,
  });
  const { data: groups = NO_GROUPS, isLoading: groupsLoading } = useQuery<StockTagGroup[]>({
    queryKey: ['stock-tag-groups'],
    queryFn: stockTagsApi.getAllGroups,
  });
  const { data: analysis, isLoading: stocksLoading } = useQuery<StocksAnalysisReport>({
    queryKey: ['stocks-analysis', currencyCode, fxRevision],
    queryFn: () => stockTagsApi.getAnalysis(currencyCode),
  });
  // Amounts in the report currency of the response (ADR 0011); null = not convertible
  const reportCurrency = analysis?.currency ?? currencyCode;
  // Every position of the report (the tag dialogs) and the held ones (the analysis)
  const allStocks = useMemo(() => (analysis?.items ?? []).map(toAnalysisPosition), [analysis]);
  const stocks = useMemo(() => heldPositions(allStocks), [allStocks]);

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
    // Only from the response of "Vše" itself, not from the previous period's one still on screen
    if (period === 'all' && performance.data && !performance.isPlaceholderData) {
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

  // --- scope and tables ------------------------------------------------------------------------

  // Derived figures (portfolio-wide, as of today)
  const portfolio = positionTotals(stocks);
  const totalValue = portfolio.value;
  const untaggedStocks = stocks.filter((x) => x.tags.length === 0);
  // The last day the response covers, for the "K {date}" headers of the tables
  const lastDay = performance.data?.dates.at(-1);
  const endDate = (lastDay ? dayFromIso(lastDay) : null) ?? range.to;
  const openAssign = (stockIds: string[]) => setAssign({ open: true, stockIds });

  // While the next response loads, `performance.data` is still the previous one. The tables read
  // it together with the selection and the end of the period, so they keep every input of that
  // previous state (dimmed, like the cards) instead of mixing old figures with the new selection.
  const tablesLive = useMemo(
    () => ({
      stocks,
      selected,
      endValueOf,
      endTotal,
      series: performance.data?.series,
      performanceById,
      filterActive,
      endsToday,
      endDate,
    }),
    [
      stocks,
      selected,
      endValueOf,
      endTotal,
      performance.data,
      performanceById,
      filterActive,
      endsToday,
      endDate,
    ]
  );
  const tables = useHeldWhile(tablesLive, performance.isPlaceholderData);

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
        positions={allStocks}
      />
      <AssignTagsDialog
        open={assign.open}
        onOpenChange={(open) => setAssign((a) => ({ ...a, open }))}
        stocks={allStocks}
        tags={tags}
        groups={groups}
        initialStockIds={assign.stockIds}
      />
    </>
  );

  if (tagsLoading || groupsLoading || stocksLoading) {
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

      {(performance.data || performance.isLoading) && (
        <div className="mb-[18px] grid grid-cols-2 gap-[18px]">
          <ContributionCard
            data={performance.data}
            loading={performance.isLoading}
            stale={performance.isPlaceholderData}
            selected={selected}
            parts={parts}
            group={group}
            filterActive={filterActive}
          />
          <FlowsCard
            data={performance.data}
            loading={performance.isLoading}
            stale={performance.isPlaceholderData}
            endsToday={endsToday}
          />
        </div>
      )}

      <TagsTable
        groups={filterGroups}
        selected={tables.selected}
        total={tables.stocks.length}
        endValueOf={tables.endValueOf}
        endTotal={tables.endTotal}
        series={tables.series}
        filterActive={tables.filterActive}
        endsToday={tables.endsToday}
        endDate={tables.endDate}
        currency={reportCurrency}
        stale={performance.isPlaceholderData}
        onManage={() => setManageOpen(true)}
      />

      <HoldingsTable
        stocks={tables.stocks}
        selected={tables.selected}
        endValueOf={tables.endValueOf}
        endTotal={tables.endTotal}
        performance={tables.performanceById}
        endsToday={tables.endsToday}
        endDate={tables.endDate}
        currency={reportCurrency}
        stale={performance.isPlaceholderData}
        onAssign={openAssign}
      />

      {dialogs}
    </>
  );
}
