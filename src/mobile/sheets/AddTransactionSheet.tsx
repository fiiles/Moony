import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useBankTransactionMutations } from '@/hooks/use-bank-account-mutations';
import { useCategoryName } from '@/hooks/use-categories';
import { AmountField } from '@/mobile/components/AmountField';
import { DateField } from '@/mobile/components/DateField';
import { TEXT_FIELD } from '@/mobile/components/field-styles';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { Segment } from '@/mobile/components/Segment';
import { Sheet } from '@/mobile/components/Sheet';
import { formChanged } from '@/mobile/lib/form-dirty';
import {
  emptyManualTxForm,
  manualTxErrors,
  manualTxInput,
  type ManualTxForm,
} from '@/mobile/lib/manual-transaction';
import { todayIso } from '@/mobile/lib/property-detail';
import { CategoryChoiceSheet } from '@/mobile/sheets/CategoryChoiceSheet';

interface AddTransactionSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accountId: string;
  /** The account's currency: a payment in another one is entered on the computer. */
  currency: string;
  categories: readonly TransactionCategory[];
}

/**
 * "Přidat transakci" (spec §8.6; the phone counterpart of the desktop's `AddTransactionModal`):
 * Odchozí · Příchozí, the amount in the account's currency (focused once the sheet is open), the
 * day (today to start, no limit), the counterparty, the note, the category and, last, the
 * counterparty's account number. The category is a row that opens the stacked choice sheet; a
 * manual entry runs no rules, so its first row reads "Bez kategorie" and the entry stays
 * uncategorized until one is chosen. A missing or wrong amount and a missing day are named only
 * after the first save attempt. The write is the desktop's (`createTransaction`, which refreshes
 * what rule 7 lists and toasts); the sheet closes once it is saved, a refusal is the mutation's own
 * toast and the sheet stays.
 *
 * Every opening starts over: an outgoing payment for today with empty fields.
 */
