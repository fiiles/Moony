import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { KeyRound } from 'lucide-react';
import { biometricApi } from '@/lib/tauri-api';
import { deviceStorage } from '@/utils/device-storage';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionPage } from '@/mobile/components/SectionPage';
import { Segment } from '@/mobile/components/Segment';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import { SwitchRow } from '@/mobile/components/SwitchRow';
import { useBiometricEnrolment } from '@/mobile/hooks/use-biometric-enrolment';
import {
  BACKGROUND_LOCK_OPTIONS,
  readBackgroundLockMinutes,
  writeBackgroundLockMinutes,
} from '@/mobile/lib/background-lock';
import { BIOMETRIC_STATUS_KEY, biometricCopy } from '@/lib/biometrics';
import { readHideAmountsOnLaunch, writeHideAmountsOnLaunch } from '@/mobile/lib/hide-amounts';
import { biometryMethodKey, lockAfterOption, renderTextRef } from '@/mobile/lib/settings-summary';
import { ChangePasswordSheet } from '@/mobile/sheets/ChangePasswordSheet';

/** The card the section's controls sit in (`.m-card`), rows `line-soft` apart. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1';

/**
 * Nastavení → Zabezpečení (spec §8.9): biometric unlock, how soon the app locks after it was left
 * and whether every launch starts with the amounts hidden. The last two are device settings in
 * `localStorage`; `useBackgroundLock` reads the lock delay on every return to the foreground, and
 * the hide-on-launch switch is read when the app starts. Under the card, "Změnit heslo" opens the
 * password change with its new recovery key (`ChangePasswordSheet`).
 */
export default function SecuritySection() {
  const { t } = useTranslation('mobile');
  const status = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  const { enable, disable } = useBiometricEnrolment();
  const [lockMinutes, setLockMinutes] = useState(() => readBackgroundLockMinutes(deviceStorage()));
  const [hideOnLaunch, setHideOnLaunch] = useState(() => readHideAmountsOnLaunch(deviceStorage()));
  const [passwordOpen, setPasswordOpen] = useState(false);

  const data = status.data;
  const enrolled = data?.enrolled ?? false;
  const available = data?.available ?? false;
  const type = data?.biometryType ?? 'none';
  // Why it cannot be on, or what it does. Without an answer (a shell without biometrics) it
  // cannot be on either.
  const sub = data
    ? available
      ? t(biometricCopy(type).lead)
      : t(data.reason ?? 'biometrics.unavailable')
    : status.isError
      ? t('biometrics.unavailable')
      : undefined;

  const chooseLock = (value: string) => {
    const minutes = BACKGROUND_LOCK_OPTIONS.find((option) => String(option) === value);
    if (minutes === undefined) return;
    writeBackgroundLockMinutes(deviceStorage(), minutes);
    setLockMinutes(minutes);
  };

  return (
    <SectionPage title={t('settings.rows.security')} lead={t('settings.security.lead')}>
      <div className={CARD}>
        <SwitchRow
          id="biometrics"
          label={renderTextRef(t, {
            key: 'settings.security.biometrics',
            params: { methodKey: biometryMethodKey(type) },
          })}
          sub={sub}
          checked={enrolled}
          disabled={
            status.isPending || enable.isPending || disable.isPending || (!available && !enrolled)
          }
          onCheckedChange={(on) => (on ? enable.mutate() : disable.mutate())}
        />
        <div className="px-1 py-3">
          <p className="m-0 mb-2 text-m-sub font-semibold text-ink-2">
            {t('settings.security.lockAfterLabel')}
          </p>
          <Segment
            value={String(lockMinutes)}
            options={BACKGROUND_LOCK_OPTIONS.map((minutes) => ({
              value: String(minutes),
              label: renderTextRef(t, lockAfterOption(minutes)),
            }))}
            onChange={chooseLock}
            label={t('settings.security.lockAfterLabel')}
          />
          <p className="m-0 mt-2 text-m-sub text-ink-4">{t('settings.security.lockAfterHint')}</p>
        </div>
        <SwitchRow
          id="hide-on-launch"
          label={t('settings.security.hideOnLaunch')}
          sub={t('settings.security.hideOnLaunchHint')}
          checked={hideOnLaunch}
          onCheckedChange={(on) => {
            writeHideAmountsOnLaunch(deviceStorage(), on);
            setHideOnLaunch(on);
          }}
        />
      </div>
      <EntityRows>
        <SettingsRow
          icon={KeyRound}
          title={t('settings.password.title')}
          sub={t('settings.password.rowSub')}
          onClick={() => setPasswordOpen(true)}
        />
      </EntityRows>
      <ChangePasswordSheet open={passwordOpen} onOpenChange={setPasswordOpen} />
    </SectionPage>
  );
}
