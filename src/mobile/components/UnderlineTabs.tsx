import { useRef, type KeyboardEvent } from 'react';
import { cn } from '@/lib/utils';

interface UnderlineTabsProps<T extends string> {
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  /** Accessible name of the tab list. */
  label: string;
  className?: string;
}

/**
 * Two (or more) text tabs under a screen title, the view switch of a tab root: full width, 44 pt
 * tall, the active one in `ink` with a 2 pt underline, the rest `ink-3`. It is not the `Segment`
 * (a raised pill for a period inside a view): this one changes what the whole screen is about.
 * Arrow keys move between the tabs.
 */
export function UnderlineTabs<T extends string>({
  value,
  options,
  onChange,
  label,
  className,
}: UnderlineTabsProps<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const move = (event: KeyboardEvent, index: number) => {
    const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = (index + step + options.length) % options.length;
    onChange(options[next].value);
    refs.current[next]?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label={label}
      className={cn('mb-3 grid border-b border-line', className)}
      style={{ gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))` }}
    >
      {options.map((option, index) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="tab"
            aria-selected={active}
            tabIndex={active ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => move(event, index)}
            className={cn(
              '-mb-px min-h-11 truncate border-b-2 px-2 text-m-row font-semibold transition-colors duration-fast focus-visible:outline-none focus-visible:shadow-focus',
              active ? 'border-ink text-ink' : 'border-transparent text-ink-3'
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
