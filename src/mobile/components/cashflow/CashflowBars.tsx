import { useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Bar, BarChart, CartesianGrid, Cell, XAxis, YAxis } from 'recharts';
import { useFormat } from '@/lib/use-format';
import { useChartTokens } from '@/theme/ThemeProvider';
import { niceTicks, valueDomain } from '@/utils/chart-scale';
import { AXIS_WIDTH, CategoryAxis } from '@/components/charts/ChartAxis';
import { ScaledChartLegend } from '@/mobile/components/Scaled';
import type { BarDatum } from '@/mobile/lib/cashflow-rows';

/** The chart's height (spec §10.4: bars 110 pt). */
const HEIGHT = 110;
/** Room above the plot for the top value tick. */
const MARGIN = { top: 10, right: 0, bottom: 0, left: 0 };
/** Months outside the selected period: both bars at this opacity. */
const MUTED_OPACITY = 0.35;
/** A month name under every third bar, from the first. */
const LABEL_EVERY = 3;

interface CashflowBarsProps {
  /** The twelve months, oldest first; a muted one lies outside the selected period. */
  bars: BarDatum[];
  /** The two series' names, income first. */
  names: [string, string];
  /** Accessible name of the chart. */
  label: string;
  /** The month as a button's name ("Říjen 2026"). */
  monthLabel: (month: number) => string;
  /** A tap on a month's slot (`month`: unix seconds of its first day). */
  onSelectMonth: (month: number) => void;
}

/**
 * The twelve-month income and expense bars (spec §8.7 item 2, §10.4, README §13, `.m-bars`): the
 * desktop chart's rules — `s1` income and `s3` expenses, 3 px rounded tops, 2 px gap, a zero
 * baseline, horizontal grid, one right-hand value axis — at 110 pt with months outside the period
 * muted. There is no tooltip: a tap on a month (a transparent button the height of the plot, one
 * per bar; disabled for a month still to come) switches the period to it. A short month name stands under every third bar, then the legend.
 */
export function CashflowBars({ bars, names, label, monthLabel, onSelectMonth }: CashflowBarsProps) {
  const fmt = useFormat();
  const tk = useChartTokens();
  const [incomeName, expenseName] = names;
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

  const plotWidth = Math.max(0, width - AXIS_WIDTH);
  // A month after the current one stays on a quarter's or year's chart but cannot be opened: the
  // period never moves forward, so its button would open the current month under another name
  const today = new Date();
  const currentMonth = Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), 1) / 1000;

  const { domain, ticks } = useMemo(() => {
    const values = bars.flatMap((bar) => [bar.income, bar.expenses]);
    // Nothing to draw: a plain zero axis instead of ticks between 0 and 0.08
    if (values.every((value) => value === 0))
      return { domain: [0, 1] as [number, number], ticks: [0] };
    const range = valueDomain(values, { zeroBaseline: true, headroomAbove: 0.08 });
    return { domain: range, ticks: niceTicks(range[0], range[1], 4) };
  }, [bars]);

  const tickFormatter = useCallback(
    (value: number) => fmt.number(value, { notation: 'compact', maximumFractionDigits: 1 }),
    [fmt]
  );

  // The bare month ("Úno"): `fmt.month` adds the year unless it is cleared, and then Czech writes
  // the long month
  const axisLabels = bars.map((bar, index) =>
    index % LABEL_EVERY === 0
      ? fmt.month(new Date(bar.month * 1000), { month: 'short', year: undefined })
      : ''
  );

  const chart = useMemo(() => {
    if (width <= 0) return null;
    return (
      <BarChart
        width={width}
        height={HEIGHT}
        data={bars}
        margin={MARGIN}
        barGap={2}
        barCategoryGap="30%"
        // The month buttons are the way in for the keyboard and assistive technology
        accessibilityLayer={false}
      >
        <CartesianGrid vertical={false} stroke={tk.grid} strokeDasharray="0" />
        <XAxis dataKey="month" hide />
        <YAxis
          orientation="right"
          width={AXIS_WIDTH}
          domain={domain}
          ticks={ticks}
          axisLine={false}
          tickLine={false}
          tickMargin={10}
          tick={{ fill: tk.axis, fontSize: 10, fontWeight: 500 }}
          tickFormatter={tickFormatter}
        />
        <Bar
          dataKey="income"
          name={incomeName}
          fill={tk.series[0]}
          radius={[3, 3, 0, 0]}
          maxBarSize={14}
          isAnimationActive={false}
        >
          {bars.map((bar) => (
            <Cell key={bar.month} fillOpacity={bar.muted ? MUTED_OPACITY : 1} />
          ))}
        </Bar>
        <Bar
          dataKey="expenses"
          name={expenseName}
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
  }, [width, bars, tk, domain, ticks, tickFormatter, incomeName, expenseName]);

  return (
    <div>
      <div ref={wrapRef} className="relative" style={{ height: HEIGHT }}>
        <div role="img" aria-label={label} className="[&_.recharts-surface]:overflow-visible">
          {chart}
        </div>
        {/* Over the plot, outside the image: one transparent button per month */}
        <div className="absolute left-0 top-0 flex" style={{ width: plotWidth, height: HEIGHT }}>
          {bars.map((bar) => {
            const future = bar.month > currentMonth;
            return (
              <button
                key={bar.month}
                type="button"
                aria-label={monthLabel(bar.month)}
                aria-pressed={future ? undefined : !bar.muted}
                disabled={future}
                onClick={() => onSelectMonth(bar.month)}
                className="h-full min-w-0 flex-1 rounded-r1 [-webkit-tap-highlight-color:transparent] focus-visible:outline-none focus-visible:shadow-focus"
              />
            );
          })}
        </div>
      </div>
      {chart && <CategoryAxis labels={axisLabels} centered />}
      <ScaledChartLegend
        className="mt-2.5"
        items={[
          { label: incomeName, swatch: { kind: 'block', color: 'var(--s1)' } },
          { label: expenseName, swatch: { kind: 'block', color: 'var(--s3)' } },
        ]}
      />
    </div>
  );
}
