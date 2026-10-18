import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent,
} from 'react';
import { Area, CartesianGrid, ComposedChart, Line, ReferenceDot, XAxis, YAxis } from 'recharts';
import { useFormat } from '@/lib/use-format';
import { chartTokens } from '@/lib/tokens';
import { cn } from '@/lib/utils';
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
import type { LinePoint, LineTip } from '@/components/charts/MoonyLineChart';
import { useHideAmounts } from '@/mobile/hooks/use-hide-amounts';
import { MASK } from '@/mobile/lib/hide-amounts';
import { selectionTick } from '@/mobile/lib/haptics';
import type { CostPoint } from '@/mobile/lib/position-series';
import { nearestPoint, stepValueAt, toPlotX, toPlotY } from '@/mobile/lib/scrub';

/** Plot margins, shared by the Recharts layer and the scrub overlay that must sit on its line. */
const MARGIN = { top: 14, right: 0, bottom: 6, left: 0 };
/** A tap within this many px of where it started is a tap (a mark's callout), not a scrub. */
const TAP_SLOP_PX = 6;
/** Hit radius of a big-step mark (spec §10.4: 28 pt hit areas). */
const MARK_HIT_RADIUS = 14;

interface ScrubChartProps<E extends ChartEvent> {
  /** Sorted by `t`; at least two to draw a line. A point may carry the cost basis of its day. */
  points: CostPoint[];
  /** Zero baseline for "Vše"; otherwise truncated with 15 % headroom below (README §8). */
  zeroBaseline?: boolean;
  /** A step line (a balance that changes only at a record): each value holds until the next point. */
  step?: boolean;
  /** The dashed step line of the cost basis ("Vloženo", a position); the value domain holds it. */
  cost?: boolean;
  /** 160 pt in the hero (spec §10.4). */
  height?: number;
  /** Value ticks ("4,2 mil."); default the locale's compact number. */
  formatTick?: (value: number) => string;
  /** Big-step marks (README §9): tappable, each opens a callout with `renderEventTip`. */
  events?: E[];
  renderEventTip?: (cluster: EventCluster<E>) => LineTip;
  /** A tap opened a mark's callout: the events it stands for (the page flashes their rows). */
  onMarkTap?: (cluster: EventCluster<E>) => void;
  /** The day under the finger while scrubbing, `null` on release. */
  onScrub: (point: LinePoint | null) => void;
  /** Accessible name of the chart. */
  label: string;
  className?: string;
}

interface Gesture {
  id: number;
  x: number;
  /** Index of the mark the finger came down on (a tap opens its callout). */
  mark: number | null;
  scrubbing: boolean;
}

/**
 * The phone's value line (spec §10.4, README §8 and §9): the desktop chart rules — one right-hand
 * value axis, `chart-grid`, the 2.25 px `chart-line` with its area fade, the last point as a white
 * ring, four time labels — with hover replaced by scrub. A finger (or a pointer drag) on the chart
 * draws the crosshair on the nearest day and reports it through `onScrub`, with a light haptic
 * tick on every day change; release reports `null` (the caller snaps back to today). A vertical
 * drag still scrolls the page (`touch-action: pan-y`). The big-step marks are tappable and open a
 * callout with the dated facts.
 *
 * The Recharts layer is static; the crosshair, the marks and the callout are an overlay on the
 * same geometry (`toPlotX` / `toPlotY`), so scrubbing never re-renders the chart.
 */
