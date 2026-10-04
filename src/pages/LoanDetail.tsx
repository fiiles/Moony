import { useMemo, useState } from 'react';
import { useLocation, useRoute } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Check, Ellipsis, Pencil, Plus, Repeat, Trash2, TriangleAlert } from 'lucide-react';
import type { InsertLoan, Loan, LoanEvent, LoanEventKind, LoanSchedule } from '@shared/schema';
import { dueDay } from '@shared/calculations/loan-amortization';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { monthsAhead, type LoanRow } from '@/utils/loans';
import {
  annuityPayment,
  clipSeries,
  stepValueAt,
  type ExtraPaymentMode,
} from '@/utils/loan-trajectory';
import { useFormat } from '@/lib/use-format';
import { useLoans, useLoanSchedule } from '@/hooks/use-loans';
import { useLoanMutations } from '@/hooks/use-loan-mutations';
import { useLoanEventMutations, useLoanEvents } from '@/hooks/use-loan-events';
import { useLoanText } from '@/hooks/use-loan-text';
import { useShellPage } from '@/components/shell/shell-context';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, Stats } from '@/components/common/Stat';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { LoanFormDialog } from '@/components/loans/LoanFormDialog';
import { LoanScheduleTable } from '@/components/loans/LoanScheduleTable';
import { LoanWhatIfCard } from '@/components/loans/LoanWhatIfCard';
import { LoanEventsCard } from '@/components/loans/LoanEventsCard';
import { LoanEventDialog } from '@/components/loans/LoanEventDialog';

type Horizon = 'past' | '5' | 'all';

interface LoanChartEvent extends ChartEvent {
  title: string;
  lines: string[];
}

/**
 * Loan detail (design system §7 Detail, prototype loan-detail.html): the
 * balance trace with refixations, extra payments, statement checks and the
 * milestones ahead, four stats, the schedule in three views, the extra-payment
 * what-if and the events list.
 */
