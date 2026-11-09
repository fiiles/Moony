import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';

/** Turning sync off on this device: local data stays, the other devices keep syncing. */
export function TurnOffSyncDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { turnOff } = useDeviceSyncMutations();

  const run = async () => {
    try {
      await turnOff.mutateAsync();
      toast(t('turnOff.success'));
      onClose();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <AlertDialog open onOpenChange={(open) => !open && !turnOff.isPending && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('turnOff.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('turnOff.description')}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={turnOff.isPending}>{tc('buttons.cancel')}</AlertDialogCancel>
          <Button type="button" variant="destructive" onClick={run} loading={turnOff.isPending}>
            {t('turnOff.confirm')}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
