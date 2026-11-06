import { useId, useState, type ComponentProps } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertSpendingPlan, SpendingPlan } from '@shared/schema';
import { currencyName, type CurrencyCode } from '@shared/currencies';
import { Button } from '@/components/ui/button';
import { CategoryIcon, CategoryIconChip } from '@/components/common/CategoryIcon';
import { ICON_MAP } from '@/components/common/category-icons-map';
import { COLOR_PRESETS, DEFAULT_COLOR } from '@/components/common/IconColorPicker';
import { ApiError } from '@/lib/api-error';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { useSpendingPlanMutations } from '@/hooks/use-spending-plan-mutations';
import { AmountField } from '@/mobile/components/AmountField';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { Segment } from '@/mobile/components/Segment';
import { Sheet } from '@/mobile/components/Sheet';
import { parseAmountInput } from '@/mobile/lib/amount-input';
import { yearChoices } from '@/mobile/lib/plan-rows';
import { CurrencyPickerSheet } from '@/mobile/sheets/CurrencyPickerSheet';

const DEFAULT_ICON = 'wallet';
/** Same limits as `InsertSpendingPlan::validate` in the backend (and the desktop form). */
const NAME_MAX = 100;
const NOTE_MAX = 500;
const ICON_NAMES = Object.keys(ICON_MAP);

interface PlanFormSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The plan being edited; null or omitted creates one. */
  plan?: SpendingPlan | null;
  /** Year of a new plan (the page's year). */
  defaultYear: number;
  /** Called with the saved plan (created or edited) once the sheet has closed. */
  onSaved?: (plan: SpendingPlan) => void;
}

interface FormInput {
  name: string;
  amountText: string;
  currency: string;
  year: number;
  icon: string;
  color: string;
  note: string;
}

type FieldError = 'nameRequired' | 'nameTooLong' | 'amountInvalid' | 'noteTooLong' | 'nameTaken';

/** What an opening starts from: the plan's values, or the desktop form's defaults. */
function initialForm(
  plan: SpendingPlan | null,
  defaultYear: number,
  currencyCode: string
): FormInput {
  if (plan) {
    const amount = parseFloat(plan.amount);
    return {
      name: plan.name,
      amountText: Number.isFinite(amount) ? String(amount) : '',
      currency: plan.currency,
      year: plan.year,
      icon: plan.icon || DEFAULT_ICON,
      color: plan.color || DEFAULT_COLOR,
      note: plan.note ?? '',
    };
  }
  return {
    name: '',
    amountText: '',
    currency: currencyCode,
    year: defaultYear,
    icon: DEFAULT_ICON,
    color: DEFAULT_COLOR,
    note: '',
  };
}

/** The form's errors (the desktop's zod rules); empty when the form can be saved. */
function formErrors(input: FormInput): Partial<Record<'name' | 'amount' | 'note', FieldError>> {
  const errors: Partial<Record<'name' | 'amount' | 'note', FieldError>> = {};
  const name = input.name.trim();
  if (name === '') errors.name = 'nameRequired';
  else if (name.length > NAME_MAX) errors.name = 'nameTooLong';
  const parsed = parseAmountInput(input.amountText);
  if (parsed === null || !(Number(parsed) > 0)) errors.amount = 'amountInvalid';
  if (input.note.length > NOTE_MAX) errors.note = 'noteTooLong';
  return errors;
}

const ERROR_KEY: Record<FieldError, string> = {
  nameRequired: 'validation.spendingPlanNameRequired',
  nameTooLong: 'validation.spendingPlanNameTooLong',
  amountInvalid: 'validation.spendingPlanAmountInvalid',
  noteTooLong: 'validation.spendingPlanNoteTooLong',
  nameTaken: 'validation.spendingPlanNameTaken',
};

/**
 * New / edit spending plan on the phone: the desktop's `PlanFormDialog` with the same fields,
 * rules and toasts, set as a full sheet. Name, planned amount (its unit is the currency), the
 * currency (a row that opens the picker), the year (a segment of the year and its neighbours), the
 * ECB hint, an optional note, and the icon and color as 44 pt touch grids with a preview chip (the
 * desktop picker's 32 pt targets are too small for a thumb). Errors show once a save has been
 * tried; a name already used in the year shows under the name. Every opening starts over.
 */
