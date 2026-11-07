import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { StockDividend } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useStockDividendMutations } from '@/hooks/use-stock-dividend-mutations';
import { AmountField } from '@/mobile/components/AmountField';
import { DateField } from '@/mobile/components/DateField';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { Sheet } from '@/mobile/components/Sheet';
import {
  dividendErrors,
  dividendForm,
  dividendInput,
  type DividendForm,
} from '@/mobile/lib/dividend-form';
import { todayIso } from '@/mobile/lib/property-detail';

interface DividendSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  investmentId: string;
  ticker: string;
  /** The currency a new dividend is entered in; an edited one keeps its own. */
  currency: string;
  /** The manual dividend to edit; `null` enters a new one. */
  dividend: StockDividend | null;
  /** Yahoo Finance has dividends for the ticker: nothing can be entered by hand. */
  fromYahoo: boolean;
}

/**
 * "Zapsat dividendu" / "Upravit dividendu" (spec A3; the phone counterpart of the desktop's
 * `StockDividendModal`): the ex-date (today to start), the gross before withholding tax in the
 * dividend's currency (fixed, shown inside the field) and a note of 500 characters at most. A
 * ticker Yahoo has dividends for says so and keeps the fields and the save disabled (desktop D16).
 * A missing or wrong amount, a missing day and a long note are named only after the first save
 * attempt. The write is the desktop's (`createDividend` / `updateDividend`, which refresh what
 * rule 7 lists and toast); the sheet closes once it is saved, a refusal is the mutation's own toast.
 *
 * Every opening starts over: with the stored values of the dividend, or empty for a new one.
 */
export function DividendSheet({
  open,
  onOpenChange,
  investmentId,
  ticker,
  currency,
  dividend,
  fromYahoo,
}: DividendSheetProps) {
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const formId = useId();
  const amountField = useRef<HTMLInputElement>(null);
  const noteField = useRef<HTMLTextAreaElement>(null);
  // Counts the openings: a save that started before a close must not close the next opening
  const opening = useRef(0);
  const { createDividend, updateDividend } = useStockDividendMutations();

  const locale = useTranslation().i18n.language;
  const [form, setForm] = useState(() => dividendForm(dividend, todayIso(), locale));
  const [attempted, setAttempted] = useState(false);
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setForm(dividendForm(dividend, todayIso(), locale));
      setAttempted(false);
    }
  }
  const change = (patch: Partial<DividendForm>) => setForm((f) => ({ ...f, ...patch }));

  // The sheet itself takes the focus on opening (the keyboard would cover it unread); the amount
  // is the field to type in, so it gets the focus once the sheet has settled
  useEffect(() => {
    if (open) opening.current += 1;
  }, [open]);
  useEffect(() => {
    if (!open || fromYahoo) return;
    const timer = window.setTimeout(() => amountField.current?.focus(), 60);
    return () => window.clearTimeout(timer);
  }, [open, fromYahoo]);

  const unit = dividend?.currency ?? currency;
  const errors = dividendErrors(form);
  const pending = createDividend.isPending || updateDividend.isPending;
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
  const noteError =
    attempted && errors.note === 'tooLong' ? tc('validation.dividendNoteTooLong') : undefined;

  const submit = () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (pending || fromYahoo) return;
    setAttempted(true);
    const data = dividendInput(form, unit);
    if (!data) return;
    const mine = opening.current;
    const close = {
      onSuccess: () => {
        if (opening.current === mine) onOpenChange(false);
      },
    };
    if (dividend) updateDividend.mutate({ id: dividend.id, data }, close);
    else createDividend.mutate({ investmentId, data }, close);
  };

  const label = dividend ? ts('detail.dividendRecords.save') : ts('detail.dividendRecords.add');
  const noteId = `${formId}-note`;
  const noteErrorId = `${noteId}-error`;

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={
        dividend ? ts('detail.dividendRecords.editTitle') : ts('detail.dividendRecords.addTitle')
      }
      description={ts('detail.dividendRecords.description', { ticker })}
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
            disabled={pending || fromYahoo}
            loading={pending}
          >
            {label}
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
        {fromYahoo && (
          <InlineAlert icon={Info} title={ts('detail.dividendRecords.yahooTitle')}>
            {ts('detail.dividendRecords.yahooBody', { ticker })}
          </InlineAlert>
        )}
        <fieldset disabled={fromYahoo} className="m-0 min-w-0 border-0 p-0 disabled:opacity-60">
          <div className="mb-3">
            <DateField
              label={ts('detail.dividendRecords.exDate')}
              value={form.dateIso}
              onChange={(dateIso) => change({ dateIso })}
              error={dateError}
            />
            {!dateError && (
              <p className="mx-0.5 mt-1.5 text-m-caption text-ink-3">
                {ts('detail.dividendRecords.exDateHint')}
              </p>
            )}
          </div>
          <AmountField
            ref={amountField}
            label={ts('detail.dividendRecords.gross')}
            unit={unit}
            value={form.amountText}
            placeholder="0"
            hint={ts('detail.dividendRecords.grossHint')}
            error={amountError}
            onChange={(event) => change({ amountText: event.target.value })}
            enterKeyHint="next"
            onKeyDown={(event) => {
              // The Enter that ends an IME composition (ja, zh) is the IME's (isComposing / 229)
              if (
                event.key !== 'Enter' ||
                event.nativeEvent.isComposing ||
                event.nativeEvent.keyCode === 229
              ) {
                return;
              }
              // Return goes on to the note; the save is the footer button's
              event.preventDefault();
              noteField.current?.focus();
            }}
          />
          <div className="mb-3">
            <label
              htmlFor={noteId}
              className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2"
            >
              {ts('detail.dividendRecords.note')}
            </label>
            <textarea
              id={noteId}
              ref={noteField}
              rows={3}
              value={form.note}
              onChange={(event) => change({ note: event.target.value })}
              aria-invalid={noteError ? true : undefined}
              aria-describedby={noteError ? noteErrorId : undefined}
              className="block min-h-24 w-full resize-none rounded-[10px] border border-line-strong bg-paper px-3.5 py-3 text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring"
            />
            {noteError && (
              <p id={noteErrorId} className="mx-0.5 mt-1.5 text-m-caption text-loss">
                {noteError}
              </p>
            )}
          </div>
        </fieldset>
      </form>
    </Sheet>
  );
}
