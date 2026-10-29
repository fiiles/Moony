import type { ComponentPropsWithoutRef, ReactNode } from 'react';
import { ChevronLeft } from 'lucide-react';
import { cn } from '@/lib/utils';

interface NavBarProps {
  title: string;
  /** Tab root: the large title sits in the content, the bar title waits until it has scrolled away. */
  large?: boolean;
  /** Content has moved under the bar (on a tab root: past the large title). */
  collapsed: boolean;
  /** At most one item: a back button or the hide-amounts eye. */
  leading?: ReactNode;
  /** At most two 44 pt buttons. */
  trailing?: ReactNode;
}

/**
 * Nav bar (design system README §13, `.m-nav`): 44 pt under the top safe area, the 20 pt title
 * centred between the slots. A collapsed bar shows its title and a hairline.
 */
export function NavBar({ title, large = false, collapsed, leading, trailing }: NavBarProps) {
  // On a tab root the large title is the page heading; the bar copy only mirrors it.
  const Title = large ? 'div' : 'h1';
  return (
    <header
      className={cn(
        'grid h-[calc(var(--m-nav-h)+var(--safe-top))] flex-none select-none grid-cols-[1fr_minmax(0,auto)_1fr] items-center gap-1 border-b border-transparent px-2.5 pt-[var(--safe-top)] transition-colors duration-base ease-moony',
        collapsed && 'border-line'
      )}
    >
      <div className="flex items-center gap-0.5">{leading}</div>
      <Title
        aria-hidden={large || undefined}
        className={cn(
          'min-w-0 truncate text-center text-m-title text-ink transition-opacity duration-base ease-moony',
          large && !collapsed && 'opacity-0'
        )}
      >
        {title}
      </Title>
      <div className="flex items-center justify-end gap-0.5">{trailing}</div>
    </header>
  );
}

/**
 * A text button of the nav bar (`.m-nav__back`): 44 pt tall, the label ends with an ellipsis at
 * 120 px. With `chevron` it is the back button (chevron and the parent's name), which the swipe
 * from the left edge presses (`data-nav-back`, `useEdgeSwipeBack`).
 */
export function NavTextButton({
  label,
  onClick,
  disabled,
  chevron = false,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  chevron?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      data-nav-back={chevron || undefined}
      className={cn(
        'inline-flex h-11 min-w-0 max-w-[120px] items-center gap-0.5 rounded-[12px] text-m-row text-ink [-webkit-touch-callout:none] active:bg-well-2 disabled:pointer-events-none disabled:opacity-45',
        chevron ? 'pl-1 pr-2' : 'px-3'
      )}
    >
      {chevron && <ChevronLeft className="size-6 shrink-0" strokeWidth={1.75} aria-hidden />}
      <span className="truncate">{label}</span>
    </button>
  );
}

/** A 44 pt icon button of the nav bar (`.m-nav__btn`); the caller names it with `aria-label`. */
export function NavIconButton({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<'button'> & { 'aria-label': string }) {
  return (
    <button
      type="button"
      className={cn(
        'inline-flex size-11 flex-none items-center justify-center rounded-[12px] text-ink [-webkit-touch-callout:none] active:bg-well-2 [&_svg]:size-6',
        className
      )}
      {...props}
    >
      {children}
    </button>
  );
}
