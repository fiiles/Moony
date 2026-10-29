import { useId } from 'react';

interface DateFieldProps {
  label: string;
  /** `YYYY-MM-DD`, or empty. */
  value: string;
  onChange: (value: string) => void;
  /** The latest day the picker offers (`YYYY-MM-DD`): a valuation or a cost is never from the future. */
  max?: string;
  /** Under the field, in `loss`; also marks the field invalid. */
  error?: string;
}

/**
 * A 48 pt date field of a phone form (README §13, `.m-field__i`) with its label above: the system's
 * date picker in the field's frame, the way the ledger filter's custom range has it, its text in the
 * field role (`text-m-field`, never under 16 px, or iOS zooms the page into it). The value is
 * the ISO day the input speaks, so it never goes through a `Date` of the local time zone; a day the
 * backend keeps is its UTC-midnight second (`isoToUtcDay`). The input carries the month field's
 * guards for WKWebView, whose date input is otherwise a centred, margined, system-styled box of
 * its own: `appearance-none`, no margin, left-aligned text and value (`MonthField`).
 */
export function DateField({ label, value, onChange, max, error }: DateFieldProps) {
  const id = useId();
  const errorId = `${id}-error`;
  return (
    <div className="min-w-0">
      <label htmlFor={id} className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2">
        {label}
      </label>
      <input
        id={id}
        type="date"
        value={value}
        max={max}
        onChange={(event) => onChange(event.target.value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        className="m-0 flex h-12 w-full min-w-0 appearance-none items-center rounded-[10px] border border-line-strong bg-paper px-3.5 text-left text-m-field text-ink num transition-[border-color,box-shadow] duration-fast focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring [&::-webkit-date-and-time-value]:text-left"
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
