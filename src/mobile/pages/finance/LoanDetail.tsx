import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { useLocation, useRoute } from 'wouter';
import type { LoanEvent, LoanEventKind, LoanSchedule as LoanScheduleData } from '@shared/schema';
import { useLoanEvents } from '@/hooks/use-loan-events';
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
import { useBack } from '@/mobile/hooks/use-back';
import { loanDetailStats, type LoanHorizon } from '@/mobile/lib/loan-detail';
import { loanPath, routes } from '@/mobile/shell/routes';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { Screen } from '@/mobile/shell/Screen';
import { ExtraPaymentSheet } from '@/mobile/sheets/ExtraPaymentSheet';

/** How long after leaving a deleted loan the screen may still be it (a restored deep link). */
const DELETED_GUARD_MS = 250;

/** The event dialog the screen has open: its kind, and for an extra payment the what-if's values. */
interface EventDialogState {
  kind: LoanEventKind;
  initial?: { amount?: number; mode?: ExtraPaymentMode };
}

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
 */
function LoanDetailPage({ id }: { id: string | undefined }) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const text = useLoanText();
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const back = useBack(routes.finance);
  const { rows, isLoading: loansLoading, today } = useLoans();
  const { data: schedule, isLoading: scheduleLoading } = useLoanSchedule(id);
  const { data: events = [] } = useLoanEvents(id);

  const [horizon, setHorizon] = useState<LoanHorizon>('5');
  const [extraOpen, setExtraOpen] = useState(false);
  const [eventDialog, setEventDialog] = useState<EventDialogState | null>(null);

  const row = rows.find((r) => r.loan.id === id);
  const loan = row?.loan;

  // The loan is gone: back to Finance as Back does, so the history keeps no ghost of it. Its own
  // queries are dropped once the screen has left (history.back lands asynchronously): removed
  // while it is still up, a re-render would rebuild them empty and flash the skeleton. On a
  // restored deep link the detail is the first entry and back has nowhere to go; the screen is
  // then replaced.
  const leaveDeleted = (loanId: string) => {
    back();
    window.setTimeout(() => {
      if (window.location.pathname === loanPath(loanId)) {
        setLocation(routes.finance, { replace: true });
      }
      queryClient.removeQueries({ queryKey: ['loan-schedule', loanId] });
      queryClient.removeQueries({ queryKey: ['loan-events', loanId] });
    }, DELETED_GUARD_MS);
  };

  const loading = loansLoading || (loan !== undefined && scheduleLoading);
  const ready = row !== undefined && loan !== undefined && schedule !== undefined;

  return (
    <Screen
      title={loan?.name ?? ''}
      leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />}
      trailing={
        loan && (
          <LoanMenu
            loan={loan}
            onOpenEvent={(kind) => setEventDialog({ kind })}
            onDeleted={() => leaveDeleted(loan.id)}
          />
        )
      }
    >
      {loading ? (
        <LoanSkeleton />
      ) : !ready ? (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{tl('detail.notFound')}</p>
      ) : (
        <LoanBody
          row={row}
          events={events}
          schedule={schedule}
          today={today}
          horizon={horizon}
          onHorizonChange={setHorizon}
          caption={[
            row.currency,
            fmt.percent((Number(loan.interestRate) || 0) / 100, 2),
            row.fixationEnd !== null
              ? tl('table.fixedUntil', { month: text.monthYear(row.fixationEnd) })
              : null,
          ]
            .filter(Boolean)
            .join(' · ')}
          onExtraPayment={() => setExtraOpen(true)}
          onRecordRate={() => setEventDialog({ kind: 'rate_change' })}
        />
      )}

      {ready && (
        <>
          <ExtraPaymentSheet
            open={extraOpen}
            onOpenChange={setExtraOpen}
            row={row}
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
              loan={row.loan}
              ledger={row.ledger}
              plan={row.plan}
              today={today}
              kind={eventDialog.kind}
              initial={eventDialog.initial}
            />
          )}
        </>
      )}
    </Screen>
  );
}

interface LoanBodyProps {
  row: LoanRow;
  events: LoanEvent[];
  schedule: LoanScheduleData;
  today: number;
  horizon: LoanHorizon;
  onHorizonChange: (horizon: LoanHorizon) => void;
  caption: string;
  onExtraPayment: () => void;
  onRecordRate: () => void;
}

/** The loaded screen, top to bottom. */
function LoanBody({
  row,
  events,
  schedule,
  today,
  horizon,
  onHorizonChange,
  caption,
  onExtraPayment,
  onRecordRate,
}: LoanBodyProps) {
  const { t: tl } = useTranslation('loans');
  const stats = loanDetailStats(row, events, schedule);

  // Mimořádná splátka needs a payment to shorten or lower, and a balance to pay down
  const noExtra = row.mode === 'static' || row.outstanding <= 0;
  const noExtraReason =
    row.outstanding <= 0 ? tl('detail.stats.paidOff') : tl('detail.hero.noPayment');

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
        className="h-12 w-full whitespace-normal rounded-[10px] text-m-row"
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
