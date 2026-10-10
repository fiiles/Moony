import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { BankBalanceRecord } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useBalanceRecordMutations } from '@/hooks/use-balance-records';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { isoDateFromUtcTimestamp, todayIsoUtc, utcDayStart } from '@/utils/period';

interface BalanceRecordDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  account: { id: string; currency: string; balance: string; balanceAsOf: number | null };
  records: readonly BankBalanceRecord[];
}

/** "Zaznamenat zůstatek": one dated balance; the same day replaces the earlier record. */
export function BalanceRecordDialog({
  open,
  onOpenChange,
  account,
  records,
}: BalanceRecordDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { recordBalance } = useBalanceRecordMutations(account.id);
  const [amount, setAmount] = useState('');
  const [asOf, setAsOf] = useState(todayIsoUtc());
  const [confirmUnknown, setConfirmUnknown] = useState(false);
  const [submitted, setSubmitted] = useState(false);

  // Reset the form each time the dialog opens (render-time state, no effect)
  const [openedFor, setOpenedFor] = useState<boolean>(false);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setAmount('');
      setAsOf(todayIsoUtc());
      setConfirmUnknown(false);
      setSubmitted(false);
    }
  }

  const currency = recordCurrency(account.currency);
  const replaces = records.find((r) => isoDateFromUtcTimestamp(r.asOf) === asOf) ?? null;
  const needsConfirm =
    records.length === 0 && account.balanceAsOf == null && Number(account.balance) !== 0;
  const amountError =
    amount.trim() === '' || !Number.isFinite(Number(amount))
      ? tc('validation.balanceRequired')
      : null;

  const submit = () => {
    setSubmitted(true);
    if (amountError || !asOf) return;
    recordBalance.mutate(
      {
        amount: amount.trim(),
        currency,
        asOf: utcDayStart(asOf),
        confirmUnknownAsOf: needsConfirm ? confirmUnknown : undefined,
      },
      { onSuccess: () => onOpenChange(false) }
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('recordDialog.title')}</DialogTitle>
          <DialogDescription>{t('recordDialog.description')}</DialogDescription>
        </DialogHeader>
        <div className="grid grid-cols-[1fr_172px] gap-4">
          <div className="grid gap-2">
            <Label htmlFor="balance-record-amount">{t('recordDialog.amount')}</Label>
            <InputWrap unit={currency}>
              <Input
                id="balance-record-amount"
                type="number"
                step="0.01"
                inputMode="decimal"
                value={amount}
                placeholder={t('recordDialog.amountPlaceholder')}
                onChange={(e) => setAmount(e.target.value)}
                aria-invalid={submitted && amountError ? true : undefined}
                autoFocus
              />
            </InputWrap>
            {submitted && amountError ? (
              <p role="alert" className="text-micro font-600 text-loss">
                {amountError}
              </p>
            ) : (
              <p className="text-micro font-500 text-ink-4">{t('recordDialog.hint')}</p>
            )}
          </div>
          <div className="grid gap-2">
            <Label htmlFor="balance-record-asof">{t('recordDialog.asOf')}</Label>
            <Input
              id="balance-record-asof"
              type="date"
              value={asOf}
              max={todayIsoUtc()}
              onChange={(e) => setAsOf(e.target.value)}
            />
          </div>
        </div>
        {replaces && (
          <p className="flex items-center gap-[5px] text-micro font-500 text-ink-3">
            <Info className="size-3.5" aria-hidden />
            {t('recordDialog.replaces', { date: fmt.day(replaces.asOf) })}
          </p>
        )}
        {needsConfirm && (
          <label className="flex items-center gap-2 text-table text-ink-2">
            <Checkbox
              checked={confirmUnknown}
              onCheckedChange={(v) => setConfirmUnknown(v === true)}
            />
            {t('recordDialog.confirmUnknown')}
          </label>
        )}
        {submitted && recordBalance.isError && (
          <p role="alert" className="text-micro font-600 text-loss">
            {translateApiError(recordBalance.error, tc)}
          </p>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            onClick={submit}
            disabled={recordBalance.isPending}
            loading={recordBalance.isPending}
          >
            {t('recordDialog.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
