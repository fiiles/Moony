import { useState, type ReactNode } from 'react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { sheetHeightClass } from '@/lib/sheet-height';
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
  /** A tall sheet (pickers): 95 % of the screen instead of 93 %, always below the top safe area. */
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
  /**
   * The sheet holds input that closing would lose (`formChanged`): the scrim, Escape, a drag down
   * and back ask "Zahodit změny?" first. The sheet's own buttons ("Zrušit") close without asking.
   * Left out, the sheet never asks (pickers, confirmations).
   */
  dirty?: boolean;
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
 * down (`useSheetDrag`). A `dirty` sheet asks before any of those but its own buttons.
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
  dirty,
}: SheetProps) {
  // "Zahodit změny?" is up, over the sheet
  const [asking, setAsking] = useState(false);
  if (asking && !open) setAsking(false);
  /** A close the person did not ask for by a button: a dirty sheet asks first and stays (false). */
  const requestClose = (): boolean => {
    if (dirty === true) {
      setAsking(true);
      return false;
    }
    onOpenChange(false);
    return true;
  };
  useSheetHistory(open, requestClose);
  const [content, setContent] = useState<HTMLDivElement | null>(null);
  // A sheet that stays open after the drag comes back up (`useSheetDrag`)
  useSheetDrag(content, () => void requestClose());

  return (
    <>
      <DialogPrimitive.Root
        open={open}
        onOpenChange={(next) => (next ? onOpenChange(true) : void requestClose())}
      >
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
              sheetHeightClass({ fill, full })
            )}
          >
            <div
              aria-hidden
              className="mx-auto mt-2 h-[5px] w-9 flex-none rounded-[3px] bg-well-3"
            />
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
      {/* Only a sheet that opted in (a form) has the question; it has no question of its own */}
      {dirty !== undefined && (
        <DiscardSheet
          open={open && asking}
          onKeep={() => setAsking(false)}
          onDiscard={() => {
            setAsking(false);
            onOpenChange(false);
          }}
        />
      )}
    </>
  );
}

/**
 * "Zahodit změny?" over a sheet with unsaved input (audit C5): "Pokračovat v úpravách" returns to
 * it, the destructive "Zahodit" closes both. Itself a plain sheet: back or the scrim keep editing.
 */
function DiscardSheet({
  open,
  onKeep,
  onDiscard,
}: {
  open: boolean;
  onKeep: () => void;
  onDiscard: () => void;
}) {
  const { t } = useTranslation('mobile');
  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next) onKeep();
      }}
      title={t('sheet.discard.title')}
      description={t('sheet.discard.description')}
      footer={
        <>
          <Button
            variant="ghost"
            className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row"
            onClick={onKeep}
          >
            {t('sheet.discard.keep')}
          </Button>
          <Button
            variant="destructive"
            className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
            onClick={onDiscard}
          >
            {t('sheet.discard.confirm')}
          </Button>
        </>
      }
    >
      {null}
    </Sheet>
  );
}
