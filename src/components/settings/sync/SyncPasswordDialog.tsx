import { useRef, useState, type FormEvent, type ReactNode } from 'react';
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
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { translateApiError } from '@/lib/translate-api-error';

interface SyncPasswordDialogProps {
  title: string;
  description: ReactNode;
  /** A second, emphasised sentence under the description (e.g. how many changes are unsent). */
  note?: string;
  confirmLabel: string;
  /** The confirm button is red for actions that delete or replace data. */
  destructive?: boolean;
  /** Runs with the typed password; a rejection is shown under the field and keeps the dialog open. */
  onSubmit: (password: string) => Promise<unknown>;
  /** `false`: the caller replaces this dialog after a successful submit, so it does not close itself. */
  closeOnSuccess?: boolean;
  /** Mounted only while open; called after a successful submit and on cancel. */
  onClose: () => void;
}

/** A confirmation that asks for the device password (show the kit, delete synced data, re-sync). */
export function SyncPasswordDialog({
  title,
  description,
  note,
  confirmLabel,
  destructive = false,
  closeOnSuccess = true,
  onSubmit,
  onClose,
}: SyncPasswordDialogProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (password === '' || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(password);
      if (closeOnSuccess) onClose();
    } catch (failure) {
      setError(translateApiError(failure as Error, tc));
      setBusy(false);
    }
  };

  return (
    <AlertDialog open onOpenChange={(open) => !open && !busy && onClose()}>
      <AlertDialogContent
        onOpenAutoFocus={(event) => {
          // The field, not Cancel (Radix's default), so typing and Enter reach it at once.
          event.preventDefault();
          field.current?.focus();
        }}
      >
        <form onSubmit={submit} className="grid gap-4">
          <AlertDialogHeader>
            <AlertDialogTitle>{title}</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2">
                <p>{description}</p>
                {note && <p className="font-600">{note}</p>}
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="sync-device-password">{t('password.label')}</Label>
            <Input
              ref={field}
              id="sync-device-password"
              type="password"
              value={password}
              onChange={(event) => {
                setPassword(event.target.value);
                setError(null);
              }}
              onKeyDown={submitOnEnter}
              autoComplete="current-password"
              aria-invalid={error ? true : undefined}
            />
            {error && (
              <p className="m-0 text-micro font-500 text-loss" role="alert">
                {error}
              </p>
            )}
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy}>{tc('buttons.cancel')}</AlertDialogCancel>
            <Button
              type="submit"
              variant={destructive ? 'destructive' : 'default'}
              disabled={password === ''}
              loading={busy}
            >
              {confirmLabel}
            </Button>
          </AlertDialogFooter>
        </form>
      </AlertDialogContent>
    </AlertDialog>
  );
}
