import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { currencyDecimals, recordCurrency } from '@shared/currencies';
import type { BankAccount } from '@shared/schema';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useRecordBalances, type AccountBalanceRecord } from '@/hooks/use-balance-records';
import { useFormat } from '@/lib/use-format';
import { todayIsoUtc } from '@/utils/period';
import { Button } from '@/components/ui/button';
import { AmountField } from '@/mobile/components/AmountField';
import { DateField } from '@/mobile/components/DateField';
import { Sheet } from '@/mobile/components/Sheet';
import { useHideAmounts } from '@/mobile/hooks/use-hide-amounts';
import { signedAmountInput } from '@/mobile/lib/amount-input';
import { isoToUtcDay } from '@/mobile/lib/property-detail';
import { maskMoney } from '@/mobile/lib/hide-amounts';

interface RecordBalancesSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The accounts to list, in order; the card freezes them when it opens the sheet. */
  accountIds: readonly string[];
  /** The account detail names the sheet by its button ("Zaznamenat zůstatek"); default `recordBalances.title`. */
  title?: string;
  /** Default `recordBalances.lead`. */
  description?: string;
}

/**
 * "Zaznamenat zůstatky" (spec §8.3 item 1, §10.3): one amount field per stale account, the last
 * balance and its date as its placeholder, and the day the new balances are as of (today unless
 * changed, never a day to come). Saving records every filled field (the empty ones are skipped)
 * through the desktop's balance write, one after the other, then says it once; a balance already
 * recorded on that day is replaced by the new one (the backend's rule).
 */
export function RecordBalancesSheet({
  open,
  onOpenChange,
  accountIds,
  title,
  description,
}: RecordBalancesSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { t: tb } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const { hidden } = useHideAmounts();
  // The last balance and its date are a hint, not an input: the amount is masked with the other
  // amounts, the date stays. The desktop's wording ("Zůstatek … k 2. 9. 2026", or "datum
  // neznámé" for a balance without a date).
  const lastBalance = (account: Pick<BankAccount, 'balance' | 'balanceAsOf' | 'currency'>) => {
    const decimals = currencyDecimals(recordCurrency(account.currency));
    const formatted = fmt.number(Number(account.balance), {
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    });
    const amount = hidden ? maskMoney(formatted) : formatted;
    return account.balanceAsOf === null
      ? tb('form.currentBalanceUnknownDate', { amount })
      : tb('form.currentBalance', { amount, date: fmt.day(account.balanceAsOf) });
  };
  const { accounts: all } = useBankAccounts();
  const record = useRecordBalances();
  const [values, setValues] = useState<Record<string, string>>({});
  // The ± toggles: the iOS decimal pad has no minus key
  const [negatives, setNegatives] = useState<Record<string, boolean>>({});
  const today = todayIsoUtc();
  const [dateIso, setDateIso] = useState(today);

  // Every opening starts with empty fields and without the last attempt's error
  const { reset: resetRecord } = record;
  useEffect(() => {
    if (open) resetRecord();
  }, [open, resetRecord]);
  const [openedFor, setOpenedFor] = useState(false);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setValues({});
      setNegatives({});
      setDateIso(todayIsoUtc());
    }
  }

  const accounts = accountIds.flatMap((id) => all.find((a) => a.id === id) ?? []);
  const parsed = accounts.map((account) => {
    const text = values[account.id] ?? '';
    const negative = negatives[account.id] ?? false;
    return { account, text, negative, amount: signedAmountInput(text, negative) };
  });
  // A filled field that is not a number blocks saving; an empty one is skipped
  const invalid = (p: (typeof parsed)[number]) => p.text.trim() !== '' && p.amount === null;
  const filled = parsed.filter((p) => p.amount !== null);
  // An emptied picker or a day to come (some webviews let `max` through) blocks saving
  const dateError =
    dateIso === ''
      ? tc('validation.required')
      : dateIso > today
        ? t('recordBalances.dateInFuture')
        : undefined;
  const canSave = filled.length > 0 && !parsed.some(invalid) && dateError === undefined;

  const save = () => {
    if (!canSave) return;
    const asOf = isoToUtcDay(dateIso);
    const records: AccountBalanceRecord[] = filled.map(({ account, amount }) => ({
      accountId: account.id,
      data: {
        amount: amount as string,
        currency: recordCurrency(account.currency),
        asOf,
        // The first dated balance of an account that only has an undated one replaces it (the
        // desktop dialog asks with a checkbox); typing a dated balance is that consent. A balance
        // with no date is exactly an account without any dated record.
        confirmUnknownAsOf:
          account.balanceAsOf === null && Number(account.balance) !== 0 ? true : undefined,
      },
    }));
    record.mutate(records, {
      onSuccess: () => {
        toast(t('recordBalances.saved'));
        onOpenChange(false);
      },
    });
  };

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={title ?? t('recordBalances.title')}
      description={description ?? t('recordBalances.lead')}
      footer={
        <>
          <Button
            variant="ghost"
            className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row"
            onClick={() => onOpenChange(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="record-balances"
            className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
            disabled={!canSave}
            loading={record.isPending}
          >
            {t('recordBalances.save')}
          </Button>
        </>
      }
    >
      <form
        id="record-balances"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          save();
        }}
      >
        {parsed.map((p) => {
          const currency = recordCurrency(p.account.currency);
          return (
            <AmountField
              key={p.account.id}
              label={p.account.name}
              unit={currency}
              value={p.text}
              placeholder={lastBalance(p.account)}
              error={invalid(p) ? tc('validation.invalidAmount') : undefined}
              negative={p.negative}
              negativeLabel={t('recordBalances.negative')}
              onNegativeChange={(negative) =>
                setNegatives((current) => ({ ...current, [p.account.id]: negative }))
              }
              onChange={(event) =>
                setValues((current) => ({ ...current, [p.account.id]: event.target.value }))
              }
            />
          );
        })}
        <DateField
          label={t('recordBalances.date')}
          value={dateIso}
          max={today}
          onChange={setDateIso}
          error={dateError}
        />
      </form>
    </Sheet>
  );
}
