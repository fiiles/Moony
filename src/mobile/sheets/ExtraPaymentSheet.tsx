import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLoanText } from '@/hooks/use-loan-text';
import {
  extraPaymentWhatIf,
  suggestedExtraPayment,
  type ExtraPaymentMode,
} from '@/utils/loan-trajectory';
import type { LoanRow } from '@/utils/loans';
import { Button } from '@/components/ui/button';
import { AmountField } from '@/mobile/components/AmountField';
import { Segment } from '@/mobile/components/Segment';
import { Sheet } from '@/mobile/components/Sheet';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { useHideAmounts } from '@/mobile/hooks/use-hide-amounts';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { parseAmountInput } from '@/mobile/lib/amount-input';
import { extraPaymentText } from '@/mobile/lib/loan-detail';

/** Whole amounts: the result shows no decimals. */
const WHOLE = { decimals: 0 } as const;

/**
 * The amount an opening starts from: a round 5 % of the balance, or nothing while the amounts are
 * hidden — the suggestion is a fraction of the balance, which would show what the eye hides.
 */
function startAmount(balance: number, hidden: boolean): string {
  return hidden ? '' : String(suggestedExtraPayment(balance));
}

interface ExtraPaymentSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  row: LoanRow;
  today: number;
  /** "Zapsat jako provedenou": the sheet has closed, open the event dialog with these values. */
  onRecord: (amount: number, mode: ExtraPaymentMode) => void;
}

/**
 * "Mimořádná splátka" (spec §8.8, the phone counterpart of the desktop's `LoanWhatIfCard`): the
 * amount (a round 5 % of the balance to start from), Zkrátit dobu · Snížit splátku and the live
 * result in one sentence — the months saved and the interest saved, or the drop of the payment —
 * worked out from the plan ahead with `extraPaymentWhatIf`. Zapsat jako provedenou closes the sheet
 * and then opens the event dialog with the amount and the mode, where the day and the note are
 * chosen. A loan without a payment says so instead of a result. Every amount goes through the eye;
 * the amount typed is an input and is never masked.
 *
 * Every opening starts over from the suggested amount and "Zkrátit dobu". With the amounts hidden the
 * field starts empty instead: the suggestion is a fraction of the balance (as Přecenit's field starts
 * empty rather than with the price).
 */
export function ExtraPaymentSheet({
  open,
  onOpenChange,
  row,
  today,
  onRecord,
}: ExtraPaymentSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const text = useLoanText();
  const { hidden } = useHideAmounts();
  const balance = row.trajectory.balanceToday;
  const planPayment = row.plan.monthlyPayment;
  const money = (value: number) => moneyText(value, row.currency, WHOLE);

  const [amountText, setAmountText] = useState(() => startAmount(balance, hidden));
  const [mode, setMode] = useState<ExtraPaymentMode>('term');
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setAmountText(startAmount(balance, hidden));
      setMode('term');
    }
  }

  const amount = Number(parseAmountInput(amountText) ?? 0) || 0;
  const whatIf =
    amount > 0 && amount <= balance
      ? extraPaymentWhatIf(row.plan, balance, today, amount, mode)
      : null;
  const result = extraPaymentText(whatIf, planPayment);
  const payoffDay = whatIf?.payoffDay ?? null;
  const payoff = payoffDay !== null ? text.monthYear(payoffDay) : '—';

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={tl('detail.whatIf.title')}
      description={tl('detail.whatIf.subtitle')}
      footer={
        <>
          <Button variant="ghost" className={FOOTER_GHOST} onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            className={FOOTER_PRIMARY}
            disabled={whatIf === null}
            onClick={() =>
              leaveAfterSheet(
                () => onOpenChange(false),
                () => onRecord(amount, mode)
              )
            }
          >
            {tl('detail.whatIf.record')}
          </Button>
        </>
      }
    >
      <AmountField
        label={tl('detail.whatIf.amount')}
        unit={row.currency}
        value={amountText}
        placeholder="0"
        hint={tl('detail.whatIf.hint')}
        onChange={(event) => setAmountText(event.target.value)}
      />
      <Segment
        value={mode}
        options={[
          { value: 'term', label: tl('detail.whatIf.modes.term') },
          { value: 'payment', label: tl('detail.whatIf.modes.payment') },
        ]}
        onChange={setMode}
        label={tl('detail.whatIf.title')}
        className="mb-3 mt-1"
      />
      <div aria-live="polite" className="rounded-r3 bg-well px-4 py-3.5">
        {whatIf && result && result.kind !== 'noEffect' ? (
          <>
            <b className="block text-m-title text-ink num">
              {result.kind === 'term'
                ? tl('detail.whatIf.termResult', { duration: text.months(result.months) })
                : tl('detail.whatIf.paymentResult', { amount: money(result.drop) })}
            </b>
            <span className="mt-1 block text-m-sub text-ink-3">
              {result.kind === 'term'
                ? tl('detail.whatIf.termNote', {
                    interest: money(result.interestSaved),
                    payment: money(planPayment),
                    month: payoff,
                  })
                : tl('detail.whatIf.paymentNote', {
                    payment: money(whatIf.payment),
                    month: payoff,
                    interest: money(result.interestSaved),
                  })}
            </span>
          </>
        ) : (
          <span className="block text-m-sub text-ink-3">
            {planPayment <= 0
              ? tl('detail.whatIf.noPayment')
              : result?.kind === 'noEffect'
                ? // A valid amount that changes neither the term nor the payment nor the interest
                  t('loan.extra.noChange')
                : tl('detail.whatIf.noEffect', { amount: money(balance) })}
          </span>
        )}
      </div>
    </Sheet>
  );
}
