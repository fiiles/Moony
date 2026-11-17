import { useTranslation } from 'react-i18next';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { Label } from '@/components/ui/label';
import { field48, fieldMessage } from '@/mobile/pages/first-run/styles';

/** A password field never feeds the keyboard's corrections or predictions. */
const SECRET_FIELD = { autoCapitalize: 'none', autoCorrect: 'off', spellCheck: false } as const;

/**
 * The Moony password inside a sheet that has its own form (the removal and new-kit sheets): the
 * pattern of `SyncPasswordSheet`, a 48 pt field at 16 px, with the failure said under it.
 */
export function PasswordField({
  id,
  value,
  onChange,
  error,
  readOnly = false,
}: {
  id: string;
  value: string;
  onChange: (value: string) => void;
  error: string | null;
  readOnly?: boolean;
}) {
  const { t } = useTranslation('deviceSync');
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{t('password.label')}</Label>
      <PasswordInput
        id={id}
        touch
        value={value}
        onChange={(event) => onChange(event.target.value)}
        autoComplete="current-password"
        enterKeyHint="go"
        className={field48}
        readOnly={readOnly}
        aria-invalid={error ? true : undefined}
        {...SECRET_FIELD}
      />
      {error && (
        <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
          {error}
        </p>
      )}
    </div>
  );
}
