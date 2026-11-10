import { useIsMutating } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { REPLACE_DEVICE_MUTATION_KEY } from '@/hooks/use-device-sync-mutations';

/**
 * Covers the whole phone shell while a repair, a re-sync or a join replaces this device's data
 * (the phone's counterpart of the desktop's `SyncBusyModal`), so nothing is edited mid-replace. It
 * takes every touch and goes away only when the mutation settles; the sheet that started it has
 * closed by then.
 */
export function SyncBusyCover() {
  const { t } = useTranslation('deviceSync');
  const running = useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY }) > 0;
  if (!running) return null;
  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-busy="true"
      aria-labelledby="sync-busy-title"
      aria-describedby="sync-busy-description"
      className="absolute inset-0 z-[70] flex items-center justify-center bg-[color:var(--scrim)] px-6"
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
