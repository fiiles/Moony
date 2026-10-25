import type { CSSProperties, ReactNode } from 'react';
import { Link } from 'wouter';
import { ChevronRight, type LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useSwipeActions } from '@/mobile/hooks/use-swipe-actions';
import { backState } from '@/mobile/lib/back-label';
import { SWIPE_MAX } from '@/mobile/lib/swipe';

/** Width of the 36 pt chip column (`.m-row`). */
const CHIP = 36;

/** List of entity rows on paper (`.m-rows`): radius 15, `sh-1`, hairline dividers. */
export function EntityRows({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <ul
      className={cn(
        'm-0 mb-3.5 list-none overflow-hidden rounded-r4 border border-line bg-paper p-0 shadow-e1',
        className
      )}
    >
      {children}
    </ul>
  );
}

type ChipTone = 'default' | 'dark' | 'gain' | 'loss';

const CHIP_TONES: Record<ChipTone, string> = {
  default: 'bg-well-2 text-ink-2',
  dark: 'bg-dark-grad text-ink-inverse shadow-dark',
  gain: 'bg-gain-soft text-gain',
  // The row of a destructive action (Nastavení's "Smazat všechna data"), never a change
  loss: 'bg-loss-soft text-loss',
};

/** The 36 pt chip (`.m-row__chip`): initials or a 16 pt icon in a tinted circle (or rounded square). */
export function RowChip({
  tone = 'default',
  square = false,
  children,
}: {
  tone?: ChipTone;
  square?: boolean;
  children: ReactNode;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        'flex size-9 flex-none items-center justify-center text-m-caption font-bold tracking-[-0.01em] [&_svg]:size-4',
        square ? 'rounded-[10px]' : 'rounded-full',
        CHIP_TONES[tone]
      )}
    >
      {children}
    </span>
  );
}

interface EntityRowProps {
  /** The `li`'s id: the anchor a chart mark scrolls to (`tx-…`). */
  id?: string;
  /**
   * Merged onto the `li`, which carries the row's background (the row takes it over): a transition
   * and a tint on it fade in and out under the row (the flash of a trade row).
   */
  className?: string;
  /** Leading element: a `RowChip`, or a wider element such as the agenda's date chip. */
  chip?: ReactNode;
  /**
   * Width of `chip` when it is not the 36 pt chip (the dividers inset to the title): a number of
   * points, or a CSS length for a chip that grows with the text size.
   */
  chipWidth?: number | string;
  title: ReactNode;
  sub?: ReactNode;
  /** Trailing value on the title line (tabular). */
  value?: ReactNode;
  /** `gain` for a credit, `loss` for an amount over its limit; never colour alone (the text says it too). */
  valueTone?: 'gain' | 'loss';
  /** Trailing sub on the second line: a change badge or a caption. */
  valueSub?: ReactNode;
  /** A third line under the sub, in the main column (a progress bar); a row with one is never single. */
  bar?: ReactNode;
  /** The row has been dealt with (moved elsewhere): the whole row, its divider included, at half strength. */
  muted?: boolean;
  /** The whole row is the tap target: a link … */
  href?: string;
  /**
   * With `href` to a screen of another tab: the label of the screen the row is on, which the
   * target's back button then shows (`backState`), as back returns here.
   */
  from?: string;
  /** … or an action. Without either the row is not interactive. */
  onClick?: () => void;
  /** Disclosure chevron; shown only on a row without a trailing value (README §13). */
  chevron?: boolean;
  onSwipeLeft?: () => void;
  swipeLeftLabel?: string;
  swipeLeftIcon?: LucideIcon;
  onSwipeRight?: () => void;
  swipeRightLabel?: string;
  swipeRightIcon?: LucideIcon;
  onLongPress?: () => void;
  /** Rendered beside the row, outside the part that slides (a menu and its trigger). */
  accessory?: ReactNode;
  /**
   * A second block under the row inside the same `li` (the categorize queue's pill row): the
   * row keeps its own 60 pt, its swipe panels and its menu stay beside it, and this block does
   * not slide.
   */
  below?: ReactNode;
}

/**
 * The entity row (spec §10.3, design system README §13, `.m-row`): a chip, then two lines — the
 * title with the trailing value, the sub with the trailing sub. Each line shares out its own
 * width: the value keeps its width (up to 60 %), the title takes the rest and ends with an
 * ellipsis. 60 pt, 52 single-line. Swipe actions uncover a 98 pt panel on either side; the long
 * press is the context menu. A `bar` (the Cashflow categories' progress) is a third line in the
 * main column; `muted` dims a row that has been dealt with.
 */
