import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
import { useBiometricStatus } from '@/hooks/use-biometric-status';
import { useDesktopBiometricEnrolment } from '@/hooks/use-desktop-biometric-enrolment';
import { useUpdater } from '@/hooks/use-updater';
import { markEnrolmentAsked, shouldOfferEnrolment, wasEnrolmentAsked } from '@/lib/biometrics';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { DesktopBiometryIcon } from '@/components/auth/BiometricUnlockButton';

function storage(): Storage | null {
  try {
    return localStorage;
  } catch {
    return null;
  }
}

/**
 * "Odemykat Moony otiskem prstu?" (Windows: "…přes Windows Hello?"): once per install, after a
 * password unlock on a machine whose biometrics can be used and are not on yet. Never after the
 * first-run setup, a recovery or a biometric unlock. "Zapnout" asks for Touch ID / Windows Hello
 * and stores the session's master key behind it (in Rust; the key never reaches the webview),
 * "Teď ne" leaves it. Either answer, or closing the dialog, sets `moony-biometrics-asked`, and
 * nothing on the desktop clears it again except deleting the account: after that, Nastavení →
 * Zabezpečení is the only way to turn it on.
 *
 * Mounted in the unlocked shell (`AppLayout`), next to the update dialog, which it gives way to.
 */
export function EnableBiometricsDialog() {
  const { t } = useTranslation('auth');
  const { appStatus, unlockMutation, lockMutation } = useAuth();
  const { promptOpen: updatePromptOpen } = useUpdater();
  // The local flag answers at once after a close; the stored one also covers the Settings switch,
  // which marks the offer as made from another component.
  const [closed, setClosed] = useState(false);
  const asked = closed || wasEnrolmentAsked(storage());

  // Unlocked with the password since the last lock: a newer unlock submission than the last lock.
  // Setup and recovery unlock through their own mutations, biometrics through another.
  const passwordUnlocked =
    appStatus === 'unlocked' &&
    unlockMutation.isSuccess &&
    unlockMutation.submittedAt > lockMutation.submittedAt;

  const { status, label } = useBiometricStatus({ enabled: passwordUnlocked && !asked });
  const { enable } = useDesktopBiometricEnrolment(label);

  const close = () => {
    markEnrolmentAsked(storage());
    setClosed(true);
  };

  // Stays up while "Zapnout" runs, then the answer closes it. It gives way to the update dialog
  // (the automatic check runs right after an unlock) and comes back once that one has closed.
  const open =
    passwordUnlocked &&
    (enable.isPending || (shouldOfferEnrolment(status, asked) && !updatePromptOpen));

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !enable.isPending) close();
      }}
    >
      <DialogContent className="max-w-[420px]" hideCloseButton={enable.isPending}>
        <div className="grid size-12 place-items-center rounded-r3 bg-well text-ink-2">
          <DesktopBiometryIcon label={label} className="size-6" />
        </div>
        <DialogHeader>
          <DialogTitle>{t(`biometric.offer.title.${label}`)}</DialogTitle>
          <DialogDescription>{t(`biometric.offer.lead.${label}`)}</DialogDescription>
        </DialogHeader>
        <p className="m-0 text-caption text-ink-4">{t('biometric.offer.settingsHint')}</p>
        <DialogFooter>
          <Button variant="ghost" disabled={enable.isPending} onClick={close}>
            {t('biometric.offer.notNow')}
          </Button>
          <Button
            loading={enable.isPending}
            onClick={() => enable.mutate(undefined, { onSettled: close })}
          >
            {t('biometric.offer.enable')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
