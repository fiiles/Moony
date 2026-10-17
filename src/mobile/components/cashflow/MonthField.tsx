import { useId } from 'react';

interface MonthFieldProps {
  label: string;
  /** `YYYY-MM`, or `''` for no month. */
  value: string;
  onChange: (value: string) => void;
  /** Under the field, in `loss`; also marks the field invalid. */
  error?: string;
  /** A small text action beside the label ("Bez konce" clears the end month). */
  action?: { label: string; onClick: () => void };
}

/**
 * Month field of a phone form (design system README §13, `.m-field`): the amount field's look with
 * the platform's own month picker (`input type="month"`: iOS and Android show their wheel) — the
 * label 12 / 600 above, a 48 pt field with radius 10 and `line-strong`, the focus ring, `loss`
 * when invalid, the error 12 / 600 under it. `min` and `max` stay unset: any month may be typed
 * and the sheet's validation says what is wrong. The optional `action` is a text link beside the
 * label; the label row is two lines of label high with or without one (a long label wraps beside
 * the action), so two fields side by side keep their inputs level. The action fills that row (32 pt)
 * and its hit area reaches 12 pt above it: 44 pt, never over the input. The field carries no margin of
 * its own: the screen spaces it (the limit sheet puts two in a grid).
 */
export function MonthField({ label, value, onChange, error, action }: MonthFieldProps) {
  const id = useId();
  const errorId = `${id}-error`;
  return (
    <div className="min-w-0">
      <div className="mx-0.5 flex min-h-8 items-center justify-between gap-2">
        <label htmlFor={id} className="block min-w-0 text-m-sub font-semibold text-ink-2">
          {label}
        </label>
        {action && (
          <button
            type="button"
            onClick={action.onClick}
            className="relative flex flex-none items-center self-stretch whitespace-nowrap text-m-caption font-semibold text-ink-3 underline underline-offset-[3px] focus-visible:text-ink focus-visible:outline-none active:text-ink after:absolute after:-inset-x-1.5 after:-top-3 after:bottom-0 after:content-['']"
          >
            {action.label}
          </button>
        )}
      </div>
      <input
        id={id}
        type="month"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        className="m-0 block h-12 w-full min-w-0 appearance-none rounded-[10px] border border-line-strong bg-paper px-3.5 text-left text-m-body text-ink num transition-[border-color,box-shadow] duration-fast focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring [&::-webkit-date-and-time-value]:text-left"
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