export default function LoanDetail() {
  const [, params] = useRoute('/loans/:id');
  const [, setLocation] = useLocation();
  const id = params?.id;
  const { t } = useTranslation('loans');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { rows, isLoading: loansLoading, today } = useLoans();
  const { data: schedule, isLoading: scheduleLoading } = useLoanSchedule(id);
  const { data: events = [] } = useLoanEvents(id);
  const { updateMutation, deleteMutation } = useLoanMutations();
  const { remove: removeEvent } = useLoanEventMutations(id);

  const [horizon, setHorizon] = useState<Horizon>('5');
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [eventKind, setEventKind] = useState<LoanEventKind | null>(null);
  const [eventInitial, setEventInitial] = useState<
    { amount?: number; mode?: ExtraPaymentMode } | undefined
  >(undefined);
  const [pendingDeleteEvent, setPendingDeleteEvent] = useState<LoanEvent | null>(null);

  const row = rows.find((r) => r.loan.id === id);
  const loan = row?.loan;

  useShellPage({
    crumb: loan?.name,
    status: row
      ? {
          text:
            row.mode === 'manual'
              ? t('detail.status.manual', {
                  date: fmt.day(loan!.balanceAnchorDate ?? loan!.startDate),
                })
              : row.mode === 'static'
                ? t('detail.status.static')
                : t('detail.status.automatic'),
          tone: row.mode === 'static' || row.belowInterest ? 'stale' : 'neutral',
        }
      : undefined,
  });

  const openEvent = (
    kind: LoanEventKind,
    initial?: { amount?: number; mode?: ExtraPaymentMode }
  ) => {
    setEventInitial(initial);
    setEventKind(kind);
  };

  if (loansLoading || (loan && scheduleLoading)) {
    return (
      <>
        <BackLink href="/loans">{t('detail.backToLoans')}</BackLink>
        <div className="mb-[26px]">
          <div className="skeleton h-2.5 w-32" />
          <div className="skeleton mt-3 h-9 w-72" />
          <div className="skeleton mt-3 h-3 w-96" />
        </div>
        <Card className="px-6 pb-4 pt-6">
          <div className="skeleton h-3 w-24" />
          <div className="skeleton mt-3 h-10 w-56" />
          <div className="skeleton mt-6 h-[220px] w-full" />
        </Card>
      </>
    );
  }
  if (!row || !loan || !schedule) {
    return (
      <>
        <BackLink href="/loans">{t('detail.backToLoans')}</BackLink>
        <p className="mt-8 text-center text-ink-3">{t('detail.notFound')}</p>
      </>
    );
  }

  const handleEditSubmit = (data: InsertLoan | (Partial<Loan> & { id: string })) => {
    if (!('id' in data)) return;
    updateMutation.mutate(data as { id: string } & Partial<InsertLoan>, {
      onSuccess: () => setEditOpen(false),
    });
  };

  return (
    <>
      <BackLink href="/loans">{t('detail.backToLoans')}</BackLink>
      <LoanDetailBody
        row={row}
        schedule={schedule}
        events={events}
        today={today}
        horizon={horizon}
        onHorizonChange={setHorizon}
        actions={
          <>
            <Button variant="outline" onClick={() => setEditOpen(true)}>
              <Pencil />
              {t('actions.edit')}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label={tc('labels.moreActions')}>
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => openEvent('balance_check')}>
                  <Check />
                  {t('actions.balanceCheck')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => openEvent('rate_change')}>
                  <Repeat />
                  {t('actions.rateChange')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="danger" onSelect={() => setDeleteOpen(true)}>
                  <Trash2 />
                  {t('actions.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button
              onClick={() => openEvent('extra_payment')}
              disabled={row.mode === 'static' || row.outstanding <= 0}
            >
              <Plus />
              {t('actions.extraPayment')}
            </Button>
          </>
        }
        onRecordExtraPayment={(amount, mode) => openEvent('extra_payment', { amount, mode })}
        onDeleteEvent={setPendingDeleteEvent}
      />

      <LoanFormDialog
        open={editOpen}
        onOpenChange={setEditOpen}
        onSubmit={handleEditSubmit}
        loan={loan}
        isLoading={updateMutation.isPending}
      />
      {eventKind && (
        <LoanEventDialog
          open
          onOpenChange={(next) => {
            if (!next) setEventKind(null);
          }}
          loan={loan}
          terms={row.terms}
          today={today}
          kind={eventKind}
          initial={eventInitial}
        />
      )}
      <ConfirmDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: loan.name })}
        onConfirm={() =>
          deleteMutation.mutate(loan.id, {
            onSuccess: () => {
              setDeleteOpen(false);
              setLocation('/loans');
            },
          })
        }
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.delete')}
      />
      <ConfirmDeleteDialog
        open={pendingDeleteEvent !== null}
        onOpenChange={(next) => {
          if (!next) setPendingDeleteEvent(null);
        }}
        title={t('detail.events.confirmDelete.title')}
        description={t('detail.events.confirmDelete.description')}
        onConfirm={() =>
          pendingDeleteEvent &&
          removeEvent.mutate(pendingDeleteEvent.id, {
            onSuccess: () => setPendingDeleteEvent(null),
          })
        }
        isPending={removeEvent.isPending}
        confirmLabel={t('detail.events.delete')}
      />
    </>
  );
}

interface LoanDetailBodyProps {
  row: LoanRow;
  schedule: LoanSchedule;
  events: LoanEvent[];
  today: number;
  horizon: Horizon;
  onHorizonChange: (h: Horizon) => void;
  actions: React.ReactNode;
  onRecordExtraPayment: (amount: number, mode: ExtraPaymentMode) => void;
  onDeleteEvent: (event: LoanEvent) => void;
}

