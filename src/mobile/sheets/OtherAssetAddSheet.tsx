import { useId, useState, type ComponentProps } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { currencyName, type CurrencyCode } from '@shared/currencies';
import { Button } from '@/components/ui/button';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { todayIsoUtc } from '@/utils/period';
import { AmountField } from '@/mobile/components/AmountField';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { PickGroup, PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';
import { useOtherAssetMutations } from '@/mobile/hooks/use-other-asset';
import {
  YIELD_TYPES,
  otherAssetFormState,
  type AmountError,
  type OtherAssetFormInput,
  type OtherAssetFormState,
  type YieldType,
} from '@/mobile/lib/other-asset-form';
import { CurrencyPickerSheet } from '@/mobile/sheets/CurrencyPickerSheet';

interface OtherAssetAddSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** The desktop's wording of each yield type (`otherAssets:modal.add.yieldTypes.*`). */
const YIELD_LABEL: Record<YieldType, string> = {
  none: 'none',
  fixed: 'fixed',
  percent_purchase: 'percentPurchase',
  percent_market: 'percentMarket',
};

/** What every opening starts from: today, the report currency, nothing typed. */
function emptyForm(currency: string): OtherAssetFormInput {
  return {
    name: '',
    currency,
    priceText: '',
    quantityText: '',
    purchasePriceText: '',
    purchaseDate: todayIsoUtc(),
    yieldType: 'none',
    yieldText: '',
  };
}

/**
 * "Přidat aktivum" (spec §8.8, §10.5): the phone's add form for an other asset, the desktop's
 * `AddOtherAssetModal` with its fields — name, currency and today's price per unit first, the
 * optional first purchase (quantity, price, day: it becomes the asset's first buy) and the yield
 * folded under "Další údaje". The logic is `otherAssetFormState`; this renders it. Errors show
 * once a save has been tried, and a save that fails on a folded field unfolds it. Every opening
 * starts over, with the fold closed.
 */
export function OtherAssetAddSheet({ open, onOpenChange }: OtherAssetAddSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: to } = useTranslation('otherAssets');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const { create } = useOtherAssetMutations();
  const formId = useId();
  const foldId = useId();

  const [input, setInput] = useState(() => emptyForm(currencyCode));
  const [expanded, setExpanded] = useState(false);
  const [attempted, setAttempted] = useState(false);
  const [currencyOpen, setCurrencyOpen] = useState(false);

  // Every opening starts over
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setInput(emptyForm(currencyCode));
      setExpanded(false);
      setAttempted(false);
    }
  }

  const edit = (patch: Partial<OtherAssetFormInput>) =>
    setInput((current) => ({ ...current, ...patch }));
  const form = otherAssetFormState(input);
  const { currency } = input;
  const currencyText = `${currency} · ${currencyName(currency, fmt.locale)}`;

  const submit = () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (create.isPending) return;
    setAttempted(true);
    if (!form.valid || form.payload === null) {
      if (form.foldError) setExpanded(true);
      return;
    }
    create.mutate(form.payload, { onSuccess: () => onOpenChange(false) });
  };

  /** The words of an amount's error, once a save has been tried (`messages` are the field's own). */
  const amountError = (error: AmountError | undefined, messages: Record<AmountError, string>) =>
    attempted && error !== undefined ? messages[error] : undefined;
  const errors: OtherAssetFormState['errors'] = attempted ? form.errors : {};
  const priceMessages: Record<AmountError, string> = {
    required: tc('validation.priceRequired'),
    invalid: tc('validation.invalidPrice'),
    positive: tc('validation.pricePositive'),
    negative: tc('validation.priceNonNegative'),
  };
  const quantityMessages: Record<AmountError, string> = {
    required: tc('validation.quantityRequired'),
    invalid: tc('validation.invalidQuantity'),
    positive: tc('validation.quantityPositive'),
    negative: tc('validation.quantityNonNegative'),
  };

  return (
    <>
      <Sheet
        open={open}
        onOpenChange={onOpenChange}
        title={to('modal.add.title')}
        description={to('modal.add.description')}
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
            <Button
              type="submit"
              form={formId}
              className={FOOTER_PRIMARY}
              loading={create.isPending}
            >
              {to('modal.add.submit')}
            </Button>
          </>
        }
      >
        <form
          id={formId}
          noValidate
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <InputField
            label={to('modal.add.assetName')}
            value={input.name}
            onChange={(event) => edit({ name: event.target.value })}
            placeholder={to('modal.add.assetNamePlaceholder')}
            error={errors.name && tc('validation.assetNameRequired')}
            autoComplete="off"
            autoCapitalize="sentences"
            maxLength={100}
            enterKeyHint="next"
          />
          <div className="mb-3 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-4 py-1 shadow-e1">
            <KeyValueRow
              label={tc('labels.currency')}
              value={currencyText}
              ariaLabel={`${tc('labels.currency')}, ${currencyText}`}
              onClick={() => setCurrencyOpen(true)}
            />
          </div>
          <AmountField
            label={to('modal.add.marketPrice')}
            aria-label={to('modal.add.marketPrice')}
            unit={currency}
            value={input.priceText}
            onChange={(event) => edit({ priceText: event.target.value })}
            placeholder="0"
            hint={to('modal.add.marketPriceHint')}
            error={amountError(errors.price, priceMessages)}
          />

          <button
            type="button"
            aria-expanded={expanded}
            aria-controls={foldId}
            onClick={() => setExpanded(!expanded)}
            className="flex min-h-11 w-full items-center justify-between px-1 py-2 text-left text-m-body font-semibold text-ink-2 [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
          >
            {t('wealth.otherAssets.moreDetails')}
            {expanded ? (
              <ChevronUp aria-hidden className="size-4 text-ink-4" strokeWidth={1.75} />
            ) : (
              <ChevronDown aria-hidden className="size-4 text-ink-4" strokeWidth={1.75} />
            )}
          </button>
          {expanded && (
            <div id={foldId}>
              <h3 className="mx-1 mb-1.5 mt-2 text-m-sub font-semibold text-ink-3">
                {to('modal.add.initialPurchase')}
              </h3>
              <AmountField
                label={tc('labels.quantity')}
                aria-label={tc('labels.quantity')}
                unit={to('table.unit')}
                value={input.quantityText}
                onChange={(event) => edit({ quantityText: event.target.value })}
                placeholder="0"
                error={amountError(errors.quantity, quantityMessages)}
              />
              <AmountField
                label={to('modal.add.pricePerUnit')}
                aria-label={to('modal.add.pricePerUnit')}
                unit={currency}
                value={input.purchasePriceText}
                onChange={(event) => edit({ purchasePriceText: event.target.value })}
                placeholder="0"
                error={amountError(errors.purchasePrice, priceMessages)}
              />
              <InputField
                type="date"
                label={tc('labels.date')}
                value={input.purchaseDate}
                onChange={(event) => edit({ purchaseDate: event.target.value })}
                // A day to come is refused with a message, as the bond form does (`max` alone can
                // block a submit silently in some webviews)
                max={todayIsoUtc()}
                error={
                  errors.date &&
                  (errors.date === 'future'
                    ? tc('validation.purchaseDateInFuture')
                    : tc('validation.dateRequired'))
                }
              />
              <p className="mx-0.5 mb-0 mt-1.5 text-m-caption text-ink-3">
                {to('modal.add.initialPurchaseHint')}
              </p>

              <h3 className="mx-1 mb-1 mt-5 text-m-sub font-semibold text-ink-3">
                {to('modal.add.yieldConfig')}
              </h3>
              <p className="mx-0.5 mb-1 mt-0 text-m-caption text-ink-3">
                {to('modal.add.yieldConfigDescription')}
              </p>
              <PickGroup title={to('modal.add.yieldType')}>
                {YIELD_TYPES.map((type) => (
                  <PickRow
                    key={type}
                    label={to(`modal.add.yieldTypes.${YIELD_LABEL[type]}`)}
                    ariaLabel={to(`modal.add.yieldTypes.${YIELD_LABEL[type]}`)}
                    checked={input.yieldType === type}
                    onSelect={() => edit({ yieldType: type })}
                  />
                ))}
              </PickGroup>
              {input.yieldType !== 'none' && (
                <div className="mt-2">
                  <AmountField
                    label={to('modal.add.yieldValue')}
                    aria-label={to('modal.add.yieldValue')}
                    unit={input.yieldType === 'fixed' ? currency : '%'}
                    value={input.yieldText}
                    onChange={(event) => edit({ yieldText: event.target.value })}
                    placeholder="0"
                    error={
                      errors.yield &&
                      (errors.yield === 'invalid'
                        ? tc('validation.invalidAmount')
                        : tc('validation.yieldNonNegative'))
                    }
                  />
                </div>
              )}
            </div>
          )}
        </form>
      </Sheet>
      <CurrencyPickerSheet
        open={currencyOpen}
        onOpenChange={setCurrencyOpen}
        value={currency as CurrencyCode}
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

/**
 * A 48 pt text or date field of the form (`.m-field__i`) with its label above and its error under
 * it: the amount field's look without the unit. A date is the platform's own picker.
 */
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
        // Named by the visible label, whatever the placeholder says
        aria-label={label}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        className={cn(
          'm-0 block h-12 w-full min-w-0 appearance-none rounded-[10px] border border-line-strong bg-paper px-3.5 text-left text-m-body text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring [&::-webkit-date-and-time-value]:text-left',
          props.type === 'date' && 'num',
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
