import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Bar,
  CartesianGrid,
  Cell,
  ComposedChart,
  LabelList,
  Line,
  ReferenceDot,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { useFormat } from '@/lib/use-format';
import { useChartTokens } from '@/theme/ThemeProvider';
import { niceTicks, valueDomain } from '@/utils/chart-scale';
import { monthDate } from '@/utils/spending-plans';
import { ChartTip } from '@/components/charts/ChartTip';
import { AXIS_WIDTH, CategoryAxis } from '@/components/charts/ChartAxis';

interface PlanSpendChartProps {
  /** "YYYY-MM" months, one bar each (`planMonthAxis`). */
  axis: string[];
  /** Net spend per month in `currency`, aligned with `axis`. */
  values: number[];
  /** Running total to today's month (null after it); omit to hide the line. */
  cumulative?: (number | null)[];
  /** Planned amount drawn as a dashed line; omit to hide it. */
  planned?: number;
  currency: string;
  /** The plan year: axis labels of other years carry the year. */
  year: number;
  /** "YYYY-MM" of today: later months are shaded as the rest of the year. */
  currentMonth: string;
  selectedMonth: string | null;
  onMonthClick: (month: string) => void;
}

const MUTED_OPACITY = 0.35;
const HEIGHT = 210;

/**
 * "Útrata v čase" (spec §9 "Detail page"): net spend per month as `s3` bars with value labels, the
 * running total as the `chart-line` line up to today's month, the planned amount as a dashed
 * `chart-cost` line, and the months after today shaded. A bar (or its month under the axis)
 * toggles the month filter; the other bars fade while one is chosen.
 */
export function PlanSpendChart({
  axis,
  values,
  cumulative,
  planned,
  currency,
  year,
  currentMonth,
  selectedMonth,
  onMonthClick,
}: PlanSpendChartProps) {
  const { t } = useTranslation('spending_plans');
  const fmt = useFormat();
  const tk = useChartTokens();

  const data = useMemo(
    () =>
      axis.map((month, i) => ({ month, spent: values[i] ?? 0, total: cumulative?.[i] ?? null })),
    [axis, values, cumulative]
  );
  const domain = useMemo(() => {
    const totals = (cumulative ?? []).filter((v): v is number => v !== null);
    return valueDomain([...values, ...totals, planned ?? 0], {
      zeroBaseline: true,
      headroomAbove: 0.14,
    });
  }, [values, cumulative, planned]);
  const ticks = useMemo(() => niceTicks(domain[0], domain[1], 4), [domain]);
  const compact = (v: number) => fmt.number(v, { notation: 'compact', maximumFractionDigits: 1 });
  const money = (v: number) => fmt.money(v, currency);
  const monthName = (month: string) =>
    fmt.month(monthDate(month), { month: 'long', year: 'numeric' });
  const monthInSentence = (month: string) =>
    fmt.monthInSentence(monthDate(month), { month: 'long', year: 'numeric' });

  const lastIndex = cumulative ? cumulative.map((v) => v !== null).lastIndexOf(true) : -1;
  const last = lastIndex >= 0 && cumulative ? cumulative[lastIndex] : null;
  const futureFrom = axis.findIndex((month) => month > currentMonth);
  const labels = axis.map((month, i) => {
    const short = fmt.month(monthDate(month), { month: 'short' });
    const y = Number(month.slice(0, 4));
    return y !== year && (i === 0 || month.endsWith('-01'))
      ? `${short} ${String(y).slice(2)}`
      : short;
  });

  return (
    <div>
      <div style={{ height: HEIGHT }} className="relative [&_.recharts-surface]:overflow-visible">
        {futureFrom >= 0 && (
          <div
            aria-hidden
            className="pointer-events-none absolute bottom-0 top-1 rounded-r2 bg-well"
            style={{
              left: `calc((100% - ${AXIS_WIDTH}px) * ${futureFrom / axis.length})`,
              right: AXIS_WIDTH,
            }}
          >
            <span className="absolute right-2 top-1.5 text-micro font-600 text-ink-4">
              {t('detail.chart.future')}
            </span>
          </div>
        )}
        {/* Positioned after the shading, so the chart paints over it */}
        <div className="relative h-full">
          <ResponsiveContainer width="100%" height="100%" debounce={50}>
            <ComposedChart
              data={data}
              margin={{ top: 18, right: 0, bottom: 0, left: 0 }}
              barCategoryGap="30%"
              onClick={(state) => {
                // Recharts 3 delivers the active index as a string ("3")
                const raw = state?.activeTooltipIndex;
                const index = typeof raw === 'string' ? Number(raw) : raw;
                if (typeof index === 'number' && axis[index]) onMonthClick(axis[index]);
              }}
              style={{ cursor: 'pointer' }}
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
                tickFormatter={compact}
              />
              <ReferenceLine y={0} stroke={tk.baseline} />
              <Tooltip
                cursor={{ fill: tk.grid, fillOpacity: 0.6 }}
                isAnimationActive={false}
                wrapperStyle={{ outline: 'none', zIndex: 6 }}
                content={({ active, payload }) => {
                  const row = payload?.[0]?.payload as (typeof data)[number] | undefined;
                  if (!active || !row) return null;
                  const lines = [`${t('detail.chart.month')} ${money(row.spent)}`];
                  if (row.total !== null) {
                    lines.push(`${t('detail.chart.cumulative')} ${money(row.total)}`);
                  }
                  return <ChartTip title={monthName(row.month)} lines={lines} />;
                }}
              />
              <Bar
                dataKey="spent"
                fill={tk.series[2]}
                radius={[3, 3, 0, 0]}
                maxBarSize={30}
                isAnimationActive={false}
              >
                {data.map((row) => (
                  <Cell
                    key={row.month}
                    fillOpacity={selectedMonth && row.month !== selectedMonth ? MUTED_OPACITY : 1}
                  />
                ))}
                <LabelList
                  dataKey="spent"
                  position="top"
                  offset={6}
                  fill={tk.axis}
                  fontSize={10}
                  fontWeight={600}
                  formatter={(value) =>
                    typeof value === 'number' && value !== 0 ? compact(value) : ''
                  }
                />
              </Bar>
              {planned !== undefined && (
                <ReferenceLine
                  y={planned}
                  stroke={tk.cost}
                  strokeDasharray="4 4"
                  strokeWidth={1.5}
                  label={{
                    value: t('detail.chart.plannedLine', { amount: money(planned) }),
                    position: 'insideTopLeft',
                    fill: tk.ink2,
                    fontSize: 10,
                    fontWeight: 650,
                  }}
                />
              )}
              {cumulative && (
                <Line
                  dataKey="total"
                  stroke={tk.line}
                  strokeWidth={2.25}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  dot={false}
                  activeDot={{ r: 4, fill: tk.paper, stroke: tk.line, strokeWidth: 2 }}
                  isAnimationActive={false}
                />
              )}
              {last !== null && (
                <ReferenceDot
                  x={axis[lastIndex]}
                  y={last}
                  r={4.5}
                  fill={tk.paper}
                  stroke={tk.line}
                  strokeWidth={2}
                  ifOverflow="visible"
                  label={{
                    value: money(last),
                    position: 'left',
                    offset: 10,
                    fill: tk.ink2,
                    fontSize: 10,
                    fontWeight: 650,
                  }}
                />
              )}
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      </div>
      <CategoryAxis
        labels={labels}
        actions={axis.map((month) => ({
          label: t('detail.chart.filterMonth', { month: monthInSentence(month) }),
          onSelect: () => onMonthClick(month),
          pressed: month === selectedMonth,
        }))}
      />
    </div>
  );
}
