import { memo, useMemo, useRef, useState, type MouseEvent } from 'react';
import {
  Area,
  ComposedChart,
  Line,
  ReferenceLine,
  ResponsiveContainer,
  XAxis,
  YAxis,
} from 'recharts';
import { ChartTip, FloatingTip } from '@/components/charts/ChartTip';
import { DateAxis } from '@/components/charts/DateAxis';
import { chartTokens, token, type ChartTokens } from '@/lib/tokens';
import { cn } from '@/lib/utils';
import { niceStep, nearestIndex } from '@/utils/chart-scale';
import { DAY } from '@/utils/stock-performance';

export interface PerformanceLine {
  id: string;
  name: string;
  /** CSS colour (`var(--s1)` …). */
  color: string;
  /** `reference` = dashed `chart-line` (the portfolio), `index` = dotted `ink-4`. */
  variant: 'solid' | 'reference' | 'index';
  twr: (number | null)[];
  drawdown: (number | null)[];
}

export interface PerformanceChartProps {
  /** Unix seconds, one per point. */
  dates: number[];
  /** Parts first, then the index, then the reference. */
  lines: PerformanceLine[];
  highlighted: string | null;
  onHighlight: (id: string | null) => void;
  /** Index of the point under the pointer; `null` when it leaves the chart. */
  onScrub: (index: number | null) => void;
  /** A dragged range of at least two days (unix seconds). */
  onRangeSelect: (from: number, to: number) => void;
  /** "Dnes" or the end date. */
  lastLabel: string;
  /** Signed percentage with the given number of decimals. */
  formatPercent: (v: number, digits: number) => string;
  formatDay: (ts: number) => string;
  labels: {
    drawdown: string;
    tipDrawdown: (v: string) => string;
    dragTip: (days: number) => string;
  };
  className?: string;
}

/** Height of the TWR panel, the label strip between the panels, and the drawdown panel. */
const TWR_HEIGHT = 280;
const PANEL_GAP = 34;
const DD_HEIGHT = 84;
const TOTAL_HEIGHT = TWR_HEIGHT + PANEL_GAP + DD_HEIGHT;
/** Right gutter shared by both panels: value ticks and, in the TWR panel, the end labels. */
const GUTTER = 128;
/** Left edge of a tick and of an end label inside the gutter. */
const GUTTER_INSET = 10;
/**
 * An end label may run past the gutter into the card's right padding: 24 px of padding less the
 * 4 px the chart is pulled out by (`-mx-1` in `PerformanceCard`), leaving 4 px to the card edge.
 */
const LABEL_OVERHANG = 20;
const LABEL_EDGE = 4;
/** Widest an end label gets before its name is ellipsized. */
const END_LABEL_MAX = GUTTER + LABEL_OVERHANG - GUTTER_INSET - LABEL_EDGE;
const TWR_PAD_TOP = 14;
const TWR_PAD_BOTTOM = 4;
const DD_PAD_BOTTOM = 2;
/** Smallest distance between two end labels. */
const LABEL_STEP = 14;
/** An end label hides the value tick that would sit under it. */
const TICK_CLEARANCE = 11;
/** Opacity of everything that is not highlighted. */
const DIM_LINE = 0.25;
const DIM_LABEL = 0.35;

/** `var(--name)` becomes the resolved token: SVG attributes need a concrete colour. */
function resolveColor(color: string): string {
  const match = /^var\(\s*--([\w-]+)\s*\)$/.exec(color);
  return match ? token(match[1]) : color;
}

/** Decimals a tick label needs for this step (0 for whole steps, 1 for 0.5, 2 for 0.25). */
function stepDigits(step: number): number {
  for (let digits = 0; digits < 3; digits++) {
    const scaled = step * 10 ** digits;
    if (Math.abs(scaled - Math.round(scaled)) < 1e-9) return digits;
  }
  return 3;
}

/** Evenly spaced multiples of `step` from `lo` to `hi`, without float noise. */
function stepTicks(lo: number, hi: number, step: number): number[] {
  const ticks: number[] = [];
  for (let k = 0; lo + k * step <= hi + step * 1e-9; k++) {
    const v = Number((lo + k * step).toPrecision(12));
    ticks.push(Math.abs(v) < step * 1e-9 ? 0 : v);
  }
  return ticks;
}

