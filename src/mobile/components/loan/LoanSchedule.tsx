import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LoanSchedule as LoanScheduleData } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import type { LoanRow } from '@/utils/loans';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { Segment } from '@/mobile/components/Segment';
import { MoreRow } from '@/mobile/components/wealth/MoreRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { scheduleYears, upcomingRows } from '@/mobile/lib/loan-detail';

type Mode = 'next12' | 'years';

/** The next-12 list shows this many payments until it is unfolded. */
const FOLDED = 4;
/** Whole amounts: the schedule shows no decimals. */
const WHOLE = { decimals: 0 } as const;

interface LoanScheduleProps {
  row: LoanRow;
  schedule: LoanScheduleData;
}

/**
 * "Splátkový kalendář" (spec §8.8, prototype frame 13): the head and the Příštích 12 · Po letech
 * segment over the plan's payments as rows without a chip — the day, "úrok X · jistina Y", the
 * payment and what is left after it — or the same grouped per year. The next 12 fold after four
 * behind "Zobrazit všech 12". The plan is the backend's schedule ahead of today, which starts from
 * a hand-entered balance while one holds. Nothing to list (no payment, paid off) says so instead
 * of leaving the section empty.
 */
export function LoanSchedule({ row, schedule }: LoanScheduleProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [mode, setMode] = useState<Mode>('next12');
  const [unfolded, setUnfolded] = useState(false);
  const money = (value: number) => moneyText(value, row.currency, WHOLE);

  const upcoming = useMemo(() => upcomingRows(schedule), [schedule]);
  const next12 = upcoming.slice(0, 12);
  const years = useMemo(() => scheduleYears(upcoming), [upcoming]);
  const shown = unfolded ? next12 : next12.slice(0, FOLDED);

  const options: { value: Mode; label: string }[] = [
    { value: 'next12', label: tl('detail.schedule.modes.next12') },
    { value: 'years', label: tl('detail.schedule.modes.years') },
  ];

  return (
    <>
      <SectionHead title={tl('detail.schedule.title')} />
      <Segment
        value={mode}
        options={options}
        onChange={setMode}
        label={tl('detail.schedule.title')}
        className="mb-2"
      />
      {upcoming.length === 0 ? (
        <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
          {Number(schedule.outstandingBalance) <= 0
            ? tl('detail.schedule.paidOff')
            : tl('detail.schedule.empty')}
        </p>
      ) : mode === 'years' ? (
        <EntityRows>
          {years.map((y) => (
            <EntityRow
              key={y.year}
              title={String(y.year)}
              sub={t('loan.schedule.yearSub', { count: y.count, interest: money(y.interest) })}
              value={money(y.payments)}
              valueSub={t('loan.schedule.left', { amount: money(y.balanceEnd) })}
            />
          ))}
        </EntityRows>
      ) : (
        <EntityRows>
          {shown.map((r) => (
            <EntityRow
              key={r.dueDay}
              title={fmt.day(r.dueDay)}
              sub={tl('detail.stats.nextPaymentNote', {
                interest: money(r.interest),
                principal: money(r.principalPart),
              })}
              value={money(r.payment)}
              valueSub={t('loan.schedule.left', { amount: money(r.balanceAfter) })}
            />
          ))}
          {next12.length > FOLDED && !unfolded && (
            <MoreRow
              label={t('loan.schedule.showAll', { count: next12.length })}
              onClick={() => setUnfolded(true)}
            />
          )}
        </EntityRows>
      )}
    </>
  );
}