export function ScrubChart<E extends ChartEvent>({
  points,
  zeroBaseline = false,
  step = false,
  cost = false,
  height = 160,
  formatTick,
  events,
  renderEventTip,
  onMarkTap,
  onScrub,
  label,
  className,
}: ScrubChartProps<E>) {
  const fmt = useFormat();
  const { hidden } = useHideAmounts();
  const tk = useMemo(chartTokens, []);
  const gradientId = useId().replace(/:/g, '');
  const wrapRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [active, setActive] = useState<number | null>(null);
  const activeRef = useRef<number | null>(null);
  const [callout, setCallout] = useState<number | null>(null);
  const gesture = useRef<Gesture | null>(null);

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const t0 = points[0]?.t ?? 0;
  const t1 = points[points.length - 1]?.t ?? t0;
  const plotWidth = Math.max(0, width - AXIS_WIDTH);
  const plotHeight = height - MARGIN.top - MARGIN.bottom;

  const domain = useMemo(
    () =>
      valueDomain(
        [...points.map((p) => p.value), ...(cost ? points.flatMap((p) => p.cost ?? []) : [])],
        { zeroBaseline }
      ),
    [points, cost, zeroBaseline]
  );
  // A refetch can shorten the series under a finger; a new period remounts the chart (`key`).
  const point = active === null ? null : (points[active] ?? null);
  const pointX = point ? toPlotX(point.t, t0, t1, plotWidth) : 0;
  const ticks = useMemo(() => niceTicks(domain[0], domain[1], 4), [domain]);
  const xs = useMemo(
    () => points.map((p) => ({ x: toPlotX(p.t, t0, t1, plotWidth) })),
    [points, t0, t1, plotWidth]
  );
  const marks = useMemo(() => {
    const clusters = events && events.length > 0 ? clusterEvents(events, t0, t1) : [];
    return clusters.map((cluster) => ({
      cluster,
      x: toPlotX(cluster.t, t0, t1, plotWidth),
      // A mark sits on the line: a step line holds its value, a plain one interpolates.
      y: toPlotY(
        step ? stepValueAt(points, cluster.t) : valueAt(points, cluster.t),
        domain,
        MARGIN.top,
        plotHeight
      ),
    }));
  }, [events, t0, t1, plotWidth, points, step, domain, plotHeight]);

  // The value axis tells the size of the line: with amounts hidden every tick is the mask (even
  // a compact "4,2 mil." would give the magnitude away).
  const tickFormatter = useCallback(
    (v: number) => {
      if (hidden) return MASK;
      return formatTick
        ? formatTick(v)
        : fmt.number(v, { notation: 'compact', maximumFractionDigits: 1 });
    },
    [fmt, formatTick, hidden]
  );

  // A tap anywhere outside the marks closes the callout.
  useEffect(() => {
    if (callout === null) return;
    const close = (event: globalThis.PointerEvent) => {
      if (!(event.target instanceof Element) || !event.target.closest('[data-scrub-mark]')) {
        setCallout(null);
      }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setCallout(null);
    };
    document.addEventListener('pointerdown', close, true);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', close, true);
      document.removeEventListener('keydown', onKey);
    };
  }, [callout]);

  const scrubTo = (clientX: number) => {
    const rect = wrapRef.current?.getBoundingClientRect();
    if (!rect) return;
    const index = nearestPoint(xs, clientX - rect.left, plotWidth);
    if (index === null || index === activeRef.current) return;
    activeRef.current = index;
    setActive(index);
    onScrub(points[index]);
    selectionTick();
  };

  const endScrub = () => {
    gesture.current = null;
    if (activeRef.current === null) return;
    activeRef.current = null;
    setActive(null);
    onScrub(null);
  };

  const beginScrub = (event: PointerEvent<HTMLDivElement>, g: Gesture) => {
    g.scrubbing = true;
    event.currentTarget.setPointerCapture(event.pointerId);
    scrubTo(event.clientX);
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    const markEl =
      event.target instanceof Element ? event.target.closest('[data-scrub-mark]') : null;
    const mark = markEl ? Number(markEl.getAttribute('data-scrub-mark')) : null;
    const g: Gesture = { id: event.pointerId, x: event.clientX, mark, scrubbing: false };
    gesture.current = g;
    if (mark === null) beginScrub(event, g);
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.id !== event.pointerId) return;
    if (g.scrubbing) scrubTo(event.clientX);
    else if (Math.abs(event.clientX - g.x) > TAP_SLOP_PX) beginScrub(event, g);
  };

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.id !== event.pointerId) return;
    if (!g.scrubbing && g.mark !== null) {
      const mark = g.mark;
      const opens = callout !== mark;
      setCallout(opens ? mark : null);
      if (opens && marks[mark]) onMarkTap?.(marks[mark].cluster);
    }
    endScrub();
  };

  // The static chart: rebuilt only when its data or size changes, never while scrubbing.
  const chart = useMemo(() => {
    if (width <= 0 || points.length < 2) return null;
    const last = points[points.length - 1];
    return (
      <ComposedChart width={width} height={height} data={points} margin={MARGIN}>
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor={tk.area} stopOpacity={0.8} />
            <stop offset="1" stopColor={tk.areaEnd} stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} stroke={tk.grid} strokeDasharray="0" />
        <XAxis dataKey="t" type="number" domain={[t0, t1]} hide allowDataOverflow scale="linear" />
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
        <Area
          type={step ? 'stepAfter' : 'linear'}
          dataKey="value"
          stroke="none"
          fill={`url(#${gradientId})`}
          isAnimationActive={false}
          activeDot={false}
          baseValue={domain[0]}
        />
        {cost && (
          <Line
            type="stepAfter"
            dataKey="cost"
            stroke={tk.cost}
            strokeWidth={1.5}
            strokeDasharray="4 4"
            dot={false}
            activeDot={false}
            isAnimationActive={false}
            connectNulls
          />
        )}
        <Line
          type={step ? 'stepAfter' : 'linear'}
          dataKey="value"
          stroke={tk.line}
          strokeWidth={2.25}
          strokeLinecap="round"
          strokeLinejoin="round"
          dot={false}
          activeDot={false}
          isAnimationActive={false}
        />
        <ReferenceDot
          x={last.t}
          y={last.value}
          r={4.5}
          fill={tk.paper}
          stroke={tk.line}
          strokeWidth={2}
          ifOverflow="visible"
        />
      </ComposedChart>
    );
  }, [width, height, points, step, cost, gradientId, tk, t0, t1, domain, ticks, tickFormatter]);

  const shown = callout === null ? null : (marks[callout] ?? null);

  return (
    <div className={cn('relative', className)}>
      <div
        ref={wrapRef}
        role="img"
        aria-label={label}
        className="relative select-none [-webkit-touch-callout:none] [&_.recharts-surface]:overflow-visible"
        style={{ height, touchAction: 'pan-y' }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={endScrub}
        onLostPointerCapture={(event) => {
          // Only the chart's own capture. A touch is captured implicitly by the element under the
          // finger (the line, the area, a mark); every touch scrub hands that capture over to this
          // wrapper, wherever it starts, and the inner element's `lostpointercapture` bubbles up to
          // here. Without this guard every touch scrub would end the moment it begins.
          if (event.target !== event.currentTarget) return;
          if (gesture.current?.id === event.pointerId && gesture.current.scrubbing) endScrub();
        }}
      >
        {chart}
        {chart && (
          <svg
            aria-hidden
            className="pointer-events-none absolute left-0 top-0 overflow-visible"
            width={plotWidth}
            height={height}
          >
            {point && (
              <g>
                <line
                  x1={pointX}
                  x2={pointX}
                  y1={MARGIN.top}
                  y2={height - MARGIN.bottom}
                  stroke={tk.crosshair}
                  strokeDasharray="2 3"
                  strokeWidth={1}
                />
                <circle
                  cx={pointX}
                  cy={toPlotY(point.value, domain, MARGIN.top, plotHeight)}
                  r={4}
                  fill={tk.paper}
                  stroke={tk.line}
                  strokeWidth={2}
                />
              </g>
            )}
            {marks.map(({ cluster, x, y }, index) => (
              <g key={cluster.id} data-scrub-mark={index} className="pointer-events-auto">
                <circle cx={x} cy={y} r={MARK_HIT_RADIUS} fill="transparent" />
                <EventMark
                  cx={x}
                  cy={y}
                  type={cluster.type}
                  count={cluster.events.length}
                  hot={callout === index}
                />
              </g>
            ))}
          </svg>
        )}
        {shown && renderEventTip && (
          <FloatingTip x={shown.x} y={shown.y} width={width}>
            <ChartTip
              {...renderEventTip(shown.cluster)}
              className="max-w-[260px] whitespace-normal"
            />
          </FloatingTip>
        )}
      </div>
      {chart && <TimeAxis t0={t0} t1={t1} count={4} />}
    </div>
  );
}
