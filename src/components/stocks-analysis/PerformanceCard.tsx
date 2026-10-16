import { Fragment, useCallback, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ChartLine, ChevronDown, CircleAlert, Info } from 'lucide-react';
import type { StockPerformance, StockTag } from '@shared/schema';
import { useStockBenchmark } from '@/hooks/use-stock-performance';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  PERIOD_PRESETS,
  dayFromIso,
  isoDay,
  maxDrawdownTo,
  rebaseReturns,
  resolveRangeInput,
  seriesValueAt,
  splitControl,
  splitDividers,
  type CompareGroup,
  type DayRange,
  type Part,
  type PeriodPreset,
  type RangeError,
} from '@/utils/stock-performance';
import { arrowOf, formatPoints, toneOf, type Tone } from '@/utils/performance-format';
import { slot } from '@/utils/rich-text';
import { HeroCard } from '@/components/common/HeroCard';
import { ConversionQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Segmented } from '@/components/ui/segmented';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Skeleton } from '@/components/ui/skeleton';
import { FilterBadge } from './FilterBadge';
import { PerformanceBoard, type BoardKind } from './PerformanceBoard';
import { PerformanceChart } from './PerformanceChart';
import { Rich } from './Rich';
import {
  INDEX_CHOICES,
  buildRows,
  chartLines,
  figuresAt,
  groupName,
  partName,
  rankedAt,
  sectorLabel,
  type IndexId,
  type RankedPart,
} from './performance-model';

const toneClass: Record<Tone, string> = { gain: 'text-gain', loss: 'text-loss', flat: '' };

export interface PerformanceCardProps {
  /** The response on screen; the previous one while the next loads (`stale`). */
  data: StockPerformance | undefined;
  loading: boolean;
  stale: boolean;
  failed: boolean;
  onRetry: () => void;
  /** Today, unix seconds at UTC midnight. */
  today: number;
  /** The period ends today: values are the analysis values of today, else those of its last day. */
  endsToday: boolean;
  period: PeriodPreset;
  custom: DayRange | null;
  /** Earliest day the custom range accepts. */
  minDate: number;
  onPeriodChange: (period: PeriodPreset) => void;
  /** A custom range from the inputs or the chart; the page switches the period to "Vlastní". */
  onCustomChange: (range: DayRange) => void;
  /**
   * The choices of "Rozdělit podle" ("don't split" first, see `withNoSplit`; none without a real
   * option) and the chosen one (`null`: nothing to split by).
   */
  splitOptions: readonly CompareGroup[];
  group: CompareGroup | null;
  onGroupChange: (key: string) => void;
  /**
   * The parts of the chosen group in the selection, by value, with their drawing. With "don't
   * split": none, or the selection while a filter is on.
   */
  parts: readonly RankedPart[];
  /** Held positions: the hero figures count the whole portfolio. */
  positionCount: number;
  filterActive: boolean;
  /** The tags the filter chose, for the badge in the toolbar (the hero stays whole-portfolio). */
  filterTags: readonly StockTag[];
  /** Value as of the end of the period: the whole portfolio and the selection. */
  totalValue: number | null;
  selectedValue: number | null;
  index: IndexId | null;
  onIndexChange: (id: IndexId | null) => void;
  /** A click on a tag row of the board: filter the page by it. */
  onFilterTag: (tag: StockTag) => void;
}

/** A big number with its label and a note (the three figures of the hero). */
function HeroFigure({
  label,
  value,
  tone,
  large = false,
  stale,
  children,
}: {
  label: string;
  value: ReactNode;
  tone: Tone;
  large?: boolean;
  stale: boolean;
  children?: ReactNode;
}) {
  return (
    <div className={cn('min-w-0 transition-opacity duration-base', stale && 'opacity-50')}>
      <div className="text-table font-600 text-ink-3">{label}</div>
      <b
        className={cn(
          'block num',
          large ? 'my-2 text-display' : 'mb-[7px] mt-[10px] text-stat tracking-[-0.05em]',
          toneClass[tone] || 'text-ink'
        )}
      >
        {value}
      </b>
      {children}
    </div>
  );
}

