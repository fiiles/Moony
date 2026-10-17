import { useEffect, useRef } from 'react';
import {
  cancelAll,
  pending,
  sendNotification,
  type Schedule,
} from '@tauri-apps/plugin-notification';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
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
import { renderTextRef } from '@/mobile/lib/settings-summary';

/**
 * Whether this build schedules notifications: iOS only. A desktop shell (the phone UI opened there
 * for development) would show every "scheduled" notification at once. Android is left out on
 * purpose: its plugin reads the schedule's date as UTC (Jackson with the date pattern and no time
 * zone), while `scheduleDateString` writes local digits, which is what iOS expects, so a
 * notification would fire hours off there. Android needs its own UTC encoding of the date and an
 * Android build to verify it on; that is a follow-up.
 */
function schedulesOnThisDevice(): boolean {
  const platform: string = import.meta.env.TAURI_ENV_PLATFORM ?? '';
  return platform === 'ios';
}

/**
 * Hands one notification to the system for `at`, a local wall-clock time. The plugin reads the
 * schedule's date as local digits with a literal `Z` (`scheduleDateString`), which its `Schedule.at`
 * helper cannot express: it takes a `Date`, whose JSON is UTC. The plugin does not wait for the
 * system (`sendNotification` returns at once), so a refusal such as a time that has passed does not
 * come back here; the plan leaves those out.
 */
export function scheduleNotification(item: {
  id: number;
  title: string;
  body: string;
  at: LocalTime;
}): void {
  sendNotification({
    id: item.id,
    title: item.title,
    body: item.body,
    schedule: {
      at: { date: scheduleDateString(item.at), repeating: false, allowWhileIdle: false },
    } as unknown as Schedule,
  });
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
    });
    const signature = `${i18n.language}#${planSignature(plan)}`;
    if (scheduled.current === signature) return;
    scheduled.current = null;

    let current = true;
    void (async () => {
      try {
        await cancelAll();
        // The inputs changed meanwhile: the run that replaced this one plans from scratch
        if (!current) return;
        for (const item of plan) {
          scheduleNotification({
            id: item.id,
            title: renderTextRef(t, item.title),
            body: renderTextRef(t, item.body),
            at: item.at,
          });
        }
        scheduled.current = signature;
        // The plugin does not report a refusal (a time that has passed, say), so count what the
        // system actually holds: a shortfall is the only trace such a refusal leaves.
        const held = (await pending()).length;
        if (held !== plan.length) {
          console.warn(`[notifications] ${plan.length} planned, ${held} pending`);
        }
      } catch (error) {
        console.warn('[notifications] scheduling or checking the schedule failed', error);
      }
    })();
    return () => {
      current = false;
    };
  }, [appStatus, permission, milestones, mutedKinds, prefs, t, i18n.language]);
}
