import { useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { LinePoint, LineTip } from '@/components/charts/MoonyLineChart';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import type { ChartPeriod } from '@/utils/period';
import { HERO_FLOOR, useFitText } from '@/mobile/hooks/use-fit-text';
import type { CostPoint } from '@/mobile/lib/position-series';
import { ScrubChart } from './ScrubChart';
import { Segment } from './Segment';

interface HeroChange {
  /** Change over the period as a ratio (0.021 = 2,1 %); the badge carries ▲ / ▼, the number unsigned. */
  ratio: number;
  /** The context after the badge: "+ 86 420 Kč za 30 dní". */
  text: ReactNode;
}

interface HeroProps<E extends ChartEvent, P extends string> {
  label: string;
  /** Today's value; `null` when it could not be valued (shown as "—"). */
  value: number | null;
  /** The shown text of a value (masked while amounts are hidden); a string, so the fit can follow it. */
  formatValue: (value: number) => string;
  /** The change line; without it `note` stands there ("zatím bez historie"). */
  change?: HeroChange;
  note?: ReactNode;
  /**
   * The chart and its period segment. Without `points` the hero is the compact one (Finance's
   * "Celkem na účtech"): no segment, no chart. With `points` the chart is drawn; the segment above
   * it needs `period`, `periods` (two at least) and `onPeriodChange` as well — a chart without them
   * has no segment (the property trace of a property that has one horizon). `P` is the segment's
   * value: a `ChartPeriod` by default, or the loan's own horizons with `periodLabel`.
   */
  period?: P;
  periods?: readonly P[];
  onPeriodChange?: (period: P) => void;
  /** The label of a segment option; default the period's long name (`common:periodsLong.<p>`). */
  periodLabel?: (period: P) => string;
  points?: CostPoint[];
  zeroBaseline?: boolean;
  /** A step line (the account's balance records): each value holds until the next point. */
  step?: boolean;
  /** The dashed cost line of a position ("Vloženo"); the points carry its values. */
  cost?: boolean;
  /** From this time on the line is a plan, drawn dashed (a loan's schedule after today). */
  projectedFrom?: number;
  events?: E[];
  renderEventTip?: (cluster: EventCluster<E>) => LineTip;
  /** A tap on a mark opened its callout: the events it stands for. */
  onMarkTap?: (cluster: EventCluster<E>) => void;
  chartLabel?: string;
  /**
   * Content that belongs to the hero's number: after the chart when it is charted, right after the
   * change line in the compact hero (Majetek's allocation bar and legend, a chart legend).
   */
  children?: ReactNode;
}

/**
 * Hero card (spec §8.3 item 2 and §10.4, README §13, `.m-card--hero`): label, the display number,
 * the change line, the period segment between the number and the chart, the 160 pt chart. While a
 * finger scrubs the chart, the number and the date rewrite to that day; release snaps back to
 * today. The desktop's decorative circle at 60 %. Without `points` it is the compact hero
 * (`.m-card--compact`, spec §8.4): the number and its change line only; with `points` and no period
 * trio the chart stands without a segment. `projectedFrom` draws the rest of the line as a dashed
 * plan (the loan's schedule); `periodLabel` words a segment that is not a `ChartPeriod`.
 */
export function Hero<E extends ChartEvent, P extends string = ChartPeriod>({
  label,
  value,
  formatValue,
  change,
  note,
  period,
  periods,
  onPeriodChange,
  periodLabel,
  points,
  zeroBaseline,
  step,
  cost,
  projectedFrom,
  events,
  renderEventTip,
  onMarkTap,
  chartLabel,
  children,
}: HeroProps<E, P>) {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const charted = points !== undefined;
  const segmented =
    period !== undefined &&
    periods !== undefined &&
    periods.length >= 2 &&
    onPeriodChange !== undefined;
  const [scrub, setScrub] = useState<LinePoint | null>(null);

  const shownValue = scrub ? formatValue(scrub.value) : value === null ? '—' : formatValue(value);
  const up = change !== undefined && change.ratio > 0;
  const down = change !== undefined && change.ratio < 0;
  // A long net worth shrinks to fit (floor `m-fs-title`) instead of ending in an ellipsis. The
  // fit follows the shown text, not the number: hiding the amounts changes the text alone.
  const heroRef = useRef<HTMLElement>(null);
  const fit = useFitText(heroRef, shownValue);

  return (
    <section
      ref={heroRef}
      aria-label={label}
      className={cn(
        "relative mb-3.5 overflow-hidden rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 pb-3 pt-4 shadow-e1 after:pointer-events-none after:absolute after:-right-[76px] after:-top-[92px] after:size-[220px] after:rounded-full after:bg-hero-orb after:content-[''] [&>*]:relative [&>*]:z-[1]",
        !charted && 'pb-3.5'
      )}
    >
      <div className="text-m-sub text-ink-3">{label}</div>
      <div
        data-fit-text
        className="mb-1.5 mt-[3px] overflow-hidden text-ellipsis whitespace-nowrap text-m-display text-ink num"
        style={
          fit === null
            ? undefined
            : { fontSize: `clamp(${HERO_FLOOR}, ${fit}px, var(--m-fs-display))` }
        }
      >
        <span>{shownValue}</span>
      </div>
      <div className="flex min-h-5 min-w-0 items-center gap-1.5 whitespace-nowrap text-m-sub text-ink-3">
        {scrub ? (
          <span className="min-w-0 truncate num">
            {fmt.day(scrub.t, { day: 'numeric', month: 'numeric', year: 'numeric' })}
          </span>
        ) : change ? (
          <>
            <span
              className={cn(
                'inline-flex min-h-5 flex-none items-center gap-1 rounded-r1 px-[7px] py-0.5 text-m-sub leading-none num',
                up
                  ? 'bg-gain-soft text-gain'
                  : down
                    ? 'bg-loss-soft text-loss'
                    : 'bg-well-2 text-ink-2'
              )}
            >
              {up ? '▲ ' : down ? '▼ ' : ''}
              {fmt.percent(Math.abs(change.ratio), 1)}
            </span>
            <span className="min-w-0 truncate">{change.text}</span>
          </>
        ) : (
          note && <span className="min-w-0 truncate">{note}</span>
        )}
      </div>
      {segmented && (
        <Segment
          value={period}
          options={periods.map((p) => ({
            value: p,
            label: periodLabel ? periodLabel(p) : t(`periodsLong.${p}`),
          }))}
          onChange={(next) => {
            setScrub(null);
            onPeriodChange(next);
          }}
          label={t('periods.label')}
          className="mb-2 mt-3"
        />
      )}
      {charted && points.length > 1 && (
        <ScrubChart<E>
          // A new period starts a new chart: no scrub or callout carries over.
          key={period ?? 'chart'}
          // Without the segment the chart follows the change line directly
          className={segmented ? undefined : 'mt-3'}
          points={points}
          zeroBaseline={zeroBaseline}
          step={step}
          cost={cost}
          projectedFrom={projectedFrom}
          events={events}
          renderEventTip={renderEventTip}
          onMarkTap={onMarkTap}
          onScrub={setScrub}
          label={chartLabel ?? label}
        />
      )}
      {children}
    </section>
  );
}
