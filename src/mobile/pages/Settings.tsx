import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getVersion } from '@tauri-apps/api/app';
import { isPermissionGranted } from '@tauri-apps/plugin-notification';
import { useTranslation } from 'react-i18next';
import { Bell, CalendarClock, FileText, Globe, Lock, Trash2, Upload, User } from 'lucide-react';
import { useAuth } from '@/hooks/use-auth';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { languageInfo } from '@/i18n/languages';
import { useLanguage } from '@/i18n/I18nProvider';
import { biometricApi, onboardingApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { deviceStorage } from '@/utils/device-storage';
import { MILESTONE_GROUP_IDS, kindsOf } from '@/utils/milestones';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import { useBack } from '@/mobile/hooks/use-back';
import { readBackgroundLockMinutes } from '@/mobile/lib/background-lock';
import { BIOMETRIC_STATUS_KEY } from '@/mobile/lib/biometrics';
import { readHideAmountsOnLaunch } from '@/mobile/lib/hide-amounts';
import { renderTextRef, securitySummary } from '@/mobile/lib/settings-summary';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes, settingsPath } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { DeleteProfileSheet } from '@/mobile/sheets/DeleteProfileSheet';
import { LanguagePickerSheet } from '@/mobile/sheets/LanguagePickerSheet';
import { PlaceholderSheet } from '@/mobile/sheets/PlaceholderSheet';

type SheetKind = 'language' | 'import' | 'delete';

/** The app version, `''` until it is known (and where the shell cannot tell, such as a browser). */
function useAppVersion(): string {
  const [version, setVersion] = useState('');
  useEffect(() => {
    let current = true;
    getVersion()
      .then((value) => current && setVersion(value))
      .catch(() => current && setVersion(''));
    return () => {
      current = false;
    };
  }, []);
  return version;
}

/**
 * Nastavení from the avatar on Přehled (spec §8.9): the profile head, then the rows by what they
 * are — profile and language; security, backup, notifications and reminders, each with its state
 * in the sub-line and a page of its own (`/settings/:section`); import; lock and delete — and the
 * version with the promise under it. Language, import and delete open sheets.
 */
