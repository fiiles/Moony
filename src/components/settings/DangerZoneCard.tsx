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
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { syncIsOn } from '@/utils/safe-copy';
import { authApi } from '@/lib/tauri-api';
import { clearEnrolmentAsked } from '@/lib/biometrics';
import { confirmWordMatches } from '@/lib/confirm-word';
import { translateApiError } from '@/lib/translate-api-error';

/** Settings → Account → Nebezpečná zóna (prototype `settings.html`): delete everything, typed confirmation. */
export function DangerZoneCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { t: tsync } = useTranslation('deviceSync');
  const { i18n } = useTranslation();

  const syncFeature = useDeviceSyncFeature();
  const { data: syncStatus } = useDeviceSyncStatus();
  const syncOn = syncIsOn(syncFeature, syncStatus);
  // Japanese and Chinese run sentences together without a space
  const sentenceGap = ['ja', 'zh'].includes(i18n.language) ? '' : ' ';

  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleteConfirmText, setDeleteConfirmText] = useState('');
  // Opt-in, off by default: deleting the profile only removes this device
  const [deleteSynced, setDeleteSynced] = useState(false);
  // Deleting the synced data deletes it for every device: the device password first (O-1)
  const [password, setPassword] = useState('');
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const withSynced = syncOn && deleteSynced;
  // Word the user must type before the account can be deleted (DZ-01), in the UI language; any case
  const confirmWord = t('dangerZone.confirmWord');
  const deleteConfirmed =
    confirmWordMatches(deleteConfirmText, confirmWord) && (!withSynced || password !== '');

  const deleteAccountMutation = useMutation({
    mutationFn: async () => {
      await authApi.deleteAccount(withSynced, withSynced ? password : undefined);
    },
    onSuccess: () => {
      // UI preferences belong to the deleted account. A new account on this machine is offered
      // biometric unlock again: named on its own so the flag goes even if the clear ever narrows.
      clearEnrolmentAsked(localStorage);
      localStorage.clear();
      window.location.reload();
    },
    onError: (error: Error) => {
      // A wrong password is said under its field and the dialog stays
      if (withSynced) setPasswordError(translateApiError(error, tc));
      else toast.error(t('toast.deletionFailed'), { description: translateApiError(error, tc) });
    },
  });

  return (
    <SettingsCard danger title={t('dangerZone.title')} description={t('dangerZone.description')}>
      <SettingsRow
        label={t('dangerZone.deleteAll')}
        hint={syncOn ? t('dangerZone.deleteAllHintSynced') : t('dangerZone.deleteAllHint')}
      >
        <AlertDialog
          open={deleteDialogOpen}
          onOpenChange={(open) => {
            if (!open && deleteAccountMutation.isPending) return;
            setDeleteDialogOpen(open);
            if (!open) {
              setDeleteConfirmText('');
              setDeleteSynced(false);
              setPassword('');
              setPasswordError(null);
            }
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
                {syncOn && !deleteSynced && (
                  <>
                    {sentenceGap}
                    <span>{t('dangerZone.syncKept')}</span>
                  </>
                )}
                {sentenceGap}
                <span>{t('dangerZone.backupFirst')}</span>
              </AlertDialogDescription>
            </AlertDialogHeader>
            {syncOn && (
              <div className="flex items-start gap-2.5">
                <Checkbox
                  id="delete-account-synced"
                  className="mt-0.5"
                  checked={deleteSynced}
                  onCheckedChange={(checked) => setDeleteSynced(checked === true)}
                />
                <Label htmlFor="delete-account-synced" className="block space-y-1 leading-snug">
                  <span className="block">{t('dangerZone.syncDeleteLabel')}</span>
                  <span className="block text-xs font-normal text-ink-3">
                    {t('dangerZone.syncDeleteHint')}
                  </span>
                </Label>
              </div>
            )}
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
            {withSynced && (
              <div className="grid gap-1.5">
                <Label htmlFor="delete-account-password">{tsync('password.label')}</Label>
                <Input
                  id="delete-account-password"
                  type="password"
                  value={password}
                  onChange={(event) => {
                    setPassword(event.target.value);
                    setPasswordError(null);
                  }}
                  autoComplete="current-password"
                  aria-invalid={passwordError ? true : undefined}
                />
                {passwordError && (
                  <p className="m-0 text-micro font-500 text-loss" role="alert">
                    {passwordError}
                  </p>
                )}
              </div>
            )}
            <AlertDialogFooter>
              <AlertDialogCancel>{tc('buttons.cancel')}</AlertDialogCancel>
              <AlertDialogAction
                disabled={!deleteConfirmed || deleteAccountMutation.isPending}
                onClick={(event) => {
                  // The dialog stays while the deletion runs; a wrong password keeps it open
                  event.preventDefault();
                  setPasswordError(null);
                  deleteAccountMutation.mutate();
                }}
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
