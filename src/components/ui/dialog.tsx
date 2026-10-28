import * as React from 'react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import { X } from 'lucide-react';

import { cn } from '@/lib/utils';

/**
 * Modal (design system §6, `.modal`): 560 px, radius 17, E5 shadow over a
 * 63 % ink scrim with 4 px blur, 180 ms open. Head = title + one sentence,
 * body without cards, foot on `well` with the text "Zrušit" left and the
 * primary action right, named like the title. Esc closes (Radix), the first
 * field receives focus (Radix autofocus).
 */
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
    { className, children, hideCloseButton, onPointerDownOutside, onCloseAutoFocus, ...props },
    ref
  ) => {
    const opener = React.useRef<HTMLElement | null>(null);
    return (
      <DialogPortal>
        <DialogOverlay />
        <DialogPrimitive.Content
          ref={ref}
          className={cn(
            'fixed left-1/2 top-1/2 z-50 grid w-full max-w-[560px] -translate-x-1/2 -translate-y-1/2 gap-4 overflow-hidden rounded-r5 border border-line-strong bg-paper px-[27px] py-[23px] text-ink shadow-modal data-[state=open]:animate-modal-in data-[state=closed]:animate-modal-out',
            className
          )}
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
          {children}
          {!hideCloseButton && (
            <DialogPrimitive.Close className="absolute right-[27px] top-[25px] grid size-7 place-items-center rounded-full bg-well text-ink-3 transition-colors duration-fast hover:bg-well-2 hover:text-ink focus-visible:outline-none focus-visible:shadow-focus disabled:pointer-events-none">
              <X className="size-[15px]" />
              <span className="sr-only">Close</span>
            </DialogPrimitive.Close>
          )}
        </DialogPrimitive.Content>
      </DialogPortal>
    );
  }
);
DialogContent.displayName = DialogPrimitive.Content.displayName;

/** `.modal__head`: spans the modal width, hairline below. */
const DialogHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      '-mx-[27px] -mt-[23px] flex flex-col border-b border-line-soft px-[27px] pb-5 pr-16 pt-[25px] text-left',
      className
    )}
    {...props}
  />
);
DialogHeader.displayName = 'DialogHeader';

/** `.modal__foot`: on `well`, the first of several actions (Zrušit) sits left, the rest right. */
const DialogFooter = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      '-mx-[27px] -mb-[23px] flex items-center justify-end gap-2 border-t border-line-soft bg-well px-[27px] py-4 [&>:first-child:not(:only-child)]:mr-auto',
      className
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
    className={cn('m-0 text-modal text-ink', className)}
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
    className={cn('mt-1.5 text-table text-ink-3', className)}
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
