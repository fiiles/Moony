import { useState, type ReactNode } from 'react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import { cn } from '@/lib/utils';
import { useSheetDrag } from '@/hooks/use-sheet-drag';
import { useSheetHistory } from '@/mobile/hooks/use-sheet-history';

interface SheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  /** The one sentence under the title (13 / 450 `ink-3`). */
  description?: string;
  /** The footer's buttons: the ghost "Zrušit" first, the primary action last (it takes the rest of the row). */
  footer: ReactNode;
  /** A tall sheet (pickers): 95 % of the screen instead of 93 %. */
  full?: boolean;
  /**
   * The body is one box that fills the sheet and scrolls itself (the document viewer,
   * `.m-sheet--fill`): the sheet is as tall as it may be (93 %, or 95 % with `full`) and its body a
   * column that does not scroll, so its one child (`flex-1 min-h-0`) is the only scroller at every
   * text size. Without it the sheet is as tall as its content and the body scrolls.
   */
  fill?: boolean;
  /** The body; it scrolls when the sheet is not tall enough (with `fill`, its child does). */
  children: ReactNode;
}

/** True when a pointer-down outside the sheet started on a toast (its "Vrátit" is not "outside"). */
function startedOnToast(event: { detail: { originalEvent: Event } }): boolean {
  const target = event.detail.originalEvent.target;
  return target instanceof Element && target.closest('[data-sonner-toaster]') !== null;
}

/**
 * Bottom sheet (spec §10.3, design system README §13, `.m-sheet`): paper with a 17 pt top radius, a
 * 36 × 5 drag handle on `well-3`, the 20 / 650 title and one sentence, a body that scrolls and a
 * footer on `well` with a hairline above it. It covers the tab bar. It closes on a tap on the
 * scrim, on Escape, from its own buttons, on Android back (`useSheetHistory`) and when dragged
 * down (`useSheetDrag`).
 *
 * Radix Dialog underneath (focus trap, scroll lock, `aria-modal`), portalled into `#root` so that
 * the sheet shares the box that `--m-vh` shortens while the keyboard is up: the footer stays above
 * the keyboard and the body scrolls. The first field is not focused on opening: the keyboard would
 * cover the sheet before it has been read.
 */
export function Sheet({
  open,
  onOpenChange,
  title,
  description,
  footer,
  full = false,
  fill = false,
  children,
}: SheetProps) {
  useSheetHistory(open, () => onOpenChange(false));
  const [content, setContent] = useState<HTMLDivElement | null>(null);
  useSheetDrag(content, () => onOpenChange(false));

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal container={document.getElementById('root')}>
        <DialogPrimitive.Overlay className="absolute inset-0 z-40 bg-[color:var(--scrim)] duration-base data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 motion-reduce:animate-none" />
        <DialogPrimitive.Content
          ref={setContent}
          // Without a sentence there is nothing to describe the sheet by (Radix would warn)
          {...(description ? {} : { 'aria-describedby': undefined })}
          onOpenAutoFocus={(event) => {
            // Focus the sheet itself: the trap and the screen reader start there, no keyboard opens.
            event.preventDefault();
            (event.currentTarget as HTMLElement).focus();
          }}
          onPointerDownOutside={(event) => {
            if (startedOnToast(event)) event.preventDefault();
          }}
          className={cn(
            'absolute inset-x-0 bottom-0 z-[41] flex flex-col rounded-t-r5 bg-paper text-ink-2 shadow-modal outline-none data-[state=open]:animate-sheet-in data-[state=closed]:animate-sheet-out motion-reduce:animate-none',
            fill ? (full ? 'h-[95%]' : 'h-[93%]') : full ? 'max-h-[95%]' : 'max-h-[93%]'
          )}
        >
          <div aria-hidden className="mx-auto mt-2 h-[5px] w-9 flex-none rounded-[3px] bg-well-3" />
          <div className="flex-none px-5 pb-1 pt-3">
            <DialogPrimitive.Title className="m-0 hyphens-auto break-words text-m-title text-ink">
              {title}
            </DialogPrimitive.Title>
            {description && (
              <DialogPrimitive.Description className="mt-[3px] text-m-lead text-ink-3">
                {description}
              </DialogPrimitive.Description>
            )}
          </div>
          <div
            className={cn(
              'min-h-0 flex-auto px-4 pb-3.5 pt-2',
              fill ? 'flex flex-col overflow-hidden' : 'overflow-y-auto overscroll-contain'
            )}
          >
            {children}
          </div>
          <div className="flex flex-none items-center gap-2.5 border-t border-line bg-well px-4 pb-[calc(12px+var(--safe-bottom))] pt-3">
            {footer}
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
