import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

import type { BankAccountWithInstitution, InsertBankAccount, AccountType } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { CurrencyCode, recordCurrency } from '@shared/currencies';
import { BankAccountZoneManager } from '@/components/bank-accounts/BankAccountZoneManager';
import { useTranslation } from 'react-i18next';
import type { Formatters } from '@/lib/format';
import { useFormat } from '@/lib/use-format';
import { todayIsoUtc } from '@/utils/period';

type UpdateBankAccountData = {
  id: string;
  name?: string;
  currency?: string;
  accountType?: AccountType;
  iban?: string;
  bban?: string;
  interestRate?: string;
  hasZoneDesignation?: boolean;
  institutionId?: string | null;
  terminationDate?: number | null;
  interestRateValidUntil?: number | null;
};

const isoDay = (sec: number) => new Date(sec * 1000).toISOString().split('T')[0];
/** 'YYYY-MM-DD' from a date input → UTC-midnight unix seconds (ADR 0008). */
const isoToUtcDaySec = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / 1000);
};

interface ZoneData {
  id?: string;
  fromAmount: string;
  toAmount?: string | null;
  interestRate: string;
}

interface BankAccountFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: InsertBankAccount | UpdateBankAccountData, zones?: ZoneData[]) => void;
  account?: BankAccountWithInstitution | null;
  isLoading?: boolean;
  initialZones?: ZoneData[];
  /** Edit mode: opens the balance-record dialog (the form closes itself first). */
  onRecordBalance?: () => void;
  /** Formats the stored balance; the phone passes its masking formatter, the desktop `fmt.money`. */
  formatMoney?: Formatters['money'];
}

