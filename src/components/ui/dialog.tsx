import * as React from 'react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import { X } from 'lucide-react';

import { useSheetDrag } from '@/hooks/use-sheet-drag';
import { isMobileShell } from '@/lib/platform';
import { cn } from '@/lib/utils';

/**
 * Modal (design system §6, `.modal`): 560 px, radius 17, E5 shadow over a
 * 63 % ink scrim with 4 px blur, 180 ms open. Head = title + one sentence,
 * body without cards, foot on `well` with the text "Zrušit" left and the
 * primary action right, named like the title. Esc closes (Radix), the first
 * field receives focus (Radix autofocus).
 *
 * In the phone shell every dialog is a bottom sheet instead (design system §13,
 * the phone's `Sheet`): the same drag handle, title, scrolling body and footer
 * on `well`, closed by the scrim, Escape, its buttons or a drag down, and no
 * field is focused on opening (the keyboard would cover the sheet before it
 * has been read).
 */
const PHONE = isMobileShell(import.meta.env);
const Dialog = DialogPrimitive.Root;

const DialogTrigger = DialogPrimitive.Trigger;

const DialogPortal = DialogPrimitive.Portal;

const DialogClose = DialogPrimitive.Close;

const DialogOverlay = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    className={cn(
      'fixed inset-0 z-50 bg-[color:var(--scrim)] backdrop-blur-[4px] duration-base data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
      // The phone sheet's scrim: inside `#root`, which the keyboard shortens, and without the blur
      PHONE && 'absolute backdrop-blur-none motion-reduce:animate-none',
      className
    )}
    {...props}
  />
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

interface DialogContentProps extends React.ComponentPropsWithoutRef<
  typeof DialogPrimitive.Content
> {
  hideCloseButton?: boolean;
}

/** True when a pointer-down outside the dialog started on a toast (sonner's toaster). */
function startedOnToast(event: { detail: { originalEvent: Event } }): boolean {
  const target = event.detail.originalEvent.target;
  return target instanceof Element && target.closest('[data-sonner-toaster]') !== null;
}

/**
 * The element focused when the dialog opens. A dialog opened from a menu item gets the menu's
 * trigger: the item leaves the page with the menu.
 */
function focusedOpener(): HTMLElement | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return null;
  const menu = active.closest('[role="menu"]');
  const trigger = menu?.id
    ? document.querySelector(`[aria-controls="${CSS.escape(menu.id)}"]`)
    : null;
  return trigger instanceof HTMLElement ? trigger : active;
}

/**
 * The phone sheet opens with no field focused: a field that took focus with `autoFocus` while the
 * sheet mounted (before Radix would have focused anything) hands it to the sheet itself, so the
 * trap and the screen reader start there and no keyboard opens.
 */
function PhoneNoAutoFocus({ sheet }: { sheet: HTMLElement | null }) {
  React.useLayoutEffect(() => {
    const active = document.activeElement;
    if (sheet && active instanceof HTMLElement && active !== sheet && sheet.contains(active)) {
      sheet.focus({ preventScroll: true });
    }
  }, [sheet]);
  return null;
}

/** The phone sheet's drag handle (`Sheet`): 36 × 5 on `well-3`, above the head. */
function PhoneGrip() {
  return (
    <div
      aria-hidden
      className="sticky top-0 z-[2] -mx-4 -mb-4 flex-none bg-paper pb-1 pt-2 [grid-area:auto]"
    >
      <div className="mx-auto h-[5px] w-9 rounded-[3px] bg-well-3" />
    </div>
  );
}

/**
 * Remembers the element focused when the dialog opens. It mounts with every opening (the
 * content unmounts while closed; the `DialogContent` wrapper does not) and reads focus while
 * rendering, before an `autoFocus` field or the dialog's focus trap moves it.
 */
function OpenerProbe({ target }: { target: React.MutableRefObject<HTMLElement | null> }) {
  const [opener] = React.useState(focusedOpener);
  React.useLayoutEffect(() => {
    target.current = opener;
  }, [opener, target]);
  return null;
}

/**
 * Content padding is 23 × 27 px; DialogHeader/DialogFooter stretch to the edges.
 * A click on a toast (its "Vrátit zpět", for one) is not a click outside the modal: the
 * toast handles it and the dialog stays open.
 * Closing returns focus to the element that opened the dialog: most dialogs are opened from
 * state (a row button, a menu item) rather than a `DialogTrigger`, and Radix alone would then
 * leave focus on the page body.
 */
const DialogContent = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Content>,
  DialogContentProps
