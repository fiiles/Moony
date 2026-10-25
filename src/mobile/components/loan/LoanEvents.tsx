import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Trash2 } from 'lucide-react';
import type { Loan, LoanEvent } from '@shared/schema';
import { useLoanEventMutations } from '@/hooks/use-loan-events';
import { useFormat } from '@/lib/use-format';
import { DateChip, DATE_CHIP_WIDTH } from '@/mobile/components/DateChip';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { RowMenu, RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';

/** Whole amounts: the events show no decimals. */
const WHOLE = { decimals: 0 } as const;

interface LoanEventsProps {
  loan: Loan;
  events: readonly LoanEvent[];
}

/**
 * "Události" of a loan (spec §8.8, prototype frame 13): extra payments, rate changes and manual
 * balances newest first, the drawdown last, each a row with a date chip (the day and its year), the
 * desktop's title and note. A long press on an event opens its menu with Odstranit (the drawdown is
 * not an event and has none); the confirm sheet says what the removal does — the balance is worked
 * out again without it, and a removed manual balance brings back the previous one or the estimate.
 */
export function LoanEvents({ loan, events }: LoanEventsProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { remove } = useLoanEventMutations(loan.id);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [pending, setPending] = useState<LoanEvent | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const money = (value: string | number | null) =>
    moneyText(Number(value) || 0, loan.currency, WHOLE);
  const year = (day: number) => String(new Date(day * 1000).getUTCFullYear());

  const describe = (event: LoanEvent): { title: string; note: string } => {
    switch (event.kind) {
      case 'extra_payment':
        return {
          title: tl('detail.events.extraPayment', { amount: money(event.amount) }),
          note:
            event.note ??
            (event.monthlyPayment
              ? tl('detail.events.newPayment', { amount: money(event.monthlyPayment) })
              : tl('detail.events.extraPaymentNote')),
        };
      case 'rate_change':
        return {
          title: tl('detail.events.rateChange', {
            rate: fmt.percent((Number(event.rate) || 0) / 100, 2),
          }),
          note:
            event.note ??
            (event.monthlyPayment
              ? tl('detail.events.newPayment', { amount: money(event.monthlyPayment) })
              : tl('detail.events.rateChangeNote')),
        };
      default:
        return {
          title: tl('detail.events.balanceCheck', { amount: money(event.amount) }),
          note: event.note ?? tl('detail.events.balanceCheckNote'),
        };
    }
  };

  const sorted = [...events].sort((a, b) => b.eventDay - a.eventDay || b.createdAt - a.createdAt);

  return (
    <>
      <SectionHead title={tl('detail.events.title')} note={String(sorted.length + 1)} />
      <EntityRows>
        {sorted.map((event) => {
          const { title, note } = describe(event);
          const open = () => setMenuFor(event.id);
          return (
            <EntityRow
              key={event.id}
              chip={<DateChip day={event.eventDay} caption={year(event.eventDay)} />}
              chipWidth={DATE_CHIP_WIDTH}
              title={title}
              sub={note}
              onLongPress={open}
              accessory={
                <RowMenu
                  open={menuFor === event.id}
                  onOpenChange={(next) => setMenuFor(next ? event.id : null)}
                  label={t('loan.events.menu')}
                >
                  <RowMenuItem
                    variant="danger"
                    onSelect={() => {
                      setPending(event);
                      setConfirmOpen(true);
                    }}
                  >
                    <Trash2 strokeWidth={1.75} aria-hidden />
                    {tl('detail.events.delete')}
                  </RowMenuItem>
                </RowMenu>
              }
            />
          );
        })}
        <EntityRow
          chip={<DateChip day={loan.startDate} caption={year(loan.startDate)} />}
          chipWidth={DATE_CHIP_WIDTH}
          title={tl('detail.events.drawdown', { amount: money(loan.principal) })}
          sub={tl('detail.events.drawdownNote')}
        />
      </EntityRows>
      <ConfirmSheet
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={tl('detail.events.confirmDelete.title')}
        description={
          pending?.kind === 'balance_check'
            ? tl('detail.events.confirmDelete.descriptionManual')
            : tl('detail.events.confirmDelete.description')
        }
        confirmLabel={tl('detail.events.delete')}
        isPending={remove.isPending}
        onConfirm={() => {
          if (!pending) return;
          remove.mutate(pending.id, { onSuccess: () => setConfirmOpen(false) });
        }}
      />
    </>
  );
}
