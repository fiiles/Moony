import { useId, useMemo, useState, type ReactNode } from 'react';
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
import {
  clusterEvents,
  niceTicks,
  valueAt,
  valueDomain,
  type ChartEvent,
  type EventCluster,
} from '@/utils/chart-scale';
import { ChartTip, FloatingTip } from '@/components/charts/ChartTip';
import { AXIS_WIDTH, TimeAxis } from '@/components/charts/ChartAxis';
import { EventMark } from '@/components/charts/EventMarkers';
import { cn } from '@/lib/utils';

export interface LinePoint {
  /** Unix seconds. */
  t: number;
  value: number;
  /** Cost basis ("Vloženo"), drawn as a dashed step line when present. */
  cost?: number;
}

export interface LineTip {
  title: ReactNode;
  lines?: ReactNode[];
}

export interface MoonyLineChartProps<E extends ChartEvent = ChartEvent> {
  points: LinePoint[];
  /** Zero baseline for "Vše"; otherwise truncated with 15 % headroom below (README §8). */
  zeroBaseline?: boolean;
  height?: number;
  /** Money formatter for ticks and the default tooltip (caller decides the currency). */
  formatValue: (value: number) => string;
  /** Compact formatter for the value ticks ("250 tis."); defaults to the locale's compact number. */
  formatTick?: (value: number) => string;
  /** Tooltip for a hovered point; default: value + date (+ cost and return when `cost` exists). */
  renderTip?: (point: LinePoint) => LineTip;
  /** Horizontal reference ("Cílová cena", a target line). */
  reference?: { value: number; label?: string; color?: string };
  /** Time-trace events (single-entity charts only, README §9). */
  events?: E[];
  renderEventTip?: (cluster: EventCluster<E>) => LineTip;
  /** Id of the event highlighted from outside (hovered table row). */
  hotEventId?: string | null;
  onHotEventChange?: (id: string | null) => void;
  onEventClick?: (cluster: EventCluster<E>) => void;
  /** Hide the time axis under the chart (when the caller draws its own). */
  hideAxis?: boolean;
  /**
   * Unix seconds from which the series is a plan (a schedule, a forecast): points
   * after it are drawn dashed, the area fade and the end ring stop at the last
   * actual point (README §8 "projected").
   */
  projectedFrom?: number;
  className?: string;
}

/** Above Recharts' active dot (1200) and cursor line (1100), below labels (2000). */
const MARK_Z_INDEX = 1300;

interface Row extends LinePoint {
  cost?: number;
  /** The actual part of the series (all of it without `projectedFrom`). */
  actual?: number;
  /** The projected part, sharing the split point with `actual`. */
  projected?: number;
}

/** Rows split at `projectedFrom`, with an interpolated reading at the split when none exists. */
function splitRows(points: LinePoint[], projectedFrom: number | undefined): Row[] {
  if (projectedFrom === undefined) return points.map((p) => ({ ...p, actual: p.value }));
  const withSplit =
    points.length > 1 &&
    projectedFrom > points[0].t &&
    projectedFrom < points[points.length - 1].t &&
    !points.some((p) => p.t === projectedFrom)
      ? [...points, { t: projectedFrom, value: valueAt(points, projectedFrom) }].sort(
          (a, b) => a.t - b.t
        )
      : points;
  return withSplit.map((p) => ({
    ...p,
    actual: p.t <= projectedFrom ? p.value : undefined,
    projected: p.t >= projectedFrom ? p.value : undefined,
  }));
}

/**
 * Value line over time (design system §8): one right-hand value axis with
 * nice ticks, light grid, 2.25 px line in `chart-line` with an area fade,
 * dashed "Vloženo" step line, last point as a white ring, crosshair and dark
 * tooltip on hover. With `events` it carries the time trace (§9).
 */
