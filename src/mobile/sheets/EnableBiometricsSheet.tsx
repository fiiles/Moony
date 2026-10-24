import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
import { biometricApi } from '@/lib/tauri-api';
import { Button } from '@/components/ui/button';
import { BiometryIcon } from '@/mobile/components/BiometryIcon';
import { Sheet } from '@/mobile/components/Sheet';
import { useBiometricEnrolment } from '@/mobile/hooks/use-biometric-enrolment';
import {
  BIOMETRIC_STATUS_KEY,
  biometricCopy,
  markEnrolmentAsked,
  shouldOfferEnrolment,
  wasEnrolmentAsked,
} from '@/mobile/lib/biometrics';

function storage(): Storage | null {
  try {
    return localStorage;
  } catch {
    return null;
  }
}

/**
 * "Odemykat Moony obličejem?" (spec §8.2 Biometrie, explained before the system asks): once,
 * after the first password unlock on a phone whose biometrics can be used and are not on yet.
 * "Zapnout" asks for Face ID / Touch ID ("Zapnout odemykání biometrií") and stores the session's
 * master key behind biometry (in Rust; the key never reaches the webview), "Teď ne" leaves it.
 * Either answer, or closing the sheet, sets `moony-biometrics-asked`.
 */
export function EnableBiometricsSheet() {
  const { t } = useTranslation('mobile');
  const { appStatus, unlockMutation, lockMutation } = useAuth();
  const [asked, setAsked] = useState(() => wasEnrolmentAsked(storage()));

  // The flag is read when a password unlock is submitted, not only at mount: a clear since then
  // (biometrics invalidated, or a new account on this phone) offers the sheet again at once.
  const unlockEvent = unlockMutation.submittedAt;
  const [readAt, setReadAt] = useState(unlockEvent);
  if (readAt !== unlockEvent) {
    setReadAt(unlockEvent);
    setAsked(wasEnrolmentAsked(storage()));
  }

  // Unlocked with the password since the last lock: a newer unlock submission than the last lock.
  const passwordUnlocked =
    appStatus === 'unlocked' &&
    unlockMutation.isSuccess &&
    unlockMutation.submittedAt > lockMutation.submittedAt;

  const status = useQuery({
    queryKey: BIOMETRIC_STATUS_KEY,
    queryFn: biometricApi.status,
    enabled: passwordUnlocked && !asked,
  });

  const close = () => {
    markEnrolmentAsked(storage());
    setAsked(true);
  };

  const { enable } = useBiometricEnrolment();

  const type = status.data?.biometryType ?? 'none';
  const copy = biometricCopy(type);
  // Stays up while "Zapnout" runs, then the answer closes it.
  const open = passwordUnlocked && (shouldOfferEnrolment(status.data, asked) || enable.isPending);

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next && !enable.isPending) close();
      }}
      title={t(copy.title)}
      description={t(copy.lead)}
      footer={
        <>
          <Button
            variant="ghost"
            className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row"
            disabled={enable.isPending}
            onClick={close}
          >
            {t('biometrics.notNow')}
          </Button>
          <Button
            className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
            loading={enable.isPending}
            disabled={enable.isPending}
            onClick={() => enable.mutate(undefined, { onSettled: close })}
          >
            {t('biometrics.enable')}
          </Button>
        </>
      }
    >
      <div className="grid place-items-center py-4">
        <div className="grid size-[76px] place-items-center rounded-[22px] border border-line bg-paper text-ink shadow-e2">
          <BiometryIcon type={type} className="size-9" />
        </div>
      </div>
    </Sheet>
  );
}
