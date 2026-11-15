import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Bar, BarChart, CartesianGrid, Cell, XAxis, YAxis } from 'recharts';
import { useChartTokens } from '@/theme/ThemeProvider';
import { niceTicks, valueDomain } from '@/utils/chart-scale';
import { CategoryAxis } from '@/components/charts/ChartAxis';
import { useTickFormatter } from '@/components/charts/use-tick-formatter';
import type { PlanBar } from '@/mobile/lib/plan-detail';

/** The chart's height (spec §10.4: bars 110 pt). */
const HEIGHT = 110;
/** Room above the plot for the top value tick. */
const MARGIN = { top: 10, right: 0, bottom: 0, left: 0 };
/** Months other than the selected one (the Cashflow bars' opacity). */
const MUTED_OPACITY = 0.35;

interface PlanMonthBarsProps {
  bars: PlanBar[];
  /** The label under each bar, blank where none stands (`sparseLabels`). */
  axisLabels: string[];
  /** Accessible name of the chart. */
  label: string;
  /** The month as a button's name ("Březen", "Prosinec 2025"). */
  monthLabel: (month: string) => string;
  /** The month filter: its month's button is pressed. */
  selected: string | null;
  /** A tap on a month's column; the page toggles the filter. */
  onSelectMonth: (month: string) => void;
}

/**
 * "Útrata po měsících" of a plan (spec M9): one `s3` series of net spend per month in the
 * `CashflowBars` look — 3 px rounded tops, a zero baseline, horizontal grid, the right-hand value
 * axis, 110 pt — with the months other than the selected one muted. No tooltip: a transparent
 * button over each month's column (the plot and its label, the column's full width) selects the
 * month, a second tap clears it.
 */
export function PlanMonthBars({
  bars,
  axisLabels,
  label,
  monthLabel,
  selected,
  onSelectMonth,
}: PlanMonthBarsProps) {
  const tk = useChartTokens();
  const wrapRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const { domain, ticks } = useMemo(() => {
    const values = bars.map((bar) => bar.value);
    // Nothing to draw: a plain zero axis instead of ticks between 0 and 0.08
    if (values.every((value) => value === 0))
      return { domain: [0, 1] as [number, number], ticks: [0] };
    const range = valueDomain(values, { zeroBaseline: true, headroomAbove: 0.08 });
    return { domain: range, ticks: niceTicks(range[0], range[1], 4) };
  }, [bars]);

  // The locale's compact ticks in a gutter measured to the longest of them (A12)
  const { tickFormatter, axisWidth } = useTickFormatter(ticks);
  const plotWidth = Math.max(0, width - axisWidth);

  const chart = useMemo(() => {
    if (width <= 0) return null;
    return (
      <BarChart
        width={width}
        height={HEIGHT}
        data={bars}
        margin={MARGIN}
        barCategoryGap="30%"
        // The month buttons are the way in for the keyboard and assistive technology
        accessibilityLayer={false}
      >
        <CartesianGrid vertical={false} stroke={tk.grid} strokeDasharray="0" />
        <XAxis dataKey="month" hide />
        <YAxis
          orientation="right"
          width={axisWidth}
          domain={domain}
          ticks={ticks}
          axisLine={false}
          tickLine={false}
          tickMargin={10}
          tick={{ fill: tk.axis, fontSize: 10, fontWeight: 500 }}
          tickFormatter={tickFormatter}
        />
        <Bar
          dataKey="value"
          fill={tk.series[2]}
          radius={[3, 3, 0, 0]}
          maxBarSize={14}
          isAnimationActive={false}
        >
          {bars.map((bar) => (
            <Cell key={bar.month} fillOpacity={bar.muted ? MUTED_OPACITY : 1} />
          ))}
        </Bar>
      </BarChart>
    );
  }, [width, bars, tk, domain, ticks, tickFormatter, axisWidth]);

  return (
    <div className="relative">
      <div ref={wrapRef} style={{ height: HEIGHT }}>
        <div role="img" aria-label={label} className="[&_.recharts-surface]:overflow-visible">
          {chart}
        </div>
      </div>
      {chart && <CategoryAxis labels={axisLabels} gutter={axisWidth} centered />}
      {/* Over the plot and its labels, outside the image: one transparent button per month */}
      <div className="absolute inset-y-0 left-0 flex" style={{ width: plotWidth }}>
        {bars.map((bar) => (
          <button
            key={bar.month}
            type="button"
            aria-label={monthLabel(bar.month)}
            aria-pressed={bar.month === selected}
            onClick={() => onSelectMonth(bar.month)}
            className="h-full min-w-0 flex-1 rounded-r1 [-webkit-tap-highlight-color:transparent] focus-visible:outline-none focus-visible:shadow-focus"
          />
        ))}
      </div>
    </div>
  );
}
