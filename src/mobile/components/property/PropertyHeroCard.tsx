import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import type { EventCluster } from '@/utils/chart-scale';
import type { PropertyRow } from '@/utils/real-estate';
import { type LegendItem, ScaledChartLegend } from '@/mobile/components/Scaled';
import { Hero } from '@/mobile/components/Hero';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { PropertyData } from '@/mobile/hooks/use-property';
import {
  appreciationOf,
  oneEstimateOnly,
  traceSpansDays,
  type TraceEvent,
} from '@/mobile/lib/property-detail';

/** Whole amounts: the hero shows no decimals. */
const WHOLE = { decimals: 0 } as const;

interface PropertyHeroCardProps {
  row: PropertyRow;
  trace: PropertyData['trace'];
  /** The main currency: every figure of the hero is in it. */
  currency: string;
}

/**
 * The value of the property (spec §8.8 "Nemovitost"): today's market value in the main currency,
 * the change against the purchase price (the badge carries ▲ / ▼ and the percent, the text the
 * amount) and the valuation trace under it — a straight line from the purchase through every
 * estimate to today, with ▲ for the purchase and ○ for each revaluation (a tap opens the callout
 * with its day, its note and the amount). The trace has one horizon, so the chart stands without a
 * period segment, and a trace that does not reach two days (one estimate dated today, no purchase)
 * is not drawn: the desktop's hint stands in its place. Every amount goes through the eye; the numbers are the desktop hero's.
 */
export function PropertyHeroCard({ row, trace, currency }: PropertyHeroCardProps) {
  const { t } = useTranslation('realEstate');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const appreciation = appreciationOf(row);
  const label = t('detail.heroValue');
  // A trace inside one day (one estimate dated today, no purchase) is not drawn: the hero is the compact one
  const charted = traceSpansDays(trace.points);
  const hint = oneEstimateOnly(trace.events);

  const eventTip = (cluster: EventCluster<TraceEvent>) => {
    const e = cluster.events[0];
    const kind =
      e.kind === 'purchase'
        ? t('detail.chart.bought')
        : e.first
          ? t('detail.chart.first')
          : t('detail.chart.revaluation');
    return {
      title: `${kind} · ${moneyText(e.value, currency, WHOLE)}`,
      lines: [fmt.day(e.t), e.note ?? ''].filter(Boolean),
    };
  };

  const legend: LegendItem[] = [
    { label: t('detail.chart.value'), swatch: { kind: 'line' } },
    ...(trace.events.some((e) => e.kind === 'purchase')
      ? [{ label: t('detail.chart.bought'), swatch: { kind: 'event', type: 'buy' } } as LegendItem]
      : []),
    { label: t('detail.chart.revaluation'), swatch: { kind: 'event', type: 'mark' } },
  ];

  return (
    <Hero<TraceEvent>
      label={label}
      value={row.value}
      formatValue={(value) => moneyText(value, currency, WHOLE)}
      change={
        appreciation
          ? {
              ratio: appreciation.ratio,
              // The key is the desktop's "{{percent}} vs. purchase price"; the badge already says
              // the percent, so this is the amount that goes before the words
              text: t('detail.vsPurchasePct', {
                percent: moneyText(appreciation.delta, currency, { signed: true, ...WHOLE }),
              }),
            }
          : undefined
      }
      points={charted ? trace.points : undefined}
      events={charted ? trace.events : undefined}
      renderEventTip={eventTip}
      chartLabel={label}
    >
      {charted ? (
        <ScaledChartLegend
          items={legend}
          // One estimate and no purchase on the line: nothing to read a trend from yet
          note={hint ? t('detail.chart.hint') : undefined}
        />
      ) : (
        hint && <p className="mb-0 mt-2 text-m-note text-ink-4">{t('detail.chart.hint')}</p>
      )}
    </Hero>
  );
}
