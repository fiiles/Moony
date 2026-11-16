import { useId, type ReactNode } from 'react';
import { Check, Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';

/**
 * A titled list of a picker sheet (the filter's Kategorie and Období, the category picker's
 * "Naposledy použité"): the 12 / 600 `ink-3` head over rows `line-soft` apart. Rows that choose
 * one of several are a `radiogroup`; a group of plain buttons says `role="group"`.
 */
export function PickGroup({
  title,
  role = 'radiogroup',
  children,
}: {
  title: string;
  role?: 'radiogroup' | 'group';
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section className="mb-2">
      <h3 id={id} className="mx-1 mb-0.5 mt-2 text-m-sub font-semibold text-ink-3">
        {title}
      </h3>
      <div role={role} aria-labelledby={id} className="divide-y divide-line-soft">
        {children}
      </div>
    </section>
  );
}

/**
 * A 44 pt row of a picker sheet (`.m-pick`): an optional chip, the label, and — for a row that
 * is one of the choices — a check on the chosen one. Without `checked` it is a plain button (an
 * action row such as "Nová kategorie"). While a pick saves (audit E3), the picked row is `pending`
 * (a spinning ring where the check goes, `aria-busy`) and the other rows are `dimmed` (faded and
 * disabled), so a second tap cannot start another save.
 */
export function PickRow({
  icon,
  label,
  sub,
  lang,
  checked,
  ariaLabel,
  pending = false,
  dimmed = false,
  onSelect,
}: {
  icon?: ReactNode;
  label: string;
  /** A second line under the label (12 / 500 `ink-4`, one line): an account's "Fio · CZK". */
  sub?: string;
  /** The language of the label when it is not the UI's (a language shown in its own name), so the right font is used. */
  lang?: string;
  /** Whether this choice is the current one; leave it out for a row that is not a choice. */
  checked?: boolean;
  /** The button's accessible name when the label (and sub) is not it, set explicitly: `aria-label`. */
  ariaLabel?: string;
  /** This row's pick is being saved. */
  pending?: boolean;
  /** Another row's pick is being saved: this one waits, faded. */
  dimmed?: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      {...(checked === undefined ? {} : { role: 'radio', 'aria-checked': checked })}
      aria-label={ariaLabel}
      aria-busy={pending || undefined}
      disabled={dimmed}
      onClick={() => {
        if (!pending) onSelect();
      }}
      className={cn(
        'flex min-h-11 w-full items-center gap-3 px-1 py-1.5 text-left text-m-body text-ink transition-opacity duration-fast [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus',
        dimmed && 'opacity-40'
      )}
    >
      {icon}
      <span className="min-w-0 flex-1">
        <span lang={lang} className="block break-words">
          {label}
        </span>
        {sub && <span className="block break-words text-m-sub text-ink-4">{sub}</span>}
      </span>
      {pending ? (
        <Loader2
          aria-hidden
          className="size-[18px] flex-none animate-spin text-ink-3 motion-reduce:animate-none"
          strokeWidth={2}
        />
      ) : (
        checked && <Check aria-hidden className="size-[18px] flex-none text-ink" strokeWidth={2} />
      )}
    </button>
  );
}

/** The 28 pt chip of the rows that are not a category (all, uncategorized, new). */
export function NeutralChip({ icon }: { icon: ReactNode }) {
  return (
    <span
      aria-hidden
      className="grid size-7 shrink-0 place-items-center rounded-full bg-well-2 text-ink-3 [&_svg]:size-[15px]"
    >
      {icon}
    </span>
  );
}
