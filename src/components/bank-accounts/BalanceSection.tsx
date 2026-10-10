import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { BankBalanceRecord, SavingsAccountZone } from '@shared/schema';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { NativeBalance } from '@/components/bank-accounts/NativeBalance';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import type { ChartEvent } from '@/utils/chart-scale';
import { useFormat } from '@/lib/use-format';
import { calculateZonedInterest } from '@/utils/bank-account-zones';
import {
  averageMonthlyChange,
  balanceSeries,
  changeThisYear,
  newestRecord,
  previousRecord,
} from '@/utils/balance-records';

type RecordEvent = ChartEvent & { record: BankBalanceRecord };

interface BalanceSectionProps {
  account: {
    balance: string;
    balanceAsOf: number | null;
    interestRate: string | null;
    hasZoneDesignation: boolean;
  };
  records: readonly BankBalanceRecord[];
  zones: readonly SavingsAccountZone[];
  /** UTC day start of today (unix seconds). */
  today: number;
  currency: string;
  /** Formats a value in the account currency (0 decimals). */
  formatValue: (value: number) => string;
}

/** Hero with the balance of the newest record, the line through the records of the last year, and the three balance stats. */
export function BalanceSection({
  account,
  records,
  zones,
  today,
  currency,
  formatValue,
}: BalanceSectionProps) {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const balance = parseFloat(account.balance || '0');
  const newest = newestRecord(records);
  const previous = previousRecord(records);
  const signed = (v: number) => fmt.money(v, currency, { signed: true, decimals: 0 });

  const series = useMemo(() => balanceSeries(records, today), [records, today]);
  const points = useMemo(() => series.map((p) => ({ t: p.t, value: p.value })), [series]);
  const events = useMemo<RecordEvent[]>(
    () =>
      series
        .filter((p) => p.record)
        .flatMap((p) => {
          const record = records.find((r) => r.asOf === p.t);
          return record ? [{ id: record.id, t: p.t, type: 'mark' as const, record }] : [];
        }),
    [series, records]
  );

  const delta = newest && previous ? parseFloat(newest.amount) - parseFloat(previous.amount) : null;
  const thisYear = changeThisYear(records, today);
  const avg = averageMonthlyChange(records);
  const zoned = account.hasZoneDesignation && zones.length > 0;
  const simpleRate = parseFloat(account.interestRate ?? '0') || 0;
  const yearlyInterest = zoned
    ? calculateZonedInterest(balance, zones)
    : balance * (simpleRate / 100);
  const hasInterest = zoned || simpleRate > 0;
  const effective = balance > 0 ? yearlyInterest / balance : 0;

  return (
    <>
      <HeroCard
        chart={
          points.length >= 2 ? (
            <>
              <MoonyLineChart<RecordEvent>
                className="-mx-1 mt-[18px]"
                points={points}
                height={180}
                formatValue={formatValue}
                renderTip={(p) => ({ title: formatValue(p.value), lines: [fmt.day(p.t)] })}
                events={events}
                renderEventTip={(cluster) => ({
                  title: formatValue(parseFloat(cluster.events[0].record.amount)),
                  lines: [
                    t('balanceHero.recordTip', {
                      date: fmt.day(cluster.events[0].t),
                      source: t(
                        cluster.events[0].record.source === 'statement'
                          ? 'balanceHistory.sourceStatement'
                          : 'balanceHistory.sourceManual'
                      ),
                    }),
                  ],
                })}
                axisEnd={today}
              />
              <ChartLegend
                items={[
                  { label: t('chart.balance'), swatch: { kind: 'line' } },
                  { label: t('balanceHero.record'), swatch: { kind: 'event', type: 'mark' } },
                ]}
              />
            </>
          ) : undefined
        }
      >
        <HeroValue
          label={
            newest
              ? t('balanceHero.label', { date: fmt.day(newest.asOf) })
              : account.balanceAsOf
                ? t('balanceHero.label', { date: fmt.day(account.balanceAsOf) })
                : t('balanceHero.unknownDate')
          }
          value={
            <NativeBalance
              amount={balance}
              currency={currency}
              align="start"
              className=""
              secondaryClassName="ml-2 text-table font-500 text-ink-4"
            />
          }
          tone={delta === null ? 'neutral' : delta > 0 ? 'gain' : delta < 0 ? 'loss' : 'neutral'}
          delta={
            delta === null
              ? undefined
              : `${delta > 0 ? '↗' : delta < 0 ? '↘' : '→'} ${signed(delta)}`
          }
          deltaNote={
            delta === null
              ? newest
                ? t('balanceHero.firstRecord')
                : undefined
              : t('balanceHero.since', { date: fmt.day(previous!.asOf) })
          }
        />
      </HeroCard>
      <Stats columns={3} className="mt-[17px]">
        <Stat
          label={t('balanceStats.changeThisYear')}
          value={thisYear ? signed(thisYear.delta) : '—'}
          tone={
            thisYear
              ? thisYear.delta > 0
                ? 'gain'
                : thisYear.delta < 0
                  ? 'loss'
                  : 'neutral'
              : 'neutral'
          }
          noteTone="neutral"
          note={
            thisYear
              ? t('balanceStats.vsRecordAt', { date: fmt.day(thisYear.baseDay) })
              : t('balanceStats.noRecordBeforeYear')
          }
        />
        <Stat
          label={t('balanceStats.avgPerMonth')}
          value={avg ? signed(avg.perMonth) : '—'}
          tone={
            avg ? (avg.perMonth > 0 ? 'gain' : avg.perMonth < 0 ? 'loss' : 'neutral') : 'neutral'
          }
          noteTone="neutral"
          note={
            avg
              ? avg.sinceFirst
                ? t('balanceStats.sinceFirstRecord', { count: avg.months })
                : t('balanceStats.last12Months')
              : t(records.length === 0 ? 'balanceStats.noRecord' : 'balanceStats.singleRecord')
          }
        />
        <Stat
          label={t('balanceStats.yearlyInterest')}
          value={hasInterest ? formatValue(yearlyInterest) : '—'}
          note={
            hasInterest
              ? zoned
                ? t('balanceStats.effectiveZoned', { rate: fmt.percent(effective, 2) })
                : t('balanceStats.simpleRate', { rate: fmt.percent(simpleRate / 100, 2) })
              : t('balanceStats.noInterest')
          }
        />
      </Stats>
    </>
  );
}
