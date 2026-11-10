import { useEffect, useLayoutEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useHoldBack } from '@/mobile/hooks/use-hold-back';
import { cancelQrScan } from '@/mobile/lib/qr-scanner';

interface ScanOverlayProps {
  /** Under the title: what to point the camera at (differs when adding a device). */
  hint: string;
  /**
   * Developer tools only (`status.devTools`): "Paste a pairing code — developer tool", for the
   * simulator, which has no camera. Left out in release builds, which say nothing about pasting.
   */
  onDevPaste?: () => void;
}

/** The 48 pt buttons of the overlay's card. */
const BUTTON =
  'h-auto min-h-12 w-full whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * What is drawn over the camera while a QR code is scanned (`scanQrCode`, windowed): the page is
 * made transparent and the shell hidden (`data-scanning` on `<html>`, `index.css`), and this layer,
 * portalled to `<body>`, darkens all but a square frame and holds a card with the title, the hint
 * and Cancel. Cancel, Android back, the app going to the background and the overlay going away all
 * cancel the scan; the scan's own promise then answers `cancelled` to whoever started it.
 */
export function ScanOverlay({ hint, onDevPaste }: ScanOverlayProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const dialogRef = useRef<HTMLDivElement>(null);
  useHoldBack(true, () => void cancelQrScan());

  useLayoutEffect(() => {
    const root = document.documentElement;
    root.setAttribute('data-scanning', '');
    return () => root.removeAttribute('data-scanning');
  }, []);

  useEffect(() => {
    dialogRef.current?.focus();
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') void cancelQrScan();
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      document.removeEventListener('visibilitychange', onVisibility);
      void cancelQrScan();
    };
  }, []);

  return createPortal(
    <div
      ref={dialogRef}
      tabIndex={-1}
      role="dialog"
      aria-modal="true"
      aria-labelledby="scan-overlay-title"
      aria-describedby="scan-overlay-hint"
      className="fixed inset-0 z-[80] flex flex-col overflow-hidden outline-none"
    >
      {/* The frame: everything around it is dimmed by its own shadow */}
      <div className="flex flex-1 items-center justify-center pt-[var(--safe-top)]">
        <div
          aria-hidden
          className="size-[min(68vw,280px)] rounded-r4 border-2 border-paper shadow-[0_0_0_100vmax_var(--scrim)]"
        />
      </div>
      <div className="relative z-[1] mx-3 mb-[calc(12px+var(--safe-bottom))] rounded-r5 bg-paper px-5 pb-4 pt-5 shadow-modal">
        <p id="scan-overlay-title" className="m-0 text-m-title text-ink">
          {t('phone.scanTitle')}
        </p>
        <p id="scan-overlay-hint" className="mb-0 mt-1 text-m-lead text-ink-3">
          {hint}
        </p>
        <div className="mt-4 grid gap-2">
          {onDevPaste && (
            <Button variant="outline" className={BUTTON} onClick={onDevPaste}>
              {t('phone.devPasteTitle')}
            </Button>
          )}
          <Button variant="ghost" className={BUTTON} onClick={() => void cancelQrScan()}>
            {tc('buttons.cancel')}
          </Button>
        </div>
      </div>
    </div>,
    document.body
  );
}
