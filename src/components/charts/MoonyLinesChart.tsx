import { useMemo, useState, type ReactNode } from 'react';
import {
  CartesianGrid,
  ComposedChart,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { chartTokens } from '@/lib/tokens';
import { niceTicks, valueDomain } from '@/utils/chart-scale';
import { ChartTip } from '@/components/charts/ChartTip';
import { ChartLegend, type LegendItem } from '@/components/charts/ChartLegend';
import { cn } from '@/lib/utils';

export interface LineSeries {
  id: string;
  name: string;
  /** Values on a shared x grid (same length for every series). */
  values: (number | null)[];
  /**
   * The benchmark the other series are read against (e.g. the whole portfolio): the dark
   * `chart-line`, 2 px, dashed, drawn on top of the others and listed first.
   */
  reference?: boolean;
}

export interface MoonyLinesChartProps {
  series: LineSeries[];
  /** Axis labels under the chart, evenly spaced. */
  axisLabels: string[];
  /** Label of the x position for the tooltip title. */
  tipLabel?: (index: number) => ReactNode;
  formatValue: (value: number) => string;
  formatTick?: (value: number) => string;
  height?: number;
  /**
   * Where each series shows its name and last value: `'end'` (default) labels the line ends in
   * the right gutter; `'legend'` lists them in a row under the chart, which keeps long names
   * away from the value axis.
   */
  labels?: 'end' | 'legend';
  className?: string;
}

/** Width of the right gutter that holds the direct end labels ("Růst + 12,4 %"). */
const LABEL_GUTTER = 96;
/** The gutter when the names live in a legend: it only holds the value ticks. */
const TICK_GUTTER = 58;
const PAD_TOP = 14;
const PAD_BOTTOM = 6;
const LABEL_STEP = 13;
/**
 * Two legend rows (14 px lines, 6 px apart): a legend that wraps differently after a filter
 * changed the number of series must not resize the card around the chart.
 */
const LEGEND_MIN_HEIGHT = 34;

function lastValueOf(s: LineSeries): number | undefined {
  return [...s.values].reverse().find((v): v is number => v !== null);
}

/**
 * Up to four monochrome series (s1…s4) plus a dashed reference, each with its name and last
 * value either directly at the line end or in a legend row (design system §8: light series
 * always carry a label with a value). One value axis, light grid, hover across all series.
 */
export function MoonyLinesChart({
  series,
  axisLabels,
  tipLabel,
  formatValue,
  formatTick,
  height = 180,
  labels: labelMode = 'end',
  className,
}: MoonyLinesChartProps) {
  const tk = useMemo(chartTokens, []);
  const [hovered, setHovered] = useState<string | null>(null);
  const n = series[0]?.values.length ?? 0;
  const gutter = labelMode === 'legend' ? TICK_GUTTER : LABEL_GUTTER;

  const data = useMemo(
    () =>
      Array.from({ length: n }, (_, i) => {
        const row: Record<string, number | null> = { x: i };
        for (const s of series) row[s.id] = s.values[i] ?? null;
        return row;
      }),
    [series, n]
  );

  const domain = useMemo(
    () =>
      valueDomain(
        series.flatMap((s) => s.values.filter((v): v is number => v !== null)),
        { headroomBelow: 0.12, headroomAbove: 0.08 }
      ),
    [series]
  );
  const ticks = useMemo(() => niceTicks(domain[0], domain[1], 4), [domain]);
  const tickFormatter = formatTick ?? formatValue;

  // The reference is drawn last (on top) and listed first, the others keep their order
  const drawn = useMemo(
    () => [...series.filter((s) => !s.reference), ...series.filter((s) => s.reference)],
    [series]
  );
  const listed = useMemo(
    () => [...series.filter((s) => s.reference), ...series.filter((s) => !s.reference)],
    [series]
  );

  // Colors: solid series take s1…s4 in order, the reference the chart line
  const colorOf = useMemo(() => {
    const colors = new Map<string, string>();
    let solid = 0;
    for (const s of series)
      colors.set(s.id, s.reference ? tk.line : tk.series[Math.min(solid++, 3)]);
    return colors;
  }, [series, tk]);
  const firstSolidId = series.find((s) => !s.reference)?.id;

  // Direct end labels in the right gutter, pushed apart by 13 px when they would overlap
  const plotH = height - PAD_TOP - PAD_BOTTOM;
  const yOf = (v: number) => PAD_TOP + (1 - (v - domain[0]) / (domain[1] - domain[0])) * plotH;
  const labels =
    labelMode === 'end'
      ? series
          .map((s) => {
            const lastValue = lastValueOf(s);
            return lastValue === undefined ? null : { s, value: lastValue, y: yOf(lastValue) };
          })
          .filter((l): l is NonNullable<typeof l> => l !== null)
          .sort((a, b) => a.y - b.y)
      : [];
  for (let i = 1; i < labels.length; i++) {
    if (labels[i].y - labels[i - 1].y < LABEL_STEP) labels[i].y = labels[i - 1].y + LABEL_STEP;
  }

  const legendItems: LegendItem[] =
    labelMode === 'legend'
      ? listed.map((s) => {
          const lastValue = lastValueOf(s);
          return {
            swatch: s.reference
              ? { kind: 'dash', color: tk.line }
              : { kind: 'line', color: colorOf.get(s.id) },
            label: (
              <span onMouseEnter={() => setHovered(s.id)} onMouseLeave={() => setHovered(null)}>
                {s.name}
                {lastValue !== undefined && (
                  <>
                    {' '}
                    <b className="font-650 text-ink num">{formatValue(lastValue)}</b>
                  </>
                )}
              </span>
            ),
          };
        })
      : [];

  if (n === 0) return <div className={className} style={{ height }} />;

  return (
    <div className={cn('relative', className)}>
      <div style={{ height }} className="relative [&_.recharts-surface]:overflow-visible">
        <ResponsiveContainer width="100%" height="100%">
          <ComposedChart
            data={data}
            margin={{ top: PAD_TOP, right: 0, bottom: PAD_BOTTOM, left: 0 }}
          >
            <CartesianGrid vertical={false} stroke={tk.grid} strokeDasharray="0" />
            <XAxis dataKey="x" type="number" domain={[0, n - 1]} hide scale="linear" />
            <YAxis
              orientation="right"
              width={gutter}
              domain={domain}
              ticks={ticks}
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              tick={{ fill: tk.axis, fontSize: 10, fontWeight: 500 }}
              tickFormatter={tickFormatter}
            />
            <Tooltip
              cursor={{ stroke: tk.crosshair, strokeDasharray: '2 3', strokeWidth: 1 }}
              isAnimationActive={false}
              wrapperStyle={{ outline: 'none', zIndex: 6 }}
              content={({ active, payload, label }) => {
                if (!active || !payload?.length) return null;
                const index = Number(label);
                return (
                  <ChartTip
                    title={tipLabel ? tipLabel(index) : (axisLabels[index] ?? '')}
                    lines={listed.map((s) => {
                      const v = s.values[index];
                      return v === null || v === undefined ? null : `${s.name} ${formatValue(v)}`;
                    })}
                  />
                );
              }}
            />
            {drawn.map((s) => (
              <Line
                key={s.id}
                type="linear"
                dataKey={s.id}
                stroke={colorOf.get(s.id)}
                strokeWidth={s.reference || s.id !== firstSolidId ? 2 : 2.25}
                strokeDasharray={s.reference ? '4 4' : undefined}
                strokeOpacity={hovered && hovered !== s.id ? 0.35 : 1}
                // Round caps would shrink the 4 px gaps of the dashes to 2 px
                strokeLinecap={s.reference ? 'butt' : 'round'}
                strokeLinejoin="round"
                dot={false}
                activeDot={{ r: 3.5, fill: tk.paper, stroke: colorOf.get(s.id), strokeWidth: 2 }}
                isAnimationActive={false}
                connectNulls
              />
            ))}
          </ComposedChart>
        </ResponsiveContainer>
        {/* Direct end labels, over the tick gutter */}
        {labelMode === 'end' && (
          <div
            aria-hidden
            className="pointer-events-none absolute inset-y-0 right-0"
            style={{ width: gutter }}
          >
            {labels.map((l) => (
              <span
                key={l.s.id}
                className="pointer-events-auto absolute left-[9px] -translate-y-1/2 whitespace-nowrap text-micro font-650 text-ink-2 num"
                style={{ top: l.y }}
                onMouseEnter={() => setHovered(l.s.id)}
                onMouseLeave={() => setHovered(null)}
              >
                <i
                  className="mr-1.5 inline-block size-[7px] -translate-y-px rounded-full border-2 bg-paper align-middle"
                  style={{ borderColor: colorOf.get(l.s.id) }}
                />
                {l.s.name} {formatValue(l.value)}
              </span>
            ))}
          </div>
        )}
      </div>
      <div
        aria-hidden
        className="mt-1.5 flex justify-between text-micro font-500 text-chart-axis"
        style={{ paddingRight: gutter }}
      >
        {axisLabels.map((l, i) => (
          <span key={i}>{l}</span>
        ))}
      </div>
      {labelMode === 'legend' && (
        <div className="mt-2.5" style={{ minHeight: LEGEND_MIN_HEIGHT }}>
          <ChartLegend items={legendItems} className="mt-0 gap-y-1.5" />
        </div>
      )}
    </div>
  );
}
