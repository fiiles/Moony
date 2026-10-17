import type { ComponentPropsWithoutRef, ReactNode } from 'react';
import { Ellipsis } from 'lucide-react';
import { cn } from '@/lib/utils';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

interface RowMenuProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The accessible name of the trigger: what the row is ("Albert", "Potraviny"). */
  label: string;
  /** `RowMenuItem`s. */
  children: ReactNode;
}

/**
 * The context menu of an entity row (README §13, the agenda row's pattern): a long press on the
 * row sets `open`; for a screen reader or a keyboard the 44 pt `···` trigger beside the row is
 * the way in, hidden until it has focus. It is the row's `accessory`, so it sits outside the part
 * that slides. The menu opens under the row's right edge, 220 pt wide at least.
 */
export function RowMenu({ open, onOpenChange, label, children }: RowMenuProps) {
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      {/* The menu's anchor, and its way in for a screen reader or a keyboard: hidden until focused. */}
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className="sr-only right-2 top-1/2 z-[3] focus-visible:not-sr-only focus-visible:absolute focus-visible:right-2 focus-visible:top-1/2 focus-visible:inline-flex focus-visible:size-11 focus-visible:-translate-y-1/2 focus-visible:items-center focus-visible:justify-center focus-visible:rounded-[12px] focus-visible:bg-paper focus-visible:text-ink focus-visible:shadow-focus"
        >
          <Ellipsis aria-hidden className="size-5" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[220px]">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** An item of a `RowMenu`: 44 pt tall, the body size, an 18 pt icon. */
export function RowMenuItem({
  className,
  ...props
}: ComponentPropsWithoutRef<typeof DropdownMenuItem>) {
  return (
    <DropdownMenuItem
      className={cn('min-h-11 text-m-body [&_svg]:size-[18px]', className)}
      {...props}
    />
  );
}