function lastIndexOf(values: readonly (number | null)[]): number {
  for (let i = values.length - 1; i >= 0; i--) if (values[i] !== null) return i;
  return -1;
}

function finite(values: readonly (number | null)[]): number[] {
  return values.filter((v): v is number => v !== null && Number.isFinite(v));
}

/** Dash pattern, caps and widths per line variant (TWR panel, drawdown panel). */
function strokeOf(variant: PerformanceLine['variant'], first: boolean) {
  switch (variant) {
    case 'reference':
      return { dash: '4 4', cap: 'butt' as const, twr: 2, dd: 1.5 };
    case 'index':
      return { dash: '0.1 4', cap: 'round' as const, twr: 2.5, dd: 2.2 };
    default:
      return { dash: undefined, cap: 'round' as const, twr: first ? 2.25 : 2, dd: 1.5 };
  }
}

/** The swatch in front of an end label: a ring, a dashed or a dotted stub. */
function Swatch({ variant, color }: { variant: PerformanceLine['variant']; color: string }) {
  if (variant === 'solid') {
    return (
      <i
        className="size-[7px] shrink-0 rounded-full border-2 bg-paper"
        style={{ borderColor: color }}
      />
    );
  }
  return (
    <i
      className={cn(
        'block h-0 w-[11px] shrink-0 border-0 border-t-2',
        variant === 'reference' ? 'border-dashed' : 'border-dotted'
      )}
      style={{ borderColor: color }}
    />
  );
}

interface PlotProps {
  rows: Record<string, number | null>[];
  lines: PerformanceLine[];
  /** Resolved stroke colour per line. */
  colors: string[];
  /** The highlighted line, if any. */
  active: string | null;
  t0: number;
  t1: number;
  tk: ChartTokens;
}

interface Scale {
  lo: number;
  ticks: number[];
}

const PLOT_CURVES =
  '[&_.recharts-curve]:transition-[stroke-opacity] [&_.recharts-curve]:duration-fast [&_.recharts-surface]:overflow-visible';

/** Axes and grid shared by both panels; the value ticks themselves are HTML in the gutter. */
function PanelFrame({
  scale,
  hi,
  t0,
  t1,
  tk,
}: {
  scale: Scale;
  hi: number;
  t0: number;
  t1: number;
  tk: ChartTokens;
}) {
  return (
    <>
      {scale.ticks.map((v) => (
        <ReferenceLine
          key={v}
          y={v}
          stroke={v === 0 ? token('line-strong') : tk.grid}
          ifOverflow="visible"
        />
      ))}
      <XAxis dataKey="t" type="number" domain={[t0, t1]} scale="linear" hide allowDataOverflow />
      <YAxis
        orientation="right"
        width={GUTTER}
        domain={[scale.lo, hi]}
        tick={false}
        axisLine={false}
        tickLine={false}
        allowDataOverflow
      />
    </>
  );
}

/**
 * The TWR lines. Memoised: hover and drag only touch the overlay, so a pointer move does not
 * rebuild the paths of every series.
 */
const TwrPlot = memo(function TwrPlot({
  rows,
  lines,
  colors,
  active,
  t0,
  t1,
  tk,
  scale,
}: PlotProps & { scale: Scale & { hi: number } }) {
  const firstSolid = lines.findIndex((l) => l.variant === 'solid');
  return (
    <ResponsiveContainer width="100%" height="100%">
      <ComposedChart
        data={rows}
        margin={{ top: TWR_PAD_TOP, right: 0, bottom: TWR_PAD_BOTTOM, left: 0 }}
      >
        <PanelFrame scale={scale} hi={scale.hi} t0={t0} t1={t1} tk={tk} />
        {lines.map((line, k) => {
          const stroke = strokeOf(line.variant, k === firstSolid);
          return (
            <Line
              key={line.id}
              type="linear"
              dataKey={`v${k}`}
              stroke={colors[k]}
              strokeWidth={stroke.twr}
              strokeDasharray={stroke.dash}
              strokeLinecap={stroke.cap}
              strokeLinejoin="round"
              strokeOpacity={active !== null && active !== line.id ? DIM_LINE : 1}
              dot={false}
              activeDot={false}
              isAnimationActive={false}
              connectNulls
            />
          );
        })}
      </ComposedChart>
    </ResponsiveContainer>
  );
});

