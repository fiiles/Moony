import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { dueDay } from '@shared/calculations/loan-amortization';
import type { ConvertFn } from '@shared/fx';
import type { LoanMetrics, LoanRow } from '@/utils/loans';
import { monthsAhead } from '@/utils/loans';
import { clipSeries, debtSeries, stepValueAt } from '@/utils/loan-trajectory';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { useCurrency } from '@/lib/currency';
import { useDatedMainConvert } from '@/hooks/use-fx';
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
  /** Into the main currency at the current quotes (the schedule ahead of today). */
  convert: ConvertFn;
}

/**
 * Debt trajectory (prototype loans.html): the sum of all balances, actual solid
 * from the first drawdown, the schedules dashed after today. Every loan marks
 * its start (the ▲ where the line steps up), payoffs, fixation ends and
 * half-repaid days are the milestones. Past balances convert at their own day,
 * the schedule ahead at today's quotes; a loan without a rate today stays out (the
 * page note names it).
 */
export function DebtTrajectoryCard({ rows, metrics, today, convert }: DebtTrajectoryCardProps) {
  const { t } = useTranslation('loans');
  const { currencyCode } = useCurrency();
  const fmt = useFormat();
  const text = useLoanText();
  const [horizon, setHorizon] = useState<Horizon>('10');

  const counted = useMemo(() => rows.filter((r) => r.converted !== null), [rows]);
  const active = useMemo(() => counted.filter((r) => !r.matured), [counted]);
  const firstStart = counted.length ? Math.min(...counted.map((r) => r.terms.startDay)) : undefined;
  const { ready, convertAt } = useDatedMainConvert(firstStart);

  /** A native amount of `row` on day `t` in the main currency (today's quotes ahead of today). */
  const at = useMemo(
    () =>
      (row: LoanRow, amount: number, t: number): number | null =>
        t > today ? convert(amount, row.currency) : convertAt(amount, row.currency, t),
    [convert, convertAt, today]
  );
  const money = (v: number) => fmt.money(v, currencyCode, { decimals: 0 });
  /** In the main currency on day `t`, or the native amount when that day has no rate. */
  const moneyAt = (row: LoanRow, amount: number, t: number) => {
    const value = at(row, amount, t);
    return value !== null ? money(value) : fmt.money(amount, row.currency, { decimals: 0 });
  };

  const series = useMemo(
    () =>
      debtSeries(
        counted.map((r) => ({
          trajectory: r.trajectory,
          convert: (amount: number, t: number) => at(r, amount, t),
        })),
        today
      ),
    [counted, today, at]
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
    // Every loan on the line, paid-off ones too: the mark says why the line steps up there
    for (const r of counted) {
      out.push({
        id: `start-${r.loan.id}`,
        t: r.terms.startDay,
        type: 'buy',
        title: t('chart.milestones.newLoan', { name: r.loan.name }),
        lines: [`${fmt.day(r.terms.startDay)} · ${moneyAt(r, r.principal, r.terms.startDay)}`],
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
                    amount: moneyAt(r, r.payment, r.payoffDay),
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
              amount: moneyAt(r, r.principal / 2, r.trajectory.halfDay),
            })}`,
          ],
        });
      }
    }
    return out.filter((e) => e.t >= start && e.t <= end).sort((a, b) => a.t - b.t);
    // `text` and `moneyAt` are rebuilt every render but only follow the locale, the currency and `at`
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t, fmt, counted, active, metrics.debtFreeDay, today, start, end, currencyCode, at]);

  // Past balances of a foreign loan need the daily quotes; without them the line would lack its past
  const waiting = !ready && counted.some((r) => r.currency !== currencyCode);
  if (waiting || series.length < 2) return null;

  const todayValue = stepValueAt(series, today);
  const endValue = points[points.length - 1]?.value ?? 0;
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
