import { useId, type ComponentProps } from 'react';
import { cn } from '@/lib/utils';

interface AmountFieldProps extends Omit<ComponentProps<'input'>, 'type' | 'inputMode' | 'id'> {
  label: string;
  /** The currency, inside the field on the right. */
  unit: string;
  /** Under the field, in `loss`; also marks the field invalid. */
  error?: string;
}

/**
 * Amount field of a phone form (design system README §13, `.m-field`): the label above, a 48 pt
 * field with the currency inside it on the right, the decimal keypad. The field is plain text
 * (`parseAmountInput` reads what the keypad of the system language types); `type="number"` would
 * refuse a comma in a WebView whose language is not the keypad's.
 */
export function AmountField({ label, unit, error, className, ...props }: AmountFieldProps) {
  const id = useId();
  const unitId = `${id}-unit`;
  const errorId = `${id}-error`;
  return (
    <div className="mb-3 last:mb-0">
      <label htmlFor={id} className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
        {label}
      </label>
      <div className="relative">
        <input
          {...props}
          id={id}
          type="text"
          inputMode="decimal"
          autoComplete="off"
          autoCorrect="off"
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${unitId} ${errorId}` : unitId}
          className={cn(
            'h-12 w-full rounded-[10px] border border-line-strong bg-paper pl-3.5 pr-16 text-m-body text-ink num transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring',
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
