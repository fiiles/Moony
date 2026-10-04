import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LoanSchedule } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useLoanText } from '@/hooks/use-loan-text';
import { Badge } from '@/components/ui/badge';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { cn } from '@/lib/utils';

type Mode = 'next12' | 'years' | 'all';

interface LoanScheduleTableProps {
  schedule: LoanSchedule;
  currency: string;
}

interface YearRow {
  year: number;
  payments: number;
  interest: number;
  principal: number;
  balanceEnd: number;
  /** Interest share of the first payment in the year. */
  firstShare: number;
}

/**
 * Repayment schedule (prototype loan-detail.html): the next twelve payments
 * with the coming one marked, the same grouped per calendar year, or every
 * payment from the anchor with the ones already due muted.
 */
export function LoanScheduleTable({ schedule, currency }: LoanScheduleTableProps) {
  const { t } = useTranslation('loans');
  const fmt = useFormat();
  const text = useLoanText();
  const [mode, setMode] = useState<Mode>('next12');

  const money = (value: string | number) => fmt.money(Number(value), currency, { decimals: 0 });
  const rows = schedule.rows;
  const firstUpcoming = rows.findIndex((row) => row.dueDay > schedule.asOfDay);
  const upcomingStart = firstUpcoming === -1 ? rows.length : firstUpcoming;
  const upcoming = rows.slice(upcomingStart);
  const next12 = upcoming.slice(0, 12);

  const years = useMemo<YearRow[]>(() => {
    const map = new Map<number, YearRow>();
    for (const row of upcoming) {
      const year = new Date(row.dueDay * 1000).getUTCFullYear();
      const payment = Number(row.payment);
      let y = map.get(year);
      if (!y) {
        y = {
          year,
          payments: 0,
          interest: 0,
          principal: 0,
          balanceEnd: 0,
          firstShare: payment > 0 ? Number(row.interest) / payment : 0,
        };
        map.set(year, y);
      }
      y.payments += payment;
      y.interest += Number(row.interest);
      y.principal += Number(row.principalPart);
      y.balanceEnd = Number(row.balanceAfter);
    }
    return [...map.values()];
  }, [upcoming]);

  const sum = (list: typeof rows, key: 'interest' | 'principalPart' | 'payment') =>
    list.reduce((s, r) => s + Number(r[key]), 0);

  const footer = (() => {
    switch (mode) {
      case 'next12':
        return t('detail.schedule.footNext12', {
          interest: money(sum(next12, 'interest')),
          principal: money(sum(next12, 'principalPart')),
        });
      case 'years':
        return years.length > 0
          ? t('detail.schedule.footYears', {
              count: years.length,
              from: fmt.percent(years[0].firstShare, 0),
              to: fmt.percent(years[years.length - 1].firstShare, 0),
            })
          : '';
      default:
        return t('detail.schedule.footAll', {
          count: rows.length,
          date: fmt.day(schedule.anchorDay),
        });
    }
  })();

  const empty = rows.length === 0 || (mode !== 'all' && upcoming.length === 0);

  return (
    <Card variant="table">
      <CardHeader>
        <div>
          <CardTitle>{t('detail.schedule.title')}</CardTitle>
          <CardDescription>{t('detail.schedule.subtitle')}</CardDescription>
        </div>
        <Segmented
          value={mode}
          onValueChange={setMode}
          options={[
            { value: 'next12', label: t('detail.schedule.modes.next12') },
            { value: 'years', label: t('detail.schedule.modes.years') },
            { value: 'all', label: t('detail.schedule.modes.all') },
          ]}
        />
      </CardHeader>
      {empty ? (
        <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
          {Number(schedule.outstandingBalance) <= 0
            ? t('detail.schedule.paidOff')
            : t('detail.schedule.empty')}
        </p>
      ) : mode === 'years' ? (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('detail.schedule.year')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.payments')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.interestTotal')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.principal')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.balanceEnd')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {years.map((y) => (
              <TableRow key={y.year} className="h-11">
                <TableCell className="font-600 text-ink">{y.year}</TableCell>
                <TableCell className="text-right num">{money(y.payments)}</TableCell>
                <TableCell className="text-right num">{money(y.interest)}</TableCell>
                <TableCell className="text-right num font-650 text-ink">
                  {money(y.principal)}
                </TableCell>
                <TableCell className="text-right num">{money(y.balanceEnd)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('detail.schedule.payment')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.amount')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.interest')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.principal')}</TableHead>
              <TableHead className="text-right">{t('detail.schedule.balanceAfter')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {(mode === 'all' ? rows : next12).map((row, i) => {
              const index = mode === 'all' ? i : upcomingStart + i;
              const past = row.dueDay <= schedule.asOfDay;
              const isNext = index === upcomingStart;
              return (
                <TableRow
                  key={row.dueDay}
                  className={cn('h-11', past && 'text-ink-4', isNext && 'bg-well')}
                >
                  <TableCell className={cn(!past && 'text-ink')}>
                    <span className="inline-flex items-center gap-1.5">
                      <span className="num text-ink-4">{index + 1}.</span>
                      {text.monthYear(row.dueDay)}
                      {isNext && <Badge variant="dark">{t('detail.schedule.next')}</Badge>}
                    </span>
                  </TableCell>
                  <TableCell className="text-right num">{money(row.payment)}</TableCell>
                  <TableCell className="text-right num">{money(row.interest)}</TableCell>
                  <TableCell className={cn('text-right num font-650', !past && 'text-ink')}>
                    {money(row.principalPart)}
                  </TableCell>
                  <TableCell className="text-right num">{money(row.balanceAfter)}</TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      <div className="flex items-center justify-between gap-6 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
        <span>{footer}</span>
        <span>{t('detail.schedule.method')}</span>
      </div>
    </Card>
  );
}
