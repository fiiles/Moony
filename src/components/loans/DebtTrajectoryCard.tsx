import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { dueDay } from '@shared/calculations/loan-amortization';
import type { CurrencyCode } from '@shared/currencies';
import type { LoanMetrics, LoanRow } from '@/utils/loans';
import { monthsAhead } from '@/utils/loans';
import { clipSeries, debtSeries, stepValueAt } from '@/utils/loan-trajectory';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useLoanText } from '@/hooks/use-loan-text';
import { Segmented } from '@/components/ui/segmented';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { TREND_CHART_HEIGHT, TrendCard } from '@/components/charts/TrendCard';

type Horizon = '5' | '10' | 'all';

/** A mark on the trace: the start of a loan (`buy`) or a milestone ahead (`mark`). */
interface DebtEvent extends ChartEvent {
  title: string;
  lines: string[];
}

interface DebtTrajectoryCardProps {
  rows: LoanRow[];
  metrics: LoanMetrics;
  today: number;
}

/**
 * Debt trajectory (prototype loans.html): the sum of all balances, actual solid
 * from the first drawdown, the schedules dashed after today. Every loan marks
 * its start (the ▲ where the line steps up), payoffs, fixation ends and
 * half-repaid days are the milestones.
 */
export function DebtTrajectoryCard({ rows, metrics, today }: DebtTrajectoryCardProps) {
  const { t } = useTranslation('loans');
  const { formatCurrency, convert, currencyCode } = useCurrency();
  const fmt = useFormat();
  const text = useLoanText();
  const [horizon, setHorizon] = useState<Horizon>('10');

  const active = useMemo(() => rows.filter((r) => !r.matured), [rows]);
  const toDisplay = (czk: number) => convert(czk, 'CZK', currencyCode as CurrencyCode);

  // Sum in CZK, then into the display currency
  const series = useMemo(
    () =>
      debtSeries(
        rows.map((r) => ({ trajectory: r.trajectory, rate: r.rate })),
        today
      ).map((p) => ({ t: p.t, value: toDisplay(p.value) })),
    // convert is stable per currency
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rows, today, currencyCode]
  );

  const lastT = series[series.length - 1]?.t ?? today;
  const end =
    horizon === 'all'
      ? Math.max(metrics.debtFreeDay ?? lastT, today + 86_400)
      : dueDay(today, Number(horizon) * 12);
  const points = useMemo(() => clipSeries(series, 0, end), [series, end]);
  const start = points[0]?.t ?? today;

  const events = useMemo<DebtEvent[]>(() => {
    const out: DebtEvent[] = [];
    // Every loan, paid-off ones too: the mark says why the line steps up there
    for (const r of rows) {
      out.push({
        id: `start-${r.loan.id}`,
        t: r.terms.startDay,
        type: 'buy',
        title: t('chart.milestones.newLoan', { name: r.loan.name }),
        lines: [`${fmt.day(r.terms.startDay)} · ${formatCurrency(r.principalCzk)}`],
      });
    }
    for (const r of active) {
      if (r.payoffDay !== null && r.payoffDay > today) {
        const isLast = metrics.debtFreeDay === r.payoffDay;
        out.push({
          id: `payoff-${r.loan.id}`,
          t: r.payoffDay,
          type: 'mark',
          title: t('chart.milestones.paidOff', { name: r.loan.name }),
          lines: [
            `${text.monthYear(r.payoffDay)} · ${
              isLast
                ? t('chart.milestones.debtFree')
                : t('chart.milestones.paidOffPayments', {
                    amount: formatCurrency(r.paymentCzk),
                  })
            }`,
          ],
        });
      }
      if (r.fixationEnd !== null) {
        out.push({
          id: `fix-${r.loan.id}`,
          t: r.fixationEnd,
          type: 'mark',
          title: t('chart.milestones.fixationEnd', { name: r.loan.name }),
          lines: [
            `${text.monthYear(r.fixationEnd)} · ${fmt.percent(
              (Number(r.loan.interestRate) || 0) / 100,
              2
            )}`,
          ],
        });
      }
      if (
        r.trajectory.halfDay !== null &&
        r.trajectory.halfDay > today &&
        r.trajectory.halfDay !== r.payoffDay
      ) {
        out.push({
          id: `half-${r.loan.id}`,
          t: r.trajectory.halfDay,
          type: 'mark',
          title: t('chart.milestones.half', { name: r.loan.name }),
          lines: [
            `${text.monthYear(r.trajectory.halfDay)} · ${t('chart.milestones.balance', {
              amount: formatCurrency(r.principalCzk / 2),
            })}`,
          ],
        });
      }
    }
    return out.filter((e) => e.t >= start && e.t <= end).sort((a, b) => a.t - b.t);
    // `text` and `formatCurrency` are rebuilt every render but only follow the locale and currency
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t, fmt, rows, active, metrics.debtFreeDay, today, start, end, currencyCode]);

  if (series.length < 2) return null;

  const todayValue = stepValueAt(series, today);
  const endValue = points[points.length - 1]?.value ?? 0;
  const money = (v: number) => fmt.money(v, currencyCode, { decimals: 0 });
  const note =
    horizon === 'all'
      ? metrics.debtFreeDay
        ? t('chart.noteDebtFree', {
            today: money(todayValue),
            duration: text.months(monthsAhead(today, metrics.debtFreeDay)),
          })
        : t('chart.noteNoPayoff', { today: money(todayValue) })
      : t('chart.note', {
          today: money(todayValue),
          duration: text.months(Number(horizon) * 12),
          amount: money(endValue),
        });

  return (
    <TrendCard
      title={t('chart.title')}
      subtitle={t('chart.subtitle')}
      aside={
        <Segmented
          value={horizon}
          onValueChange={setHorizon}
          options={[
            { value: '5', label: t('chart.horizon.5') },
            { value: '10', label: t('chart.horizon.10') },
            { value: 'all', label: t('chart.horizon.all') },
          ]}
        />
      }
      legend={
        <ChartLegend
          items={[
            { label: t('chart.legend.outstanding'), swatch: { kind: 'line' } },
            {
              label: t('chart.legend.schedule'),
              swatch: { kind: 'dash', color: 'var(--chart-line)' },
            },
            { label: t('chart.legend.newLoan'), swatch: { kind: 'event', type: 'buy' } },
            { label: t('chart.legend.milestone'), swatch: { kind: 'ring' } },
          ]}
          note={note}
        />
      }
    >
      <MoonyLineChart<DebtEvent>
        points={points}
        height={TREND_CHART_HEIGHT}
        zeroBaseline
        projectedFrom={today}
        formatValue={money}
        renderTip={(p) => ({
          title: money(p.value),
          lines: [`${text.monthYear(p.t)}${p.t > today ? ` · ${t('chart.tipSchedule')}` : ''}`],
        })}
        events={events}
        renderEventTip={(cluster: EventCluster<DebtEvent>) =>
          cluster.events.length === 1
            ? { title: cluster.events[0].title, lines: cluster.events[0].lines }
            : {
                // Only milestones fold into "N milestones"; a new loan among them makes it events
                title: cluster.events.every((e) => e.type === 'mark')
                  ? t('chart.milestones.several', { count: cluster.events.length })
                  : t('chart.events.several', { count: cluster.events.length }),
                lines: cluster.events.map((e) => e.title),
              }
        }
      />
    </TrendCard>
  );
}