export function PlanFormSheet({
  open,
  onOpenChange,
  plan = null,
  defaultYear,
  onSaved,
}: PlanFormSheetProps) {
  const { t } = useTranslation('spending_plans');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const { createPlan, updatePlan } = useSpendingPlanMutations();
  const formId = useId();
  const isEdit = plan !== null;
  const isPending = createPlan.isPending || updatePlan.isPending;

  const [input, setInput] = useState(() => initialForm(plan, defaultYear, currencyCode));
  const [attempted, setAttempted] = useState(false);
  const [nameTaken, setNameTaken] = useState(false);
  const [currencyOpen, setCurrencyOpen] = useState(false);

  // Every opening starts over
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setInput(initialForm(plan, defaultYear, currencyCode));
      setAttempted(false);
      setNameTaken(false);
    }
  }

  const edit = (patch: Partial<FormInput>) => setInput((current) => ({ ...current, ...patch }));
  const errors = formErrors(input);
  const nameError: FieldError | undefined = nameTaken
    ? 'nameTaken'
    : attempted
      ? errors.name
      : undefined;
  const amountError = attempted ? errors.amount : undefined;
  const noteError = attempted ? errors.note : undefined;
  const message = (error: FieldError | undefined) => (error ? tc(ERROR_KEY[error]) : undefined);

  const years = yearChoices(plan?.year ?? defaultYear);
  const currencyText = `${input.currency} · ${currencyName(input.currency, fmt.locale)}`;

  const submit = async () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (isPending) return;
    setAttempted(true);
    if (Object.keys(errors).length > 0) return;
    const data: InsertSpendingPlan = {
      name: input.name.trim(),
      year: input.year,
      amount: parseAmountInput(input.amountText) ?? '0',
      currency: input.currency,
      icon: input.icon,
      color: input.color,
      note: input.note.trim() ? input.note.trim() : null,
    };
    try {
      let saved: SpendingPlan;
      if (plan) {
        saved = await updatePlan.mutateAsync({ id: plan.id, data });
      } else {
        saved = await createPlan.mutateAsync(data);
        toast.success(t('toast.created', { name: saved.name }));
      }
      onOpenChange(false);
      onSaved?.(saved);
    } catch (error) {
      // A taken name belongs under the field; every other error was toasted by the mutation.
      if (error instanceof ApiError && error.key === 'validation.spendingPlanNameTaken') {
        setNameTaken(true);
      }
    }
  };

  return (
    <>
      <Sheet
        open={open}
        onOpenChange={onOpenChange}
        title={isEdit ? t('form.editTitle') : t('form.newTitle')}
        description={
          plan
            ? t('form.editDescription', { name: plan.name, year: plan.year })
            : tm('plans.form.newDescription')
        }
        full
        footer={
          <>
            <Button
              type="button"
              variant="ghost"
              className={FOOTER_GHOST}
              onClick={() => onOpenChange(false)}
            >
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" form={formId} className={FOOTER_PRIMARY} loading={isPending}>
              {isEdit ? t('form.save') : t('form.create')}
            </Button>
          </>
        }
      >
        <form
          id={formId}
          noValidate
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          <InputField
            label={t('form.name')}
            value={input.name}
            onChange={(event) => {
              setNameTaken(false);
              edit({ name: event.target.value });
            }}
            placeholder={t('form.namePlaceholder')}
            error={message(nameError)}
            autoComplete="off"
            autoCapitalize="sentences"
            maxLength={NAME_MAX}
            enterKeyHint="next"
          />
          <AmountField
            label={t('form.amount')}
            aria-label={t('form.amount')}
            unit={input.currency}
            value={input.amountText}
            onChange={(event) => edit({ amountText: event.target.value })}
            placeholder="0"
            hint={t('form.fxHint')}
            error={message(amountError)}
          />
          <div className="mb-3 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-4 py-1 shadow-e1">
            <KeyValueRow
              label={tc('labels.currency')}
              value={currencyText}
              ariaLabel={`${tc('labels.currency')}, ${currencyText}`}
              onClick={() => setCurrencyOpen(true)}
            />
          </div>

          <div className="mb-3">
            <span className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
              {t('form.year')}
            </span>
            <Segment
              label={t('form.year')}
              value={String(input.year)}
              options={years.map((year) => ({ value: String(year), label: String(year) }))}
              onChange={(value) => edit({ year: Number(value) })}
            />
          </div>

          <div className="mb-3 min-w-0">
            <label
              htmlFor={`${formId}-note`}
              className="mx-0.5 mb-1.5 flex items-baseline justify-between text-m-sub font-semibold text-ink-2"
            >
              <span>{t('form.note')}</span>
              <span className="font-normal text-ink-4">{tc('labels.optional')}</span>
            </label>
            <textarea
              id={`${formId}-note`}
              value={input.note}
              onChange={(event) => edit({ note: event.target.value })}
              rows={3}
              maxLength={NOTE_MAX}
              placeholder={t('form.notePlaceholder')}
              autoCapitalize="sentences"
              aria-invalid={noteError ? true : undefined}
              className="m-0 block w-full resize-none rounded-[10px] border border-line-strong bg-paper px-3.5 py-3 text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss"
            />
            {noteError && (
              <p role="alert" className="mx-0.5 mb-0 mt-1.5 text-m-sub font-semibold text-loss">
                {message(noteError)}
              </p>
            )}
          </div>

          <div className="mb-3">
            <span className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
              {tb('categoryManagement.icon')}
            </span>
            <div
              role="radiogroup"
              aria-label={tb('categoryManagement.icon')}
              className="grid grid-cols-6"
            >
              {ICON_NAMES.map((iconName) => (
                <button
                  key={iconName}
                  type="button"
                  role="radio"
                  aria-checked={input.icon === iconName}
                  aria-label={iconName}
                  onClick={() => edit({ icon: iconName })}
                  className="flex h-11 items-center justify-center [-webkit-touch-callout:none] focus-visible:outline-none"
                >
                  <span
                    className={cn(
                      'flex size-9 items-center justify-center rounded-r2 border [&_svg]:size-[18px]',
                      input.icon === iconName
                        ? 'border-dark bg-well-2 text-ink'
                        : 'border-transparent text-ink-3'
                    )}
                  >
                    <CategoryIcon iconName={iconName} />
                  </span>
                </button>
              ))}
            </div>
          </div>

          <div className="mb-3">
            <span className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
              {tb('categoryManagement.color')}
            </span>
            <div
              role="radiogroup"
              aria-label={tb('categoryManagement.color')}
              className="grid grid-cols-7"
            >
              {COLOR_PRESETS.map((preset) => {
                const selected = input.color.toLowerCase() === preset.toLowerCase();
                return (
                  <button
                    key={preset}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    aria-label={preset}
                    onClick={() => edit({ color: preset })}
                    className="flex h-11 items-center justify-center [-webkit-touch-callout:none] focus-visible:outline-none"
                  >
                    <span
                      className={cn(
                        'size-7 rounded-full border border-line',
                        selected && 'ring-2 ring-ink ring-offset-2 ring-offset-paper'
                      )}
                      style={{ backgroundColor: preset }}
                    />
                  </button>
                );
              })}
            </div>
          </div>

          <div className="flex items-center gap-3 rounded-r4 border border-line bg-well px-3.5 py-3">
            <CategoryIconChip category={{ icon: input.icon, color: input.color }} shape="square" />
            <span className="min-w-0 flex-1 truncate text-m-body font-semibold text-ink">
              {input.name.trim() || t('form.previewName')}
            </span>
          </div>
        </form>
      </Sheet>
      <CurrencyPickerSheet
        open={currencyOpen}
        onOpenChange={setCurrencyOpen}
        value={input.currency as CurrencyCode}
        onPick={(code) => edit({ currency: code })}
        title={tc('labels.currency')}
      />
    </>
  );
}

interface InputFieldProps extends Omit<ComponentProps<'input'>, 'id'> {
  label: string;
  /** Under the field, in `loss`; also marks the field invalid. */
  error?: string;
}

/** A 48 pt text field of the form (`.m-field__i`) with its label above and its error under it. */
function InputField({ label, error, className, ...props }: InputFieldProps) {
  const id = useId();
  const errorId = `${id}-error`;
  return (
    <div className="mb-3 min-w-0 last:mb-0">
      <label htmlFor={id} className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
        {label}
      </label>
      <input
        {...props}
        id={id}
        aria-label={label}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        className={cn(
          'm-0 block h-12 w-full min-w-0 appearance-none rounded-[10px] border border-line-strong bg-paper px-3.5 text-left text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring',
          className
        )}
      />
      {error && (
        <p
          id={errorId}
          role="alert"
          className="mx-0.5 mb-0 mt-1.5 text-m-sub font-semibold text-loss"
        >
          {error}
        </p>
      )}
    </div>
  );
}
