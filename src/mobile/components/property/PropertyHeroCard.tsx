import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import type { EventCluster } from '@/utils/chart-scale';
import type { PropertyRow } from '@/utils/real-estate';
import { ChartLegend, type LegendItem } from '@/components/charts/ChartLegend';
import { Hero } from '@/mobile/components/Hero';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { PropertyData } from '@/mobile/hooks/use-property';
import { appreciationOf, type TraceEvent } from '@/mobile/lib/property-detail';

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
 * period segment. Every amount goes through the eye; the numbers are the desktop hero's.
 */
export function PropertyHeroCard({ row, trace, currency }: PropertyHeroCardProps) {
  const { t } = useTranslation('realEstate');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const appreciation = appreciationOf(row);
  const label = t('detail.heroValue');
  const charted = trace.points.length > 1;

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

  const marks = trace.events.filter((e) => e.kind === 'estimate');
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
      points={trace.points}
      events={trace.events}
      renderEventTip={eventTip}
      chartLabel={label}
    >
      {charted && (
        <ChartLegend
          items={legend}
          // One estimate and no purchase on the line: nothing to read a trend from yet
          note={
            marks.length === 1 && marks.length === trace.events.length
              ? t('detail.chart.hint')
              : undefined
          }
        />
      )}
    </Hero>
  );
}
