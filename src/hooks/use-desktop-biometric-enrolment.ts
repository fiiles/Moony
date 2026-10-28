import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { isAuthApiError } from '@/lib/api-error';
import { queryClient } from '@/lib/queryClient';
import { biometricApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import {
  BIOMETRIC_STATUS_KEY,
  biometricFailure,
  markEnrolmentAsked,
  type DesktopBiometryLabel,
} from '@/lib/biometrics';

function storage(): Storage | null {
  try {
    return localStorage;
  } catch {
    return null;
  }
}

/**
 * Turns biometric unlock on and off on the desktop: the one-time "Odemykat Moony otiskem prstu?"
 * dialog and the switch in Nastavení → Zabezpečení share it. Both refresh the status afterwards
 * and mark the offer as made, so switching it off never brings the dialog back.
 *
 * The master key stays in Rust: `enable` asks for Touch ID / Windows Hello there and stores the
 * session's key behind it, `disable` removes it. The phone has its own copy of this hook
 * (`src/mobile/hooks/use-biometric-enrolment.ts`) with the phone's wording.
 */
export function useDesktopBiometricEnrolment(label: DesktopBiometryLabel) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');

  const settled = () => {
    void queryClient.invalidateQueries({ queryKey: BIOMETRIC_STATUS_KEY });
    markEnrolmentAsked(storage());
  };

  const enable = useMutation<void, Error, void>({
    // The reason shows only in the macOS prompt ("Moony is trying to …"); Windows Hello takes none.
    mutationFn: () => biometricApi.enable(t('biometric.enableReason')),
    onSuccess: () => toast(t(`biometric.enabled.${label}`)),
    onError: (error) => {
      // A dismissed prompt is an answer. A refused one (not recognised, locked out) is said
      // without the backend's words, which are about unlocking. Anything else says what went wrong.
      const failure = biometricFailure(isAuthApiError(error) ? error.key : undefined);
      if (failure === 'cancelled') {
        toast(t(`biometric.notEnabled.${label}`));
      } else if (failure === 'other') {
        toast.error(t(`biometric.notEnabled.${label}`), { description: error.message });
      } else {
        toast.error(t(`biometric.notEnabled.${label}`));
      }
    },
    onSettled: settled,
  });

  const disable = useMutation<void, Error, void>({
    mutationFn: () => biometricApi.disable(),
    onSuccess: () => toast(t(`biometric.disabled.${label}`)),
    onError: (error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
    onSettled: settled,
  });

  return { enable, disable };
}