/** Skeleton in the shape of the card: three figures, the toolbar and the chart. */
export function PerformanceSkeleton() {
  return (
    <HeroCard
      className="mb-[18px] pb-6 pt-[22px]"
      chart={
        <>
          <div className="mt-5 flex justify-between gap-4">
            <Skeleton className="h-[30px] w-80" />
            <Skeleton className="h-[30px] w-96" />
          </div>
          <Skeleton className="mt-6 h-[300px] w-full" />
        </>
      }
    >
      {[0, 1, 2].map((i) => (
        <div key={i} aria-hidden className="min-w-0">
          <Skeleton className="h-3 w-40" />
          <Skeleton className={cn('mt-3 h-9', i === 0 ? 'w-44' : 'w-28')} />
          <Skeleton className="mt-3 h-3 w-36" />
        </div>
      ))}
    </HeroCard>
  );
}

/**
 * The performance card, the hero of the stocks analysis page: the time-weighted and money-weighted
 * return and the change of value of the whole portfolio, the controls (how to split, index,
 * period), the chart with the drawdown under it and the board that doubles as its legend.
 *
 * Hovering the chart rewrites the figures and the board to that day. The figures and the dashed
 * line always stay whole-portfolio; the selection narrows the parts. Dragging across the chart,
 * or the two date inputs, set a custom period.
 */
