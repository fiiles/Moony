import { useId, useMemo, useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Download, TriangleAlert } from 'lucide-react';
import { MIN_PASSWORD_LENGTH, changePasswordSchema } from '@shared/schema';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { RecoveryKeyBox } from '@/components/auth/RecoveryKeyBox';
import { Button } from '@/components/ui/button';
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { biometricApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import {
  pickVerificationGroups,
  recoveryKeyFileContents,
  recoveryKeyGroups,
  verifyGroups,
} from '@/utils/recovery-key-check';
import { Sheet } from '@/mobile/components/Sheet';
import {
  useChangePassword,
  type ChangePasswordFlow,
  type ChangePasswordValues,
} from '@/mobile/hooks/use-change-password';
import { useExportTextFile } from '@/mobile/hooks/use-export-text-file';
import { BIOMETRIC_STATUS_KEY } from '@/mobile/lib/biometrics';
import { canConfirmChange } from '@/mobile/lib/change-password';
import { changedRecoveryKeyFileName, exportsOnThisDevice } from '@/mobile/lib/export-file';
import { passwordStrength } from '@/mobile/lib/password-strength';
import { biometryMethodKey, renderTextRef } from '@/mobile/lib/settings-summary';
import { field48, fieldMessage, primaryButton } from '@/mobile/pages/first-run/styles';

/** The footer's ghost "Zrušit" and its primary, which takes the rest of the row. */
const FOOTER_GHOST =
  'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row';
const FOOTER_PRIMARY =
  'h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row';

/** A quiet 12 pt sentence under the key's check. */
const QUIET = 'm-0 px-0.5 text-m-sub text-ink-4';

const EMPTY_FORM: ChangePasswordValues = {
  currentPassword: '',
  newPassword: '',
  confirmPassword: '',
};

const NOT_TYPED: [string, string] = ['', ''];

/** A password field never feeds the keyboard's corrections or predictions, shown or hidden. */
const SECRET_FIELD = {
  autoCapitalize: 'none',
  autoCorrect: 'off',
  spellCheck: false,
} as const;

interface ChangePasswordSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * Zabezpečení → Změnit heslo (spec §8.9, rulings 13–15): the desktop's two-phase change in one
 * sheet. The form step asks for the current password, the new one with the first run's strength
 * sentence and its confirmation; "Pokračovat" checks the current password (a wrong one is said
 * under its field and nothing else happens) and brings a new recovery key. The key step is the
 * first run's: the warning that the old key stops working, the key box with "Zkopírovat" and, on
 * iOS, "Uložit do Souborů", and two groups to retype; "Změnit heslo" is on once they match and
 * only then is anything written. Quiet sentences say that nothing has changed yet, that backups
 * made before keep the old password, and — while it is on — that biometric unlock stays on (the
 * master key is the same, ruling 14). Closing the sheet (or a lock, which unmounts it) before
 * "Změnit heslo" changes nothing; every closing forgets the passwords and the key. While the change
 * runs the sheet stays.
 */
export function ChangePasswordSheet({ open, onOpenChange }: ChangePasswordSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const formId = useId();
  // The two groups retyped from the new key; every closing empties them, so each key starts with none
  const [groups, setGroups] = useState<[string, string]>(NOT_TYPED);
  const flow = useChangePassword({
    onChanged: () => {
      setGroups(NOT_TYPED);
      onOpenChange(false);
    },
  });
  const { recoveryKey } = flow;
  const confirming = flow.pending.confirm;

  const verifyIndexes = useMemo<[number, number]>(
    () => pickVerificationGroups(recoveryKey ? recoveryKeyGroups(recoveryKey).length : 0),
    [recoveryKey]
  );
  const verified = recoveryKey !== null && verifyGroups(recoveryKey, verifyIndexes, groups);

  // Zrušit, the scrim, Escape and back: the passwords and the key are forgotten, nothing changes.
  // The form's own fields go with the sheet's content, which unmounts once it has slid away.
  const change = (next: boolean) => {
    // The new key is being written: closing now would hide whether it was
    if (!next && confirming) return;
    if (!next) {
      flow.reset();
      setGroups(NOT_TYPED);
    }
    onOpenChange(next);
  };

  const onKey = flow.step === 'key' && recoveryKey !== null;

  return (
    <Sheet
      open={open}
      onOpenChange={change}
      title={onKey ? ts('password.recoveryKeyTitle') : t('settings.password.title')}
      description={onKey ? t('firstRun.recoveryLead') : ts('password.description')}
      footer={
        <>
          <Button
            variant="ghost"
            className={FOOTER_GHOST}
            disabled={confirming}
            onClick={() => change(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          {onKey ? (
            <Button
              className={FOOTER_PRIMARY}
              disabled={!canConfirmChange({ verified, confirming })}
              loading={confirming}
              onClick={flow.confirm}
            >
              {t('settings.password.title')}
            </Button>
          ) : (
            <Button
              type="submit"
              form={formId}
              className={FOOTER_PRIMARY}
              disabled={flow.pending.prepare}
              loading={flow.pending.prepare}
            >
              {tc('buttons.continue')}
            </Button>
          )}
        </>
      }
    >
      {onKey ? (
        <KeyStep
          recoveryKey={recoveryKey}
          verifyIndexes={verifyIndexes}
          groups={groups}
          verified={verified}
          confirming={confirming}
          onGroupsChange={setGroups}
        />
      ) : (
        <FormStep id={formId} pending={flow.pending.prepare} onSubmit={flow.prepare} />
      )}
    </Sheet>
  );
}

/**
 * The three passwords. Its own component, so what was typed goes with it once the key step
 * replaces it; `useChangePassword` keeps the two it needs.
 */
function FormStep({
  id,
  pending,
  onSubmit,
}: {
  id: string;
  pending: boolean;
  onSubmit: ChangePasswordFlow['prepare'];
}) {
  const { t: ts } = useTranslation('settings');
  const { t: ta } = useTranslation('auth');
  const form = useForm<ChangePasswordValues>({
    resolver: zodResolver(changePasswordSchema),
    defaultValues: EMPTY_FORM,
  });
  const strength = passwordStrength(form.watch('newPassword'));

  const submit = (values: ChangePasswordValues) =>
    onSubmit(values, {
      onCurrentPasswordError: (message) => {
        form.resetField('currentPassword');
        form.setError('currentPassword', { type: 'server', message });
        form.setFocus('currentPassword');
      },
    });

  return (
    <Form {...form}>
      <form
        id={id}
        onSubmit={form.handleSubmit(submit)}
        aria-busy={pending}
        noValidate
        className="space-y-3"
      >
        <FormField
          control={form.control}
          name="currentPassword"
          render={({ field }) => (
            <FormItem>
              <FormLabel>{ts('password.current')}</FormLabel>
              <FormControl>
                <PasswordInput
                  touch
                  autoComplete="current-password"
                  enterKeyHint="next"
                  className={field48}
                  readOnly={pending}
                  {...SECRET_FIELD}
                  {...field}
                />
              </FormControl>
              <FormMessage className={fieldMessage} />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="newPassword"
          render={({ field }) => (
            <FormItem>
              <FormLabel>{ts('password.new')}</FormLabel>
              <FormControl>
                <PasswordInput
                  touch
                  autoComplete="new-password"
                  enterKeyHint="next"
                  className={field48}
                  readOnly={pending}
                  {...SECRET_FIELD}
                  {...field}
                />
              </FormControl>
              {strength && (
                <FormDescription
                  className={cn(
                    'text-m-sub',
                    strength === 'weak' && 'text-loss',
                    strength === 'strong' && 'text-gain'
                  )}
                >
                  {ta(`onboarding.account.strength.${strength}`, { min: MIN_PASSWORD_LENGTH })}
                </FormDescription>
              )}
              <FormMessage className={fieldMessage} />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="confirmPassword"
          render={({ field }) => (
            <FormItem>
              <FormLabel>{ts('password.confirm')}</FormLabel>
              <FormControl>
                <PasswordInput
                  touch
                  autoComplete="new-password"
                  enterKeyHint="go"
                  className={field48}
                  readOnly={pending}
                  {...SECRET_FIELD}
                  {...field}
                />
              </FormControl>
              <FormMessage className={fieldMessage} />
            </FormItem>
          )}
        />
      </form>
    </Form>
  );
}

/**
 * The new recovery key, as the first run's step shows it (`RecoveryStep`): the warning, the key
 * box, "Uložit do Souborů" where the Files picker exports, the two groups to retype, and the quiet
 * sentences under them. The file is not the first run's: it is named and titled by the day of this
 * attempt (`moony-recovery-key-2026-10-25.txt`, „… nový od 25. 10. 2026“) and its note says the key
 * holds only once the change is done, so saving it never replaces the copy of the key that stays
 * valid when the change is abandoned, and two attempts' files never read the same.
 */
function KeyStep({
  recoveryKey,
  verifyIndexes,
  groups,
  verified,
  confirming,
  onGroupsChange,
}: {
  recoveryKey: string;
  verifyIndexes: [number, number];
  groups: [string, string];
  verified: boolean;
  confirming: boolean;
  onGroupsChange: (groups: [string, string]) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const { t: ta } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { exportText } = useExportTextFile();
  const [saving, setSaving] = useState(false);
  // The attempt's day: this step mounts when its key arrives
  const [attemptedAt] = useState(() => new Date());
  const biometrics = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  const showMismatch = !verified && groups[0].length >= 4 && groups[1].length >= 4;

  const saveToFiles = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const outcome = await exportText(
        changedRecoveryKeyFileName(attemptedAt),
        recoveryKeyFileContents(recoveryKey, {
          title: t('settings.password.fileTitle', { date: fmt.date(attemptedAt) }),
          // When it holds first, then what the key is for (the first run's note)
          note: `${t('settings.password.fileNote')}\n\n${t('firstRun.recoveryFileNote')}`,
        })
      );
      if (outcome === 'saved') toast(ta('onboarding.recovery.saved'));
    } catch (error) {
      toast.error(tc('status.error'), {
        description: translateApiError(error instanceof Error ? error : String(error), tc),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div>
      <div
        role="alert"
        className="mb-3 flex items-start gap-2 rounded-r2 border border-loss-line bg-loss-soft px-3 py-2.5 text-m-sub text-loss"
      >
        <TriangleAlert aria-hidden className="mt-px size-[15px] flex-none" strokeWidth={1.75} />
        <span className="min-w-0">{ts('password.oldKeyInvalid')}</span>
      </div>

      <RecoveryKeyBox recoveryKey={recoveryKey} touch />
      {/* Where the Files picker does not export (not iOS) the key is copied; no button that only apologises */}
      {exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '') && (
        <Button
          type="button"
          variant="secondary"
          className={cn(primaryButton, 'mt-3 [&_svg]:size-[18px]')}
          loading={saving}
          disabled={saving || confirming}
          onClick={() => void saveToFiles()}
        >
          <Download aria-hidden />
          {t('firstRun.saveToFiles')}
        </Button>
      )}

      <div className="mt-5 space-y-1.5">
        <Label>
          {ta('onboarding.recovery.verifyLabel', {
            a: verifyIndexes[0] + 1,
            b: verifyIndexes[1] + 1,
          })}
        </Label>
        <div className="grid grid-cols-2 gap-3">
          {verifyIndexes.map((groupIndex, i) => (
            <Input
              key={groupIndex}
              aria-label={ta('onboarding.recovery.groupLabel', { n: groupIndex + 1 })}
              value={groups[i]}
              maxLength={6}
              inputMode="text"
              autoCapitalize="characters"
              autoComplete="off"
              autoCorrect="off"
              spellCheck={false}
              enterKeyHint={i === 0 ? 'next' : 'done'}
              readOnly={confirming}
              className={cn(field48, 'uppercase tracking-[0.06em] num')}
              onChange={(event) => {
                const next: [string, string] = [groups[0], groups[1]];
                next[i] = event.target.value;
                onGroupsChange(next);
              }}
            />
          ))}
        </div>
        <p
          role="status"
          className={cn(
            'm-0 px-0.5 pt-0.5 text-m-sub text-ink-4',
            showMismatch && 'font-semibold text-loss',
            verified && 'font-semibold text-gain'
          )}
        >
          {verified
            ? ta('onboarding.recovery.verified')
            : showMismatch
              ? ta('onboarding.recovery.mismatch')
              : ta('onboarding.recovery.verifyHint')}
        </p>
      </div>

      <div className="mt-5 space-y-2">
        <p className={QUIET}>{t('settings.password.confirmHint')}</p>
        <p className={QUIET}>{t('settings.password.backupsKeepOld')}</p>
        {biometrics.data?.enrolled && (
          <p className={QUIET}>
            {renderTextRef(t, {
              key: 'settings.password.biometricsStay',
              params: { methodKey: biometryMethodKey(biometrics.data.biometryType) },
            })}
          </p>
        )}
      </div>
    </div>
  );
}
