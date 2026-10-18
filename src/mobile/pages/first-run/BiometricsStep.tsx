import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { biometricApi } from '@/lib/tauri-api';
import { deviceStorage } from '@/utils/device-storage';
import { BiometryIcon } from '@/mobile/components/BiometryIcon';
import { useBiometricEnrolment } from '@/mobile/hooks/use-biometric-enrolment';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { BIOMETRIC_STATUS_KEY, biometricCopy, markEnrolmentAsked } from '@/mobile/lib/biometrics';
import { StepHead } from './StepHead';
import { primaryButton, textLink } from './styles';

/**
 * Biometrie (only on a phone that can use biometrics): "Odemykat Moony obličejem?" explained
 * before the system asks. "Zapnout" asks for Face ID / Touch ID and stores the session's master key
 * behind it (in Rust), and the flow moves on whatever the answer; "Teď ne" marks the offer as made
 * and moves on, so the one-time sheet after the first password unlock does not ask again.
 */
export function BiometricsStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('mobile');
  const status = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  const { enable } = useBiometricEnrolment();
  const type = status.data?.biometryType ?? 'none';
  const copy = biometricCopy(type);
  const pending = enable.isPending;

  return (
    <div data-testid="first-run-biometrics">
      <div className="mb-5 grid size-[76px] place-items-center rounded-[22px] border border-line bg-paper text-ink shadow-e2">
        <BiometryIcon type={type} className="size-9" />
      </div>
      <StepHead title={t(copy.title)} lead={t(copy.lead)} />
      <Button
        className={primaryButton}
        loading={pending}
        disabled={pending}
        onClick={() => enable.mutate(undefined, { onSettled: flow.goNext })}
      >
        {t('biometrics.enable')}
      </Button>
      <button
        type="button"
        className={textLink}
        disabled={pending}
        onClick={() => {
          markEnrolmentAsked(deviceStorage() ?? null);
          flow.goNext();
        }}
      >
        {t('biometrics.notNow')}
      </button>
    </div>
  );
}