/** The drawdown lines, with the reference also as an area under the zero line. */
const DdPlot = memo(function DdPlot({
  rows,
  lines,
  colors,
  active,
  t0,
  t1,
  tk,
  scale,
}: PlotProps & { scale: Scale }) {
  const firstSolid = lines.findIndex((l) => l.variant === 'solid');
  const reference = lines.findIndex((l) => l.variant === 'reference');
  return (
    <ResponsiveContainer width="100%" height="100%">
      <ComposedChart data={rows} margin={{ top: 0, right: 0, bottom: DD_PAD_BOTTOM, left: 0 }}>
        <PanelFrame scale={scale} hi={0} t0={t0} t1={t1} tk={tk} />
        {reference >= 0 && (
          <Area
            type="linear"
            dataKey={`d${reference}`}
            baseValue={0}
            stroke="none"
            fill={tk.area}
            fillOpacity={0.75}
            isAnimationActive={false}
            connectNulls
          />
        )}
        {lines.map((line, k) => {
          const stroke = strokeOf(line.variant, k === firstSolid);
          return (
            <Line
              key={line.id}
              type="linear"
              dataKey={`d${k}`}
              stroke={colors[k]}
              strokeWidth={stroke.dd}
              strokeDasharray={stroke.dash}
              strokeLinecap={stroke.cap}
              strokeLinejoin="round"
              strokeOpacity={active !== null && active !== line.id ? DIM_LINE : 1}
              dot={false}
              activeDot={false}
              isAnimationActive={false}
              connectNulls
            />
          );
        })}
      </ComposedChart>
    </ResponsiveContainer>
  );
});

interface Pointer {
  index: number;
  /** Width of the plot, to place the tooltip. */
  width: number;
}

interface Drag extends Pointer {
  from: number;
}

/**
 * Time-weighted return of up to four parts, the whole portfolio (dashed) and an index (dotted),
 * with the drawdown from the running peak in a second panel under one shared date axis
 * (design system §8). Hovering draws a crosshair across both panels and a tooltip sorted by
 * value; dragging across the plot selects a custom range.
 *
 * Recharts draws the lines and areas. Pointer handling lives on one overlay that spans both
 * panels and the strip between them: a chart-level handler per panel would end a drag at the
 * strip and could not draw the crosshair through it.
 */
