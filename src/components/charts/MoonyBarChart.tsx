import { useMemo, useState, type ReactNode } from 'react';
import {
  Bar,
  BarChart,
  CartesianGrid,
  ReferenceDot,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { useFormat } from '@/lib/use-format';
import { chartTokens } from '@/lib/tokens';
import { niceTicks, valueDomain } from '@/utils/chart-scale';
import { ChartTip, FloatingTip } from '@/components/charts/ChartTip';
import { AXIS_WIDTH, CategoryAxis } from '@/components/charts/ChartAxis';
import { cn } from '@/lib/utils';

export interface BarGroup {
  label: string;
  /** First series: income (s1). */
  a: number;
  /** Second series: expense (s3). */
  b: number;
}

/** A milestone ring on top of one bar (e.g. "Jistina převýší úrok"). */
export interface BarMark {
  index: number;
  title: ReactNode;
  lines?: ReactNode[];
}

export interface MoonyBarChartProps {
  bars: BarGroup[];
  marks?: BarMark[];
  /** Names of the two series for the tooltip ("Příjmy", "Výdaje"). */
  names: [string, string];
  /** Stack `b` on top of `a` instead of drawing them side by side. */
  stacked?: boolean;
  height?: number;
  formatValue: (value: number) => string;
  formatTick?: (value: number) => string;
  /** Extra tooltip line, e.g. the balance "bilance + 12 300 Kč". */
  renderExtra?: (bar: BarGroup) => ReactNode;
  className?: string;
}

/**
 * Income/expense bars (design system §8): s1 and s3, 3 px rounded tops, 2 px
 * gap, no borders, zero baseline, right-hand ticks, dark tooltip.
 */
export function MoonyBarChart({
  bars,
  marks = [],
  names,
  stacked = false,
  height = 180,
  formatValue,
  formatTick,
  renderExtra,
  className,
}: MoonyBarChartProps) {
  const fmt = useFormat();
  const tk = useMemo(chartTokens, []);
  const [hoverMark, setHoverMark] = useState<{ m: BarMark; x: number; y: number } | null>(null);
  const [width, setWidth] = useState(0);
  const topOf = (i: number) => (stacked ? bars[i].a + bars[i].b : Math.max(bars[i].a, bars[i].b));
  const domain = useMemo(() => {
    const values = stacked ? bars.map((b) => b.a + b.b) : bars.flatMap((b) => [b.a, b.b]);
    return valueDomain(values, { zeroBaseline: true, headroomAbove: 0.08 });
  }, [bars, stacked]);
  const ticks = useMemo(() => niceTicks(domain[0], domain[1], 4), [domain]);
  const tickFormatter =
    formatTick ?? ((v: number) => fmt.number(v, { notation: 'compact', maximumFractionDigits: 1 }));

  return (
    <div className={cn('relative', className)}>
      <div style={{ height }} className="relative [&_.recharts-surface]:overflow-visible">
        <ResponsiveContainer width="100%" height="100%" onResize={(w) => setWidth(w)} debounce={50}>
          <BarChart
            data={bars}
            margin={{ top: 14, right: 0, bottom: 0, left: 0 }}
            barGap={2}
            barCategoryGap="30%"
          >
            <CartesianGrid vertical={false} stroke={tk.grid} strokeDasharray="0" />
            <XAxis dataKey="label" hide />
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
            <ReferenceLine y={0} stroke={tk.baseline} />
            <Tooltip
              cursor={{ fill: tk.grid, fillOpacity: 0.6 }}
              isAnimationActive={false}
              active={hoverMark ? false : undefined}
              wrapperStyle={{ outline: 'none', zIndex: 6 }}
              content={({ active, payload }) => {
                const bar = payload?.[0]?.payload as BarGroup | undefined;
                if (!active || !bar || hoverMark) return null;
                const lines: ReactNode[] = [
                  `${names[0]} ${formatValue(bar.a)}`,
                  `${names[1]} ${formatValue(bar.b)}`,
                ];
                const extra = renderExtra?.(bar);
                if (extra) lines.push(extra);
                return <ChartTip title={bar.label} lines={lines} />;
              }}
            />
            <Bar
              dataKey="a"
              name={names[0]}
              fill={tk.series[0]}
              radius={stacked ? [2, 2, 0, 0] : [3, 3, 0, 0]}
              maxBarSize={stacked ? 26 : 22}
              stackId={stacked ? 'flow' : undefined}
              isAnimationActive={false}
            />
            <Bar
              dataKey="b"
              name={names[1]}
              fill={tk.series[2]}
              radius={[3, 3, 0, 0]}
              maxBarSize={stacked ? 26 : 22}
              stackId={stacked ? 'flow' : undefined}
              isAnimationActive={false}
            />
            {marks
              .filter((m) => bars[m.index])
              .map((m, i) => (
                <ReferenceDot
                  key={`mark-${i}`}
                  x={bars[m.index].label}
                  y={topOf(m.index)}
                  r={4.5}
                  fill={tk.paper}
                  stroke={tk.line}
                  strokeWidth={2}
                  ifOverflow="visible"
                  className="cursor-pointer"
                  onMouseEnter={(_props: unknown, e: unknown) => {
                    const target = (e as React.MouseEvent).currentTarget as SVGCircleElement | null;
                    setHoverMark({
                      m,
                      x: Number(target?.getAttribute('cx') ?? 0),
                      y: Number(target?.getAttribute('cy') ?? 0),
                    });
                  }}
                  onMouseLeave={() => setHoverMark(null)}
                />
              ))}
          </BarChart>
        </ResponsiveContainer>
        {hoverMark && (
          <FloatingTip x={hoverMark.x} y={hoverMark.y + 16} width={width}>
            <ChartTip title={hoverMark.m.title} lines={hoverMark.m.lines} />
          </FloatingTip>
        )}
      </div>
      <CategoryAxis labels={bars.map((b) => b.label)} />
    </div>
  );
}
