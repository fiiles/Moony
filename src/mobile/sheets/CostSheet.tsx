import { useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RealEstate, RealEstateOneTimeCost } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { Button } from '@/components/ui/button';
import { isoDateFromUtcTimestamp } from '@/utils/period';
import { AmountField } from '@/mobile/components/AmountField';
import { DateField } from '@/mobile/components/DateField';
import { Segment } from '@/mobile/components/Segment';
import { Sheet } from '@/mobile/components/Sheet';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { useHideAmounts } from '@/mobile/hooks/use-hide-amounts';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { usePropertyCostMutations } from '@/mobile/hooks/use-property-mutations';
import {
  costErrors,
  costInput,
  latestCostDay,
  recurringInput,
  todayIso,
  type CostErrors,
  type CostForm,
  type CostFrequency,
  type CostKind,
} from '@/mobile/lib/property-detail';

interface CostSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  property: RealEstate;
  /** The one-time cost being edited; none adds a new cost. Set together with `open`. */
  cost: RealEstateOneTimeCost | null;
}

/** The classes of a plain text field of a sheet (the name, the note): 48 pt, the field role. */
const TEXT_FIELD =
  'h-12 w-full rounded-[10px] border border-line-strong bg-paper px-3.5 text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring';

/**
 * The form a sheet opens with. A new cost starts as a recurring one for today; an edited cost with
 * its own values — except while the amounts are hidden, when the field starts empty (a prefilled
 * amount would show what the eye hides) and the stored amount waits beside it (`keptAmount`): it is
 * what is saved until something is typed. The stored day is kept too, so that a cost the computer
 * dated ahead can be edited without moving it.
 */
function startForm(
  property: RealEstate,
  cost: RealEstateOneTimeCost | null,
  hidden: boolean
): CostForm {
  return {
    realEstateId: property.id,
    kind: cost ? 'oneTime' : 'recurring',
    name: cost?.name ?? '',
    amountText: cost && !hidden ? String(Number(cost.amount) || 0) : '',
    keptAmount: cost && hidden ? cost.amount : null,
    frequency: 'monthly',
    dateIso: cost ? isoDateFromUtcTimestamp(cost.date) : todayIso(),
    description: cost?.description ?? '',
    originalDateIso: cost ? isoDateFromUtcTimestamp(cost.date) : null,
  };
}

const FREQUENCIES: readonly CostFrequency[] = ['monthly', 'quarterly', 'yearly'];

/**
 * "Přidat náklad" and "Upravit jednorázový náklad" (spec §8.8; the phone counterpart of the
 * desktop's `AddRecurringCostModal` and `OneTimeCostModal`): Pravidelný · Jednorázový (a stored
 * cost keeps its kind, so editing has no segment), the name, the amount in the property's currency
 * — a cost in another one is added on the computer, and an edited cost keeps its own — then the
 * frequency of a recurring cost, or the day (today at most; an edited cost that the computer dated
 * ahead may keep that day) and an optional note of a one-time one. Return on the name goes on to
 * the amount. The cost is written through the property's cost mutations, every write refreshing
 * what rule 7 lists, and the sheet closes once it is saved; a refusal is the mutation's own toast
 * and the sheet stays. While the amounts are hidden an edited cost's amount field starts empty and
 * shows the masked amount as its placeholder: left empty it saves the stored amount, so the name,
 * the day or the note can change without typing the amount again; the amount typed is an input and
 * is never masked.
 *
 * Every opening starts over, from the stored cost or from a recurring one.
 */