export function PerformanceChart({
  dates,
  lines,
  highlighted,
  onHighlight,
  onScrub,
  onRangeSelect,
  lastLabel,
  formatPercent,
  formatDay,
  labels,
  className,
}: PerformanceChartProps) {
  const tk = useMemo(chartTokens, []);
  const n = dates.length;
  const t0 = dates[0] ?? 0;
  const t1 = dates[n - 1] ?? 0;
  const span = t1 - t0;

  const overlayRef = useRef<HTMLDivElement>(null);
  const scrubbed = useRef<number | null>(null);
  // Where the drag started; a ref so a move right after the press already sees the drag
  const dragFrom = useRef<number | null>(null);
  const [hover, setHover] = useState<Pointer | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);

  const points = useMemo(() => dates.map((t) => ({ t })), [dates]);
  const colors = useMemo(() => lines.map((l) => resolveColor(l.color)), [lines]);
  // A highlight that no longer matches a line (a filter changed) dims nothing
  const active = lines.some((l) => l.id === highlighted) ? highlighted : null;
  const reference = lines.findIndex((l) => l.variant === 'reference');

  const rows = useMemo(
    () =>
      dates.map((t, i) => {
        const row: Record<string, number | null> = { t };
        lines.forEach((l, k) => {
          row[`v${k}`] = l.twr[i] ?? null;
          row[`d${k}`] = l.drawdown[i] ?? null;
        });
        return row;
      }),
    [dates, lines]
  );

  // TWR scale: zero always inside, 4 % / 6 % headroom, rounded out to whole steps
  const twr = useMemo(() => {
    let lo0 = 0;
    let hi0 = 0;
    for (const l of lines) {
      for (const v of finite(l.twr)) {
        lo0 = Math.min(lo0, v);
        hi0 = Math.max(hi0, v);
      }
    }
    const raw = hi0 - lo0 || 1;
    const step = niceStep(raw * 1.1, 5);
    const lo = Math.floor((lo0 - raw * 0.04) / step) * step;
    const hi = Math.ceil((hi0 + raw * 0.06) / step) * step;
    return { lo, hi, step, ticks: stepTicks(lo, hi, step), digits: stepDigits(step) };
  }, [lines]);

  // Drawdown scale: zero at the top, down to the deepest drawdown in whole steps
  const dd = useMemo(() => {
    let deepest = 0;
    for (const l of lines) for (const v of finite(l.drawdown)) deepest = Math.min(deepest, v);
    const step = niceStep(Math.max(1, -deepest), 2);
    let lo = Math.floor(deepest / step) * step;
    if (lo === 0) lo = -step;
    return { lo, ticks: stepTicks(lo, 0, step), digits: stepDigits(step) };
  }, [lines]);

  const twrY = (v: number) =>
    TWR_PAD_TOP +
    (1 - (v - twr.lo) / (twr.hi - twr.lo)) * (TWR_HEIGHT - TWR_PAD_TOP - TWR_PAD_BOTTOM);
  const ddY = (v: number) => (v / dd.lo) * (DD_HEIGHT - DD_PAD_BOTTOM);
  const xOf = (t: number) => (span > 0 ? ((t - t0) / span) * 100 : 0);

  // End labels in the right gutter, pushed apart when their lines end close together
  const ends = lines
    .map((line, k) => {
      const at = lastIndexOf(line.twr);
      const value = at < 0 ? null : line.twr[at];
      return value === null ? null : { line, k, at, value, y: twrY(value) };
    })
    .filter((e): e is NonNullable<typeof e> => e !== null)
    .sort((a, b) => a.y - b.y);
  for (let i = 1; i < ends.length; i++) {
    if (ends[i].y - ends[i - 1].y < LABEL_STEP) ends[i].y = ends[i - 1].y + LABEL_STEP;
  }

  // --- pointer ---------------------------------------------------------------------------------

  const pointerAt = (e: MouseEvent<HTMLDivElement>): Pointer | null => {
    const rect = overlayRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0 || n === 0) return null;
    const x = Math.min(Math.max(e.clientX - rect.left, 0), rect.width);
    const index = nearestIndex(points, t0 + (x / rect.width) * span);
    return index < 0 ? null : { index, width: rect.width };
  };

  const scrub = (index: number | null) => {
    if (scrubbed.current === index) return;
    scrubbed.current = index;
    onScrub(index);
  };

  const handleMove = (e: MouseEvent<HTMLDivElement>) => {
    const p = pointerAt(e);
    if (!p) return;
    if (dragFrom.current !== null) {
      setDrag({ ...p, from: dragFrom.current });
      return;
    }
    setHover(p);
    scrub(p.index);
  };

  const handleDown = (e: MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const p = pointerAt(e);
    if (!p) return;
    dragFrom.current = p.index;
    setHover(null);
    setDrag({ ...p, from: p.index });
  };

  const handleUp = (e: MouseEvent<HTMLDivElement>) => {
    const from = dragFrom.current;
    if (from === null) return;
    dragFrom.current = null;
    const p = pointerAt(e);
    const lo = Math.min(from, p?.index ?? from);
    const hi = Math.max(from, p?.index ?? from);
    setDrag(null);
    // The next move shows the crosshair again (the data may change under the pointer)
    setHover(null);
    if (dates[hi] - dates[lo] >= 2 * DAY) {
      scrub(null);
      onRangeSelect(dates[lo], dates[hi]);
    }
  };

  const handleLeave = () => {
    dragFrom.current = null;
    setHover(null);
    setDrag(null);
    scrub(null);
  };

  if (n < 2 || span <= 0) return <div className={className} style={{ height: TOTAL_HEIGHT }} />;

  const hoverIndex = hover && !drag && hover.index < n ? hover.index : null;
  const dragSpan =
    drag && drag.index < n && drag.from < n
      ? { lo: Math.min(drag.from, drag.index), hi: Math.max(drag.from, drag.index) }
      : null;

  // Tooltip: the lines sorted by value at the hovered day, then the drawdown of the reference
  let tip: { x: number; width: number; title: string; lines: string[] } | null = null;
  if (hoverIndex !== null && hover) {
    const at = hoverIndex;
    const tipLines = lines
      .map((l) => ({ l, v: l.twr[at] }))
      .filter((e): e is { l: PerformanceLine; v: number } => e.v !== null && e.v !== undefined)
      .sort((a, b) => b.v - a.v)
      .map((e) => `${e.l.name} ${formatPercent(e.v, 1)}`);
    const refDrawdown = reference >= 0 ? lines[reference].drawdown[at] : null;
    if (refDrawdown !== null && refDrawdown !== undefined) {
      tipLines.push(labels.tipDrawdown(formatPercent(refDrawdown, 1)));
    }
    tip = {
      x: (xOf(dates[at]) / 100) * hover.width,
      width: hover.width,
      title: formatDay(dates[at]),
      lines: tipLines,
    };
  } else if (dragSpan && drag) {
    tip = {
      x: (xOf(dates[drag.index]) / 100) * drag.width,
      width: drag.width,
      title: `${formatDay(dates[dragSpan.lo])} – ${formatDay(dates[dragSpan.hi])}`,
      lines: [labels.dragTip(Math.round((dates[dragSpan.hi] - dates[dragSpan.lo]) / DAY))],
    };
  }

  const dimmed = (id: string) => active !== null && active !== id;
  const plotProps = { rows, lines, colors, active, t0, t1, tk };

  return (
    <div className={cn('relative', className)}>
      {/* TWR panel */}
      <div className={cn('relative', PLOT_CURVES)} style={{ height: TWR_HEIGHT }}>
        <TwrPlot {...plotProps} scale={twr} />
        {/* Value ticks and direct end labels in the gutter */}
        <div
          aria-hidden
          className="pointer-events-none absolute inset-y-0 right-0"
          style={{ width: GUTTER }}
        >
          {twr.ticks.map((v) => {
            const y = twrY(v);
            if (ends.some((e) => Math.abs(e.y - y) < TICK_CLEARANCE)) return null;
            return (
              <span
                key={v}
                className={cn(
                  'absolute -translate-y-1/2 whitespace-nowrap text-micro num',
                  v === 0 ? 'font-600 text-ink-3' : 'font-500 text-chart-axis'
                )}
                style={{ top: y, left: GUTTER_INSET }}
              >
                {formatPercent(v, twr.digits)}
              </span>
            );
          })}
          {ends.map((e) => (
            <span
              key={e.line.id}
              title={e.line.name}
              className="pointer-events-auto absolute flex -translate-y-1/2 cursor-default items-center gap-1.5 whitespace-nowrap text-micro font-650 text-ink-2 transition-opacity duration-fast num"
              style={{
                top: e.y,
                left: GUTTER_INSET,
                maxWidth: END_LABEL_MAX,
                opacity: dimmed(e.line.id) ? DIM_LABEL : 1,
              }}
              onMouseEnter={() => onHighlight(e.line.id)}
              onMouseLeave={() => onHighlight(null)}
            >
              <Swatch variant={e.line.variant} color={e.line.color} />
              <span className="min-w-0 truncate">{e.line.name}</span>
              <span className="shrink-0">{formatPercent(e.value, 1)}</span>
            </span>
          ))}
        </div>
      </div>

      {/* Caption of the drawdown panel */}
      <div className="flex items-end pb-[9px]" style={{ height: PANEL_GAP }} aria-hidden>
        <span className="text-eyebrow uppercase text-ink-4">{labels.drawdown}</span>
      </div>

      {/* Drawdown panel */}
      <div className={cn('relative', PLOT_CURVES)} style={{ height: DD_HEIGHT }}>
        <DdPlot {...plotProps} scale={dd} />
        <div
          aria-hidden
          className="pointer-events-none absolute inset-y-0 right-0"
          style={{ width: GUTTER }}
        >
          {dd.ticks.map((v) => (
            <span
              key={v}
              className={cn(
                'absolute -translate-y-1/2 whitespace-nowrap text-micro num',
                v === 0 ? 'font-600 text-ink-3' : 'font-500 text-chart-axis'
              )}
              style={{ top: ddY(v), left: GUTTER_INSET }}
            >
              {formatPercent(v, dd.digits)}
            </span>
          ))}
        </div>
      </div>

      <DateAxis t0={t0} t1={t1} gutter={GUTTER} lastLabel={lastLabel} className="mt-1.5" />

      {/* End rings, crosshair, selection and tooltip; one pointer target over both panels */}
      <div
        ref={overlayRef}
        aria-hidden
        className={cn(
          'absolute left-0 top-0 select-none',
          drag ? 'cursor-ew-resize' : 'cursor-crosshair'
        )}
        style={{ right: GUTTER, height: TOTAL_HEIGHT }}
        onMouseMove={handleMove}
        onMouseDown={handleDown}
        onMouseUp={handleUp}
        onMouseLeave={handleLeave}
      >
        {ends.map((e) => (
          <span
            key={e.line.id}
            className="pointer-events-none absolute size-[10px] -translate-x-1/2 -translate-y-1/2 rounded-full border-2 bg-paper transition-opacity duration-fast"
            style={{
              left: `${xOf(dates[e.at])}%`,
              top: twrY(e.value),
              borderColor: colors[e.k],
              opacity: dimmed(e.line.id) ? DIM_LINE : 1,
            }}
          />
        ))}
        {hoverIndex !== null && (
          <>
            <span
              className="pointer-events-none absolute w-px -translate-x-1/2"
              style={{
                left: `${xOf(dates[hoverIndex])}%`,
                top: TWR_PAD_TOP,
                bottom: 0,
                background: `repeating-linear-gradient(to bottom, ${tk.crosshair} 0 2px, transparent 2px 5px)`,
              }}
            />
            {lines.map((line, k) => {
              const left = `${xOf(dates[hoverIndex])}%`;
              const v = line.twr[hoverIndex];
              const d = line.drawdown[hoverIndex];
              return (
                <span key={line.id} className="contents">
                  {v !== null && v !== undefined && (
                    <span
                      className="pointer-events-none absolute size-[9px] -translate-x-1/2 -translate-y-1/2 rounded-full border-2 bg-paper"
                      style={{ left, top: twrY(v), borderColor: colors[k] }}
                    />
                  )}
                  {d !== null && d !== undefined && (
                    <span
                      className="pointer-events-none absolute size-[8px] -translate-x-1/2 -translate-y-1/2 rounded-full border-[1.5px] bg-paper"
                      style={{ left, top: TWR_HEIGHT + PANEL_GAP + ddY(d), borderColor: colors[k] }}
                    />
                  )}
                </span>
              );
            })}
          </>
        )}
        {dragSpan && (
          <>
            <span
              className="pointer-events-none absolute"
              style={{
                left: `${xOf(dates[dragSpan.lo])}%`,
                width: `max(1px, ${xOf(dates[dragSpan.hi]) - xOf(dates[dragSpan.lo])}%)`,
                top: TWR_PAD_TOP,
                bottom: 0,
                background: 'color-mix(in srgb, var(--ink) 6%, transparent)',
              }}
            />
            {(['lo', 'hi'] as const).map((edge) => (
              <span
                key={edge}
                className="pointer-events-none absolute w-px -translate-x-1/2"
                style={{
                  left: `${xOf(dates[dragSpan[edge]])}%`,
                  top: TWR_PAD_TOP,
                  bottom: 0,
                  background: `repeating-linear-gradient(to bottom, var(--ink-3) 0 2px, transparent 2px 5px)`,
                }}
              />
            ))}
          </>
        )}
        {tip && (
          <FloatingTip x={tip.x} y={TWR_PAD_TOP + 8} width={tip.width}>
            <ChartTip title={tip.title} lines={tip.lines} />
          </FloatingTip>
        )}
      </div>
    </div>
  );
}
