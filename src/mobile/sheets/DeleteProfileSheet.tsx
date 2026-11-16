import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { confirmWordMatches } from '@/lib/confirm-word';
import { authApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { syncIsOn } from '@/utils/safe-copy';
import { Sheet } from '@/mobile/components/Sheet';
import { cancelAllNotifications } from '@/mobile/hooks/use-notification-schedule';

interface DeleteProfileSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Smazat všechna data?" (spec §8.9, §10.5): the destructive confirmation with the typed word of
 * the desktop's danger zone, translated. The database, the key files and the attachments of this phone go
 * (`authApi.deleteAccount`); afterwards the UI preferences of the account go too and the webview
 * reloads, which leaves the shell at `needs_setup` — the first run.
 */
export function DeleteProfileSheet({ open, onOpenChange }: DeleteProfileSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { t: ts } = useTranslation('settings');
  const syncFeature = useDeviceSyncFeature();
  const { data: syncStatus } = useDeviceSyncStatus();
  const syncOn = syncIsOn(syncFeature, syncStatus);
  const [typed, setTyped] = useState('');
  // Opt-in, off by default: deleting the profile only removes this phone
  const [deleteSynced, setDeleteSynced] = useState(false);
  // The word to type before everything is deleted, in the UI language and as the desktop's
  // danger zone asks for it ("SMAZAT"); any case will do (audit B5)
  const word = ts('dangerZone.confirmWord');
  const confirmed = confirmWordMatches(typed, word);

  const remove = useMutation({
    mutationFn: () => authApi.deleteAccount(syncOn && deleteSynced),
    onSuccess: async () => {
      // The notifications scheduled for the deleted profile must not outlive it (best effort, iOS)
      await cancelAllNotifications();
      // UI preferences belong to the deleted account. The database is already gone: storage that
      // cannot be cleared must not turn this into a failure or skip the reload.
      try {
        localStorage.clear();
      } catch {
        // Blocked storage
      }
      window.location.reload();
    },
    onError: (error: Error) => {
      toast.error(t('settings.delete.failed'), { description: translateApiError(error, tc) });
    },
  });

  const change = (next: boolean) => {
    // Nothing leaves the sheet while the database is being deleted
    if (!next && remove.isPending) return;
    if (!next) {
      setTyped('');
      setDeleteSynced(false);
    }
    onOpenChange(next);
  };

  return (
    <Sheet
      open={open}
      onOpenChange={change}
      title={t('settings.delete.title')}
      description={
        syncOn && !deleteSynced ? t('settings.delete.leadSynced') : t('settings.delete.lead')
      }
      footer={
        <>
          <Button
            variant="ghost"
            className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row"
            disabled={remove.isPending}
            onClick={() => change(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            variant="destructive"
            className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
            loading={remove.isPending}
            disabled={!confirmed || remove.isPending}
            onClick={() => remove.mutate()}
          >
            {t('settings.delete.confirm')}
          </Button>
        </>
      }
    >
      {syncOn && (
        <label
          htmlFor="delete-profile-synced"
          className="mb-4 flex items-start gap-3 text-m-sub text-ink-2"
        >
          <Checkbox
            id="delete-profile-synced"
            className="mt-0.5"
            checked={deleteSynced}
            disabled={remove.isPending}
            onCheckedChange={(checked) => setDeleteSynced(checked === true)}
          />
          <span className="block">
            <span className="block font-semibold text-ink">
              {t('settings.delete.syncDeleteLabel')}
            </span>
            <span className="block">{t('settings.delete.syncDeleteHint')}</span>
          </span>
        </label>
      )}
      <label htmlFor="delete-profile-confirm" className="block text-m-sub font-semibold text-ink-2">
        {t('settings.delete.confirmLabel', { word })}
      </label>
      <Input
        id="delete-profile-confirm"
        value={typed}
        onChange={(event) => setTyped(event.target.value)}
        placeholder={word}
        autoComplete="off"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        enterKeyHint="done"
        readOnly={remove.isPending}
        className="mt-1.5 h-12 rounded-[10px] text-m-field"
      />
    </Sheet>
  );
}
