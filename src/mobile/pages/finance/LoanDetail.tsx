import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import type { LoanEvent, LoanEventKind, LoanSchedule as LoanScheduleData } from '@shared/schema';
import { useLoanEvents } from '@/hooks/use-loan-events';
import { useLoanMutations } from '@/hooks/use-loan-mutations';
import { useLoans, useLoanSchedule } from '@/hooks/use-loans';
import { useLoanText } from '@/hooks/use-loan-text';
import { useFormat } from '@/lib/use-format';
import type { ExtraPaymentMode } from '@/utils/loan-trajectory';
import type { LoanRow } from '@/utils/loans';
import { Button } from '@/components/ui/button';
import { LoanEventDialog } from '@/components/loans/LoanEventDialog';
import { LoanAlerts } from '@/mobile/components/loan/LoanAlerts';
import { LoanEvents } from '@/mobile/components/loan/LoanEvents';
import { LoanHeroCard } from '@/mobile/components/loan/LoanHeroCard';
import { LoanMenu } from '@/mobile/components/loan/LoanMenu';
import { LoanSchedule } from '@/mobile/components/loan/LoanSchedule';
import { LoanStats } from '@/mobile/components/loan/LoanStats';
import { useBack, useBackLabel } from '@/mobile/hooks/use-back';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { useRetained } from '@/mobile/hooks/use-retained';
import { loanDetailStats, type LoanHorizon } from '@/mobile/lib/loan-detail';
import { loanPath, routes } from '@/mobile/shell/routes';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { Screen } from '@/mobile/shell/Screen';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { ExtraPaymentSheet } from '@/mobile/sheets/ExtraPaymentSheet';

/** The event dialog the screen has open: its kind, and for an extra payment the what-if's values. */
interface EventDialogState {
  kind: LoanEventKind;
  initial?: { amount?: number; mode?: ExtraPaymentMode };
}

/** What the screen shows of a loan: the row, its schedule and its events. */
interface LoanView {
  row: LoanRow;
  schedule: LoanScheduleData;
  events: LoanEvent[];
}

/** A stable empty list: a new one per render would keep `LoanView` from ever settling. */
const NO_EVENTS: LoanEvent[] = [];

/** Route entry: another loan remounts the page, so its horizon, sheets and dialogs start over. */
export default function LoanDetail() {
  const [, params] = useRoute(routes.loanDetail);
  return <LoanDetailPage key={params?.id} id={params?.id} />;
}

/**
 * The loan detail (spec §8.8 "Úvěr", prototype frame 13): the name in the nav bar with `···`, the
 * "CZK · 3,1 % · fixace do 3/2027" line, the balance hero (the next payment in its note, the line
 * with the plan dashed after today), the warnings under it, four stats, Mimořádná splátka, the
 * schedule and the events. The numbers are the desktop detail's (`useLoans`, the schedule and the
 * events queries) in the loan's own currency, a hand-entered balance holding exactly as it does
 * there; the dialogs for the loan's form and for the events are the desktop's.
 *
 * The page owns the delete (its confirm sheet and the mutation, `useDeleteAndLeave`): the loan
 * drops out of `useLoans` while the delete is still finishing, which takes the `···` menu away, and
 * the page keeps showing the loan until it has left.
 */
