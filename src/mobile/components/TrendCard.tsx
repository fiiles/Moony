import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { LinePoint } from '@/components/charts/MoonyLineChart';
import { useFormat } from '@/lib/use-format';
import type { ChartPeriod } from '@/utils/period';
import { ScrubChart } from './ScrubChart';
import { SectionHead } from './SectionHead';
import { Segment } from './Segment';

/** Height of the trend chart (spec §10.4: hero 160, trend 120). */
const TREND_HEIGHT = 120;

interface TrendCardProps {
  title: string;
  /** The head's one number or context ("za 30 dní"); a scrub rewrites it to that day's value. */
  note?: ReactNode;
  points: LinePoint[];
  period: ChartPeriod;
  periods: readonly ChartPeriod[];
  onPeriodChange: (period: ChartPeriod) => void;
  formatValue: (value: number) => ReactNode;
  /** Accessible name of the chart. */
  label: string;
  /** Zero baseline; default for "Vše" only (README §8). */
  zeroBaseline?: boolean;
}

/**
 * Trend card (spec §8.4 "Vývoj hotovosti", README §13, `.m-trend`): a section head and a card
 * holding the period segment and the 120 pt scrub chart. While a finger scrubs, the head's note
 * shows that day's value and date instead of `note`, as the hero's number does. Without two
 * points the chart's room stays empty, so a period switch never shrinks the card under the
 * segment.
 */
export function TrendCard({
  title,
  note,
  points,
  period,
  periods,
  onPeriodChange,
  formatValue,
  label,
  zeroBaseline = period === 'All',
}: TrendCardProps) {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const [scrub, setScrub] = useState<LinePoint | null>(null);

  return (
    <section>
      <SectionHead
        title={title}
        note={
          scrub ? (
            <>
              {formatValue(scrub.value)} ·{' '}
              {fmt.day(scrub.t, { day: 'numeric', month: 'numeric', year: 'numeric' })}
            </>
          ) : (
            note
          )
        }
      />
      <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 pb-2 pt-3 shadow-e1">
        <Segment
          value={period}
          options={periods.map((p) => ({ value: p, label: t(`periodsLong.${p}`) }))}
          onChange={(next) => {
            setScrub(null);
            onPeriodChange(next);
          }}
          label={t('periods.label')}
          className="mb-2"
        />
        {points.length > 1 ? (
          <ScrubChart
            // A new period starts a new chart: no scrub or callout carries over.
            key={period}
            points={points}
            height={TREND_HEIGHT}
            zeroBaseline={zeroBaseline}
            onScrub={setScrub}
            label={label}
          />
        ) : (
          <div aria-hidden style={{ height: TREND_HEIGHT }} />
        )}
      </div>
    </section>
  );
}
