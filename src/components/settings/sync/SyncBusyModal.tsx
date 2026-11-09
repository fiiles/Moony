import { useIsMutating } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { REPLACE_DEVICE_MUTATION_KEY } from '@/hooks/use-device-sync-mutations';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

/**
 * Covers the whole app while a repair or re-sync replaces this device's data, so nothing is edited
 * mid-replace. It has no close button and ignores Escape and outside clicks: it goes away only
 * when the mutation settles.
 */
export function SyncBusyModal() {
  const { t } = useTranslation('deviceSync');
  const running = useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY }) > 0;
  if (!running) return null;
  const refuse = (event: Event) => event.preventDefault();
  return (
    <Dialog open>
      <DialogContent
        hideCloseButton
        className="max-w-[420px]"
        onEscapeKeyDown={refuse}
        onPointerDownOutside={refuse}
        onInteractOutside={refuse}
      >
        <DialogHeader>
          <DialogTitle>{t('busy.title')}</DialogTitle>
          <DialogDescription>{t('busy.description')}</DialogDescription>
        </DialogHeader>
      </DialogContent>
    </Dialog>
  );
}