export function BankAccountFormDialog({
  open,
  onOpenChange,
  onSubmit,
  account,
  isLoading = false,
  initialZones,
  onRecordBalance,
  formatMoney,
}: BankAccountFormDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const { currencyCode: userCurrency } = useCurrency();
  const fmt = useFormat();
  const isEditMode = !!account;

  // Form state
  const [name, setName] = useState('');
  // Create only: the optional opening balance and the day it was true. On edit balances are
  // recorded through the balance dialog, never typed into this form.
  const [balance, setBalance] = useState('');
  const [balanceAsOf, setBalanceAsOf] = useState(todayIsoUtc());
  const [selectedCurrency, setSelectedCurrency] = useState<CurrencyCode>(userCurrency);
  const [accountType, setAccountType] = useState<AccountType>('checking');
  const [iban, setIban] = useState('');
  const [bban, setBban] = useState('');
  const [interestRate, setInterestRate] = useState('0');
  const [hasZoneDesignation, setHasZoneDesignation] = useState(false);
  // ISO `yyyy-mm-dd` of the day the promotional rate ends, or empty when unknown.
  const [rateValidUntil, setRateValidUntil] = useState('');
  const [includeInNetWorth, setIncludeInNetWorth] = useState(true);
  const [zones, setZones] = useState<ZoneData[]>([]);
  // Set by the first submit attempt: until then no field shows an error, afterwards the
  // errors follow the inputs.
  const [submitted, setSubmitted] = useState(false);

  // BBAN ↔ IBAN conversion functions for Czech accounts
  // BBAN format: [prefix-]accountNumber/bankCode (e.g., "123456-1234567890/0800" or "1234567890/0800")
  // IBAN format: CZ + 2 check digits + 4 bank code + 6 prefix + 10 account number

  const bbanToIban = (bbanValue: string): string | null => {
    try {
      // Parse BBAN: [prefix-]accountNumber/bankCode
      const match = bbanValue.match(/^(?:(\d{1,6})-)?(\d{1,10})\/(\d{4})$/);
      if (!match) return null;

      const prefix = (match[1] || '0').padStart(6, '0');
      const accountNumber = match[2].padStart(10, '0');
      const bankCode = match[3];

      // Build BBAN part for IBAN: bankCode + prefix + accountNumber
      const bbanForIban = bankCode + prefix + accountNumber;

      // Calculate check digits: Move "CZ00" to end and convert letters
      // CZ = 12 35, then append 00 for calculation
      const numericString = bbanForIban + '123500';
      const checkDigits = 98n - (BigInt(numericString) % 97n);

      // Format IBAN with spaces
      const ibanRaw = 'CZ' + checkDigits.toString().padStart(2, '0') + bbanForIban;
      return ibanRaw.match(/.{1,4}/g)?.join(' ') || ibanRaw;
    } catch {
      return null;
    }
  };

  const ibanToBban = (ibanValue: string): string | null => {
    try {
      // Remove spaces and validate Czech IBAN
      const cleanIban = ibanValue.replace(/\s/g, '').toUpperCase();
      if (!cleanIban.match(/^CZ\d{22}$/)) return null;

      // Extract parts: CZ + 2 check + 4 bank + 6 prefix + 10 account
      const bankCode = cleanIban.substring(4, 8);
      const prefix = cleanIban.substring(8, 14);
      const accountNumber = cleanIban.substring(14, 24);

      // Remove leading zeros
      const cleanPrefix = parseInt(prefix, 10);
      const cleanAccount = parseInt(accountNumber, 10);

      // Format BBAN
      if (cleanPrefix > 0) {
        return `${cleanPrefix}-${cleanAccount}/${bankCode}`;
      }
      return `${cleanAccount}/${bankCode}`;
    } catch {
      return null;
    }
  };

  const handleBbanChange = (value: string) => {
    setBban(value);
    // Auto-convert to IBAN if valid
    const convertedIban = bbanToIban(value);
    if (convertedIban) {
      setIban(convertedIban);
    }
  };

  const handleIbanChange = (value: string) => {
    setIban(value);
    // Auto-convert to BBAN if valid
    const convertedBban = ibanToBban(value);
    if (convertedBban) {
      setBban(convertedBban);
    }
  };

  // (Re)load the form whenever the dialog opens or its source data changes. Done
  // during render instead of in an effect: React re-renders right away, and this
  // avoids the extra render pass (and the set-state-in-effect lint warning).
  const sourceKey = open
    ? `${account?.id ?? 'new'}|${userCurrency}|${JSON.stringify(initialZones ?? [])}`
    : null;
  const [loadedKey, setLoadedKey] = useState<string | null>(null);
  if (sourceKey !== loadedKey) {
    setLoadedKey(sourceKey);
    if (open) {
      setSubmitted(false);
      if (account) {
        // Edit mode
        setName(account.name);
        setBalance('');
        setBalanceAsOf(todayIsoUtc());
        setSelectedCurrency(recordCurrency(account.currency) as CurrencyCode);
        setAccountType(account.accountType as AccountType);
        setIban(account.iban || '');
        setBban(account.bban || '');
        setInterestRate(account.interestRate?.toString() || '0');
        setHasZoneDesignation(account.hasZoneDesignation || false);
        setRateValidUntil(
          account.interestRateValidUntil ? isoDay(account.interestRateValidUntil) : ''
        );
        setIncludeInNetWorth(!account.excludeFromBalance);

        // Use initial zones if provided (for edit mode)
        if (initialZones && initialZones.length > 0) {
          setZones(initialZones);
        } else {
          setZones([]);
        }
      } else {
        // Add mode
        setName('');
        setBalance('');
        setBalanceAsOf(todayIsoUtc());
        setSelectedCurrency(userCurrency);
        setAccountType('checking');
        setIban('');
        setBban('');
        setInterestRate('0');
        setHasZoneDesignation(false);
        setRateValidUntil('');
        setIncludeInNetWorth(true);
        setZones([]);
      }
    }
  }

  // Validation keys live in `common.json` (`validation.*`), like the zod form messages.
  const nameError = !name.trim() ? tc('validation.accountNameRequired') : null;
  const balanceError =
    balance.trim() !== '' && !Number.isFinite(Number(balance))
      ? tc('validation.balanceRequired')
      : balance.trim() !== '' && (!balanceAsOf || balanceAsOf > todayIsoUtc())
        ? tc('validation.balanceAsOfInvalid')
        : null;

  const handleSubmit = () => {
    setSubmitted(true);
    if (nameError || balanceError) {
      document.getElementById(nameError ? 'name' : 'balance')?.focus();
      return;
    }
    if (isEditMode && account) {
      onSubmit(
        {
          id: account.id,
          name: name.trim(),
          currency: selectedCurrency,
          accountType,
          iban: iban || undefined,
          bban: bban || undefined,
          interestRate: hasZoneDesignation ? '0' : interestRate,
          hasZoneDesignation,
          interestRateValidUntil: rateValidUntil ? isoToUtcDaySec(rateValidUntil) : null,
          excludeFromBalance: !includeInNetWorth,
          // The backend update overwrites these columns: send back the stored values, or an
          // edit would silently detach the account from its bank (and its CSV preset).
          institutionId: account.institutionId,
          terminationDate: account.terminationDate,
        },
        hasZoneDesignation ? zones : undefined
      );
    } else {
      onSubmit(
        {
          name: name.trim(),
          balance: balance.trim() === '' ? undefined : balance.trim(),
          balanceAsOf: balance.trim() === '' ? undefined : isoToUtcDaySec(balanceAsOf),
          currency: selectedCurrency,
          accountType,
          iban: iban || undefined,
          bban: bban || undefined,
          interestRate: hasZoneDesignation ? '0' : interestRate,
          hasZoneDesignation,
          interestRateValidUntil: rateValidUntil ? isoToUtcDaySec(rateValidUntil) : null,
          excludeFromBalance: !includeInNetWorth,
        } as InsertBankAccount,
        hasZoneDesignation ? zones : undefined
      );
    }
  };

  const handleClose = () => {
    onOpenChange(false);
  };

  // Edit mode: the stored balance in the account's own currency (it is not editable here).
  const storedBalance = account
    ? (formatMoney ?? fmt.money)(Number(account.balance), recordCurrency(account.currency), {
        decimals: 0,
      })
    : '';

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className="max-w-3xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{isEditMode ? t('editAccount') : t('addAccount')}</DialogTitle>
          <DialogDescription>
            {isEditMode
              ? t('form.editDescription', { name: account?.name })
              : t('form.addDescription')}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="grid gap-2">
            <Label htmlFor="name">{t('fields.name')} *</Label>
            <Input
              id="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('form.namePlaceholder')}
              maxLength={100}
              aria-invalid={submitted && nameError ? true : undefined}
              aria-describedby={submitted && nameError ? 'name-error' : undefined}
            />
            {submitted && nameError && (
              <p
                id="name-error"
                role="alert"
                className="flex items-center gap-[5px] text-micro font-600 text-loss"
              >
                {nameError}
              </p>
            )}
          </div>

          <div className="grid gap-2">
            <Label htmlFor="accountType">{t('fields.accountType')}</Label>
            <Select value={accountType} onValueChange={(v) => setAccountType(v as AccountType)}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="checking">{t('accountTypes.checking')}</SelectItem>
                <SelectItem value="savings">{t('accountTypes.savings')}</SelectItem>
                <SelectItem value="credit_card">{t('accountTypes.credit_card')}</SelectItem>
                <SelectItem value="investment">{t('accountTypes.investment')}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="grid grid-cols-2 items-start gap-4">
            {isEditMode && account ? (
              <div className="grid gap-2">
                <Label>{t('fields.balance')}</Label>
                <p className="text-table text-ink-2">
                  {Number(account.balance) === 0 && !account.balanceAsOf
                    ? t('form.noBalance')
                    : account.balanceAsOf
                      ? t('form.currentBalance', {
                          amount: storedBalance,
                          date: fmt.day(account.balanceAsOf),
                        })
                      : t('form.currentBalanceUnknownDate', { amount: storedBalance })}
                </p>
                {onRecordBalance && (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="justify-self-start"
                    onClick={() => {
                      onOpenChange(false);
                      onRecordBalance();
                    }}
                  >
                    {t('form.recordNew')}
                  </Button>
                )}
              </div>
            ) : (
              <div className="grid gap-2">
                <Label htmlFor="balance">{t('form.openingBalance')}</Label>
                <div className="grid grid-cols-[1fr_150px] gap-2">
                  <Input
                    id="balance"
                    type="number"
                    step="0.01"
                    value={balance}
                    onChange={(e) => setBalance(e.target.value)}
                    placeholder="0"
                    aria-invalid={submitted && balanceError ? true : undefined}
                    aria-describedby={submitted && balanceError ? 'balance-error' : undefined}
                  />
                  <Input
                    type="date"
                    value={balanceAsOf}
                    max={todayIsoUtc()}
                    onChange={(e) => setBalanceAsOf(e.target.value)}
                    aria-label={t('form.openingBalanceAsOf')}
                  />
                </div>
                {submitted && balanceError ? (
                  <p
                    id="balance-error"
                    role="alert"
                    className="flex items-center gap-[5px] text-micro font-600 text-loss"
                  >
                    {balanceError}
                  </p>
                ) : (
                  <p className="text-micro font-500 text-ink-4">{t('form.openingBalanceHint')}</p>
                )}
              </div>
            )}
            <div className="grid gap-2">
              <Label htmlFor="currency">{tc('labels.currency')}</Label>
              <CurrencyCombobox value={selectedCurrency} onChange={setSelectedCurrency} />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="grid gap-2">
              <Label htmlFor="bban">{t('fields.accountNumberOptional')}</Label>
              <Input
                id="bban"
                value={bban}
                onChange={(e) => handleBbanChange(e.target.value)}
                placeholder={t('form.accountNumberPlaceholder')}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="iban">IBAN</Label>
              <Input
                id="iban"
                value={iban}
                onChange={(e) => handleIbanChange(e.target.value)}
                placeholder={t('form.ibanPlaceholder')}
              />
            </div>
          </div>

          <div className="flex items-start space-x-3 p-3 rounded-r2 border">
            <Checkbox
              id="hasZoneDesignation"
              checked={hasZoneDesignation}
              onCheckedChange={(checked) => setHasZoneDesignation(checked as boolean)}
              className="mt-1"
            />
            <div className="space-y-1">
              <Label
                htmlFor="hasZoneDesignation"
                className="cursor-pointer text-table font-500 text-ink-2"
              >
                {t('form.useZones')}
              </Label>
              <p className="text-micro font-500 text-ink-4">{t('form.zonesHelp')}</p>
            </div>
          </div>

          <div className="flex items-start space-x-3 p-3 rounded-r2 border">
            <Checkbox
              id="includeInNetWorth"
              checked={includeInNetWorth}
              onCheckedChange={(checked) => setIncludeInNetWorth(checked !== false)}
              className="mt-1"
            />
            <div className="space-y-1">
              <Label
                htmlFor="includeInNetWorth"
                className="cursor-pointer text-table font-500 text-ink-2"
              >
                {t('form.includeInNetWorth')}
              </Label>
              <p className="text-micro font-500 text-ink-4">{t('form.includeInNetWorthHelp')}</p>
            </div>
          </div>

          {!hasZoneDesignation && (
            <div className="grid gap-2">
              <Label htmlFor="interestRate">{t('fields.interestRate')}</Label>
              <InputWrap unit="%">
                <Input
                  id="interestRate"
                  type="number"
                  step="0.01"
                  value={interestRate}
                  onChange={(e) => setInterestRate(e.target.value)}
                  placeholder="0.00"
                />
              </InputWrap>
              <p className="text-micro font-500 text-ink-4">{t('form.apyHelp')}</p>
            </div>
          )}

          {hasZoneDesignation && (
            <div className="space-y-2">
              <Label>{t('form.zonesLabel')}</Label>
              <p className="mb-3 text-micro font-500 text-ink-4">{t('form.zonesDescription')}</p>
              <BankAccountZoneManager
                zones={zones}
                onChange={setZones}
                currency={selectedCurrency}
              />
            </div>
          )}

          <div className="grid gap-2">
            <Label htmlFor="rateValidUntil">{t('fields.rateValidUntil')}</Label>
            <Input
              id="rateValidUntil"
              type="date"
              value={rateValidUntil}
              onChange={(e) => setRateValidUntil(e.target.value)}
            />
            <p className="text-micro font-500 text-ink-4">{t('form.rateValidUntilHelp')}</p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={handleClose}>
            {tc('buttons.cancel')}
          </Button>
          <Button onClick={handleSubmit} disabled={isLoading || (isEditMode && !account)}>
            {isLoading
              ? tc('status.saving')
              : isEditMode
                ? tc('buttons.saveChanges')
                : tc('buttons.add')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
