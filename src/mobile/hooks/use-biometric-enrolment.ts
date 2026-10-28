import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { isAuthApiError } from '@/lib/api-error';
import { queryClient } from '@/lib/queryClient';
import { biometricApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { deviceStorage } from '@/utils/device-storage';
import { BIOMETRIC_STATUS_KEY, biometricFailure, markEnrolmentAsked } from '@/lib/biometrics';

/**
 * Turns biometric unlock on and off (spec §8.2): the one-time "Odemykat Moony obličejem?" sheet
 * and the switch in Nastavení → Zabezpečení share it. Both refresh the status afterwards and
 * mark the offer as made, so switching biometrics off never brings the sheet back.
 *
 * The master key stays in Rust: `enable` asks Face ID / Touch ID there and stores the session's
 * key behind it, `disable` removes it.
 */
export function useBiometricEnrolment() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');

  const settled = () => {
    void queryClient.invalidateQueries({ queryKey: BIOMETRIC_STATUS_KEY });
    markEnrolmentAsked(deviceStorage() ?? null);
  };

  const enable = useMutation<void, Error, void>({
    mutationFn: () => biometricApi.enable(t('biometrics.enableReason')),
    onSuccess: () => toast(t('biometrics.enabled')),
    onError: (error) => {
      // A dismissed system prompt is an answer. A refused one (not recognised, locked out, the
      // permission denied) is said without the backend's words, which are about unlocking.
      // Anything else says what went wrong.
      const failure = biometricFailure(isAuthApiError(error) ? error.key : undefined);
      if (failure === 'cancelled') {
        toast(t('biometrics.notEnabled'));
      } else if (failure === 'other') {
        toast.error(t('biometrics.notEnabled'), { description: error.message });
      } else {
        toast.error(t('biometrics.notEnabled'));
      }
    },
    onSettled: settled,
  });

  const disable = useMutation<void, Error, void>({
    mutationFn: () => biometricApi.disable(),
    onSuccess: () => toast(t('settings.security.biometricsDisabled')),
    onError: (error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
    onSettled: settled,
  });

  return { enable, disable };
}
