import { useId, type ComponentProps } from 'react';
import { cn } from '@/lib/utils';

interface AmountFieldProps extends Omit<ComponentProps<'input'>, 'type' | 'inputMode' | 'id'> {
  label: string;
  /** The currency, inside the field on the right. */
  unit: string;
  /** Under the field, in `loss`; also marks the field invalid. */
  error?: string;
  /** Under the field, 11 `ink-3`, while there is no error (the budget limit's average spending). */
  hint?: string;
  /**
   * The ± toggle inside the field on the left (the iOS decimal pad has no minus key): pressed means
   * the amount is negative. Shown when `onNegativeChange` is given.
   */
  negative?: boolean;
  onNegativeChange?: (negative: boolean) => void;
  /** Accessible name of the toggle ("Záporná hodnota"). */
  negativeLabel?: string;
}

/**
 * Amount field of a phone form (design system README §13, `.m-field`): the label above, a 48 pt
 * field with the currency inside it on the right and, optionally, the ± toggle on the left (a 48 pt
 * hit area around a 32 pt chip), the decimal keypad, an optional hint under it. The field is plain text
 * (`parseAmountInput` reads what the keypad of the system language types); `type="number"` would
 * refuse a comma in a WebView whose language is not the keypad's.
 */
export function AmountField({
  label,
  unit,
  error,
  hint,
  negative = false,
  onNegativeChange,
  negativeLabel,
  className,
  ...props
}: AmountFieldProps) {
  const id = useId();
  const unitId = `${id}-unit`;
  const errorId = `${id}-error`;
  const hintId = `${id}-hint`;
  return (
    <div className="mb-3 last:mb-0">
      <label htmlFor={id} className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
        {label}
      </label>
      <div className="relative">
        {onNegativeChange && (
          <button
            type="button"
            aria-pressed={negative}
            aria-label={negativeLabel}
            // Keeps the focus (and the keypad) in the field while the sign changes
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onNegativeChange(!negative)}
            className="group absolute left-0 top-1/2 z-[1] flex size-12 -translate-y-1/2 items-center justify-center [-webkit-touch-callout:none] focus-visible:outline-none"
          >
            <span
              aria-hidden
              className={cn(
                'flex size-8 items-center justify-center rounded-[8px] text-m-row font-semibold leading-none transition-colors duration-fast',
                negative ? 'bg-dark-grad text-ink-inverse shadow-dark' : 'bg-well-2 text-ink-3',
                'group-focus-visible:shadow-focus'
              )}
            >
              {negative ? '\u2212' : '\u00b1'}
            </span>
          </button>
        )}
        <input
          {...props}
          id={id}
          type="text"
          inputMode="decimal"
          autoComplete="off"
          autoCorrect="off"
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${unitId} ${errorId}` : hint ? `${unitId} ${hintId}` : unitId}
          className={cn(
            'h-12 w-full rounded-[10px] border border-line-strong bg-paper pr-16 text-m-body text-ink num transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring',
            onNegativeChange ? 'pl-[52px]' : 'pl-3.5',
            className
          )}
        />
        <span
          id={unitId}
          className="pointer-events-none absolute right-3.5 top-1/2 -translate-y-1/2 text-m-sub font-semibold text-ink-4"
        >
          {unit}
        </span>
      </div>
      {error ? (
        <p
          id={errorId}
          role="alert"
          className="mx-0.5 mb-0 mt-1.5 text-m-sub font-semibold text-loss"
        >
          {error}
        </p>
      ) : (
        hint && (
          <p id={hintId} className="mx-0.5 mb-0 mt-1.5 text-m-caption text-ink-3">
            {hint}
          </p>
        )
      )}
    </div>
  );
}
