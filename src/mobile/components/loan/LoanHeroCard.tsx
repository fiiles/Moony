import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { LoanEvent } from '@shared/schema';
import { useLoanText } from '@/hooks/use-loan-text';
import { useFormat } from '@/lib/use-format';
import type { EventCluster } from '@/utils/chart-scale';
import type { LoanRow } from '@/utils/loans';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { Hero } from '@/mobile/components/Hero';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { shortDayOptions } from '@/mobile/lib/format-day';
import {
  loanChartEvents,
  loanWindow,
  type LoanChartEvent,
  type LoanDetailStats,
  type LoanHorizon,
} from '@/mobile/lib/loan-detail';

/** The segment's options, in order (`loans:detail.horizon.*`). */
const HORIZONS: readonly LoanHorizon[] = ['past', '5', 'all'];

/** Whole amounts: the loan screen shows no decimals. */
const WHOLE = { decimals: 0 } as const;

interface LoanHeroCardProps {
  row: LoanRow;
  events: readonly LoanEvent[];
  stats: LoanDetailStats;
  today: number;
  horizon: LoanHorizon;
  onHorizonChange: (horizon: LoanHorizon) => void;
}

/**
 * The balance of the loan (spec §8.8 "Úvěr", prototype frame 13): the outstanding amount with the
 * next payment under it, the Dosud · 5 let · Do splacení segment and the balance line — solid up to
 * today (the ledger walk: a hand-entered balance holds flat), dashed after it (the plan ahead) —
 * with a mark for the drawdown, every extra payment, rate change and manual balance, and the
 * fixation, halfway and payoff milestones. A tap on a mark opens its callout; the legend sits under
 * the chart. Every amount goes through the eye.
 */
export function LoanHeroCard({
  row,
  events,
  stats,
  today,
  horizon,
  onHorizonChange,
}: LoanHeroCardProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const text = useLoanText();
  const { loan, currency, trajectory } = row;
  const money = (value: number | string) => moneyText(Number(value) || 0, currency, WHOLE);

  const view = useMemo(() => loanWindow(trajectory, today, horizon), [trajectory, today, horizon]);
  // Marks outside the window would pile up on its edges
  const marks = useMemo(
    () => loanChartEvents(row, events).filter((mark) => mark.t >= view.start && mark.t <= view.end),
    [row, events, view]
  );

  const note = (() => {
    if (stats.balance <= 0) return tl('detail.hero.paidOff');
    if (stats.next) {
      return t('loan.nextPayment', {
        amount: money(stats.next.payment),
        date: fmt.day(stats.next.dueDay, shortDayOptions(stats.next.dueDay)),
      });
    }
    return row.mode === 'static' ? tl('detail.hero.noPayment') : tl('detail.stats.noSchedule');
  })();

  /** The callout of one mark: the desktop's titles and lines (`LoanDetail.tsx`). */
  const describe = (mark: LoanChartEvent): { title: string; lines: string[] } => {
    const day = fmt.day(mark.t);
    const e = mark.event;
    switch (mark.kind) {
      case 'drawdown':
        return {
          title: tl('detail.chart.events.drawdown', { amount: money(loan.principal) }),
          lines: [day],
        };
      case 'extraPayment':
        return {
          title: tl('detail.chart.events.extraPayment', { amount: money(e?.amount ?? 0) }),
          lines: [day, e?.note ?? ''].filter(Boolean),
        };
      case 'rateChange':
        return {
          title: tl('detail.chart.events.rateChange', {
            rate: fmt.percent((Number(e?.rate) || 0) / 100, 2),
          }),
          lines: [
            `${day}${
              e?.monthlyPayment
                ? ` · ${tl('detail.chart.events.newPayment', { amount: money(e.monthlyPayment) })}`
                : ''
            }`,
            e?.note ?? '',
          ].filter(Boolean),
        };
      case 'balanceCheck':
        return {
          title: tl('detail.chart.events.balanceCheck', { amount: money(e?.amount ?? 0) }),
          lines: [day, e?.note ?? ''].filter(Boolean),
        };
      case 'fixationEnd':
        return {
          title: tl('detail.chart.events.fixationEnd'),
          lines: [
            `${text.monthYear(mark.t)} · ${tl('detail.chart.events.balance', {
              amount: money(mark.balance ?? 0),
            })}`,
          ],
        };
      case 'half':
        return {
          title: tl('detail.chart.events.half'),
          lines: [
            `${text.monthYear(mark.t)} · ${tl('detail.chart.events.balance', {
              amount: money(mark.balance ?? 0),
            })}`,
          ],
        };
      case 'payoff':
        return { title: tl('detail.chart.events.payoff'), lines: [text.monthYear(mark.t)] };
    }
  };

  const eventTip = (cluster: EventCluster<LoanChartEvent>) => {
    if (cluster.events.length === 1) return describe(cluster.events[0]);
    return {
      // Only milestones fold into "N milestones"; the drawdown or an extra payment among them
      // makes it events
      title: cluster.events.every((e) => e.type === 'mark')
        ? tl('chart.milestones.several', { count: cluster.events.length })
        : tl('chart.events.several', { count: cluster.events.length }),
      lines: cluster.events.map((e) => describe(e).title),
    };
  };

  const label = tl('detail.hero.outstanding');

  return (
    <Hero<LoanChartEvent, LoanHorizon>
      label={label}
      value={stats.balance}
      formatValue={(value) => moneyText(value, currency, WHOLE)}
      note={note}
      period={horizon}
      periods={HORIZONS}
      onPeriodChange={onHorizonChange}
      periodLabel={(h) => tl(`detail.horizon.${h}`)}
      points={view.points}
      step
      zeroBaseline
      projectedFrom={today}
      events={marks}
      renderEventTip={eventTip}
      chartLabel={label}
    >
      {view.points.length > 1 && (
        <ChartLegend
          items={[
            { label: tl('detail.chart.legend.balance'), swatch: { kind: 'line' } },
            {
              label: tl('detail.chart.legend.schedule'),
              swatch: { kind: 'dash', color: 'var(--chart-line)' },
            },
            { label: tl('detail.chart.legend.drawdown'), swatch: { kind: 'event', type: 'buy' } },
            {
              label: tl('detail.chart.legend.extraPayment'),
              swatch: { kind: 'event', type: 'income' },
            },
            {
              label: tl('detail.chart.legend.milestones'),
              swatch: { kind: 'event', type: 'mark' },
            },
          ]}
        />
      )}
    </Hero>
  );
}
