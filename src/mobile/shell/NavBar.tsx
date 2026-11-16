import {
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type ReactNode,
} from 'react';
import { ChevronLeft } from 'lucide-react';
import { cn } from '@/lib/utils';

/** The back button never grows past this (`.m-nav__back`). */
const NAV_BACK_MAX = 120;
/** The bar's grid gap (`gap-1`). */
const NAV_GAP = 4;

interface NavBarProps {
  title: string;
  /** Tab root: the large title sits in the content, the bar title waits until it has scrolled away. */
  large?: boolean;
  /** Content has moved under the bar (on a tab root: past the large title). */
  collapsed: boolean;
  /** At most one item: a back button or the lock button. */
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
        data-nav-title=""
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
 * A text button of the nav bar (`.m-nav__back`): 44 pt tall. With `chevron` it is the back button
 * (chevron and the parent's name), which the swipe from the left edge presses (`data-nav-back`,
 * `useEdgeSwipeBack`). A back label is never cut: when the slot beside a long title is too narrow
 * for the whole word, the button shows its chevron alone and keeps the label as its accessible
 * name.
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
  const buttonRef = useRef<HTMLButtonElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const [fits, setFits] = useState(true);

  useLayoutEffect(() => {
    if (!chevron) return;
    const button = buttonRef.current;
    const text = textRef.current;
    const header = button?.closest('header');
    if (!button || !text || !header) return;
    const title = header.querySelector<HTMLElement>('[data-nav-title]');
    const measure = () => {
      // The label's natural width (scrollWidth holds it even while it is folded to 0)
      const chrome = button.offsetWidth - text.offsetWidth;
      const style = getComputedStyle(header);
      const inner =
        header.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      // The title is centred and keeps its whole width; the back label gets what is left of the
      // left half beside it (and the gap), at most the button's own maximum
      const titleHalf = title ? title.scrollWidth / 2 : 0;
      const room = inner / 2 - titleHalf - NAV_GAP;
      setFits(text.scrollWidth + chrome <= Math.min(NAV_BACK_MAX, room));
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(header);
    if (title) observer.observe(title);
    return () => observer.disconnect();
  }, [chevron, label]);

  const folded = chevron && !fits;
  return (
    <button
      ref={buttonRef}
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={folded ? label : undefined}
      data-nav-back={chevron || undefined}
      className={cn(
        'inline-flex h-11 min-w-0 max-w-[120px] items-center gap-0.5 rounded-[12px] text-m-row text-ink [-webkit-touch-callout:none] active:bg-well-2 disabled:pointer-events-none disabled:opacity-45',
        chevron ? 'pl-1 pr-2' : 'px-3',
        folded && 'min-w-11'
      )}
    >
      {chevron && <ChevronLeft className="size-6 shrink-0" strokeWidth={1.75} aria-hidden />}
      <span
        ref={textRef}
        aria-hidden={folded || undefined}
        className={cn(chevron ? 'whitespace-nowrap' : 'truncate', folded && 'w-0 overflow-hidden')}
      >
        {label}
      </span>
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
