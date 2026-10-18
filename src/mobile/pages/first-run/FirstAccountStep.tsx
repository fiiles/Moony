import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { currencyName, type CurrencyCode } from '@shared/currencies';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useFormat } from '@/lib/use-format';
import { AmountField } from '@/mobile/components/AmountField';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { signedAmountInput } from '@/mobile/lib/amount-input';
import { CurrencyPickerSheet } from '@/mobile/sheets/CurrencyPickerSheet';
import { StepHead } from './StepHead';
import { field48, fieldLabel, primaryButton, rowCard, textLink } from './styles';

/**
 * První účet: a name and today's balance, in the main currency unless the person picks another.
 * The balance is typed on the decimal keypad with the ± toggle for an overdrawn account; an empty
 * field means no balance yet. Errors show under their field once "Vytvořit účet" has been tried.
 * "Zatím přeskočit" goes on without an account. The statement import is not part of the phone's
 * first run (it comes with the import in Phase 2b).
 */
export function FirstAccountStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();
  const [name, setName] = useState('');
  const [balance, setBalance] = useState('');
  const [negative, setNegative] = useState(false);
  const [currency, setCurrency] = useState<CurrencyCode>(flow.currency);
  const [currencyOpen, setCurrencyOpen] = useState(false);
  const [attempted, setAttempted] = useState(false);
  const pending = flow.pending.account;

  // '' is no balance, `null` is text that is not a number
  const amount = balance.trim() === '' ? '' : signedAmountInput(balance, negative);
  const nameMissing = name.trim() === '';
  const showNameError = attempted && nameMissing;
  const showAmountError = attempted && amount === null;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (pending) return;
    setAttempted(true);
    if (nameMissing || amount === null) return;
    void flow.createFirstAccount({ name, balance: amount, currency });
  };

  return (
    <div data-testid="first-run-first-account">
      <StepHead title={t('onboarding.firstAccount.title')} lead={tm('firstRun.firstAccountLead')} />
      <form noValidate onSubmit={submit} aria-busy={pending}>
        <div className="mb-3">
          <label htmlFor="first-account-name" className={fieldLabel}>
            {t('onboarding.firstAccount.name')}
          </label>
          <Input
            id="first-account-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('onboarding.firstAccount.namePlaceholder')}
            autoComplete="off"
            autoCapitalize="sentences"
            enterKeyHint="next"
            aria-invalid={showNameError || undefined}
            aria-describedby={showNameError ? 'first-account-name-error' : undefined}
            readOnly={pending}
            className={field48}
          />
          {showNameError && (
            <p
              id="first-account-name-error"
              role="alert"
              className="mx-0.5 mb-0 mt-1.5 text-m-sub font-semibold text-loss"
            >
              {tc('validation.nameRequired')}
            </p>
          )}
        </div>

        <AmountField
          label={t('onboarding.firstAccount.balance')}
          unit={currency}
          value={balance}
          onChange={(event) => setBalance(event.target.value)}
          placeholder="0"
          error={showAmountError ? tc('validation.invalidAmount') : undefined}
          negative={negative}
          negativeLabel={tm('recordBalances.negative')}
          onNegativeChange={setNegative}
          enterKeyHint="done"
          readOnly={pending}
        />

        <div className={`${rowCard} mt-3`}>
          <KeyValueRow
            label={t('onboarding.firstAccount.currency')}
            value={`${currency} · ${currencyName(currency, fmt.locale)}`}
            onClick={() => setCurrencyOpen(true)}
          />
        </div>

        <Button type="submit" className={primaryButton} disabled={pending} loading={pending}>
          {t('onboarding.firstAccount.create')}
        </Button>
      </form>
      <button type="button" className={textLink} onClick={flow.skipFirstAccount} disabled={pending}>
        {t('onboarding.firstAccount.skip')}
      </button>

      <CurrencyPickerSheet
        open={currencyOpen}
        onOpenChange={setCurrencyOpen}
        value={currency}
        onPick={setCurrency}
        title={t('onboarding.firstAccount.currency')}
      />
    </div>
  );
}
