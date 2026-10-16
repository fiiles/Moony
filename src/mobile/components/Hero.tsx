import { useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { LinePoint, LineTip } from '@/components/charts/MoonyLineChart';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import type { ChartPeriod } from '@/utils/period';
import { useFitText } from '@/mobile/hooks/use-fit-text';
import { ScrubChart } from './ScrubChart';
import { Segment } from './Segment';

interface HeroChange {
  /** Change over the period as a ratio (0.021 = 2,1 %); the badge carries ▲ / ▼, the number unsigned. */
  ratio: number;
  /** The context after the badge: "+ 86 420 Kč za 30 dní". */
  text: ReactNode;
}

interface HeroProps<E extends ChartEvent> {
  label: string;
  /** Today's value; `null` when it could not be valued (shown as "—"). */
  value: number | null;
  formatValue: (value: number) => ReactNode;
  /** The change line; without it `note` stands there ("zatím bez historie"). */
  change?: HeroChange;
  note?: ReactNode;
  /**
   * The chart and its period segment: `points` and the other four come together. Without `points`
   * the hero is the compact one (Finance's "Celkem na účtech"): no segment, no chart.
   */
  period?: ChartPeriod;
  periods?: readonly ChartPeriod[];
  onPeriodChange?: (period: ChartPeriod) => void;
  points?: LinePoint[];
  zeroBaseline?: boolean;
  events?: E[];
  renderEventTip?: (cluster: EventCluster<E>) => LineTip;
  chartLabel?: string;
}

/**
 * Hero card (spec §8.3 item 2 and §10.4, README §13, `.m-card--hero`): label, the display number,
 * the change line, the period segment between the number and the chart, the 160 pt chart. While a
 * finger scrubs the chart, the number and the date rewrite to that day; release snaps back to
 * today. The desktop's decorative circle at 60 %. Without `points` it is the compact hero
 * (`.m-card--compact`, spec §8.4): the number and its change line only.
 */
export function Hero<E extends ChartEvent>({
  label,
  value,
  formatValue,
  change,
  note,
  period,
  periods,
  onPeriodChange,
  points,
  zeroBaseline,
  events,
  renderEventTip,
  chartLabel,
}: HeroProps<E>) {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const charted =
    points !== undefined &&
    period !== undefined &&
    periods !== undefined &&
    onPeriodChange !== undefined;
  const [scrub, setScrub] = useState<LinePoint | null>(null);

  const shownValue = scrub ? formatValue(scrub.value) : value === null ? '—' : formatValue(value);
  const up = change !== undefined && change.ratio > 0;
  const down = change !== undefined && change.ratio < 0;
  // A long net worth shrinks to fit (floor `m-fs-title`) instead of ending in an ellipsis.
  const heroRef = useRef<HTMLElement>(null);
  const fit = useFitText(heroRef, scrub ? scrub.value : value);

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
            : { fontSize: `clamp(var(--m-fs-title), ${fit}px, var(--m-fs-display))` }
        }
      >
        <span>{shownValue}</span>
      </div>
      <div className="flex h-5 min-w-0 items-center gap-1.5 whitespace-nowrap text-m-sub text-ink-3">
        {scrub ? (
          <span className="min-w-0 truncate num">
            {fmt.day(scrub.t, { day: 'numeric', month: 'numeric', year: 'numeric' })}
          </span>
        ) : change ? (
          <>
            <span
              className={cn(
                'inline-flex h-5 flex-none items-center gap-1 rounded-r1 px-[7px] text-m-sub leading-none num',
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
      {charted && (
        <Segment
          value={period}
          options={periods.map((p) => ({ value: p, label: t(`periodsLong.${p}`) }))}
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
          key={period}
          points={points}
          zeroBaseline={zeroBaseline}
          events={events}
          renderEventTip={renderEventTip}
          onScrub={setScrub}
          label={chartLabel ?? label}
        />
      )}
    </section>
  );
}
