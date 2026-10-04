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
import { cn } from '@/lib/utils';

export interface LineSeries {
  id: string;
  name: string;
  /** Values on a shared x grid (same length for every series). */
  values: (number | null)[];
  /** Dashed reference series in `chart-cost` (e.g. the whole portfolio). */
  dashed?: boolean;
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
  className?: string;
}

/** Width of the right gutter that holds the direct end labels ("Růst + 12,4 %"). */
const LABEL_GUTTER = 96;
const PAD_TOP = 14;
const PAD_BOTTOM = 6;
const LABEL_STEP = 13;

/**
 * Up to four monochrome series (s1…s4) plus a dashed reference, with direct
 * end labels instead of a legend (design system §8: light series always carry
 * a label with a value). One value axis, light grid, hover across all series.
 */
export function MoonyLinesChart({
  series,
  axisLabels,
  tipLabel,
  formatValue,
  formatTick,
  height = 180,
  className,
}: MoonyLinesChartProps) {
  const tk = useMemo(chartTokens, []);
  const [hovered, setHovered] = useState<string | null>(null);
  const n = series[0]?.values.length ?? 0;

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

  // Colors: solid series take s1…s4 in order, dashed ones the cost grey
  let solid = 0;
  const colorOf = series.map((s) => (s.dashed ? tk.cost : tk.series[Math.min(solid++, 3)]));

  // Direct end labels in the right gutter, pushed apart by 13 px when they would overlap
  const plotH = height - PAD_TOP - PAD_BOTTOM;
  const yOf = (v: number) => PAD_TOP + (1 - (v - domain[0]) / (domain[1] - domain[0])) * plotH;
  const labels = series
    .map((s, k) => {
      const lastValue = [...s.values].reverse().find((v): v is number => v !== null);
      return lastValue === undefined ? null : { k, s, value: lastValue, y: yOf(lastValue) };
    })
    .filter((l): l is NonNullable<typeof l> => l !== null)
    .sort((a, b) => a.y - b.y);
  for (let i = 1; i < labels.length; i++) {
    if (labels[i].y - labels[i - 1].y < LABEL_STEP) labels[i].y = labels[i - 1].y + LABEL_STEP;
  }

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
              width={LABEL_GUTTER}
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
                    lines={series.map((s) => {
                      const v = s.values[index];
                      return v === null || v === undefined ? null : `${s.name} ${formatValue(v)}`;
                    })}
                  />
                );
              }}
            />
            {series.map((s, k) => (
              <Line
                key={s.id}
                type="linear"
                dataKey={s.id}
                stroke={colorOf[k]}
                strokeWidth={s.dashed ? 1.5 : k === 0 ? 2.25 : 2}
                strokeDasharray={s.dashed ? '3 4' : undefined}
                strokeOpacity={hovered && hovered !== s.id ? 0.35 : 1}
                strokeLinecap="round"
                strokeLinejoin="round"
                dot={false}
                activeDot={{ r: 3.5, fill: tk.paper, stroke: colorOf[k], strokeWidth: 2 }}
                isAnimationActive={false}
                connectNulls
              />
            ))}
          </ComposedChart>
        </ResponsiveContainer>
        {/* Direct end labels, over the tick gutter */}
        <div
          aria-hidden
          className="pointer-events-none absolute inset-y-0 right-0"
          style={{ width: LABEL_GUTTER }}
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
                style={{ borderColor: colorOf[l.k] }}
              />
              {l.s.name} {formatValue(l.value)}
            </span>
          ))}
        </div>
      </div>
      <div
        aria-hidden
        className="mt-1.5 flex justify-between text-micro font-500 text-chart-axis"
        style={{ paddingRight: LABEL_GUTTER }}
      >
        {axisLabels.map((l, i) => (
          <span key={i}>{l}</span>
        ))}
      </div>
    </div>
  );
}
