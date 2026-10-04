import * as React from 'react';
import { cn } from '@/lib/utils';

export interface ChipProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** Selected: dark material. */
  active?: boolean;
  /** Small glyph before the label (a series dot, an icon). */
  leading?: React.ReactNode;
}

/**
 * Chip (design system, `.chip`): a 26 px pill for filters and tag pickers;
 * `active` fills it with the dark material. Renders a <button>; use
 * <ChipLabel> for a non-interactive one (a tag in a list).
 */
const Chip = React.forwardRef<HTMLButtonElement, ChipProps>(
  ({ className, active = false, leading, children, type = 'button', ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      aria-pressed={props.role === undefined ? active : undefined}
      className={cn(
        'inline-flex h-[26px] items-center gap-1.5 whitespace-nowrap rounded-[13px] border px-[9px] text-micro font-650 transition-colors duration-fast focus-visible:outline-none focus-visible:shadow-focus disabled:pointer-events-none disabled:opacity-45',
        active
          ? 'border-dark bg-dark text-ink-inverse [&_svg]:text-ink-inverse'
          : 'border-line-strong bg-paper text-ink-2 hover:border-line-hover hover:text-ink',
        '[&_svg]:size-3',
        className
      )}
      {...props}
    >
      {leading}
      {children}
    </button>
  )
);
Chip.displayName = 'Chip';

/** The same pill as a static label. */
function ChipLabel({ className, ...props }: React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={cn(
        'inline-flex h-[26px] items-center gap-1.5 whitespace-nowrap rounded-[13px] border border-line-strong bg-paper px-[9px] text-micro font-650 text-ink-2',
        className
      )}
      {...props}
    />
  );
}

export { Chip, ChipLabel };
