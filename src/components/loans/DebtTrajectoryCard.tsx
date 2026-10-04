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
import { Card } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';

type Horizon = '5' | '10' | 'all';

interface Milestone extends ChartEvent {
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
 * from the first drawdown, the schedules dashed after today, with payoffs,
 * fixation ends and half-repaid marks as milestones.
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

  const milestones = useMemo<Milestone[]>(() => {
    const out: Milestone[] = [];
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
    return out.filter((m) => m.t <= end).sort((a, b) => a.t - b.t);
    // formatters are stable per locale/currency
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, metrics.debtFreeDay, today, end, currencyCode]);

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
    <Card className='relative mb-7 overflow-hidden px-[21px] pb-[14px] pt-5 after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-[""]'>
      <div className="relative z-[1] flex items-center justify-between gap-6">
        <div>
          <h3 className="m-0 text-h3 text-ink">{t('chart.title')}</h3>
          <p className="mt-[5px] text-micro font-500 text-ink-4">{t('chart.subtitle')}</p>
        </div>
        <Segmented
          value={horizon}
          onValueChange={setHorizon}
          options={[
            { value: '5', label: t('chart.horizon.5') },
            { value: '10', label: t('chart.horizon.10') },
            { value: 'all', label: t('chart.horizon.all') },
          ]}
        />
      </div>
      <MoonyLineChart<Milestone>
        className="relative z-[1] -mx-1 mt-[14px]"
        points={points}
        height={170}
        zeroBaseline
        projectedFrom={today}
        formatValue={money}
        renderTip={(p) => ({
          title: money(p.value),
          lines: [`${text.monthYear(p.t)}${p.t > today ? ` · ${t('chart.tipSchedule')}` : ''}`],
        })}
        events={milestones}
        renderEventTip={(cluster: EventCluster<Milestone>) =>
          cluster.events.length === 1
            ? { title: cluster.events[0].title, lines: cluster.events[0].lines }
            : {
                title: t('chart.milestones.several', { count: cluster.events.length }),
                lines: cluster.events.map((e) => e.title),
              }
        }
      />
      <div className="relative z-[1]">
        <ChartLegend
          items={[
            { label: t('chart.legend.outstanding'), swatch: { kind: 'line' } },
            {
              label: t('chart.legend.schedule'),
              swatch: { kind: 'dash', color: 'var(--chart-line)' },
            },
            { label: t('chart.legend.milestone'), swatch: { kind: 'ring' } },
          ]}
          note={note}
        />
      </div>
    </Card>
  );
}
