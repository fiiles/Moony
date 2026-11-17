import { useEffect, useRef } from 'react';
import { cancelAll, pending, type Schedule } from '@tauri-apps/plugin-notification';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useMilestoneMutedKinds, useMilestones } from '@/hooks/use-milestones';
import { useNotificationPermission } from '@/mobile/hooks/use-notification-permission';
import { useNotificationPrefs } from '@/mobile/hooks/use-notification-prefs';
import {
  buildNotificationPlan,
  localTimeOf,
  planSignature,
  scheduleDateString,
  type LocalTime,
} from '@/mobile/lib/notification-plan';
import { notificationsApi } from '@/lib/tauri-api';
import { exportsOnThisDevice } from '@/mobile/lib/export-file';
import { notificationPaths } from '@/mobile/lib/notification-target';
import { renderTextRef } from '@/mobile/lib/settings-summary';
import { syncIsOn } from '@/utils/safe-copy';

/**
 * Whether this build schedules notifications: the phones. A desktop shell (the phone UI opened
 * there for development) would show every "scheduled" notification at once. The two plugins read
 * the schedule's date differently, which `scheduleDateString` takes care of.
 */
function schedulesOnThisDevice(): boolean {
  const platform: string = import.meta.env.TAURI_ENV_PLATFORM ?? '';
  return platform === 'ios' || platform.startsWith('android');
}

/**
 * Hands one notification to the system for `at`, a local wall-clock time. On iOS the plugin reads
 * the schedule's date as local digits with a literal `Z`, on Android as UTC (`scheduleDateString`
 * writes either), which its `Schedule.at` helper cannot tell apart. Resolves once the plugin holds
 * the notification and rejects when it refuses one (a time that has passed, say; the plan leaves
 * those out). `path`, the screen a tap opens, rides in `extra`, which the plugin hands back on a
 * tap (on iOS through the vendored patch).
 */
export function scheduleNotification(item: {
  id: number;
  title: string;
  body: string;
  at: LocalTime;
  path?: string;
}): Promise<void> {
  const platform: string = import.meta.env.TAURI_ENV_PLATFORM ?? '';
  return notificationsApi.schedule({
    id: item.id,
    title: item.title,
    body: item.body,
    // Android's status bar icon, a drawable of the app (`gen/android/.../ic_stat_moony.xml`); the
    // plugin's own config cannot name one (its Rust side takes no config). iOS uses the app icon.
    ...(platform.startsWith('android') ? { icon: 'ic_stat_moony' } : {}),
    ...(item.path ? { extra: { path: item.path } } : {}),
    schedule: {
      at: {
        date: scheduleDateString(item.at, platform),
        repeating: false,
        allowWhileIdle: false,
      },
    } as unknown as Schedule,
  });
}

/**
 * Cancels every notification the app has scheduled, best effort: phones only (`schedulesOnThisDevice`)
 * and never a failure. The pending notifications carry the milestone titles of the profile that
 * is being deleted or replaced, so they must not outlive it; the restored profile is planned again
 * at its next unlock. A plugin that refuses changes nothing for the caller, which reloads next.
 */
export async function cancelAllNotifications(): Promise<void> {
  notificationPaths.clear();
  if (!schedulesOnThisDevice()) return;
  try {
    await cancelAll();
  } catch {
    // Nothing to cancel, or the plugin refused: deleting or restoring goes on regardless
  }
}

/**
 * Plans the phone's local notifications (spec §8.9), mounted once in the shell while the app is
 * unlocked. Once the system allows notifications and the milestones and the muted kinds have
 * loaded, it builds the plan (`buildNotificationPlan`) and, when it differs from what is
 * scheduled, cancels everything pending and schedules the plan again. That happens on every
 * unlock and whenever a preference, a milestone, the muted kinds or the language change; texts are
 * translated here, in the language of the moment. Nothing is scheduled without the permission, and
 * a failure (or a plan the system did not take in full) is a console warning, never a toast: the
 * person did not ask for this work.
 */
export function useNotificationSchedule(): void {
  const { t, i18n } = useTranslation('mobile');
  const { appStatus } = useAuth();
  const { state: permission } = useNotificationPermission();
  const { data: milestones } = useMilestones();
  const { data: mutedKinds } = useMilestoneMutedKinds();
  const [prefs] = useNotificationPrefs();
  const featureOn = useDeviceSyncFeature();
  const { data: syncStatus } = useDeviceSyncStatus();
  const syncOn = syncIsOn(featureOn, syncStatus);
  // Flag on, sync off (once its status is read): the backup reminder offers sync first
  const offerSync = featureOn && syncStatus !== undefined && !syncOn;
  // The signature of what is scheduled: `null` while the schedule is being replaced or incomplete
  const scheduled = useRef<string | null>(null);

  useEffect(() => {
    if (!schedulesOnThisDevice() || appStatus !== 'unlocked' || permission !== 'granted') return;
    if (!milestones || !mutedKinds) return;

    const plan = buildNotificationPlan({
      now: localTimeOf(new Date()),
      milestones,
      mutedKinds,
      prefs,
      canBackup: exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? ''),
      syncOn,
      offerSync,
    });
    // The screens a tap opens, by id: the fallback when a tap reports no `extra` (the unpatched
    // iOS plugin did not). Kept through a lock, when a tap must still find its screen after it.
    notificationPaths.clear();
    for (const item of plan) notificationPaths.set(item.id, item.path);
    const signature = `${i18n.language}#${planSignature(plan)}`;
    if (scheduled.current === signature) return;
    scheduled.current = null;

    let current = true;
    void (async () => {
      try {
        await cancelAll();
        // The inputs changed meanwhile: the run that replaced this one plans from scratch
        if (!current) return;
        let refused = 0;
        for (const item of plan) {
          if (!current) return;
          try {
            await scheduleNotification({
              id: item.id,
              title: renderTextRef(t, item.title),
              body: renderTextRef(t, item.body),
              at: item.at,
              path: item.path,
            });
          } catch {
            // A refusal (a time that has passed, say) leaves the rest of the plan standing
            refused += 1;
          }
        }
        if (!current) return;
        scheduled.current = signature;
        // Each call has resolved once the plugin held its notification, so the count is the
        // system's own: a shortfall the refusals do not explain is worth a trace.
        const held = (await pending()).length;
        if (refused > 0 || held !== plan.length) {
          console.warn(
            `[notifications] ${plan.length} planned, ${refused} refused, ${held} pending`
          );
        }
      } catch (error) {
        console.warn('[notifications] scheduling or checking the schedule failed', error);
      }
    })();
    return () => {
      current = false;
    };
  }, [appStatus, permission, milestones, mutedKinds, prefs, syncOn, offerSync, t, i18n.language]);
}