>(
  (
    {
      className,
      children,
      hideCloseButton,
      onPointerDownOutside,
      onCloseAutoFocus,
      onOpenAutoFocus,
      ...props
    },
    ref
  ) => {
    const opener = React.useRef<HTMLElement | null>(null);
    const [sheet, setSheet] = React.useState<HTMLDivElement | null>(null);
    const closeRef = React.useRef<HTMLButtonElement>(null);
    // A drag down closes the phone sheet as its X would (the X is hidden there); not while the
    // dialog withholds its X (it is busy)
    useSheetDrag(sheet, () => closeRef.current?.click(), PHONE && !hideCloseButton);
    const setRefs = React.useCallback(
      (node: HTMLDivElement | null) => {
        setSheet(node);
        if (typeof ref === 'function') ref(node);
        else if (ref) ref.current = node;
      },
      [ref]
    );
    return (
      <DialogPortal container={PHONE ? document.getElementById('root') : undefined}>
        <DialogOverlay />
        <DialogPrimitive.Content
          ref={setRefs}
          className={cn(
            'fixed left-1/2 top-1/2 z-50 grid w-full max-w-[560px] -translate-x-1/2 -translate-y-1/2 gap-4 overflow-hidden rounded-r5 border border-line-strong bg-paper px-[27px] py-[23px] text-ink shadow-modal data-[state=open]:animate-modal-in data-[state=closed]:animate-modal-out',
            className,
            // The phone sheet (`Sheet`): pinned to the bottom of `#root`, as tall as its content up
            // to 93 %, scrolling as one with its head and footer held at the edges. Last, so it
            // wins over a desktop width, height or centring the dialog asks for.
            PHONE &&
              'absolute inset-x-0 bottom-0 left-0 top-auto w-full max-w-none sm:max-w-none md:max-w-none lg:max-w-none translate-x-0 translate-y-0 max-h-[93%] overflow-y-auto overscroll-contain rounded-b-none rounded-t-r5 border-x-0 border-b-0 border-line px-4 pb-0 pt-0 outline-none data-[state=open]:animate-sheet-in data-[state=closed]:animate-sheet-out motion-reduce:animate-none'
          )}
          // `index.css` lines the desktop form up with the phone's fields inside it
          data-phone-sheet={PHONE ? '' : undefined}
          onOpenAutoFocus={(event) => {
            onOpenAutoFocus?.(event);
            if (!PHONE || event.defaultPrevented) return;
            // Focus the sheet itself: the trap and the screen reader start there, no keyboard opens
            event.preventDefault();
            (event.currentTarget as HTMLElement).focus();
          }}
          onPointerDownOutside={(event) => {
            if (startedOnToast(event)) event.preventDefault();
            onPointerDownOutside?.(event);
          }}
          onCloseAutoFocus={(event) => {
            onCloseAutoFocus?.(event);
            if (event.defaultPrevented) return;
            const element = opener.current;
            if (element && element !== document.body && element.isConnected) {
              event.preventDefault();
              element.focus({ preventScroll: true });
            }
          }}
          {...props}
        >
          <OpenerProbe target={opener} />
          {PHONE && <PhoneNoAutoFocus sheet={sheet} />}
          {PHONE && <PhoneGrip />}
          {children}
          {PHONE ? (
            // No X on the phone sheet (the scrim, Zrušit and a drag close it); the drag uses this
            <DialogPrimitive.Close
              ref={closeRef}
              tabIndex={-1}
              aria-hidden
              disabled={hideCloseButton}
              className="hidden"
            />
          ) : (
            !hideCloseButton && (
              <DialogPrimitive.Close className="absolute right-[27px] top-[25px] grid size-7 place-items-center rounded-full bg-well text-ink-3 transition-colors duration-fast hover:bg-well-2 hover:text-ink focus-visible:outline-none focus-visible:shadow-focus disabled:pointer-events-none">
                <X className="size-[15px]" />
                <span className="sr-only">Close</span>
              </DialogPrimitive.Close>
            )
          )}
        </DialogPrimitive.Content>
      </DialogPortal>
    );
  }
);
DialogContent.displayName = DialogPrimitive.Content.displayName;

/**
 * `.modal__head`: spans the modal width, hairline below. On the phone sheet the head of `Sheet`:
 * under the drag handle, held at the top while the body scrolls.
 */
const DialogHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      '-mx-[27px] -mt-[23px] flex flex-col border-b border-line-soft px-[27px] pb-5 pr-16 pt-[25px] text-left',
      className,
      PHONE && 'sticky top-[17px] z-[1] -mx-4 mt-0 border-b-0 bg-paper px-5 pb-1 pr-5 pt-3'
    )}
    {...props}
  />
);
DialogHeader.displayName = 'DialogHeader';

/**
 * `.modal__foot`: on `well`, the first of several actions (Zrušit) sits left, the rest right. On
 * the phone sheet the footer of `Sheet`: held at the bottom over the safe area, its buttons 48 pt
 * and the last one (the primary action) taking the rest of the row.
 */
const DialogFooter = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      '-mx-[27px] -mb-[23px] flex items-center justify-end gap-2 border-t border-line-soft bg-well px-[27px] py-4 [&>:first-child:not(:only-child)]:mr-auto',
      className,
      PHONE &&
        'sticky bottom-0 z-[1] -mx-4 mb-0 mt-auto gap-2.5 border-line px-4 pb-[calc(12px+var(--safe-bottom))] pt-3 [&>:first-child:not(:only-child)]:mr-0 [&>:last-child]:flex-1 [&>button]:h-auto [&>button]:min-h-12 [&>button]:whitespace-normal [&>button]:rounded-[10px] [&>button]:text-m-row'
    )}
    {...props}
  />
);
DialogFooter.displayName = 'DialogFooter';

const DialogTitle = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn(
      'm-0 text-modal text-ink',
      className,
      PHONE && 'hyphens-auto break-words text-m-title'
    )}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn('mt-1.5 text-table text-ink-3', className, PHONE && 'mt-[3px] text-m-lead')}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogPortal,
  DialogOverlay,
  DialogClose,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
};
