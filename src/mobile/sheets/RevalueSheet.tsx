import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { priceDecimals } from '@shared/currencies';
import { useValuationMutations, type ValuationKind } from '@/hooks/use-valuation-mutations';
import { Button } from '@/components/ui/button';
import { AmountField } from '@/mobile/components/AmountField';
import { DateField } from '@/mobile/components/DateField';
import { Sheet } from '@/mobile/components/Sheet';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { useHideAmounts } from '@/mobile/hooks/use-hide-amounts';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { todayIso } from '@/mobile/lib/property-detail';
import {
  revaluedTotal,
  revalueData,
  startRevalueForm,
  type RevalueErrors,
} from '@/mobile/lib/revalue';

interface RevalueSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: ValuationKind;
  assetId: string;
  name: string;
  /** Today's estimate in `currency`: the whole value of a property, the price of one unit of an asset held by the unit. */
  value: number;
  currency: string;
  /** Unit label of an asset held by the unit ("ks", "oz"); omitted for a whole property. */
  unit?: string;
  /** The units held: the hint then writes the total the typed price makes of them. */
  quantity?: number;
}

/**
 * "Přecenit" (spec §8.8, the phone counterpart of the desktop's `RevalueDialog`): a new dated
 * estimate, in the asset's own currency — the amount (today's estimate to start from: the market
 * value of a property, the price of one unit of another asset), the day (today at most) and an
 * optional note. The older estimates stay on the trace. Přecenit writes it through the valuation
 * mutations (the domain keys, the metrics, the cashflow and a fresh snapshot) and closes the sheet
 * once it is saved; a refusal is the mutation's own toast and the sheet stays. The hint under the
 * amount is the previous estimate, or — for an asset held by the unit, once a price is typed — the
 * total it makes of the units held; both go through the eye, and with the amounts hidden the field
 * starts empty instead of with the estimate. The amount typed is an input and is never masked.
 *
 * Every opening starts over from today's estimate and today.
 */
export function RevalueSheet({
  open,
  onOpenChange,
  kind,
  assetId,
  name,
  value,
  currency,
  unit,
  quantity,
}: RevalueSheetProps) {
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const { hidden } = useHideAmounts();
  const formId = useId();
  const { add } = useValuationMutations(kind, assetId);

  const [form, setForm] = useState(() => startRevalueForm(value, hidden, todayIso()));
  // An empty amount is only called missing once the field has been left
  const [amountTouched, setAmountTouched] = useState(false);
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setForm(startRevalueForm(value, hidden, todayIso()));
      setAmountTouched(false);
    }
  }

  const today = todayIso();
  const result = revalueData(form, { assetId, currency, today });
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

  // The hint: the total the typed price makes of the units held, else the estimate it replaces. A
  // price per unit is written with its currency's own decimals, a whole property without any.
  const total = quantity === undefined ? null : revaluedTotal(form.valueText, quantity);
  const hint =
    total === null
      ? tc('revalue.previousNote', {
          previous: moneyText(value, currency, {
            decimals: unit === undefined ? 0 : priceDecimals(value, currency),
          }),
        })
      : tc('revalue.totalNote', {
          total: moneyText(total, currency, { decimals: 0 }),
          previous: moneyText(value, currency, { decimals: priceDecimals(value, currency) }),
        });

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
      title={tc('revalue.title', { name })}
      description={tc('revalue.description')}
      footer={
        <>
          <Button variant="ghost" className={FOOTER_GHOST} onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={`${formId}-form`}
            className={FOOTER_PRIMARY}
            disabled={!result.ok}
            loading={add.isPending}
          >
            {tc('revalue.submit')}
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
          label={unit ? tc('revalue.pricePerUnit', { unit }) : tc('revalue.marketValue')}
          unit={currency}
          value={form.valueText}
          placeholder="0"
          error={amountError}
          hint={hint}
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