export function MoonyLineChart<E extends ChartEvent = ChartEvent>({
  points,
  zeroBaseline = false,
  height = 200,
  formatValue,
  formatTick,
  renderTip,
  reference,
  events,
  renderEventTip,
  hotEventId = null,
  onHotEventChange,
  onEventClick,
  hideAxis = false,
  projectedFrom,
  className,
}: MoonyLineChartProps<E>) {
  const fmt = useFormat();
  const tk = useMemo(chartTokens, []);
  const gradientId = useId().replace(/:/g, '');
  const [hover, setHover] = useState<{ cluster: EventCluster<E>; x: number; y: number } | null>(
    null
  );
  const [width, setWidth] = useState(0);

  const data: Row[] = useMemo(() => splitRows(points, projectedFrom), [points, projectedFrom]);
  const t0 = data[0]?.t ?? 0;
  const t1 = data[data.length - 1]?.t ?? t0;
  const hasCost = data.some((p) => p.cost !== undefined);
  const hasProjected = data.some((p) => p.projected !== undefined);

  const domain = useMemo(() => {
    const values = data.flatMap((p) => (p.cost !== undefined ? [p.value, p.cost] : [p.value]));
    if (reference) values.push(reference.value);
    return valueDomain(values, { zeroBaseline });
  }, [data, zeroBaseline, reference]);
  const ticks = useMemo(() => niceTicks(domain[0], domain[1], 4), [domain]);

  const clusters = useMemo(
    () => (events && events.length > 0 ? clusterEvents(events, t0, t1) : []),
    [events, t0, t1]
  );
  const markers = useMemo(
    () => clusters.map((c) => ({ t: c.t, y: valueAt(data, c.t), cluster: c })),
    [clusters, data]
  );

  const tickFormatter =
    formatTick ?? ((v: number) => fmt.number(v, { notation: 'compact', maximumFractionDigits: 1 }));
  // The end ring sits on the last actual reading ("Dnes" when the rest is a plan).
  const last = [...data].reverse().find((p) => p.actual !== undefined);

  const defaultTip = (p: LinePoint): LineTip => {
    const lines: ReactNode[] = [fmt.day(p.t)];
    if (p.cost !== undefined) {
      lines.push(formatValue(p.cost));
      lines.push(formatValue(p.value - p.cost));
    }
    return { title: formatValue(p.value), lines };
  };
  const tip = renderTip ?? defaultTip;

  const clearHover = () => {
    setHover(null);
    onHotEventChange?.(null);
  };

  if (data.length === 0) return <div className={className} style={{ height }} />;

  return (
    <div className={cn('relative', className)}>
      <div
        style={{ height }}
        className="[&_.recharts-surface]:overflow-visible"
        onMouseLeave={() => {
          // Safety net: a mark that vanished under the pointer never fires its own leave.
          if (hover) clearHover();
        }}
      >
        <ResponsiveContainer width="100%" height="100%" onResize={(w) => setWidth(w)} debounce={50}>
          <ComposedChart data={data} margin={{ top: 14, right: 0, bottom: 6, left: 0 }}>
            <defs>
              <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor={tk.area} stopOpacity={0.8} />
                <stop offset="1" stopColor={tk.areaEnd} stopOpacity={0} />
              </linearGradient>
            </defs>
            <CartesianGrid vertical={false} stroke={tk.grid} strokeDasharray="0" />
            <XAxis
              dataKey="t"
              type="number"
              domain={[t0, t1]}
              hide
              allowDataOverflow
              scale="linear"
            />
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
              allowDataOverflow
            />
            <Tooltip
              cursor={{ stroke: tk.crosshair, strokeDasharray: '2 3', strokeWidth: 1 }}
              isAnimationActive={false}
              active={hover ? false : undefined}
              wrapperStyle={{ outline: 'none', zIndex: 6 }}
              content={({ active, payload }) => {
                const row = payload?.[0]?.payload as Row | undefined;
                if (!active || !row || hover) return null;
                const { title, lines } = tip(row);
                return <ChartTip title={title} lines={lines} />;
              }}
            />
            <Area
              type="linear"
              dataKey="actual"
              stroke="none"
              fill={`url(#${gradientId})`}
              isAnimationActive={false}
              activeDot={false}
              baseValue={domain[0]}
            />
            {hasCost && (
              <Line
                type="stepAfter"
                dataKey="cost"
                stroke={tk.cost}
                strokeWidth={1.5}
                strokeDasharray="3 4"
                dot={false}
                activeDot={false}
                isAnimationActive={false}
                connectNulls
              />
            )}
            <Line
              type="linear"
              dataKey="actual"
              stroke={tk.line}
              strokeWidth={2.25}
              strokeLinecap="round"
              strokeLinejoin="round"
              dot={false}
              activeDot={{ r: 4, fill: tk.paper, stroke: tk.line, strokeWidth: 2 }}
              isAnimationActive={false}
            />
            {hasProjected && (
              <Line
                type="linear"
                dataKey="projected"
                stroke={tk.line}
                strokeWidth={2}
                strokeDasharray="4 4"
                strokeLinecap="round"
                strokeLinejoin="round"
                dot={false}
                activeDot={{ r: 4, fill: tk.paper, stroke: tk.line, strokeWidth: 2 }}
                isAnimationActive={false}
              />
            )}
            {reference && (
              <ReferenceLine
                y={reference.value}
                stroke={reference.color ?? tk.gain}
                strokeDasharray="4 4"
                strokeWidth={1.5}
                ifOverflow="extendDomain"
                label={
                  reference.label
                    ? {
                        value: reference.label,
                        position: 'insideTopRight',
                        fill: reference.color ?? tk.gain,
                        fontSize: 10,
                        fontWeight: 650,
                      }
                    : undefined
                }
              />
            )}
            {last && (
              <ReferenceDot
                x={last.t}
                y={last.value}
                r={4.5}
                fill={tk.paper}
                stroke={tk.line}
                strokeWidth={2}
                ifOverflow="visible"
              />
            )}
            {/* Marks are reference dots, not a Scatter: in Recharts 3 a graphical item with its
                own `data` replaces the chart data as the axis data, so the tooltip would resolve
                against the marks and stop following the line. Reference elements hold no data.
                They sit above the active dot (z-index 1200), which would otherwise cover a mark
                that falls on a data point and swallow its hover. */}
            {markers.map(({ t, y, cluster }) => {
              const hot =
                hover?.cluster.id === cluster.id ||
                (hotEventId !== null && cluster.events.some((e) => e.id === hotEventId));
              return (
                <ReferenceDot
                  key={cluster.id}
                  x={t}
                  y={y}
                  r={8}
                  ifOverflow="visible"
                  zIndex={MARK_Z_INDEX}
                  shape={({ cx, cy }) =>
                    cx === undefined || cy === undefined ? (
                      <g />
                    ) : (
                      <EventMark
                        cx={cx}
                        cy={cy}
                        type={cluster.type}
                        count={cluster.events.length}
                        hot={hot}
                        onEnter={() => {
                          setHover({ cluster, x: cx, y: cy });
                          onHotEventChange?.(cluster.events[0].id);
                        }}
                        onLeave={clearHover}
                        onClick={() => onEventClick?.(cluster)}
                      />
                    )
                  }
                />
              );
            })}
          </ComposedChart>
        </ResponsiveContainer>
        {hover && renderEventTip && (
          <FloatingTip x={hover.x} y={hover.y + 14} width={width}>
            <ChartTip {...renderEventTip(hover.cluster)} />
          </FloatingTip>
        )}
      </div>
      {!hideAxis && <TimeAxis t0={t0} t1={t1} />}
    </div>
  );
}