function LoanDetailBody({
  row,
  schedule,
  events,
  today,
  horizon,
  onHorizonChange,
  actions,
  onRecordExtraPayment,
  onDeleteEvent,
}: LoanDetailBodyProps) {
  const { t } = useTranslation('loans');
  const fmt = useFormat();
  const text = useLoanText();
  const { loan, terms, trajectory } = row;
  const currency = loan.currency || 'CZK';
  const money = (v: number | string) => fmt.money(Number(v), currency, { decimals: 0 });
  const rate = Number(loan.interestRate) || 0;
  const payment = Number(loan.monthlyPayment) || 0;
  const principal = Number(loan.principal) || 0;
  const balance = trajectory.balanceToday;
  const repaidShare = principal > 0 ? Math.max(0, 1 - balance / principal) : 0;

  // ---- trace window ----
  const series = useMemo(
    () => [...trajectory.past, ...trajectory.future],
    [trajectory.past, trajectory.future]
  );
  const startDay = series[0]?.t ?? today;
  const lastDay = series[series.length - 1]?.t ?? today;
  const [winStart, winEnd] = (() => {
    switch (horizon) {
      case 'past':
        return [startDay, today];
      case '5':
        return [Math.max(startDay, dueDay(today, -12)), dueDay(today, 60)];
      default:
        return [startDay, Math.max(trajectory.payoffDay ?? lastDay, today + 86_400)];
    }
  })();
  const points = useMemo(() => clipSeries(series, winStart, winEnd), [series, winStart, winEnd]);

  // ---- events on the trace ----
  const chartEvents = useMemo<LoanChartEvent[]>(() => {
    const out: LoanChartEvent[] = [
      {
        id: 'drawdown',
        t: startDay,
        type: 'mark',
        title: t('detail.chart.events.drawdown', { amount: money(principal) }),
        lines: [fmt.day(startDay)],
      },
    ];
    for (const e of events) {
      if (e.kind === 'extra_payment') {
        out.push({
          id: e.id,
          t: e.eventDay,
          type: 'income',
          title: t('detail.chart.events.extraPayment', { amount: money(e.amount ?? 0) }),
          lines: [fmt.day(e.eventDay), e.note ?? ''].filter(Boolean),
        });
      } else if (e.kind === 'rate_change') {
        out.push({
          id: e.id,
          t: e.eventDay,
          type: 'mark',
          title: t('detail.chart.events.rateChange', {
            rate: fmt.percent((Number(e.rate) || 0) / 100, 2),
          }),
          lines: [
            `${fmt.day(e.eventDay)}${
              e.monthlyPayment
                ? ` · ${t('detail.chart.events.newPayment', { amount: money(e.monthlyPayment) })}`
                : ''
            }`,
            e.note ?? '',
          ].filter(Boolean),
        });
      } else {
        out.push({
          id: e.id,
          t: e.eventDay,
          type: 'mark',
          title: t('detail.chart.events.balanceCheck'),
          lines: [`${fmt.day(e.eventDay)} · ${money(e.amount ?? 0)}`, e.note ?? ''].filter(Boolean),
        });
      }
    }
    if (row.fixationEnd !== null) {
      out.push({
        id: 'fixation',
        t: row.fixationEnd,
        type: 'mark',
        title: t('detail.chart.events.fixationEnd'),
        lines: [
          `${text.monthYear(row.fixationEnd)} · ${t('detail.chart.events.balance', {
            amount: money(stepValueAt(series, row.fixationEnd)),
          })}`,
        ],
      });
    }
    if (trajectory.halfDay !== null && trajectory.halfDay !== trajectory.payoffDay) {
      out.push({
        id: 'half',
        t: trajectory.halfDay,
        type: 'mark',
        title: t('detail.chart.events.half'),
        lines: [
          `${text.monthYear(trajectory.halfDay)} · ${t('detail.chart.events.balance', {
            amount: money(principal / 2),
          })}`,
        ],
      });
    }
    if (trajectory.payoffDay !== null) {
      out.push({
        id: 'payoff',
        t: trajectory.payoffDay,
        type: 'mark',
        title: t('detail.chart.events.payoff'),
        lines: [text.monthYear(trajectory.payoffDay)],
      });
    }
    return out.filter((e) => e.t >= winStart && e.t <= winEnd).sort((a, b) => a.t - b.t);
    // formatters are stable per locale
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [events, series, startDay, principal, row.fixationEnd, trajectory, winStart, winEnd]);

  // ---- hero numbers ----
  const twelveAgo = dueDay(today, -12);
  const sinceStart = startDay > twelveAgo;
  const before = sinceStart ? principal : stepValueAt(trajectory.past, twelveAgo);
  const delta = balance - before;
  const remainingInterest = trajectory.upcoming.reduce((s, r) => s + r.interest, 0);
  const remainingTotal = trajectory.upcoming.reduce((s, r) => s + r.payment, 0);
  const payoff = trajectory.payoffDay;
  const extraPaid = events
    .filter((e) => e.kind === 'extra_payment')
    .reduce((s, e) => s + (Number(e.amount) || 0), 0);
  // Since the drawdown (an event re-anchors the backend's "since anchor" totals to 0)
  const interestPaid = trajectory.interestPaid;
  const principalRepaid = Math.max(0, principal - balance);
  const paidTotal = interestPaid + principalRepaid;
  const remaining = schedule.paymentsRemaining;
  const nextRow = trajectory.upcoming[0];
  // The schedule stops at the contractual end date with a balance left (payment a bit low)
  const endBalance =
    payoff === null && trajectory.end === 'endDate' && loan.endDate !== null
      ? (trajectory.upcoming[trajectory.upcoming.length - 1]?.balanceAfter ?? balance)
      : null;

  const chartNote = (() => {
    switch (horizon) {
      case 'past':
        return t('detail.chart.notePast', {
          date: fmt.day(startDay),
          principal: money(principalRepaid),
          interest: money(interestPaid),
        });
      case '5': {
        const at = stepValueAt(series, winEnd);
        return row.fixationEnd !== null && row.fixationEnd <= winEnd
          ? t('detail.chart.note5Fixation', {
              amount: money(at),
              month: text.monthYear(row.fixationEnd),
            })
          : t('detail.chart.note5', { amount: money(at) });
      }
      default:
        return payoff !== null || endBalance !== null
          ? t('detail.chart.noteAll', {
              total: money(remainingTotal),
              interest: money(remainingInterest),
            })
          : t('detail.chart.noteNoPayoff');
    }
  })();

  const description = (() => {
    const base = {
      principal: money(principal),
      month: text.monthYear(loan.startDate),
      rate: fmt.percent(rate / 100, 2),
      fixation:
        row.fixationEnd !== null
          ? t('detail.withFixation', { month: text.monthYear(row.fixationEnd) })
          : '',
      payment: money(payment),
    };
    return payment > 0 ? t('detail.description', base) : t('detail.descriptionNoPayment', base);
  })();

  return (
    <>
      <PageHead
        eyebrow={[t('detail.eyebrow'), currency]}
        title={loan.name}
        description={description}
        actions={actions}
      />

      <LoanWarnings loan={loan} schedule={schedule} />

      <HeroCard
        aside={
          <Segmented
            value={horizon}
            onValueChange={onHorizonChange}
            options={[
              { value: 'past', label: t('detail.horizon.past') },
              { value: '5', label: t('detail.horizon.5') },
              { value: 'all', label: t('detail.horizon.all') },
            ]}
          />
        }
        chart={
          points.length > 1 ? (
            <>
              <MoonyLineChart<LoanChartEvent>
                className="-mx-1 mt-[18px]"
                points={points}
                height={220}
                zeroBaseline
                projectedFrom={today}
                formatValue={money}
                renderTip={(p) => ({
                  title: money(p.value),
                  lines: [
                    `${text.monthYear(p.t)} · ${
                      p.t > today ? t('detail.chart.tipSchedule') : t('detail.chart.tipBalance')
                    }`,
                  ],
                })}
                events={chartEvents}
                renderEventTip={(cluster: EventCluster<LoanChartEvent>) =>
                  cluster.events.length === 1
                    ? { title: cluster.events[0].title, lines: cluster.events[0].lines }
                    : {
                        title: t('chart.milestones.several', { count: cluster.events.length }),
                        lines: cluster.events.map((e) => e.title),
                      }
                }
              />
              <ChartLegend
                items={[
                  { label: t('detail.chart.legend.balance'), swatch: { kind: 'line' } },
                  {
                    label: t('detail.chart.legend.schedule'),
                    swatch: { kind: 'dash', color: 'var(--chart-line)' },
                  },
                  {
                    label: t('detail.chart.legend.extraPayment'),
                    swatch: { kind: 'event', type: 'income' },
                  },
                  {
                    label: t('detail.chart.legend.milestones'),
                    swatch: { kind: 'event', type: 'mark' },
                  },
                ]}
                note={chartNote}
              />
            </>
          ) : undefined
        }
      >
        <HeroValue
          label={t('detail.hero.outstanding')}
          value={money(balance)}
          tone={delta < 0 ? 'gain' : delta > 0 ? 'loss' : 'neutral'}
          delta={
            delta === 0
              ? money(0)
              : `${delta < 0 ? '↘' : '↗'} ${fmt.money(delta, currency, { signed: true, decimals: 0 })}`
          }
          deltaNote={t(sinceStart ? 'detail.hero.deltaSinceStart' : 'detail.hero.delta12', {
            percent: fmt.percent(repaidShare, 0),
          })}
        />
        <HeroValue
          compact
          label={t('detail.hero.payoff')}
          value={
            balance <= 0
              ? t('detail.hero.paidOff')
              : payoff !== null
                ? text.monthYear(payoff)
                : endBalance !== null
                  ? text.monthYear(loan.endDate!)
                  : '—'
          }
          deltaNote={
            balance <= 0
              ? undefined
              : payoff !== null
                ? t('detail.hero.payoffNote', {
                    duration: text.months(monthsAhead(today, payoff)),
                  })
                : endBalance !== null
                  ? t('detail.hero.balanceAtEnd', {
                      month: text.monthYear(loan.endDate!),
                      amount: money(endBalance),
                    })
                  : row.mode === 'static'
                    ? t('detail.hero.noPayment')
                    : t('detail.hero.noPayoff')
          }
        />
      </HeroCard>

      <Stats className="mt-[17px]">
        <Stat
          label={t('detail.stats.interest')}
          value={money(interestPaid)}
          note={t('detail.stats.interestNote', {
            date: fmt.day(startDay),
            share: fmt.percent(paidTotal > 0 ? interestPaid / paidTotal : 0, 0),
          })}
        />
        <Stat
          label={t('detail.stats.principal')}
          value={money(principalRepaid)}
          note={[
            t('detail.stats.principalNote', { percent: fmt.percent(repaidShare, 0) }),
            extraPaid > 0 ? t('detail.stats.principalExtra', { amount: money(extraPaid) }) : null,
          ]
            .filter(Boolean)
            .join(' · ')}
        />
        <Stat
          label={t('detail.stats.remaining')}
          value={
            remaining !== null
              ? fmt.number(remaining)
              : endBalance !== null
                ? fmt.number(trajectory.upcoming.length)
                : '—'
          }
          note={
            balance <= 0
              ? t('detail.stats.paidOff')
              : remaining !== null
                ? t('detail.stats.remainingNote', {
                    duration: text.months(remaining),
                    interest: money(remainingInterest),
                  })
                : endBalance !== null
                  ? t('detail.stats.remainingToEnd', {
                      month: text.monthYear(loan.endDate!),
                      amount: money(endBalance),
                      interest: money(remainingInterest),
                    })
                  : t('detail.stats.remainingNone')
          }
        />
        {row.fixationEnd !== null ? (
          <Stat
            label={t('detail.stats.fixation')}
            value={text.monthYear(row.fixationEnd)}
            note={t('detail.stats.fixationNote', {
              duration: text.months(monthsAhead(today, row.fixationEnd)),
              up: fmt.percent((rate + 1) / 100, 0),
              upPayment: money(annuityPayment(balance, rate + 1, remaining ?? 360)),
              down: fmt.percent(Math.max(0, rate - 1) / 100, 0),
              downPayment: money(annuityPayment(balance, Math.max(0, rate - 1), remaining ?? 360)),
            })}
          />
        ) : (
          <Stat
            label={t('detail.stats.nextPayment')}
            value={nextRow ? fmt.day(nextRow.dueDay) : '—'}
            note={
              nextRow
                ? t('detail.stats.nextPaymentNote', {
                    interest: money(nextRow.interest),
                    principal: money(nextRow.principalPart),
                  })
                : t('detail.stats.noSchedule')
            }
          />
        )}
      </Stats>

      <div className="mt-[17px] grid grid-cols-[1.4fr_0.6fr] items-start gap-[18px]">
        <LoanScheduleTable schedule={schedule} currency={currency} />
        <aside className="grid gap-[14px]">
          <LoanWhatIfCard
            terms={terms}
            today={today}
            currency={currency}
            balanceToday={balance}
            onRecord={onRecordExtraPayment}
          />
          <LoanEventsCard loan={loan} events={events} onDelete={onDeleteEvent} />
        </aside>
      </div>
    </>
  );
}

function LoanWarnings({ loan, schedule }: { loan: Loan; schedule: LoanSchedule }) {
  const { t } = useTranslation('loans');
  const fmt = useFormat();

  const messages = schedule.warnings.flatMap((code) => {
    switch (code) {
      case 'noPayment':
        return [t('detail.warnings.noPayment')];
      case 'paymentBelowInterest': {
        const monthlyInterest =
          (Number(schedule.anchorAmount) * Number(loan.interestRate || 0)) / 100 / 12;
        return [
          t('detail.warnings.paymentBelowInterest', {
            interest: fmt.money(monthlyInterest, loan.currency),
          }),
        ];
      }
      case 'balanceAtEndDate': {
        const last = schedule.rows[schedule.rows.length - 1];
        return [
          t('detail.warnings.balanceAtEndDate', {
            date: loan.endDate ? fmt.day(loan.endDate) : '',
            amount: fmt.money(Number(last?.balanceAfter ?? 0), loan.currency),
          }),
        ];
      }
      case 'scheduleTruncated':
        return [t('detail.warnings.scheduleTruncated', { count: schedule.rows.length })];
      default:
        return [];
    }
  });

  if (messages.length === 0) return null;
  return (
    <div className="mb-[17px] grid gap-2">
      {messages.map((message) => (
        <Alert key={message}>
          <TriangleAlert aria-hidden />
          <AlertDescription>{message}</AlertDescription>
        </Alert>
      ))}
    </div>
  );
}
