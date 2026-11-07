import type { ComponentPropsWithoutRef, ReactNode } from 'react';
import { Ellipsis } from 'lucide-react';
import { cn } from '@/lib/utils';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

/**
 * The phone's menu item (README §13): 44 pt tall, the body size, an 18 pt icon. Every menu of the
 * phone uses it — a row's context menu, a nav bar's `···` and `+`, the desktop's milestone items
 * on an agenda row (`itemClassName`).
 */
export const ROW_MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

/** A checkbox or radio item: 44 pt tall, the body size; its indicator keeps the menu's size. */
const ROW_MENU_CHOICE = 'min-h-11 text-m-body';

interface RowMenuProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The full accessible name of the trigger, already worded and translated ("Další akce: Albert"). */
  label: string;
  /**
   * The `···` is visible: the menu holds an action that is neither on the row nor behind its tap
   * (README §13, Gestures). Otherwise the trigger is there for a screen reader or a keyboard only.
   */
  shown?: boolean;
  /** `RowMenuItem`s. */
  children: ReactNode;
}

/** The trigger hidden until it has keyboard or screen-reader focus. */
const HIDDEN_TRIGGER =
  'sr-only right-2 top-1/2 focus-visible:not-sr-only focus-visible:absolute focus-visible:right-2 focus-visible:top-1/2 focus-visible:inline-flex focus-visible:size-11 focus-visible:-translate-y-1/2 focus-visible:items-center focus-visible:justify-center focus-visible:rounded-[12px] focus-visible:bg-paper focus-visible:text-ink focus-visible:shadow-focus';

/** The visible `···`: 44 pt at the row's end, `ink-4`, the icon 20 pt. */
const SHOWN_TRIGGER =
  'absolute right-1 top-1/2 inline-flex size-11 -translate-y-1/2 items-center justify-center rounded-[12px] text-ink-4 [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus';

/**
 * The context menu of an entity row (README §13, Gestures; the agenda row's pattern): a long press
 * on the row sets `open`, and the 44 pt `···` trigger beside the row is the other way in — visible
 * when `shown`, else for a screen reader or a keyboard only, hidden until it has focus. It is the
 * row's own (`EntityRow`'s `menu`), outside the part that slides. The menu opens under the row's
 * right edge, 220 pt wide at least.
 */
export function RowMenu({ open, onOpenChange, label, shown = false, children }: RowMenuProps) {
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      {/* The menu's anchor, and its way in without a long press. */}
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className={cn('z-[3]', shown ? SHOWN_TRIGGER : HIDDEN_TRIGGER)}
        >
          <Ellipsis aria-hidden className="size-5" strokeWidth={1.75} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[220px]">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** An item of a phone menu (`ROW_MENU_ITEM`): a `RowMenu`, a nav bar's `···` or `+`. */
export function RowMenuItem({
  className,
  ...props
}: ComponentPropsWithoutRef<typeof DropdownMenuItem>) {
  return <DropdownMenuItem className={cn(ROW_MENU_ITEM, className)} {...props} />;
}

/** A checkbox item of a phone menu ("Zobrazit splacené"): 44 pt tall, the body size. */
export function RowMenuCheckboxItem({
  className,
  ...props
}: ComponentPropsWithoutRef<typeof DropdownMenuCheckboxItem>) {
  return <DropdownMenuCheckboxItem className={cn(ROW_MENU_CHOICE, className)} {...props} />;
}

/** A radio item of a phone menu (a sort): 44 pt tall, the body size. */
export function RowMenuRadioItem({
  className,
  ...props
}: ComponentPropsWithoutRef<typeof DropdownMenuRadioItem>) {
  return <DropdownMenuRadioItem className={cn(ROW_MENU_CHOICE, className)} {...props} />;
}