export default function Settings() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { t: ts } = useTranslation('settings');
  const { user, lockMutation } = useAuth();
  const { language } = useLanguage();
  const fmt = useFormat();
  const back = useBack(routes.overview);
  const version = useAppVersion();
  const [sheet, setSheet] = useState<SheetKind | null>(null);
  const closeSheet = (open: boolean) => {
    if (!open) setSheet(null);
  };

  const biometrics = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  // The key Task 3's backup invalidates after a backup
  const progress = useQuery({
    queryKey: ['onboarding-progress'],
    queryFn: () => onboardingApi.getProgress(),
  });
  // A rejection (a shell without the plugin) reads as "not granted"
  const notifications = useQuery({
    queryKey: ['notification-permission'],
    queryFn: () => isPermissionGranted().catch(() => false),
  });
  const muted = useMilestoneMutedKinds();

  // Where the device settings stand now: this page mounts again on the way back from a section
  const lockMinutes = readBackgroundLockMinutes(deviceStorage());
  const hideOnLaunch = readHideAmountsOnLaunch(deviceStorage());
  const securitySub = biometrics.isPending
    ? undefined
    : securitySummary({
        enrolled: biometrics.data?.enrolled ?? false,
        biometryType: biometrics.data?.biometryType ?? 'none',
        lockMinutes,
        hideOnLaunch,
      })
        .map((part) => renderTextRef(t, part))
        .join(' · ');

  const lastBackupAt = progress.data?.lastBackupAt;
  const backupSub = progress.data
    ? lastBackupAt != null
      ? t('backup.lastOn', { date: fmt.dateTime(lastBackupAt) })
      : t('backup.never')
    : undefined;

  const notificationsSub =
    notifications.data === undefined
      ? undefined
      : t(
          notifications.data
            ? 'notifications.permission.granted'
            : 'notifications.permission.notGranted'
        );

  const remindersSub = muted.data
    ? t('settings.rows.remindersSub', {
        on: MILESTONE_GROUP_IDS.filter(
          (group) => !kindsOf(group).every((kind) => muted.data.includes(kind))
        ).length,
        total: MILESTONE_GROUP_IDS.length,
      })
    : undefined;

  // The same letters as the avatar on Přehled
  const avatar = `${user?.name?.charAt(0) ?? ''}${user?.surname?.charAt(0) ?? ''}`.toUpperCase();
  const fullName = [user?.name, user?.surname].filter(Boolean).join(' ');

  return (
    <Screen
      title={tc('nav.settings')}
      tabBar={false}
      leading={<NavTextButton label={t('tabs.overview')} onClick={back} chevron />}
    >
      <div className="mx-0.5 mb-4 mt-1 flex items-center gap-3">
        <span
          aria-hidden
          className="inline-flex size-[42px] flex-none items-center justify-center rounded-full bg-well-2 text-m-row font-bold text-ink-2 shadow-[inset_0_0_0_1px_var(--line)]"
        >
          {avatar}
        </span>
        <div className="min-w-0">
          <p className="m-0 truncate text-m-title text-ink">{fullName}</p>
          {user?.email && (
            <p className="m-0 mt-0.5 truncate text-[13px] font-450 leading-[1.4] text-ink-3">
              {user.email}
            </p>
          )}
        </div>
      </div>

      <EntityRows>
        <SettingsRow
          icon={User}
          title={t('settings.rows.profile')}
          sub={user?.email || t('settings.rows.profileNoEmail')}
          href={settingsPath('profile')}
        />
        <SettingsRow
          icon={Globe}
          title={t('settings.rows.language')}
          sub={languageInfo(language).native}
          onClick={() => setSheet('language')}
        />
      </EntityRows>

      <EntityRows>
        <SettingsRow
          icon={Lock}
          title={t('settings.rows.security')}
          sub={securitySub}
          href={settingsPath('security')}
        />
        <SettingsRow
          icon={Upload}
          title={t('settings.rows.backup')}
          sub={backupSub}
          href={settingsPath('backup')}
        />
        <SettingsRow
          icon={Bell}
          title={t('settings.rows.notifications')}
          sub={notificationsSub}
          href={settingsPath('notifications')}
        />
        <SettingsRow
          icon={CalendarClock}
          title={t('settings.rows.reminders')}
          sub={remindersSub}
          href={settingsPath('reminders')}
        />
      </EntityRows>

      <EntityRows>
        <SettingsRow
          icon={FileText}
          title={t('settings.rows.import')}
          sub={t('placeholder.import')}
          onClick={() => setSheet('import')}
        />
      </EntityRows>

      <EntityRows>
        <SettingsRow
          icon={Lock}
          title={tc('nav.lockApp')}
          onClick={() => {
            if (!lockMutation.isPending) lockMutation.mutate();
          }}
        />
        <SettingsRow
          icon={Trash2}
          danger
          title={ts('dangerZone.deleteAll')}
          sub={t('settings.rows.deleteSub')}
          onClick={() => setSheet('delete')}
        />
      </EntityRows>

      <p className="mb-0 mt-6 text-center text-m-caption text-ink-4">
        {version
          ? t('settings.version', { version, privacy: t('settings.noTelemetry') })
          : t('settings.noTelemetry')}
      </p>

      <LanguagePickerSheet open={sheet === 'language'} onOpenChange={closeSheet} />
      <PlaceholderSheet
        open={sheet === 'import'}
        onOpenChange={closeSheet}
        title={t('settings.rows.import')}
        text={t('placeholder.import')}
      />
      <DeleteProfileSheet open={sheet === 'delete'} onOpenChange={closeSheet} />
    </Screen>
  );
}
