import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RealEstate } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { useValuationMutations } from '@/hooks/use-valuation-mutations';
import { Button } from '@/components/ui/button';
import { AmountField } from '@/mobile/components/AmountField';
import { DateField } from '@/mobile/components/DateField';
import { Sheet } from '@/mobile/components/Sheet';
import { useHideAmounts } from '@/mobile/hooks/use-hide-amounts';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import {
  revalueData,
  todayIso,
  type RevalueErrors,
  type RevalueForm,
} from '@/mobile/lib/property-detail';

interface RevalueSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  property: RealEstate;
}

/**
 * The form of a new estimate: today's market price in the property's currency, today, no note.
 * With the amounts hidden the field starts empty: a prefilled price would show what the eye hides.
 */
function startForm(property: RealEstate, hidden: boolean): RevalueForm {
  return {
    valueText: hidden ? '' : String(Number(property.marketPrice) || 0),
    dateIso: todayIso(),
    note: '',
  };
}

/**
 * "Přecenit" (spec §8.8, the phone counterpart of the desktop's `RevalueDialog`): a new dated
 * estimate of the market value, in the property's own currency — the amount (today's estimate to
 * start from), the day (today at most) and an optional note. The older estimates stay on the
 * trace. Přecenit writes it through the valuation mutations (the domain keys, the metrics, the
 * cashflow and a fresh snapshot) and closes the sheet once it is saved; a refusal is the mutation's
 * own toast and the sheet stays. The estimate it replaces in the hint goes through the eye, and
 * with the amounts hidden the field starts empty instead of with that estimate; the amount typed is
 * an input and is never masked.
 *
 * Every opening starts over from today's estimate and today.
 */
export function RevalueSheet({ open, onOpenChange, property }: RevalueSheetProps) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const { hidden } = useHideAmounts();
  const formId = useId();
  const { add } = useValuationMutations('realEstate', property.id);
  const currency = recordCurrency(property.marketPriceCurrency);

  const [form, setForm] = useState(() => startForm(property, hidden));
  // An empty amount is only called missing once the field has been left
  const [amountTouched, setAmountTouched] = useState(false);
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setForm(startForm(property, hidden));
      setAmountTouched(false);
    }
  }

  const today = todayIso();
  const result = revalueData(form, { assetId: property.id, currency, today });
  const errors: RevalueErrors = result.ok ? {} : result.errors;
  const amountError =
    errors.value === undefined || (errors.value === 'required' && !amountTouched)
      ? undefined
      : errors.value === 'required'
        ? tc('validation.amountRequired')
        : tc('validation.invalidAmount');
  const dateError =
    errors.date === 'required'
      ? tc('validation.dateRequired')
      : errors.date === 'future'
        ? tc('validation.valuationDateInFuture')
        : undefined;

  const submit = () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (!result.ok || add.isPending) return;
    add.mutate(result.data, { onSuccess: () => onOpenChange(false) });
  };

  const noteId = `${formId}-note`;
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={tc('revalue.title', { name: property.name })}
      description={tc('revalue.description')}
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
            form={`${formId}-form`}
            className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
            disabled={!result.ok}
            loading={add.isPending}
          >
            {t('detail.revalue')}
          </Button>
        </>
      }
    >
      <form
        id={`${formId}-form`}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <AmountField
          label={tc('revalue.marketValue')}
          unit={currency}
          value={form.valueText}
          placeholder="0"
          error={amountError}
          hint={tc('revalue.previousNote', {
            previous: moneyText(Number(property.marketPrice) || 0, currency, { decimals: 0 }),
          })}
          onChange={(event) => setForm((f) => ({ ...f, valueText: event.target.value }))}
          onBlur={() => setAmountTouched(true)}
        />
        <div className="mb-3">
          <DateField
            label={tc('revalue.date')}
            value={form.dateIso}
            max={today}
            onChange={(dateIso) => setForm((f) => ({ ...f, dateIso }))}
            error={dateError}
          />
        </div>
        <label
          htmlFor={noteId}
          className="mx-0.5 mb-1.5 flex justify-between text-m-sub font-semibold text-ink-2"
        >
          <span>{tc('revalue.note')}</span>
          <span className="font-medium text-ink-4">{tc('labels.optional')}</span>
        </label>
        <input
          id={noteId}
          type="text"
          value={form.note}
          onChange={(event) => setForm((f) => ({ ...f, note: event.target.value }))}
          placeholder={tc('revalue.notePlaceholder')}
          autoComplete="off"
          enterKeyHint="done"
          className="h-12 w-full rounded-[10px] border border-line-strong bg-paper px-3.5 text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none"
        />
      </form>
    </Sheet>
  );
}