function LoanDetailPage({ id }: { id: string | undefined }) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const text = useLoanText();
  const back = useBack(routes.finance);
  // The screen the push left when it came from another tab (a property's loan), else the parent
  const backText = useBackLabel(t('tabs.finance'));
  const { rows, isLoading: loansLoading, today } = useLoans();
  const { data: schedule, isLoading: scheduleLoading } = useLoanSchedule(id);
  const { data: events = NO_EVENTS } = useLoanEvents(id);
  const { deleteMutation } = useLoanMutations();

  const [horizon, setHorizon] = useState<LoanHorizon>('5');
  const [extraOpen, setExtraOpen] = useState(false);
  const [eventDialog, setEventDialog] = useState<EventDialogState | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);

  const row = rows.find((r) => r.loan.id === id);
  const current = useMemo<LoanView | null>(
    () => (row && schedule ? { row, schedule, events } : null),
    [row, schedule, events]
  );
  const kept = useRetained(current);
  const exit = useDeleteAndLeave({
    home: routes.finance,
    pathOf: loanPath,
    keysOf: (loanId) => [
      ['loan-schedule', loanId],
      ['loan-events', loanId],
    ],
    remove: (loanId) => deleteMutation.mutateAsync(loanId),
    closeSheet: () => setDeleteOpen(false),
  });
  // A loan being deleted stays on screen (as it was) until the screen has left
  const view = exit.deleting ? kept : current;
  const loan = view?.row.loan;
  const loading = loansLoading || (row !== undefined && scheduleLoading);

  return (
    <Screen
      title={loan?.name ?? ''}
      leading={<NavTextButton label={backText} onClick={back} chevron />}
      trailing={
        loan && (
          <LoanMenu
            loan={loan}
            onOpenEvent={(kind) => setEventDialog({ kind })}
            onDelete={() => setDeleteOpen(true)}
          />
        )
      }
    >
      {view ? (
        <LoanBody
          view={view}
          today={today}
          horizon={horizon}
          onHorizonChange={setHorizon}
          caption={[
            view.row.currency,
            fmt.percent((Number(view.row.loan.interestRate) || 0) / 100, 2),
            // As the Finance list and the desktop's loans table word it: the fixation still ahead,
            // else the month an ended one ran out, else a fixed rate
            view.row.fixationEnd !== null
              ? tl('table.fixedUntil', { month: text.monthYear(view.row.fixationEnd) })
              : view.row.loan.interestRateValidityDate !== null
                ? tl('table.fixationEnded', {
                    month: text.monthYear(view.row.loan.interestRateValidityDate),
                  })
                : tl('table.fixedRate'),
          ].join(' · ')}
          onExtraPayment={() => setExtraOpen(true)}
          onRecordRate={() => setEventDialog({ kind: 'rate_change' })}
        />
      ) : loading ? (
        <LoanSkeleton />
      ) : (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{tl('detail.notFound')}</p>
      )}

      {view && (
        <>
          <ExtraPaymentSheet
            open={extraOpen}
            onOpenChange={setExtraOpen}
            row={view.row}
            today={today}
            onRecord={(amount, mode) =>
              setEventDialog({ kind: 'extra_payment', initial: { amount, mode } })
            }
          />
          {eventDialog && (
            <LoanEventDialog
              open
              onOpenChange={(next) => {
                if (!next) setEventDialog(null);
              }}
              loan={view.row.loan}
              ledger={view.row.ledger}
              plan={view.row.plan}
              today={today}
              kind={eventDialog.kind}
              initial={eventDialog.initial}
            />
          )}
          <ConfirmSheet
            open={deleteOpen}
            onOpenChange={setDeleteOpen}
            title={tl('confirmDelete.title')}
            description={tl('confirmDelete.description', { name: view.row.loan.name })}
            confirmLabel={tl('actions.delete')}
            isPending={deleteMutation.isPending}
            onConfirm={() => exit.run(view.row.loan.id)}
          />
        </>
      )}
    </Screen>
  );
}

interface LoanBodyProps {
  view: LoanView;
  today: number;
  horizon: LoanHorizon;
  onHorizonChange: (horizon: LoanHorizon) => void;
  caption: string;
  onExtraPayment: () => void;
  onRecordRate: () => void;
}

/** The loaded screen, top to bottom. */
function LoanBody({
  view,
  today,
  horizon,
  onHorizonChange,
  caption,
  onExtraPayment,
  onRecordRate,
}: LoanBodyProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const { row, schedule, events } = view;
  const stats = loanDetailStats(row, events, schedule, today);

  // Mimořádná splátka needs a loan that has started, a payment to shorten or lower, and a balance
  // to pay down
  const noExtra = !stats.started || row.mode === 'static' || row.outstanding <= 0;
  const noExtraReason = !stats.started
    ? t('loan.notStarted', { date: fmt.day(row.loan.startDate) })
    : row.outstanding <= 0
      ? tl('detail.stats.paidOff')
      : tl('detail.hero.noPayment');

  return (
    <>
      {/* The nav bar keeps its one title, so the currency, the rate and the fixation are the first line */}
      <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
        {caption}
      </p>
      <LoanHeroCard
        row={row}
        events={events}
        stats={stats}
        today={today}
        horizon={horizon}
        onHorizonChange={onHorizonChange}
      />
      <LoanAlerts row={row} schedule={schedule} today={today} onRecordRate={onRecordRate} />
      <LoanStats row={row} stats={stats} today={today} />
      <Button
        className="h-auto min-h-12 w-full whitespace-normal py-2 text-center rounded-[10px] text-m-row"
        disabled={noExtra}
        onClick={onExtraPayment}
      >
        {tl('actions.extraPayment')}
      </Button>
      {noExtra && (
        <p className="mx-0.5 mb-0 mt-1.5 text-center text-m-caption text-ink-3">{noExtraReason}</p>
      )}
      <LoanSchedule row={row} schedule={schedule} />
      <LoanEvents loan={row.loan} events={events} />
    </>
  );
}

/** While the loan loads: the hero, the stats and a few rows in outline. */
function LoanSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-32" />
        <div className="skeleton mt-3 h-9 w-56" />
        <div className="skeleton mt-3 h-3 w-48" />
        <div className="skeleton mt-4 h-[160px] w-full" />
      </div>
      <div className="mb-3.5 grid grid-cols-2 gap-2.5">
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className="rounded-r3 border border-line bg-paper px-3 py-2.5 shadow-e1">
            <div className="skeleton h-2.5 w-3/5" />
            <div className="skeleton mt-2.5 h-4 w-4/5" />
          </div>
        ))}
      </div>
      <div className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1">
        {[0, 1, 2].map((i) => (
          <div key={i} className="flex min-h-[60px] items-center gap-3 px-3.5 py-2.5">
            <div className="min-w-0 flex-1">
              <div className="skeleton h-3.5 w-2/5" />
              <div className="skeleton mt-2 h-3 w-3/5" />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
