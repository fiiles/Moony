import type { ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';

interface KeyValueRowProps {
  label: string;
  value: ReactNode;
  /** A line under the value ("2100123456/2010"). */
  sub?: ReactNode;
  /** Makes the row a button (a settings-style row that opens a picker). */
  onClick?: () => void;
  /** The chevron of a row that opens something; on by default with `onClick`. */
  chevron?: boolean;
  /**
   * The accessible name of a row that opens something, when its content should be said as one
   * phrase ("Měna, CZK · česká koruna"): `aria-label` of the button.
   */
  ariaLabel?: string;
  /**
   * The label above the value, which takes the row's full width and wraps (a note, a control)
   * instead of sitting at the right of a one-line value.
   */
  stack?: boolean;
}

/**
 * A label and its value (README §13, `.m-kv`): the transaction sheet's rows. The label is 12 / 500
 * `ink-3` on the left, the value 15 / 450 `ink` at the right, one line ending in an ellipsis, with
 * an optional 11 `ink-4` line under it. 44 pt at least. With `onClick` the row is a button that
 * opens a picker and shows the chevron — the contract's exception to "no chevron on a row with a
 * value", as settings rows are.
 */
export function KeyValueRow({
  label,
  value,
  sub,
  onClick,
  chevron = onClick !== undefined,
  ariaLabel,
  stack = false,
}: KeyValueRowProps) {
  const content = stack ? (
    <div className="min-w-0 flex-1">
      <span className="block text-m-sub text-ink-3">{label}</span>
      <div className="mt-1 hyphens-auto whitespace-pre-line break-words text-m-body text-ink">
        {value}
      </div>
      {sub && <div className="mt-1 text-m-caption text-ink-4">{sub}</div>}
    </div>
  ) : (
    <>
      <span className="max-w-[40%] flex-none hyphens-auto break-words text-m-sub text-ink-3">
        {label}
      </span>
      <div className="min-w-0 flex-1 text-right">
        <div className="truncate text-m-body text-ink num">{value}</div>
        {sub && <div className="truncate text-m-caption text-ink-4 num">{sub}</div>}
      </div>
    </>
  );
  const chevronIcon = chevron && (
    <ChevronRight aria-hidden className="size-4 flex-none text-ink-5" strokeWidth={1.75} />
  );
  const className = cn(
    'flex min-h-11 w-full gap-3 px-1 py-2 text-left',
    stack ? 'items-start' : 'items-center'
  );

  if (onClick) {
    return (
      <button
        type="button"
        aria-label={ariaLabel}
        onClick={onClick}
        className={cn(
          className,
          '[-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus'
        )}
      >
        {content}
        {chevronIcon}
      </button>
    );
  }
  return (
    <div className={className}>
      {content}
      {chevronIcon}
    </div>
  );
}
