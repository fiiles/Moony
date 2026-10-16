import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { currencyDecimals, recordCurrency } from '@shared/currencies';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useRecordBalances, type AccountBalanceRecord } from '@/hooks/use-balance-records';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { Button } from '@/components/ui/button';
import { AmountField } from '@/mobile/components/AmountField';
import { Sheet } from '@/mobile/components/Sheet';
import { signedAmountInput } from '@/mobile/lib/amount-input';

interface RecordBalancesSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The accounts to list, in order; the card freezes them when it opens the sheet. */
  accountIds: readonly string[];
}

/**
 * "Zaznamenat zůstatky" (spec §8.3 item 1, §10.3): one amount field per stale account, the last
 * balance as its placeholder, dated today. Saving records every filled field (the empty ones are
 * skipped) through the desktop's balance write, one after the other, then says it once.
 */
export function RecordBalancesSheet({ open, onOpenChange, accountIds }: RecordBalancesSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { accounts: all } = useBankAccounts();
  const record = useRecordBalances();
  const [values, setValues] = useState<Record<string, string>>({});
  // The ± toggles: the iOS decimal pad has no minus key
  const [negatives, setNegatives] = useState<Record<string, boolean>>({});

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
  const canSave = filled.length > 0 && !parsed.some(invalid);

  const save = () => {
    if (!canSave) return;
    const asOf = utcDayFloor(Date.now() / 1000);
    const records: AccountBalanceRecord[] = filled.map(({ account, amount }) => ({
      accountId: account.id,
      data: {
        amount: amount as string,
        currency: recordCurrency(account.currency),
        asOf,
        // The first dated balance of an account that only has an undated one replaces it (the
        // desktop dialog asks with a checkbox); typing today's balance is that consent. A balance
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
      title={t('recordBalances.title')}
      description={t('recordBalances.lead')}
      footer={
        <>
          <Button
            variant="ghost"
            className="h-12 rounded-[10px] px-[18px] text-m-row"
            onClick={() => onOpenChange(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="record-balances"
            className="h-12 flex-1 rounded-[10px] text-m-row"
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
          const decimals = currencyDecimals(currency);
          return (
            <AmountField
              key={p.account.id}
              label={p.account.name}
              unit={currency}
              value={p.text}
              placeholder={fmt.number(Number(p.account.balance), {
                minimumFractionDigits: decimals,
                maximumFractionDigits: decimals,
              })}
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
      </form>
    </Sheet>
  );
}
