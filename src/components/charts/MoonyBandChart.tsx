import { useMemo, useState, type ReactNode } from 'react';
import {
  Area,
  CartesianGrid,
  ComposedChart,
  Line,
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
import { AXIS_WIDTH } from '@/components/charts/ChartAxis';
import { cn } from '@/lib/utils';

export interface BandPoint {
  /** Position on the x axis (index, year, month number) — evenly spaced. */
  x: number;
  /** Expected value (solid line). */
  value: number;
  /** Scenario band. */
  lo: number;
  hi: number;
  /** Dashed reference, e.g. "today + contributions only". */
  base?: number;
}

export interface BandMilestone {
  x: number;
  title: ReactNode;
  lines?: ReactNode[];
}

export interface MoonyBandChartProps {
  points: BandPoint[];
  height?: number;
  formatValue: (value: number) => string;
  formatTick?: (value: number) => string;
  /** Axis labels under the chart, evenly spaced (years). */
  axisLabels: string[];
  renderTip?: (point: BandPoint) => { title: ReactNode; lines?: ReactNode[] };
  milestones?: BandMilestone[];
  className?: string;
}

/**
 * Expected line with a scenario band (design system §7 Outlook): band in s4
 * at 38 %, dashed base in `chart-cost`, milestones as white rings with a
 * dashed drop line and their own tooltip. Zero baseline.
 */
export function MoonyBandChart({
  points,
  height = 200,
  formatValue,
  formatTick,
  axisLabels,
  renderTip,
  milestones = [],
  className,
}: MoonyBandChartProps) {
  const fmt = useFormat();
  const tk = useMemo(chartTokens, []);
  const [hoverMilestone, setHoverMilestone] = useState<{
    m: BandMilestone;
    x: number;
    y: number;
  } | null>(null);
  const [width, setWidth] = useState(0);
  const gutter = AXIS_WIDTH + 4;

  const data = useMemo(
    () => points.map((p) => ({ ...p, band: [p.lo, p.hi] as [number, number] })),
    [points]
  );
  const domain = useMemo(
    () =>
      valueDomain(
        points.flatMap((p) => [p.hi, p.lo, p.value, ...(p.base !== undefined ? [p.base] : [])]),
        { zeroBaseline: true, headroomAbove: 0.06 }
      ),
    [points]
  );
  const ticks = useMemo(() => niceTicks(domain[0], domain[1], 4), [domain]);
  const tickFormatter =
    formatTick ?? ((v: number) => fmt.number(v, { notation: 'compact', maximumFractionDigits: 1 }));
  const x0 = points[0]?.x ?? 0;
  const x1 = points[points.length - 1]?.x ?? x0;
  const last = points[points.length - 1];
  const hasBase = points.some((p) => p.base !== undefined);
  const valueAtX = (x: number) => points.find((p) => p.x === x)?.value ?? 0;

  if (points.length === 0) return <div className={className} style={{ height }} />;

  return (
    <div className={cn('relative', className)}>
      <div style={{ height }} className="[&_.recharts-surface]:overflow-visible">
        <ResponsiveContainer width="100%" height="100%" onResize={(w) => setWidth(w)} debounce={50}>
          <ComposedChart data={data} margin={{ top: 16, right: 0, bottom: 6, left: 0 }}>
            <CartesianGrid vertical={false} stroke={tk.grid} strokeDasharray="0" />
            <XAxis dataKey="x" type="number" domain={[x0, x1]} hide scale="linear" />
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
              active={hoverMilestone ? false : undefined}
              wrapperStyle={{ outline: 'none', zIndex: 6 }}
              content={({ active, payload }) => {
                const p = payload?.[0]?.payload as BandPoint | undefined;
                if (!active || !p || hoverMilestone) return null;
                const content = renderTip?.(p) ?? {
                  title: formatValue(p.value),
                  lines: [`${formatValue(p.lo)} – ${formatValue(p.hi)}`],
                };
                return <ChartTip {...content} />;
              }}
            />
            <Area
              type="linear"
              dataKey="band"
              stroke="none"
              fill={tk.series[3]}
              fillOpacity={0.38}
              isAnimationActive={false}
              activeDot={false}
            />
            {hasBase && (
              <Line
                type="linear"
                dataKey="base"
                stroke={tk.cost}
                strokeWidth={1.5}
                strokeDasharray="3 4"
                dot={false}
                activeDot={false}
                isAnimationActive={false}
              />
            )}
            <Line
              type="linear"
              dataKey="value"
              stroke={tk.line}
              strokeWidth={2.25}
              strokeLinecap="round"
              strokeLinejoin="round"
              dot={false}
              activeDot={{ r: 4, fill: tk.paper, stroke: tk.line, strokeWidth: 2 }}
              isAnimationActive={false}
            />
            {milestones.map((m, i) => (
              <ReferenceLine
                key={`ml-${i}`}
                x={m.x}
                stroke={tk.crosshair}
                strokeDasharray="2 3"
                strokeWidth={1}
              />
            ))}
            {milestones.map((m, i) => (
              <ReferenceDot
                key={`md-${i}`}
                x={m.x}
                y={valueAtX(m.x)}
                r={4.5}
                fill={tk.paper}
                stroke={tk.line}
                strokeWidth={2}
                ifOverflow="visible"
                className="cursor-pointer"
                onMouseEnter={(_props: unknown, e: unknown) => {
                  const target = (e as React.MouseEvent).currentTarget as SVGCircleElement | null;
                  const cx = Number(target?.getAttribute('cx') ?? 0);
                  const cy = Number(target?.getAttribute('cy') ?? 0);
                  setHoverMilestone({ m, x: cx, y: cy });
                }}
                onMouseLeave={() => setHoverMilestone(null)}
              />
            ))}
            {last && (
              <ReferenceDot
                x={last.x}
                y={last.value}
                r={4.5}
                fill={tk.paper}
                stroke={tk.line}
                strokeWidth={2}
                ifOverflow="visible"
              />
            )}
          </ComposedChart>
        </ResponsiveContainer>
        {hoverMilestone && (
          <FloatingTip x={hoverMilestone.x} y={hoverMilestone.y + 16} width={width}>
            <ChartTip title={hoverMilestone.m.title} lines={hoverMilestone.m.lines} />
          </FloatingTip>
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
    </div>
  );
}
