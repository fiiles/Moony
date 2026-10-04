import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LoanTerms } from '@shared/calculations/loan-amortization';
import { extraPaymentWhatIf, type ExtraPaymentMode } from '@/utils/loan-trajectory';
import { useFormat } from '@/lib/use-format';
import { useLoanText } from '@/hooks/use-loan-text';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Segmented } from '@/components/ui/segmented';

interface LoanWhatIfCardProps {
  terms: LoanTerms;
  today: number;
  currency: string;
  balanceToday: number;
  /** "Zapsat jako provedenou": opens the extra-payment dialog with these values. */
  onRecord: (amount: number, mode: ExtraPaymentMode) => void;
}

/** A round starting amount: 5 % of the balance to the nearest thousand, at least 1 000. */
function suggestedAmount(balance: number): number {
  return Math.max(1000, Math.round((balance * 0.05) / 1000) * 1000);
}

/**
 * Extra-payment what-if (prototype loan-detail.html): the amount, the two
 * things a bank offers (shorter term or lower payment) and the result in one
 * sentence — months saved and interest saved, or the new payment.
 */
export function LoanWhatIfCard({
  terms,
  today,
  currency,
  balanceToday,
  onRecord,
}: LoanWhatIfCardProps) {
  const { t } = useTranslation('loans');
  const fmt = useFormat();
  const text = useLoanText();
  const [amountText, setAmountText] = useState(() => String(suggestedAmount(balanceToday)));
  const [mode, setMode] = useState<ExtraPaymentMode>('term');

  const amount = Number(amountText.replace(/\s/g, '').replace(',', '.')) || 0;
  const result = amount > 0 ? extraPaymentWhatIf(terms, today, amount, mode) : null;
  const money = (v: number) => fmt.money(v, currency, { decimals: 0 });

  return (
    <Card>
      <CardHeader>
        <div>
          <CardTitle className="text-[16px]">{t('detail.whatIf.title')}</CardTitle>
          <CardDescription>{t('detail.whatIf.subtitle')}</CardDescription>
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <div className="space-y-1.5">
          <Label htmlFor="loan-what-if-amount">{t('detail.whatIf.amount')}</Label>
          <InputWrap unit={currency}>
            <Input
              id="loan-what-if-amount"
              type="number"
              min="0"
              step="1000"
              value={amountText}
              onChange={(e) => setAmountText(e.target.value)}
            />
          </InputWrap>
          <p className="text-micro font-500 text-ink-4">{t('detail.whatIf.hint')}</p>
        </div>
        <Segmented
          className="mt-3"
          fullWidth
          value={mode}
          onValueChange={setMode}
          options={[
            { value: 'term', label: t('detail.whatIf.modes.term') },
            { value: 'payment', label: t('detail.whatIf.modes.payment') },
          ]}
        />
        <div className="mt-[14px] rounded-r3 bg-well px-4 py-[14px]">
          {result && result.balanceAfter >= 0 && amount <= balanceToday ? (
            <>
              <b className="block text-[20px] font-700 tracking-[-0.04em] text-ink num">
                {mode === 'term'
                  ? t('detail.whatIf.termResult', {
                      duration: text.months(result.monthsBefore - result.monthsAfter),
                    })
                  : t('detail.whatIf.paymentResult', {
                      amount: money(terms.monthlyPayment - result.payment),
                    })}
              </b>
              <span className="mt-1 block text-caption text-ink-3">
                {mode === 'term'
                  ? t('detail.whatIf.termNote', {
                      interest: money(result.interestBefore - result.interestAfter),
                      payment: money(terms.monthlyPayment),
                      month: result.payoffDay !== null ? text.monthYear(result.payoffDay) : '—',
                    })
                  : t('detail.whatIf.paymentNote', {
                      payment: money(result.payment),
                      month: result.payoffDay !== null ? text.monthYear(result.payoffDay) : '—',
                      interest: money(result.interestBefore - result.interestAfter),
                    })}
              </span>
            </>
          ) : (
            <span className="block text-caption text-ink-3">
              {terms.monthlyPayment <= 0
                ? t('detail.whatIf.noPayment')
                : t('detail.whatIf.noEffect', { amount: money(balanceToday) })}
            </span>
          )}
        </div>
        <Button
          variant="outline"
          size="sm"
          className="mt-3"
          disabled={!result || amount <= 0 || amount > balanceToday}
          onClick={() => onRecord(amount, mode)}
        >
          {t('detail.whatIf.record')}
        </Button>
      </CardContent>
    </Card>
  );
}
