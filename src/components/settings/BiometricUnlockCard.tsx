import { useTranslation } from 'react-i18next';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useBiometricStatus } from '@/hooks/use-biometric-status';
import { useDesktopBiometricEnrolment } from '@/hooks/use-desktop-biometric-enrolment';
import { biometricSettingState } from '@/lib/biometrics';

/**
 * Settings → Security → Touch ID / Windows Hello: the switch that turns biometric unlock on (the
 * system prompt, then the key is stored) and off (the stored key is removed). It is also the only
 * way back after the one-time offer was answered or the unlock switched itself off.
 *
 * Not rendered where the platform has no biometric unlock (Linux). A Mac without Touch ID, or a PC
 * without Windows Hello set up, shows the row disabled with the reason.
 */
export function BiometricUnlockCard() {
  const { t } = useTranslation('settings');
  const { query, status, label } = useBiometricStatus();
  const { enable, disable } = useDesktopBiometricEnrolment(label);

  const state = biometricSettingState(status);
  if (state === 'hidden') return null;

  const busy = query.isFetching || enable.isPending || disable.isPending;

  return (
    <SettingsCard
      title={t(`biometricUnlock.title.${label}`)}
      description={t('biometricUnlock.description')}
    >
      <SettingsRow
        htmlFor="biometric-unlock"
        label={t(`biometricUnlock.label.${label}`)}
        hint={
          state === 'unavailable'
            ? t(`biometricUnlock.unavailable.${label}`)
            : t(`biometricUnlock.hint.${label}`)
        }
      >
        <Switch
          id="biometric-unlock"
          checked={status?.enrolled ?? false}
          disabled={state === 'unavailable' || busy}
          onCheckedChange={(on) => (on ? enable.mutate() : disable.mutate())}
        />
      </SettingsRow>
    </SettingsCard>
  );
}
