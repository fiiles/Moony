import { useId, useState, type FormEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { translateApiError } from '@/lib/translate-api-error';
import { useServerGate } from '@/hooks/use-server-gate';
import { Sheet } from '@/mobile/components/Sheet';
import { field48, fieldMessage } from '@/mobile/pages/first-run/styles';

interface SyncPasswordSheetProps {
  open: boolean;
  /** Called on every close that is allowed (never while the submit runs). */
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  /** A second, emphasised sentence (how many changes are unsent). */
  note?: string;
  /** Below the note: what is at stake (a replace's counts). */
  extra?: ReactNode;
  /** Holds the confirm back (the numbers it asks the person to agree to are not in yet). */
  submitDisabled?: boolean;
  /** Only the sync server can carry it out: while it is out of reach the confirm waits, saying why. */
  needsServer?: boolean;
  confirmLabel: string;
  /** The confirm button is red for what deletes or replaces data. */
  destructive?: boolean;
  /** Runs with the typed password; a rejection is shown under the field and keeps the sheet open. */
  onSubmit: (password: string) => Promise<unknown>;
  /** `false`: the caller replaces this sheet after a successful submit, so it does not close itself. */
  closeOnSuccess?: boolean;
}

/** A password field never feeds the keyboard's corrections or predictions. */
const SECRET_FIELD = { autoCapitalize: 'none', autoCorrect: 'off', spellCheck: false } as const;

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * The sheet that asks for the device password before what needs it (show the Recovery Kit, delete
 * the synced data, re-sync this device): the phone's counterpart of the desktop's
 * `SyncPasswordDialog`. The password lives in this component only and goes when the sheet closes;
 * a failed submit is said under the field and the sheet stays.
 */
export function SyncPasswordSheet({
  open,
  onOpenChange,
  title,
  description,
  note,
  extra,
  submitDisabled = false,
  needsServer = false,
  confirmLabel,
  destructive = false,
  onSubmit,
  closeOnSuccess = true,
}: SyncPasswordSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const server = useServerGate();
  const waits = needsServer && server.blocked;

  // Whatever closes the sheet takes the password and the message with it
  if (!open && (password !== '' || error !== null)) {
    setPassword('');
    setError(null);
  }

  const change = (next: boolean) => {
    // Nothing leaves the sheet while the password is being checked
    if (!next && busy) return;
    onOpenChange(next);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (password === '' || busy || submitDisabled || waits) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(password);
      setBusy(false);
      if (closeOnSuccess) onOpenChange(false);
    } catch (failure) {
      setError(translateApiError(failure as Error, tc));
      setBusy(false);
    }
  };

  const formId = `${fieldId}-form`;
  return (
    <Sheet
      open={open}
      onOpenChange={change}
      locked={busy}
      title={title}
      description={description}
      footer={
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            disabled={busy}
            onClick={() => change(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={formId}
            variant={destructive ? 'destructive' : 'default'}
            className={`${BUTTON} flex-1`}
            disabled={password === '' || submitDisabled}
            loading={busy}
            {...(needsServer ? server.props : {})}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} aria-busy={busy} noValidate className="space-y-3">
        {note && <p className="m-0 text-m-lead font-semibold text-ink-2">{note}</p>}
        {extra}
        <div className="space-y-1.5">
          <Label htmlFor={fieldId}>{t('password.label')}</Label>
          <PasswordInput
            id={fieldId}
            touch
            value={password}
            onChange={(event) => {
              setPassword(event.target.value);
              setError(null);
            }}
            autoComplete="current-password"
            enterKeyHint="go"
            className={field48}
            readOnly={busy}
            aria-invalid={error ? true : undefined}
            {...SECRET_FIELD}
          />
          {error && (
            <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
              {error}
            </p>
          )}
        </div>
        {waits && (
          <p id={server.noteId} className="m-0 text-m-sub text-ink-3">
            {t('status.needsServer')}
          </p>
        )}
      </form>
    </Sheet>
  );
}
