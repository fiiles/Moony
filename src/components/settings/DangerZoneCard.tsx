import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { authApi } from '@/lib/tauri-api';
import { clearEnrolmentAsked } from '@/lib/biometrics';
import { confirmWordMatches } from '@/lib/confirm-word';
import { translateApiError } from '@/lib/translate-api-error';

/** Settings → Account → Nebezpečná zóna (prototype `settings.html`): delete everything, typed confirmation. */
export function DangerZoneCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { i18n } = useTranslation();

  const syncFeature = useDeviceSyncFeature();
  const { data: syncStatus } = useDeviceSyncStatus();
  const syncOn =
    syncFeature &&
    syncStatus !== undefined &&
    ['idle', 'syncing', 'waiting', 'paused', 'error'].includes(syncStatus.phase);
  // Japanese and Chinese run sentences together without a space
  const sentenceGap = ['ja', 'zh'].includes(i18n.language) ? '' : ' ';

  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleteConfirmText, setDeleteConfirmText] = useState('');
  // Word the user must type before the account can be deleted (DZ-01), in the UI language; any case
  const confirmWord = t('dangerZone.confirmWord');
  const deleteConfirmed = confirmWordMatches(deleteConfirmText, confirmWord);

  const deleteAccountMutation = useMutation({
    mutationFn: async () => {
      await authApi.deleteAccount();
    },
    onSuccess: () => {
      // UI preferences belong to the deleted account. A new account on this machine is offered
      // biometric unlock again: named on its own so the flag goes even if the clear ever narrows.
      clearEnrolmentAsked(localStorage);
      localStorage.clear();
      window.location.reload();
    },
    onError: (error: Error) => {
      toast.error(t('toast.deletionFailed'), { description: translateApiError(error, tc) });
    },
  });

  return (
    <SettingsCard danger title={t('dangerZone.title')} description={t('dangerZone.description')}>
      <SettingsRow label={t('dangerZone.deleteAll')} hint={t('dangerZone.deleteAllHint')}>
        <AlertDialog
          open={deleteDialogOpen}
          onOpenChange={(open) => {
            setDeleteDialogOpen(open);
            if (!open) setDeleteConfirmText('');
          }}
        >
          <AlertDialogTrigger asChild>
            <Button variant="danger">{t('dangerZone.deleteButton')}</Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{t('dangerZone.confirmTitle')}</AlertDialogTitle>
              <AlertDialogDescription>
                <span>
                  {syncOn
                    ? t('dangerZone.confirmDescriptionSynced')
                    : t('dangerZone.confirmDescription')}
                </span>
                {syncOn && (
                  <>
                    {sentenceGap}
                    <span>{t('dangerZone.syncDeleted')}</span>
                  </>
                )}
                {sentenceGap}
                <span>{t('dangerZone.backupFirst')}</span>
              </AlertDialogDescription>
            </AlertDialogHeader>
            <div className="space-y-2">
              <Label htmlFor="delete-account-confirm">
                {t('dangerZone.confirmLabel', { word: confirmWord })}
              </Label>
              <Input
                id="delete-account-confirm"
                value={deleteConfirmText}
                onChange={(e) => setDeleteConfirmText(e.target.value)}
                placeholder={confirmWord}
                autoComplete="off"
                autoCapitalize="off"
                spellCheck={false}
              />
            </div>
            <AlertDialogFooter>
              <AlertDialogCancel>{tc('buttons.cancel')}</AlertDialogCancel>
              <AlertDialogAction
                disabled={!deleteConfirmed || deleteAccountMutation.isPending}
                onClick={() => deleteAccountMutation.mutate()}
              >
                {deleteAccountMutation.isPending
                  ? t('dangerZone.deleting')
                  : t('dangerZone.deleteButton')}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </SettingsRow>
    </SettingsCard>
  );
}