export function EntityRow({
  id,
  className,
  chip,
  chipWidth = CHIP,
  title,
  sub,
  value,
  valueTone,
  valueSub,
  bar,
  muted = false,
  href,
  from,
  onClick,
  chevron = false,
  onSwipeLeft,
  swipeLeftLabel,
  swipeLeftIcon,
  onSwipeRight,
  swipeRightLabel,
  swipeRightIcon,
  onLongPress,
  accessory,
  below,
}: EntityRowProps) {
  const swipe = useSwipeActions({ onSwipeLeft, onSwipeRight, onLongPress });
  const gestures = onSwipeLeft !== undefined || onSwipeRight !== undefined || !!onLongPress;
  const hasValue = value !== undefined && value !== null;
  const showChevron = chevron && !hasValue;
  const hasSubLine = !!sub || !!valueSub;
  const single = !hasSubLine && !bar;
  const hasChip = chip !== undefined;
  const chipColumn = typeof chipWidth === 'number' ? `${chipWidth}px` : chipWidth;
  const columns = [hasChip ? chipColumn : null, 'minmax(0, 1fr)', showChevron ? '16px' : null]
    .filter(Boolean)
    .join(' ');
  // 14 pt padding + chip + 12 pt gap: the divider starts at the title's left edge.
  const inset = hasChip ? `calc(26px + ${chipColumn})` : '14px';

  const content = (
    <>
      {chip}
      <span className="block min-w-0">
        <span className="flex min-w-0 items-baseline gap-2.5">
          <span className="min-w-0 flex-auto truncate text-m-row text-ink">{title}</span>
          {hasValue && (
            <span
              className={cn(
                'ml-auto max-w-[60%] flex-none truncate text-m-row num',
                valueTone === 'gain' ? 'text-gain' : valueTone === 'loss' ? 'text-loss' : 'text-ink'
              )}
            >
              {value}
            </span>
          )}
        </span>
        {hasSubLine && (
          <span className="mt-0.5 flex min-w-0 items-baseline gap-2.5">
            <span className="min-w-0 flex-auto truncate text-m-sub text-ink-4">{sub}</span>
            {valueSub !== undefined && valueSub !== null && (
              <span className="ml-auto max-w-[60%] flex-none truncate text-m-caption text-ink-4 num">
                {valueSub}
              </span>
            )}
          </span>
        )}
        {bar && <span className="mt-1.5 block">{bar}</span>}
      </span>
      {showChevron && (
        <ChevronRight aria-hidden className="size-4 flex-none text-ink-5" strokeWidth={1.75} />
      )}
    </>
  );

  const rowClass = cn(
    'relative z-[1] grid w-full items-center gap-3 border-0 bg-inherit px-3.5 text-left [-webkit-touch-callout:none]',
    single ? 'min-h-[52px] py-2' : 'min-h-[60px] py-2.5',
    (href || onClick) && 'active:bg-well',
    gestures && 'select-none',
    !swipe.dragging && 'transition-transform duration-base ease-moony'
  );
  const rowStyle: CSSProperties = {
    gridTemplateColumns: columns,
    transform: swipe.offset ? `translateX(${swipe.offset}px)` : undefined,
    touchAction: gestures ? 'pan-y' : undefined,
  };
  const gestureProps = gestures ? swipe.bind : {};

  // The panels, the row and its menu: under a `below` block they are boxed in a block of their own,
  // so that the panels cover the row's height and the menu's anchor sits at the row's middle.
  const swipeable = (
    <>
      {/* Under the row, uncovered as it slides away from them. */}
      {onSwipeRight && <SwipePanel side="left" label={swipeRightLabel} icon={swipeRightIcon} />}
      {onSwipeLeft && <SwipePanel side="right" label={swipeLeftLabel} icon={swipeLeftIcon} />}
      {href ? (
        <Link
          href={href}
          state={backState(from)}
          className={rowClass}
          style={rowStyle}
          {...gestureProps}
        >
          {content}
        </Link>
      ) : onClick ? (
        <button
          type="button"
          onClick={onClick}
          className={rowClass}
          style={rowStyle}
          {...gestureProps}
        >
          {content}
        </button>
      ) : (
        <div className={rowClass} style={rowStyle} {...gestureProps}>
          {content}
        </div>
      )}
      {accessory}
    </>
  );

  return (
    <li
      id={id}
      className={cn(
        "relative overflow-hidden bg-paper before:pointer-events-none before:absolute before:right-0 before:top-0 before:z-[2] before:left-[var(--m-row-inset)] before:border-t before:border-line-soft before:content-[''] first:before:hidden",
        muted && 'opacity-50',
        className
      )}
      style={{ '--m-row-inset': inset } as CSSProperties}
    >
      {below ? <div className="relative overflow-hidden bg-inherit">{swipeable}</div> : swipeable}
      {below}
    </li>
  );
}

/**
 * The panel a swipe uncovers (prototype `.row__action`): dark material, icon over a 11 / 650 label.
 * The label is a fixed 11 px (`text-caption`, the desktop role, not a `text-m-*` one): the panel is
 * as tall as its row, and a label that grew with the text size clipped at 130 % ("Připomenout za
 * týden", "In einer Woche erinnern" take three or four lines). The same actions are in the row's
 * long-press menu, which does scale.
 */
function SwipePanel({
  side,
  label,
  icon: Icon,
}: {
  side: 'left' | 'right';
  label?: string;
  icon?: LucideIcon;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        'absolute inset-y-0 flex flex-col items-center justify-center gap-1 hyphens-auto break-words bg-dark-grad px-1 text-center text-caption font-650 leading-tight text-ink-inverse',
        side === 'left' ? 'left-0' : 'right-0'
      )}
      style={{ width: SWIPE_MAX }}
    >
      {Icon && <Icon className="size-[18px]" strokeWidth={1.75} />}
      {label}
    </span>
  );
}
