/**
 * The warning before a document leaves the encrypted store (D3): another app needs a plain copy,
 * which Moony deletes on lock and quit. "Don't ask again" is a per-device setting.
 */
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
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
import { Checkbox } from '@/components/ui/checkbox';

interface OpenElsewhereDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (dontAskAgain: boolean) => void;
}

export function OpenElsewhereDialog({ open, onOpenChange, onConfirm }: OpenElsewhereDialogProps) {
  const { t } = useTranslation('common');
  const [dontAskAgain, setDontAskAgain] = useState(false);

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('attachments.openElsewhere.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('attachments.openElsewhere.body')}</AlertDialogDescription>
        </AlertDialogHeader>
        <label className="flex items-center gap-2 text-caption text-ink-2">
          <Checkbox checked={dontAskAgain} onCheckedChange={(v) => setDontAskAgain(v === true)} />
          {t('attachments.openElsewhere.dontAsk')}
        </label>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('actions.cancel')}</AlertDialogCancel>
          {/* Opening is not destructive: the primary button, not the alert dialog's red action. */}
          <Button onClick={() => onConfirm(dontAskAgain)}>
            {t('attachments.openElsewhere.confirm')}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