export function PerformanceCard({
  data,
  loading,
  stale,
  failed,
  onRetry,
  today,
  endsToday,
  period,
  custom,
  minDate,
  onPeriodChange,
  onCustomChange,
  splitOptions,
  group,
  onGroupChange,
  parts,
  positionCount,
  filterActive,
  filterTags,
  totalValue,
  selectedValue,
  index,
  onIndexChange,
  onFilterTag,
}: PerformanceCardProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  const pp = t('stocksAnalysis.pp');

  /** Point under the pointer on the chart; `null` reads the last point. */
  const [hover, setHover] = useState<number | null>(null);
  const [highlighted, setHighlighted] = useState<string | null>(null);
  /** The date inputs while they hold something that is not a valid range yet. */
  const [draft, setDraft] = useState<{ from: string; to: string } | null>(null);
  const [rangeError, setRangeError] = useState<RangeError | null>(null);

  const dates = useMemo(() => (data?.dates ?? []).map((iso) => dayFromIso(iso) ?? 0), [data]);
  const last = dates.length - 1;
  const point = hover !== null && hover <= last ? hover : last;
  const currency = data?.currency ?? '';

  // --- index ---------------------------------------------------------------------------------

  const choice = INDEX_CHOICES.find((c) => c.id === index) ?? null;
  const benchmark = useStockBenchmark(
    choice && data && dates.length >= 2
      ? {
          ticker: choice.ticker,
          fromTs: dates[0],
          toTs: dates[last],
          reportCurrency: data.currency,
        }
      : null
  );
  const benchmarkData = benchmark.data;
  // The previous period's index stands in while the next loads: dim it, and read it from the
  // first day of the period on screen (see `rebaseReturns`)
  const indexStale = choice !== null && benchmark.isPlaceholderData;
  const indexLine = useMemo(() => {
    if (!choice || !benchmarkData || benchmarkData.dates.length === 0) return null;
    const benchDates = benchmarkData.dates.map((iso) => dayFromIso(iso) ?? 0);
    const twr = seriesValueAt(dates, benchDates, benchmarkData.twr);
    const shown = indexStale ? rebaseReturns(twr) : twr;
    // A stand-in that does not reach back to the first day has nothing sound to show yet
    if (indexStale && shown.every((value) => value === null)) return null;
    return { name: choice.name, ticker: choice.ticker, twr: shown };
  }, [choice, benchmarkData, dates, indexStale]);
  const indexFailed =
    choice !== null &&
    data !== undefined &&
    !benchmark.isLoading &&
    (benchmark.isError || benchmarkData?.dates.length === 0);

  // --- rows, lines -----------------------------------------------------------------------------

  // "Don't split" has no group to name above the board's first column
  const groupLabel =
    group === null || group.kind === 'none'
      ? ''
      : groupName(group, (kind) => t(`stocksAnalysis.groups.${kind}`));
  const nameOf = useCallback(
    (part: Part): string =>
      partName(
        part,
        group?.kind,
        {
          tag: t('stocksAnalysis.board.noneTag'),
          sector: t('stocksAnalysis.board.noneSector'),
          selection: t('stocksAnalysis.board.selection'),
        },
        (name) => sectorLabel(name, t)
      ),
    [group, t]
  );
  const rows = useMemo(
    () =>
      data
        ? buildRows({
            series: data.series,
            portfolio: data.portfolio,
            ranked: parts,
            nameOf,
            portfolioName: t('stocksAnalysis.performance.portfolio'),
            totalValue,
            selectedValue,
            filterActive,
            index: indexLine,
            reportCurrency: data.currency,
            splitKind: group?.kind ?? null,
          })
        : [],
    [data, parts, nameOf, t, totalValue, selectedValue, filterActive, indexLine, group]
  );

  // While the next response loads, the previous one stays on screen (`stale`). What is derived
  // from it (rows, wording, the group) is then held back too: the new parts have no series in the
  // old response and would blank rows and flash "nothing in the selection".
  const live = useMemo(
    () => ({ rows, group, groupLabel, filterActive, filterTags, endsToday, positionCount }),
    [rows, group, groupLabel, filterActive, filterTags, endsToday, positionCount]
  );
  const [shown, setShown] = useState(live);
  if (!stale && shown !== live) setShown(live);
  const view = stale ? shown : live;
  const lines = useMemo(() => chartLines(view.rows), [view.rows]);

  // --- figures ---------------------------------------------------------------------------------

  const figures = data ? figuresAt(data, point) : null;
  const percent = (value: number, digits = 1) => fmt.percent(value / 100, digits, { signed: true });
  const points = (value: number, signed = true) => formatPoints(fmt.number, value, pp, { signed });
  const bold = (text: string) => <b className="font-650 text-ink-2">{text}</b>;

  const delta = (): ReactNode => {
    if (!figures) return null;
    const indexRow = view.rows.find((row) => row.kind === 'index');
    const indexReturn = indexRow?.twr[point];
    if (indexRow && indexReturn !== null && indexReturn !== undefined) {
      const gap = figures.twr - indexReturn;
      const tone = toneOf(gap);
      return (
        <>
          <span className={toneClass[tone]}>
            {arrowOf(tone)}
            {points(gap, false)}
          </span>
          <span className="font-450 text-ink-4">
            {t(
              gap >= 0
                ? 'stocksAnalysis.performance.aheadOfIndex'
                : 'stocksAnalysis.performance.behindIndex',
              { index: indexRow.name, value: percent(indexReturn) }
            )}
          </span>
        </>
      );
    }
    if (view.group === null) return null;
    const ranked = rankedAt(view.rows, point);
    const named = (entry: (typeof ranked)[number]) => {
      const tone = toneOf(entry.twr);
      return (
        <span className={toneClass[tone]}>
          {arrowOf(tone)}
          {entry.row.name} {percent(entry.twr)}
        </span>
      );
    };
    if (view.group.kind === 'none') {
      // The selection is the only part; without it (no filter) the deepest drawdown fills the line
      if (ranked.length > 0) {
        return (
          <>
            {named(ranked[0])}
            <span className="font-450 text-ink-4">
              {t('stocksAnalysis.performance.selectionNote')}
            </span>
          </>
        );
      }
      const portfolioRow = view.rows.find((row) => row.kind === 'portfolio');
      if (!portfolioRow) return null;
      return (
        <span className="font-450 text-ink-4">
          {t('stocksAnalysis.performance.maxDrawdown', {
            value: fmt.percent(maxDrawdownTo(portfolioRow.drawdown, point) / 100, 1, {
              signed: true,
            }),
          })}
        </span>
      );
    }
    if (ranked.length > 1) {
      return (
        <>
          {named(ranked[0])}
          <span className="font-450 text-ink-5">·</span>
          {named(ranked[ranked.length - 1])}
          <span className="font-450 text-ink-4">
            {t(
              view.filterActive
                ? 'stocksAnalysis.performance.bestWorstSelection'
                : 'stocksAnalysis.performance.bestWorst'
            )}
          </span>
        </>
      );
    }
    if (ranked.length === 1) {
      return (
        <>
          {named(ranked[0])}
          <span className="font-450 text-ink-4">{t('stocksAnalysis.performance.onlyPart')}</span>
        </>
      );
    }
    return (
      <span className="font-450 text-ink-4">
        {t('stocksAnalysis.performance.noPart', { group: view.groupLabel })}
      </span>
    );
  };

  const when = (): ReactNode => {
    const dateNode = (ts: number) => <b className="font-650 text-ink-3">{fmt.day(ts)}</b>;
    const key = point !== last ? 'fromAt' : view.endsToday ? 'fromToday' : 'fromTo';
    return (
      <>
        <Rich
          text={t(`stocksAnalysis.performance.${key}`, {
            from: slot('from'),
            to: slot('to'),
            at: slot('at'),
          })}
          slots={{
            from: dateNode(dates[0]),
            to: dateNode(dates[last]),
            at: dateNode(dates[point]),
          }}
        />
        {' · '}
        {t('stocksAnalysis.performance.positions', { count: view.positionCount })}
      </>
    );
  };

  // --- split caption ---------------------------------------------------------------------------

  const caption = (): ReactNode => {
    if (view.group === null) return t('stocksAnalysis.performance.captionNoGroups');
    const drawn = view.rows.filter((row) => row.kind === 'part' && row.drawn);
    if (view.group.kind === 'none') {
      // The selection line exists only while a filter selects something
      return t(
        drawn.length > 0
          ? 'stocksAnalysis.performance.captionNoneSelected'
          : 'stocksAnalysis.performance.captionNone'
      );
    }
    if (drawn.length === 0) {
      return t('stocksAnalysis.performance.captionEmpty', { group: view.groupLabel });
    }
    const key = {
      tags: 'captionTags',
      sector: 'captionSector',
      currency: 'captionCurrency',
    }[view.group.kind];
    const names = drawn.map((row, i) => (
      <Fragment key={row.id}>
        {i > 0 && ', '}
        {bold(row.name)}
      </Fragment>
    ));
    const more = view.rows.some((row) => row.kind === 'part' && !row.drawn && !row.isNone);
    return (
      <>
        <Rich
          text={t(`stocksAnalysis.performance.${key}${view.filterActive ? 'Selected' : ''}`, {
            group: view.groupLabel,
            currency,
            parts: slot('parts'),
          })}
          slots={{ parts: names }}
        />{' '}
        {t('stocksAnalysis.performance.captionPortfolio')}
        {more && ` ${t('stocksAnalysis.performance.captionMore')}`}
      </>
    );
  };

  // --- period and custom range -----------------------------------------------------------------

  const changePeriod = (next: PeriodPreset) => {
    setDraft(null);
    setRangeError(null);
    onPeriodChange(next);
  };
  const selectRange = (from: number, to: number) => {
    setDraft(null);
    setRangeError(null);
    onCustomChange({ from, to });
  };
  /** Commit the range the inputs hold, or keep them as a draft (see `resolveRangeInput`). */
  const applyInputs = (next: { from: string; to: string }, typing: 'from' | 'to' | null) => {
    const decision = resolveRangeInput(next.from, next.to, minDate, today, typing);
    if (decision.kind === 'draft') {
      setDraft(next);
      setRangeError(decision.error);
      return;
    }
    setDraft(null);
    setRangeError(null);
    onCustomChange(decision.range);
  };
  const changeInput = (side: 'from' | 'to', value: string) => {
    if (!custom) return;
    applyInputs(
      {
        from: draft?.from ?? isoDay(custom.from),
        to: draft?.to ?? isoDay(custom.to),
        [side]: value,
      },
      side
    );
  };
  /** Blur or Enter: a draft outside the bounds is committed clamped to them. */
  const settleInputs = () => {
    if (draft) applyInputs(draft, null);
  };

  // --- states ----------------------------------------------------------------------------------

  if (failed && !data) return <ReportError onRetry={onRetry} />;
  if (loading && !data) return <PerformanceSkeleton />;
  if (!data) return null;

  const splitLabel = (g: CompareGroup) =>
    groupName(g, (kind) => t(`stocksAnalysis.groups.${kind}`));
  const control = splitControl(splitOptions);
  const dividers = splitDividers(splitOptions);
  const boardKind: BoardKind = view.group?.kind ?? null;
  const hasChart = dates.length >= 2;
  const lastLabel = view.endsToday
    ? t('stocksAnalysis.performance.today')
    : fmt.day(dates[last] ?? today);

  const money = (value: number, signed: boolean) =>
    fmt.money(value, currency, { decimals: 0, signed });
  const dim = (stale || indexStale) && 'opacity-50';

  return (
    <HeroCard
      className="mb-[18px] pb-0 pt-[22px]"
      chart={
        <>
          {/* Toolbar */}
          <div className="mt-5 flex flex-wrap items-center justify-between gap-x-[18px] gap-y-[10px]">
            <div className="flex items-center gap-[10px]">
              {control !== 'hidden' && group && (
                <>
                  <span
                    className="text-micro font-600 text-ink-4"
                    title={t('stocksAnalysis.performance.splitByTitle')}
                  >
                    {t('stocksAnalysis.performance.splitBy')}
                  </span>
                  {control === 'select' ? (
                    <Select value={group.key} onValueChange={onGroupChange}>
                      <SelectTrigger
                        aria-label={t('stocksAnalysis.performance.splitBy')}
                        className="h-[30px] w-auto min-w-[150px] text-caption"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {splitOptions.map((g, i) => (
                          <Fragment key={g.key}>
                            {dividers[i] && <SelectSeparator />}
                            <SelectItem value={g.key}>{splitLabel(g)}</SelectItem>
                          </Fragment>
                        ))}
                      </SelectContent>
                    </Select>
                  ) : (
                    <Segmented
                      value={group.key}
                      onValueChange={onGroupChange}
                      aria-label={t('stocksAnalysis.performance.splitBy')}
                      options={splitOptions.map((g, i) => ({
                        value: g.key,
                        label:
                          g.kind === 'sector' || g.kind === 'currency' ? (
                            <span title={t('stocksAnalysis.groups.autoTitle')}>
                              {splitLabel(g)}
                            </span>
                          ) : (
                            splitLabel(g)
                          ),
                        divider: dividers[i],
                      }))}
                    />
                  )}
                </>
              )}
              <FilterBadge tags={view.filterTags} partial />
            </div>
            <div className="ml-auto flex items-center gap-[10px]">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="outline"
                    size="sm"
                    className={cn(choice && 'border-focus text-ink')}
                  >
                    <ChartLine />
                    {choice
                      ? t('stocksAnalysis.performance.indexActive', { name: choice.name })
                      : t('stocksAnalysis.performance.indexButton')}
                    <ChevronDown className="!size-3" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuRadioGroup
                    value={index ?? 'none'}
                    onValueChange={(value) =>
                      onIndexChange(INDEX_CHOICES.find((c) => c.id === value)?.id ?? null)
                    }
                  >
                    <DropdownMenuRadioItem value="none">
                      {t('stocksAnalysis.performance.indexNone')}
                    </DropdownMenuRadioItem>
                    {INDEX_CHOICES.map((c) => (
                      <DropdownMenuRadioItem key={c.id} value={c.id}>
                        {c.name}
                        <DropdownMenuShortcut className="tracking-normal">
                          {t('stocksAnalysis.performance.indexVia', { ticker: c.ticker })}
                        </DropdownMenuShortcut>
                      </DropdownMenuRadioItem>
                    ))}
                  </DropdownMenuRadioGroup>
                </DropdownMenuContent>
              </DropdownMenu>
              <span className="text-micro font-600 text-ink-4">
                {t('stocksAnalysis.periods.label')}
              </span>
              <Segmented
                value={period}
                onValueChange={changePeriod}
                aria-label={t('stocksAnalysis.periods.label')}
                options={PERIOD_PRESETS.map((p) => ({
                  value: p,
                  label: t(`stocksAnalysis.periods.${p}`),
                }))}
              />
            </div>
          </div>

          {/* What the lines add up, and the custom range */}
          <div className="mt-3 flex items-start justify-between gap-[18px]">
            <div className="min-w-0">
              <p className="m-0 flex max-w-[600px] items-start gap-[7px] text-micro font-500 leading-[1.55] text-ink-3">
                <Info aria-hidden className="mt-px size-[13px] shrink-0 text-ink-4" />
                <span>{caption()}</span>
              </p>
              {indexFailed && (
                <p role="note" className="mt-1.5 text-micro font-500 text-loss">
                  {t('stocksAnalysis.performance.indexFailed')}
                </p>
              )}
            </div>
            {period === 'custom' && custom && (
              <div className="flex shrink-0 flex-col items-end gap-[5px]">
                <div className="flex items-center gap-2">
                  <span className="text-micro font-600 text-ink-4">
                    {t('stocksAnalysis.performance.from')}
                  </span>
                  <Input
                    type="date"
                    value={draft?.from ?? isoDay(custom.from)}
                    min={isoDay(minDate)}
                    max={isoDay(today)}
                    aria-label={t('stocksAnalysis.performance.from')}
                    aria-invalid={rangeError !== null}
                    onChange={(e) => changeInput('from', e.target.value)}
                    onBlur={settleInputs}
                    onKeyDown={(e) => e.key === 'Enter' && settleInputs()}
                    className="h-[35px] w-[140px] text-table"
                  />
                  <span className="text-ink-5">→</span>
                  <span className="text-micro font-600 text-ink-4">
                    {t('stocksAnalysis.performance.to')}
                  </span>
                  <Input
                    type="date"
                    value={draft?.to ?? isoDay(custom.to)}
                    min={isoDay(minDate)}
                    max={isoDay(today)}
                    aria-label={t('stocksAnalysis.performance.to')}
                    aria-invalid={rangeError !== null}
                    onChange={(e) => changeInput('to', e.target.value)}
                    onBlur={settleInputs}
                    onKeyDown={(e) => e.key === 'Enter' && settleInputs()}
                    className="h-[35px] w-[140px] text-table"
                  />
                </div>
                {rangeError ? (
                  <span
                    role="alert"
                    className="flex items-center gap-[5px] text-micro font-600 text-loss"
                  >
                    <CircleAlert aria-hidden className="size-3" />
                    {t(
                      rangeError === 'missing'
                        ? 'stocksAnalysis.performance.rangeMissing'
                        : 'stocksAnalysis.performance.rangeOrder'
                    )}
                  </span>
                ) : (
                  <span className="text-micro font-500 text-ink-4">
                    {t('stocksAnalysis.performance.dragHint')}
                  </span>
                )}
              </div>
            )}
          </div>

          <div className={cn('transition-opacity duration-base', dim)}>
            {hasChart ? (
              <PerformanceChart
                className="-mx-1 mt-[14px]"
                dates={dates}
                lines={lines}
                highlighted={highlighted}
                onHighlight={setHighlighted}
                onScrub={setHover}
                onRangeSelect={selectRange}
                lastLabel={lastLabel}
                formatPercent={percent}
                formatDay={(ts) => fmt.day(ts)}
                labels={{
                  drawdown: t('stocksAnalysis.performance.drawdown'),
                  tipDrawdown: (value) => t('stocksAnalysis.performance.tipDrawdown', { value }),
                  dragTip: (days) => t('stocksAnalysis.performance.dragTip', { days }),
                }}
              />
            ) : (
              <p className="mt-[14px] grid h-[364px] place-items-center text-table text-ink-3">
                {t('stocksAnalysis.performance.empty')}
              </p>
            )}
            <ConversionQualityNote quality={data.fxQuality} currency={currency} className="mt-2" />
            <PerformanceBoard
              rows={view.rows}
              groupLabel={view.groupLabel}
              kind={boardKind}
              currency={currency}
              point={Math.max(point, 0)}
              endsToday={view.endsToday}
              endDate={dates[last] ?? today}
              filterActive={view.filterActive}
              highlighted={highlighted}
              onHighlight={setHighlighted}
              onFilterTag={onFilterTag}
            />
          </div>
        </>
      }
    >
      <HeroFigure
        label={t('stocksAnalysis.performance.twrLabel')}
        value={figures ? percent(figures.twr) : '—'}
        tone={toneOf(figures?.twr)}
        large
        stale={stale || indexStale}
      >
        <div className="flex min-h-[18px] flex-wrap items-baseline gap-x-1.5 gap-y-1 text-table font-650">
          {delta()}
        </div>
        {hasChart && <div className="mt-[5px] text-micro font-500 text-ink-4">{when()}</div>}
      </HeroFigure>
      <HeroFigure
        label={t('stocksAnalysis.performance.mwrLabel')}
        value={figures ? percent(figures.mwr) : '—'}
        tone={toneOf(figures?.mwr)}
        stale={stale}
      >
        <div className="text-micro font-500 leading-[1.65] text-ink-4 min-[1360px]:whitespace-nowrap">
          {t('stocksAnalysis.performance.mwrNote')}
          <br />
          {figures &&
            (toneOf(figures.mwrGap) === 'flat' ? (
              t('stocksAnalysis.performance.mwrSame')
            ) : (
              <Rich
                text={t('stocksAnalysis.performance.mwrGap', { gap: slot('gap') })}
                slots={{ gap: bold(points(figures.mwrGap)) }}
              />
            ))}
        </div>
      </HeroFigure>
      <HeroFigure
        label={t('stocksAnalysis.performance.changeLabel')}
        value={figures?.change != null ? money(figures.change, true) : '—'}
        tone={toneOf(figures?.change, 0)}
        stale={stale}
      >
        {figures && figures.deposits !== null && figures.market !== null && (
          <div className="text-micro font-500 leading-[1.65] text-ink-4 min-[1360px]:whitespace-nowrap">
            <Rich
              text={t('stocksAnalysis.performance.netDeposits', { amount: slot('amount') })}
              slots={{ amount: bold(money(figures.deposits, true)) }}
            />
            <br />
            <Rich
              text={t('stocksAnalysis.performance.market', { amount: slot('amount') })}
              slots={{ amount: bold(money(figures.market, true)) }}
            />
          </div>
        )}
      </HeroFigure>
    </HeroCard>
  );
}
