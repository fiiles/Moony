import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import type { Loan, LoanEvent } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

interface LoanEventsCardProps {
  loan: Loan;
  events: LoanEvent[];
  onDelete: (event: LoanEvent) => void;
}

/**
 * Events list (prototype loan-detail.html): extra payments, rate changes and
 * balance checks newest first, the drawdown at the bottom. Removing an entry
 * keeps the balance it set — the confirm dialog says so.
 */
export function LoanEventsCard({ loan, events, onDelete }: LoanEventsCardProps) {
  const { t } = useTranslation('loans');
  const fmt = useFormat();
  const money = (v: string | number | null) =>
    fmt.money(Number(v) || 0, loan.currency, { decimals: 0 });

  const describe = (event: LoanEvent): { title: string; note: string } => {
    switch (event.kind) {
      case 'extra_payment':
        return {
          title: t('detail.events.extraPayment', { amount: money(event.amount) }),
          note:
            event.note ??
            (event.monthlyPayment
              ? t('detail.events.newPayment', { amount: money(event.monthlyPayment) })
              : t('detail.events.extraPaymentNote')),
        };
      case 'rate_change':
        return {
          title: t('detail.events.rateChange', {
            rate: fmt.percent((Number(event.rate) || 0) / 100, 2),
          }),
          note:
            event.note ??
            (event.monthlyPayment
              ? t('detail.events.newPayment', { amount: money(event.monthlyPayment) })
              : t('detail.events.rateChangeNote')),
        };
      default:
        return {
          title: t('detail.events.balanceCheck'),
          note: `${money(event.amount)} · ${event.note ?? t('detail.events.balanceCheckNote')}`,
        };
    }
  };

  const sorted = [...events].sort((a, b) => b.eventDay - a.eventDay || b.createdAt - a.createdAt);

  return (
    <Card variant="flat">
      <CardHeader className="pb-1">
        <CardTitle className="text-[16px]">{t('detail.events.title')}</CardTitle>
      </CardHeader>
      <CardContent className="pt-0">
        <ul className="m-0 list-none p-0">
          {sorted.map((event) => {
            const { title, note } = describe(event);
            return (
              <li
                key={event.id}
                className="grid grid-cols-[64px_1fr_auto] gap-3 border-b border-line-soft py-[9px] text-table text-ink-2"
              >
                <small className="pt-0.5 text-micro font-600 text-ink-4">
                  {fmt.day(event.eventDay)}
                </small>
                <div className="min-w-0">
                  <b className="block font-600 text-ink">{title}</b>
                  <span className="mt-px block text-micro text-ink-4">{note}</span>
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="-mr-1.5 text-ink-4"
                  aria-label={t('detail.events.delete')}
                  title={t('detail.events.delete')}
                  onClick={() => onDelete(event)}
                >
                  <X />
                </Button>
              </li>
            );
          })}
          <li className="grid grid-cols-[64px_1fr] gap-3 py-[9px] text-table text-ink-2">
            <small className="pt-0.5 text-micro font-600 text-ink-4">
              {fmt.day(loan.startDate)}
            </small>
            <div>
              <b className="block font-600 text-ink">
                {t('detail.events.drawdown', { amount: money(loan.principal) })}
              </b>
              <span className="mt-px block text-micro text-ink-4">
                {t('detail.events.drawdownNote')}
              </span>
            </div>
          </li>
        </ul>
      </CardContent>
    </Card>
  );
}
