import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Sheet } from '@/mobile/components/Sheet';
import { phoneDeviceKey, type ScanMode } from '@/mobile/lib/sync-scan';
import { field48, fieldMessage } from '@/mobile/pages/first-run/styles';

/** What the person typed before the camera opens; the password goes to the scan and no further. */
export interface ScanDetails {
  password: string;
  /** The name this phone joins under (`join` only). */
  deviceName?: string;
}

interface JoinDetailsSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** `join`: the replace warning, the device name and the password; `add`: the password only. */
  mode: ScanMode;
  /** What went wrong after the camera (a wrong password, a refused code), shown under the password. */
  error: string | null;
  onSubmit: (details: ScanDetails) => void;
  /** The code is typed or pasted, not scanned: the sheet says nothing of scanning. */
  manual?: boolean;
}

/** A password field never feeds the keyboard's corrections or predictions. */
const SECRET_FIELD = { autoCapitalize: 'none', autoCorrect: 'off', spellCheck: false } as const;

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * What a scan needs before the camera opens (review PF1: the password comes first on every pairing
 * path). Joining (sync off): the warning that this phone's data is replaced, the name it joins under
 * ("iPhone" / "Android phone" to start with) and the device password; the same details serve a
 * scanned Recovery Kit. Adding a device (sync on): the password only. The password lives here until
 * "Continue" hands it on, and goes when the sheet closes.
 */
export function JoinDetailsSheet({
  open,
  onOpenChange,
  mode,
  error,
  onSubmit,
  manual = false,
}: JoinDetailsSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
  const [name, setName] = useState(() =>
    t(`setup.defaultName.${phoneDeviceKey(import.meta.env.TAURI_ENV_PLATFORM ?? '')}`)
  );
  const [password, setPassword] = useState('');

  // Whatever closes the sheet takes the password with it
  if (!open && password !== '') setPassword('');

  const joining = mode === 'join';
  const ready = password !== '' && (!joining || name.trim() !== '');

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!ready) return;
    onSubmit(joining ? { password, deviceName: name.trim() } : { password });
  };

  const formId = `${fieldId}-form`;
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={
        joining ? t(manual ? 'phone.enterCodeTitle' : 'phone.scanTitle') : t('pairing.addTitle')
      }
      description={
        joining ? t('join.warning') : t(manual ? 'phone.passwordFirstTyped' : 'phone.passwordFirst')
      }
      footer={
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            onClick={() => onOpenChange(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button type="submit" form={formId} className={`${BUTTON} flex-1`} disabled={!ready}>
            {tc('buttons.continue')}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} noValidate className="space-y-3">
        {joining && (
          <div className="space-y-1.5">
            <Label htmlFor={`${fieldId}-name`}>{t('join.nameLabel')}</Label>
            <Input
              id={`${fieldId}-name`}
              value={name}
              maxLength={60}
              onChange={(event) => setName(event.target.value)}
              autoComplete="off"
              enterKeyHint="next"
              className={field48}
            />
          </div>
        )}
        <div className="space-y-1.5">
          <Label htmlFor={`${fieldId}-password`}>{t('join.passwordLabel')}</Label>
          <PasswordInput
            id={`${fieldId}-password`}
            touch
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="current-password"
            enterKeyHint="go"
            className={field48}
            aria-invalid={error ? true : undefined}
            {...SECRET_FIELD}
          />
          {error && (
            <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
              {error}
            </p>
          )}
        </div>
      </form>
    </Sheet>
  );
}
