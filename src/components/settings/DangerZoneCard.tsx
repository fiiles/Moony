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
import { authApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

// Word the user must type before the account can be deleted (DZ-01)
const DELETE_ACCOUNT_CONFIRM_WORD = 'DELETE';

/** Settings → Account → Nebezpečná zóna (prototype `settings.html`): delete everything, typed confirmation. */
export function DangerZoneCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');

  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleteConfirmText, setDeleteConfirmText] = useState('');
  const deleteConfirmed = deleteConfirmText.trim().toUpperCase() === DELETE_ACCOUNT_CONFIRM_WORD;

  const deleteAccountMutation = useMutation({
    mutationFn: async () => {
      await authApi.deleteAccount();
    },
    onSuccess: () => {
      // Consent flags and UI preferences belong to the deleted account
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
                {t('dangerZone.confirmDescription')} {t('dangerZone.backupFirst')}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <div className="space-y-2">
              <Label htmlFor="delete-account-confirm">
                {t('dangerZone.confirmLabel', { word: DELETE_ACCOUNT_CONFIRM_WORD })}
              </Label>
              <Input
                id="delete-account-confirm"
                value={deleteConfirmText}
                onChange={(e) => setDeleteConfirmText(e.target.value)}
                placeholder={DELETE_ACCOUNT_CONFIRM_WORD}
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
