import { useState, type CSSProperties, type ReactNode } from 'react';
import { Link } from 'wouter';
import { ChevronRight, type LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useSwipeActions } from '@/mobile/hooks/use-swipe-actions';
import { backState } from '@/mobile/lib/back-label';
import { SWIPE_MAX } from '@/mobile/lib/swipe';
import { RowMenu } from './RowMenu';

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

/**
 * The context menu of a row (README §13, Gestures): the long press opens it, its `···` trigger sits
 * at the row's end outside the part that slides.
 */
export interface EntityRowMenu {
  /** The trigger's full accessible name, worded and translated ("Další akce: Albert"). */
  label: string;
  /** `RowMenuItem`s: every action of the row, its swipes included. */
  children: ReactNode;
  /**
   * The `···` is visible: the menu holds an action that is neither on the row nor behind its tap.
   * The row then keeps 48 pt at its end for it.
   */
  shown?: boolean;
  /** A menu whose opening the caller owns (a tap that opens it, one menu open in a list). */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
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
  /** The surface the row lies on: `well` inside an action card (Údržba), `paper` in a list. */
  surface?: 'paper' | 'well';
  /** The whole row is the tap target: a link … */
  href?: string;
  /**
   * With `href` to a screen of another tab: the label of the screen the row is on, which the
   * target's back button then shows (`backState`), as back returns here.
   */
  from?: string;
  /** … or an action. Without either the row is not interactive. */
  onClick?: () => void;
  /** With `onClick`, a row that toggles (a filter): its pressed state (`aria-pressed`). */
  pressed?: boolean;
  /** A row whose action is not available (yet): announced so (`aria-disabled`); the caller drops `href` and `onClick`. */
  disabled?: boolean;
  /** Disclosure chevron; shown only on a row without a trailing value or a visible `···` (README §13). */
  chevron?: boolean;
  /** Swipe left (README §13, Gestures): Kategorie on a payment, else taking the row out of the list. */
  onSwipeLeft?: () => void;
  swipeLeftLabel?: string;
  swipeLeftIcon?: LucideIcon;
  /** The left swipe deletes (through its confirm sheet): its panel is `loss`, not dark material. */
  swipeLeftDestructive?: boolean;
  /** Swipe right: look closer or later (Podrobnosti, Upravit, Připomenout), never destructive. */
  onSwipeRight?: () => void;
  swipeRightLabel?: string;
  swipeRightIcon?: LucideIcon;
  /** The row's menu: a long press opens it. */
  menu?: EntityRowMenu;
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
 * width: the value and the trailing sub keep their whole width (an amount or a date is never cut,
 * audit C1), the title and the sub take the rest and end with an ellipsis. 60 pt, 52 single-line. Swipe actions uncover a 98 pt panel on either side; the long
 * press opens the row's `menu`, whose `···` follows the row as it slides (README §13, Gestures). A
 * `bar` (the Cashflow categories' progress) is a third line in the main column; `muted` dims a row
 * that has been dealt with.
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
  surface = 'paper',
  href,
  from,
  onClick,
  pressed,
  disabled = false,
  chevron = false,
  onSwipeLeft,
  swipeLeftLabel,
  swipeLeftIcon,
  swipeLeftDestructive = false,
  onSwipeRight,
  swipeRightLabel,
  swipeRightIcon,
  menu,
  below,
}: EntityRowProps) {
  const [ownMenuOpen, setOwnMenuOpen] = useState(false);
  const menuOpen = menu?.open ?? ownMenuOpen;
  const setMenuOpen = menu?.onOpenChange ?? setOwnMenuOpen;
  const onLongPress = menu ? () => setMenuOpen(true) : undefined;
  const swipe = useSwipeActions({ onSwipeLeft, onSwipeRight, onLongPress });
  const gestures = onSwipeLeft !== undefined || onSwipeRight !== undefined || !!menu;
  const hasValue = value !== undefined && value !== null;
  // A visible `···` is the row's trailing mark: no chevron beside it
  const showChevron = chevron && !hasValue && !menu?.shown;
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
                // An amount is never cut (C1): the title gives way; `max-w-full` only guards the row
                'ml-auto max-w-full flex-none truncate whitespace-nowrap text-m-row num',
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
              <span className="ml-auto max-w-full flex-none truncate whitespace-nowrap text-m-caption text-ink-4 num">
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
    'relative z-[1] grid w-full items-center gap-3 border-0 bg-inherit pl-3.5 text-left [-webkit-touch-callout:none]',
    // A visible `···` keeps its 44 pt (and 4 pt of edge) at the row's end
    menu?.shown ? 'pr-12' : 'pr-3.5',
    single ? 'min-h-[52px] py-2' : 'min-h-[60px] py-2.5',
    (href || onClick) && (surface === 'well' ? 'active:bg-well-2' : 'active:bg-well'),
    gestures && 'select-none',
    !swipe.dragging && 'transition-transform duration-base ease-moony'
  );
  const rowStyle: CSSProperties = {
    gridTemplateColumns: columns,
    transform: swipe.offset ? `translateX(${swipe.offset}px)` : undefined,
    touchAction: gestures ? 'pan-y' : undefined,
  };
  const gestureProps = gestures ? swipe.bind : {};
  // The trigger rides with the row: a visible `···` slides away with it instead of sitting on the
  // uncovered panel
  const slide = cn(
    'absolute inset-y-0 right-0 z-[3]',
    !swipe.dragging && 'transition-transform duration-base ease-moony'
  );

  // The panels, the row and its menu: under a `below` block they are boxed in a block of their own,
  // so that the panels cover the row's height and the menu's anchor sits at the row's middle.
  const swipeable = (
    <>
      {/* Under the row, uncovered as it slides away from them. */}
      {onSwipeRight && <SwipePanel side="left" label={swipeRightLabel} icon={swipeRightIcon} />}
      {onSwipeLeft && (
        <SwipePanel
          side="right"
          label={swipeLeftLabel}
          icon={swipeLeftIcon}
          destructive={swipeLeftDestructive}
        />
      )}
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
          aria-pressed={pressed}
          className={rowClass}
          style={rowStyle}
          {...gestureProps}
        >
          {content}
        </button>
      ) : (
        <div
          className={rowClass}
          style={rowStyle}
          aria-disabled={disabled || undefined}
          {...gestureProps}
        >
          {content}
        </div>
      )}
      {menu && (
        <span className={slide} style={{ transform: rowStyle.transform }}>
          <RowMenu open={menuOpen} onOpenChange={setMenuOpen} label={menu.label} shown={menu.shown}>
            {menu.children}
          </RowMenu>
        </span>
      )}
    </>
  );

  return (
    <li
      id={id}
      className={cn(
        "relative overflow-hidden before:pointer-events-none before:absolute before:right-0 before:top-0 before:z-[2] before:left-[var(--m-row-inset)] before:border-t before:border-line-soft before:content-[''] first:before:hidden",
        surface === 'well' ? 'bg-well' : 'bg-paper',
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
 * The panel a swipe uncovers (prototype `.row__action`): dark material, icon over a 11 / 650 label;
 * `loss` for a delete (README §13, Gestures), which then asks in its confirm sheet.
 * The label is a fixed 11 px (`text-caption`, the desktop role, not a `text-m-*` one): the panel is
 * as tall as its row, and a label that grew with the text size clipped at 130 % ("Připomenout za
 * týden", "In einer Woche erinnern" take three or four lines). The same actions are in the row's
 * long-press menu, which does scale.
 */
function SwipePanel({
  side,
  label,
  icon: Icon,
  destructive = false,
}: {
  side: 'left' | 'right';
  label?: string;
  icon?: LucideIcon;
  destructive?: boolean;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        'absolute inset-y-0 flex flex-col items-center justify-center gap-1 hyphens-auto break-words px-1 text-center text-caption font-650 leading-tight text-ink-inverse',
        destructive ? 'bg-loss' : 'bg-dark-grad',
        side === 'left' ? 'left-0' : 'right-0'
      )}
      style={{ width: SWIPE_MAX }}
    >
      {Icon && <Icon className="size-[18px]" strokeWidth={1.75} />}
      {label}
    </span>
  );
}