export function CostSheet({ open, onOpenChange, property, cost }: CostSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const { hidden } = useHideAmounts();
  const formId = useId();
  const amountField = useRef<HTMLInputElement>(null);
  const { addRecurring, saveOneTime } = usePropertyCostMutations(property.id);
  const currency = recordCurrency(cost ? cost.currency : property.marketPriceCurrency);

  const [form, setForm] = useState(() => startForm(property, cost, hidden));
  // An empty field is only called missing once it has been left
  const [touched, setTouched] = useState({ name: false, amount: false });
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setForm(startForm(property, cost, hidden));
      setTouched({ name: false, amount: false });
    }
  }
  const change = (patch: Partial<CostForm>) => setForm((f) => ({ ...f, ...patch }));

  const today = todayIso();
  const errors: CostErrors = costErrors(form, today);
  const valid = Object.keys(errors).length === 0;
  const pending = addRecurring.isPending || saveOneTime.isPending;

  const nameError =
    errors.name === 'required' && touched.name ? t('property.costNameRequired') : undefined;
  const amountError =
    errors.amount === undefined || (errors.amount === 'required' && !touched.amount)
      ? undefined
      : errors.amount === 'required'
        ? tc('validation.amountRequired')
        : errors.amount === 'invalid'
          ? tc('validation.invalidAmount')
          : tc('validation.amountPositive');
  const dateError =
    errors.date === 'required'
      ? tc('validation.dateRequired')
      : errors.date === 'future'
        ? t('property.costDateInFuture')
        : undefined;

  const submit = () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (!valid || pending) return;
    const done = { onSuccess: () => onOpenChange(false) };
    if (form.kind === 'recurring') {
      const recurring = recurringInput(form, currency);
      if (recurring) addRecurring.mutate({ realEstate: property, cost: recurring }, done);
    } else {
      const data = costInput(form, currency);
      if (data) saveOneTime.mutate({ cost, data }, done);
    }
  };

  const nameId = `${formId}-name`;
  const noteId = `${formId}-note`;
  const nameErrorId = `${nameId}-error`;
  const editing = cost !== null;
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={editing ? tr('modal.cost.editTitle') : tr('modal.cost.addCost')}
      description={
        editing
          ? tr('modal.cost.editDescription')
          : form.kind === 'recurring'
            ? tr('modal.recurringCost.description')
            : tr('modal.cost.description')
      }
      footer={
        <>
          <Button variant="ghost" className={FOOTER_GHOST} onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={`${formId}-form`}
            className={FOOTER_PRIMARY}
            disabled={!valid}
            loading={pending}
          >
            {editing ? tr('modal.cost.updateCost') : tr('modal.cost.addCost')}
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
        {!editing && (
          <Segment<CostKind>
            value={form.kind}
            options={[
              { value: 'recurring', label: t('property.costKindRecurring') },
              { value: 'oneTime', label: t('property.costKindOneTime') },
            ]}
            onChange={(kind) => change({ kind })}
            label={tc('labels.type')}
            className="mb-3.5"
          />
        )}
        <div className="mb-3">
          <label
            htmlFor={nameId}
            className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2"
          >
            {tr('modal.cost.name')}
          </label>
          <input
            id={nameId}
            type="text"
            value={form.name}
            onChange={(event) => change({ name: event.target.value })}
            onBlur={() => setTouched((state) => ({ ...state, name: true }))}
            placeholder={
              form.kind === 'recurring'
                ? tr('modal.recurringCost.namePlaceholder')
                : tr('modal.cost.namePlaceholder')
            }
            maxLength={100}
            autoComplete="off"
            enterKeyHint="next"
            onKeyDown={(event) => {
              // The Enter that ends an IME composition (ja, zh) is the IME's: WebKit reports it with
              // `isComposing` already false and the legacy keyCode 229
              if (
                event.key !== 'Enter' ||
                event.nativeEvent.isComposing ||
                event.nativeEvent.keyCode === 229
              ) {
                return;
              }
              // Return goes on to the amount; the form is submitted from its last field
              event.preventDefault();
              amountField.current?.focus();
            }}
            aria-invalid={nameError ? true : undefined}
            aria-describedby={nameError ? nameErrorId : undefined}
            className={TEXT_FIELD}
          />
          {nameError && (
            <p
              id={nameErrorId}
              role="alert"
              className="mx-0.5 mb-0 mt-1.5 text-m-sub font-semibold text-loss"
            >
              {nameError}
            </p>
          )}
        </div>
        <AmountField
          ref={amountField}
          label={tr('modal.cost.amount')}
          unit={currency}
          value={form.amountText}
          // The stored amount that an empty field saves, masked while the amounts are hidden
          placeholder={
            form.keptAmount !== null
              ? moneyText(Number(form.keptAmount) || 0, currency, { decimals: 0 })
              : '0'
          }
          error={amountError}
          hint={
            cost
              ? t('property.costCurrent', {
                  amount: moneyText(Number(cost.amount) || 0, currency, { decimals: 0 }),
                })
              : undefined
          }
          onChange={(event) => change({ amountText: event.target.value })}
          onBlur={() => setTouched((state) => ({ ...state, amount: true }))}
        />
        {form.kind === 'recurring' ? (
          <>
            <div className="mx-0.5 mb-1.5 text-m-sub font-semibold text-ink-2">
              {tr('detail.recurringTable.frequency')}
            </div>
            <Segment<CostFrequency>
              value={form.frequency}
              options={FREQUENCIES.map((frequency) => ({
                value: frequency,
                label: tr(`modal.add.${frequency}`),
              }))}
              onChange={(frequency) => change({ frequency })}
              label={tr('detail.recurringTable.frequency')}
            />
          </>
        ) : (
          <>
            <div className="mb-3">
              <DateField
                label={tr('modal.cost.date')}
                value={form.dateIso}
                max={latestCostDay(form, today)}
                onChange={(dateIso) => change({ dateIso })}
                error={dateError}
              />
            </div>
            <label
              htmlFor={noteId}
              className="mx-0.5 mb-1.5 flex justify-between gap-3 text-m-sub font-semibold text-ink-2"
            >
              <span>{tr('modal.cost.descriptionLabel')}</span>
              <span className="font-medium text-ink-4">{tc('labels.optional')}</span>
            </label>
            <input
              id={noteId}
              type="text"
              value={form.description}
              onChange={(event) => change({ description: event.target.value })}
              placeholder={tr('modal.cost.descriptionPlaceholder')}
              autoComplete="off"
              enterKeyHint="done"
              className={TEXT_FIELD}
            />
          </>
        )}
      </form>
    </Sheet>
  );
}