export function AddTransactionSheet({
  open,
  onOpenChange,
  accountId,
  currency,
  categories,
}: AddTransactionSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  const formId = useId();
  const amountField = useRef<HTMLInputElement>(null);
  const counterpartyField = useRef<HTMLInputElement>(null);
  const descriptionField = useRef<HTMLInputElement>(null);
  const ibanField = useRef<HTMLInputElement>(null);
  // Counts the openings: a save that started before a close must not close the next opening
  const opening = useRef(0);
  const { createTransaction } = useBankTransactionMutations();

  const [form, setForm] = useState(() => emptyManualTxForm(todayIso()));
  // What the form opened with: a sheet that differs from it asks before it closes
  const [formStart, setFormStart] = useState(form);
  const [attempted, setAttempted] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      const fresh = emptyManualTxForm(todayIso());
      setForm(fresh);
      setFormStart(fresh);
      setAttempted(false);
      setChoosing(false);
    }
  }
  const change = (patch: Partial<ManualTxForm>) => setForm((f) => ({ ...f, ...patch }));

  // The sheet itself takes the focus on opening (the keyboard would cover it unread); the amount
  // is the field to type in, so it gets the focus once the sheet has settled
  useEffect(() => {
    if (!open) return;
    opening.current += 1;
    const timer = window.setTimeout(() => amountField.current?.focus(), 60);
    return () => window.clearTimeout(timer);
  }, [open]);

  const errors = manualTxErrors(form);
  const pending = createTransaction.isPending;
  const amountError = !attempted
    ? undefined
    : errors.amount === 'required'
      ? tc('validation.amountRequired')
      : errors.amount === 'invalid'
        ? tc('validation.invalidAmount')
        : errors.amount === 'notPositive'
          ? tc('validation.amountPositive')
          : undefined;
  const dateError =
    attempted && errors.date === 'required' ? tc('validation.dateRequired') : undefined;

  const submit = () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (pending) return;
    setAttempted(true);
    const input = manualTxInput(form, accountId, currency);
    if (!input) return;
    const mine = opening.current;
    createTransaction.mutate(input, {
      onSuccess: () => {
        if (opening.current === mine) onOpenChange(false);
      },
    });
  };

  // Return goes on to the next field; only the last one (the IBAN) submits the form. The Enter that
  // ends an IME composition (ja, zh) is the IME's: WebKit reports it with `isComposing` already
  // false and the legacy keyCode 229
  const nextOnReturn =
    (next: { current: HTMLInputElement | null }) => (event: KeyboardEvent<HTMLInputElement>) => {
      if (
        event.key !== 'Enter' ||
        event.nativeEvent.isComposing ||
        event.nativeEvent.keyCode === 229
      ) {
        return;
      }
      event.preventDefault();
      next.current?.focus();
    };

  const category = categories.find((c) => c.id === form.categoryId) ?? null;
  const noCategory = t('account.ledger.noCategory');
  const textField = (
    id: string,
    label: string,
    value: string,
    onChange: (value: string) => void,
    field: {
      ref: { current: HTMLInputElement | null };
      next?: { current: HTMLInputElement | null };
      placeholder?: string;
      iban?: boolean;
    }
  ) => (
    <div className="mb-3">
      <label htmlFor={id} className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
        {label}
      </label>
      <input
        id={id}
        type="text"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        ref={field.ref}
        placeholder={field.placeholder}
        autoComplete="off"
        autoCapitalize={field.iban ? 'characters' : undefined}
        autoCorrect={field.iban ? 'off' : undefined}
        spellCheck={field.iban ? false : undefined}
        enterKeyHint={field.next ? 'next' : 'done'}
        onKeyDown={field.next ? nextOnReturn(field.next) : undefined}
        className={TEXT_FIELD}
      />
    </div>
  );

  return (
    <>
      <Sheet
        open={open}
        onOpenChange={onOpenChange}
        dirty={!pending && formChanged(formStart, form)}
        title={tb('transaction.add')}
        fill
        full
        footer={
          <>
            <Button variant="ghost" className={FOOTER_GHOST} onClick={() => onOpenChange(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              type="submit"
              form={`${formId}-form`}
              className={FOOTER_PRIMARY}
              disabled={pending}
              loading={pending}
            >
              {tb('transaction.add')}
            </Button>
          </>
        }
      >
        <form
          id={`${formId}-form`}
          noValidate
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <Segment<ManualTxForm['type']>
            value={form.type}
            options={[
              { value: 'debit', label: tb('transaction.debit') },
              { value: 'credit', label: tb('transaction.credit') },
            ]}
            onChange={(type) => change({ type })}
            label={tc('labels.type')}
            className="mb-3.5"
          />
          <AmountField
            ref={amountField}
            label={tb('transaction.amount')}
            unit={currency}
            value={form.amountText}
            placeholder="0"
            error={amountError}
            onChange={(event) => change({ amountText: event.target.value })}
            enterKeyHint="next"
            onKeyDown={nextOnReturn(counterpartyField)}
          />
          <div className="mb-3">
            <DateField
              label={tb('transaction.date')}
              value={form.dateIso}
              onChange={(dateIso) => change({ dateIso })}
              error={dateError}
            />
          </div>
          {textField(
            `${formId}-counterparty`,
            tb('transaction.counterparty'),
            form.counterparty,
            (counterparty) => change({ counterparty }),
            {
              ref: counterpartyField,
              next: descriptionField,
              placeholder: tb('transaction.counterpartyPlaceholder'),
            }
          )}
          {textField(
            `${formId}-description`,
            tb('transaction.description'),
            form.description,
            (description) => change({ description }),
            {
              ref: descriptionField,
              // The category is a button: Return goes straight on to the account number
              next: ibanField,
              placeholder: tb('transaction.descriptionPlaceholder'),
            }
          )}
          <div className="mb-3">
            <span className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
              {tb('transaction.category')}
            </span>
            <button
              type="button"
              aria-label={tb('transaction.category')}
              onClick={() => setChoosing(true)}
              className={cn(TEXT_FIELD, 'flex items-center justify-between text-left')}
            >
              <span className={category ? 'truncate text-ink' : 'truncate text-ink-5'}>
                {category ? categoryName(category) : noCategory}
              </span>
              <ChevronRight
                aria-hidden
                className="ml-2 size-4 flex-none text-ink-5"
                strokeWidth={1.75}
              />
            </button>
          </div>
          {textField(
            `${formId}-iban`,
            tb('transaction.counterpartyIban'),
            form.counterpartyIban,
            (counterpartyIban) => change({ counterpartyIban }),
            { ref: ibanField, iban: true }
          )}
        </form>
      </Sheet>
      <CategoryChoiceSheet
        open={open && choosing}
        onOpenChange={setChoosing}
        categories={categories}
        value={form.categoryId}
        onChoose={(categoryId) => change({ categoryId })}
        noneLabel={noCategory}
        noneSub={t('account.addTx.noCategorySub')}
      />
    </>
  );
}
