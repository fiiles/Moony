import { useEffect, useRef } from 'react';
import { useIsMutating } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { REPLACE_DEVICE_MUTATION_KEY } from '@/hooks/use-device-sync-mutations';
import { useHoldBack } from '@/mobile/hooks/use-hold-back';

/**
 * Covers the whole phone shell while a repair, a re-sync or a join replaces this device's data
 * (the phone's counterpart of the desktop's `SyncBusyModal`), so nothing is edited mid-replace. It
 * takes every touch, holds back navigation (the edge swipe and Android's back, `useHoldBack`) and
 * keeps the focus, and goes away only when the mutation settles. The sheet that started the replace
 * is closed while it runs (a join's sheet comes back with a failure), so no sheet, and no focus
 * trap of one, is left open under the cover.
 */
export function SyncBusyCover() {
  const { t } = useTranslation('deviceSync');
  const running = useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY }) > 0;
  const coverRef = useRef<HTMLDivElement>(null);
  useHoldBack(running);

  // The focus moves onto the cover and stays there: anything focused under it goes back to it
  useEffect(() => {
    const cover = coverRef.current;
    if (!running || !cover) return;
    cover.focus();
    const keep = (event: FocusEvent) => {
      if (!cover.contains(event.target as Node | null)) cover.focus();
    };
    document.addEventListener('focusin', keep);
    return () => document.removeEventListener('focusin', keep);
  }, [running]);

  if (!running) return null;
  return (
    <div
      ref={coverRef}
      tabIndex={-1}
      role="alertdialog"
      aria-modal="true"
      aria-busy="true"
      aria-labelledby="sync-busy-title"
      aria-describedby="sync-busy-description"
      className="absolute inset-0 z-[70] flex items-center justify-center bg-[color:var(--scrim)] px-6 outline-none"
    >
      <div className="w-full max-w-[320px] rounded-r5 bg-paper px-5 py-6 text-center shadow-modal">
        <RefreshCw
          aria-hidden
          className="mx-auto mb-3 size-6 animate-spin text-ink-3 motion-reduce:animate-none"
          strokeWidth={1.75}
        />
        <p id="sync-busy-title" className="m-0 text-m-title text-ink">
          {t('busy.title')}
        </p>
        <p id="sync-busy-description" className="mb-0 mt-1.5 text-m-lead text-ink-3">
          {t('phone.busy')}
        </p>
      </div>
    </div>
  );
}
