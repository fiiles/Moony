import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { isAuthApiError } from '@/lib/api-error';
import { authApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { ChangePasswordStep } from '@/mobile/lib/change-password';

/** What the form step hands over (`changePasswordSchema` has checked it). */
export interface ChangePasswordValues {
  currentPassword: string;
  newPassword: string;
  confirmPassword: string;
}

export interface ChangePasswordFlow {
  step: ChangePasswordStep;
  /** The new recovery key while the key step shows it; `null` otherwise. */
  recoveryKey: string | null;
  /**
   * Checks the current password and asks for a new recovery key (nothing is written). A refused
   * password (`auth.*`) goes to `onCurrentPasswordError`, translated, for the field to show; any
   * other failure is a toast. Ignored while a call is running.
   */
  prepare(
    values: ChangePasswordValues,
    handlers?: { onCurrentPasswordError?: (message: string) => void }
  ): void;
  /** Changes the password and the recovery key ("Změnit heslo"). Ignored while a call is running. */
  confirm(): void;
  /** Back to an empty form: the passwords and the key are forgotten and a late answer is dropped. */
  reset(): void;
  pending: { prepare: boolean; confirm: boolean };
}

/** The two passwords between the phases. */
interface Secrets {
  currentPassword: string;
  newPassword: string;
}

function asError(error: unknown): Error | string {
  return error instanceof Error ? error : String(error);
}

/**
 * Zabezpečení → Změnit heslo (spec §8.9, rulings 13–15): the desktop's two commands, unchanged.
 * `prepare` verifies the current password and returns a new recovery key without writing anything;
 * `confirm` re-wraps the same master key with the new password (`key.enc`) and the new key
 * (`recovery.enc`). The master key does not change, so biometric unlock's stored copy stays valid
 * and nothing here touches the keychain (ruling 14).
 *
 * The passwords live in this hook only, in a ref between the two phases: never in React Query's
 * caches (a mutation would keep its variables there), a query key, a toast or a log. They are
 * forgotten on success, on `reset` (the sheet calls it whenever it closes) and on unmount (a lock
 * unmounts the page). Every `reset` and the unmount start a new generation, so an answer that
 * arrives after them is dropped: a sheet closed or locked before "Změnit heslo" changes nothing
 * (ruling 15). The success toast is the one exception: once `confirm` has resolved the password
 * has changed, and the toast says so even if the lock screen is up by then.
 */
export function useChangePassword(options: { onChanged?: () => void } = {}): ChangePasswordFlow {
  const { t: ts } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const [step, setStep] = useState<ChangePasswordStep>('form');
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const secrets = useRef<Secrets | null>(null);
  const generation = useRef(0);
  // A second tap in the same frame, before `pending` re-renders the button, sends nothing
  const busy = useRef(false);
  const onChanged = useRef(options.onChanged);
  useEffect(() => {
    onChanged.current = options.onChanged;
  });

  useEffect(
    () => () => {
      generation.current += 1;
      secrets.current = null;
      busy.current = false;
    },
    []
  );

  const reset = useCallback(() => {
    generation.current += 1;
    secrets.current = null;
    busy.current = false;
    setStep('form');
    setRecoveryKey(null);
    setPreparing(false);
    setConfirming(false);
  }, []);

  const prepare = useCallback<ChangePasswordFlow['prepare']>(
    (values, handlers) => {
      if (busy.current) return;
      busy.current = true;
      const run = generation.current;
      setPreparing(true);
      authApi
        .prepareChangePassword({ currentPassword: values.currentPassword })
        .then(
          (result) => {
            if (run !== generation.current) return;
            secrets.current = {
              currentPassword: values.currentPassword,
              newPassword: values.newPassword,
            };
            setRecoveryKey(result.recoveryKey);
            setStep('key');
          },
          (error: unknown) => {
            if (run !== generation.current) return;
            // A refused password is the field's to show (as on the lock screen); nothing else happens
            if (isAuthApiError(error)) {
              handlers?.onCurrentPasswordError?.(translateApiError(error, tc));
              return;
            }
            toast.error(ts('toast.verificationFailed'), {
              description: translateApiError(asError(error), tc),
            });
          }
        )
        .finally(() => {
          if (run !== generation.current) return;
          busy.current = false;
          setPreparing(false);
        });
    },
    [tc, ts]
  );

  const confirm = useCallback(() => {
    const kept = secrets.current;
    if (busy.current || kept === null || recoveryKey === null) return;
    busy.current = true;
    const run = generation.current;
    setConfirming(true);
    authApi
      .confirmChangePassword({
        currentPassword: kept.currentPassword,
        newPassword: kept.newPassword,
        recoveryKey,
      })
      .then(
        () => {
          // Written: say so whatever happened to the sheet meanwhile
          toast(ts('toast.passwordChanged'));
          if (run !== generation.current) return;
          secrets.current = null;
          busy.current = false;
          setRecoveryKey(null);
          setConfirming(false);
          setStep('done');
          onChanged.current?.();
        },
        (error: unknown) => {
          if (run !== generation.current) return;
          // The key step stays, for another try or Zrušit
          busy.current = false;
          setConfirming(false);
          toast.error(ts('toast.updateFailed'), {
            description: translateApiError(asError(error), tc),
          });
        }
      );
  }, [recoveryKey, tc, ts]);

  return {
    step,
    recoveryKey,
    prepare,
    confirm,
    reset,
    pending: { prepare: preparing, confirm: confirming },
  };
}
