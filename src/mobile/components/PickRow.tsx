import { useId, type ReactNode } from 'react';
import { Check } from 'lucide-react';

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
 * action row such as "Nová kategorie").
 */
export function PickRow({
  icon,
  label,
  checked,
  onSelect,
}: {
  icon?: ReactNode;
  label: string;
  /** Whether this choice is the current one; leave it out for a row that is not a choice. */
  checked?: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      {...(checked === undefined ? {} : { role: 'radio', 'aria-checked': checked })}
      onClick={onSelect}
      className="flex min-h-11 w-full items-center gap-3 px-1 py-1.5 text-left text-m-body text-ink [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {checked && <Check aria-hidden className="size-[18px] flex-none text-ink" strokeWidth={2} />}
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
