import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { FileKey, KeyRound, LifeBuoy } from 'lucide-react';
import { biometricApi } from '@/lib/tauri-api';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { deviceStorage } from '@/utils/device-storage';
import { keepSafeItems, type KeepSafeItem } from '@/utils/keep-safe';
import { syncIsOn } from '@/utils/safe-copy';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
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
import { biometryMethodKey, lockAfterOption, renderTextRef } from '@/mobile/lib/settings-summary';
import { SyncKitSheet } from '@/mobile/components/sync/SyncKitSheet';
import { ChangePasswordSheet } from '@/mobile/sheets/ChangePasswordSheet';

const KEEP_SAFE_ICONS = { password: KeyRound, recoveryKey: FileKey, kit: LifeBuoy } as const;

/** The card the section's controls sit in (`.m-card`), rows `line-soft` apart. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1';

/**
 * Nastavení → Zabezpečení (spec §8.9): biometric unlock, and how soon the app locks after it was left.
 * The lock delay is a device setting in `localStorage`; `useBackgroundLock` reads it on every
 * return to the foreground. Under the card, "Změnit heslo" opens the
 * password change with its new recovery key (`ChangePasswordSheet`).
 */
export default function SecuritySection() {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const status = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  const { enable, disable } = useBiometricEnrolment();
  const [lockMinutes, setLockMinutes] = useState(() => readBackgroundLockMinutes(deviceStorage()));
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [kitOpen, setKitOpen] = useState(false);
  const featureOn = useDeviceSyncFeature();
  const { data: syncStatus } = useDeviceSyncStatus();
  const keepSafe = keepSafeItems(featureOn, syncIsOn(featureOn, syncStatus));

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
      </div>
      <EntityRows>
        <SettingsRow
          icon={KeyRound}
          title={t('settings.password.title')}
          sub={t('settings.password.rowSub')}
          onClick={() => setPasswordOpen(true)}
        />
      </EntityRows>
      {keepSafe.length > 0 && (
        <>
          <SectionHead title={ts('keepSafe.title')} caption={ts('keepSafe.description')} />
          <EntityRows>
            {keepSafe.map((item) => (
              <KeepSafeRow
                key={item}
                item={item}
                title={ts(`keepSafe.${item}.title`)}
                sub={ts(`keepSafe.${item}.text`)}
                onShow={() => setKitOpen(true)}
              />
            ))}
          </EntityRows>
          {keepSafe.includes('kit') && <SyncKitSheet open={kitOpen} onOpenChange={setKitOpen} />}
        </>
      )}
      <ChangePasswordSheet open={passwordOpen} onOpenChange={setPasswordOpen} />
    </SectionPage>
  );
}

/**
 * One row of "Co si uschovat". The Kit row opens its sheet (which asks for the device password
 * first); the other two only say what the secret is for, so they carry no chevron and no press
 * state (`EntityRow` without `onClick`).
 */
function KeepSafeRow({
  item,
  title,
  sub,
  onShow,
}: {
  item: KeepSafeItem;
  title: string;
  sub: string;
  onShow: () => void;
}) {
  const Icon = KEEP_SAFE_ICONS[item];
  // The sub-line is the explanation itself, so it wraps instead of ending with an ellipsis
  const text = <span className="block whitespace-normal">{sub}</span>;
  if (item === 'kit') return <SettingsRow icon={Icon} title={title} sub={text} onClick={onShow} />;
  return (
    <EntityRow
      chip={
        <RowChip square>
          <Icon strokeWidth={1.75} />
        </RowChip>
      }
      title={title}
      sub={text}
    />
  );
}
