/**
 * The local notifications of the phone (spec §8.9): what to schedule, worked out from the
 * milestones and the preferences. Pure: `buildNotificationPlan` returns the items with their
 * wording as `TextRef`s, and `useNotificationSchedule` translates and schedules them. Nothing here
 * reads the clock or talks to the plugin; `now` is passed in.
 *
 * Times are local wall-clock times (`LocalTime`, month 1–12): a reminder is due at 09:00 where the
 * person is, not at 09:00 UTC. A milestone's day is a UTC day (unix seconds), so it is read as a
 * calendar date first (`utcDayToLocalTime`).
 */
import type { Milestone, MilestoneKind } from '@shared/schema';
import { isDeadline } from '@/utils/milestones';
import type { NotificationPrefs } from '@/mobile/lib/notification-prefs';
import type { TextRef } from '@/mobile/lib/settings-summary';
import { milestonePath } from '@/mobile/lib/milestone-path';
import { routes, settingsPath } from '@/mobile/shell/routes';

/** The hour (local) every reminder is due at. */
export const NOTIFY_HOUR = 9;
/** iOS keeps 64 pending requests; the rest of the plan waits for the next unlock. */
export const MAX_PENDING = 60;

const DAY = 86_400;

/** A wall-clock time without a zone: the date and the time as the person's clock shows them. */
export interface LocalTime {
  year: number;
  /** 1–12. */
  month: number;
  day: number;
  hour: number;
  minute: number;
  /**
   * Only the one notification that is due in seconds (the test notification) sets it; every
   * planned reminder is on the minute, and the comparisons ignore it.
   */
  second?: number;
}

export type PlannedKind = 'deadline14' | 'deadline1' | 'upkeep' | 'backup';

export interface PlannedNotification {
  /** Stable 31-bit identifier (`notificationId`): what the plugin knows the notification by. */
  id: number;
  kind: PlannedKind;
  at: LocalTime;
  title: TextRef;
  body: TextRef;
  /**
   * The screen a tap opens (spec §6 "notifications open the target directly"): a deadline's
   * record (`milestonePath`, else Co vás čeká), Přehled for the monthly upkeep (its Údržba card
   * holds every due action), Nastavení → Záloha for the backup.
   */
  path: string;
}

/** The date's local components. */
export function localTimeOf(date: Date): LocalTime {
  return {
    year: date.getFullYear(),
    month: date.getMonth() + 1,
    day: date.getDate(),
    hour: date.getHours(),
    minute: date.getMinutes(),
  };
}

function compare(a: LocalTime, b: LocalTime): number {
  return (
    a.year - b.year || a.month - b.month || a.day - b.day || a.hour - b.hour || a.minute - b.minute
  );
}

/** Whether `a` is strictly later than `b`. */
export function isAfter(a: LocalTime, b: LocalTime): boolean {
  return compare(a, b) > 0;
}

const pad = (value: number, width = 2) => String(value).padStart(width, '0');

/**
 * The text the notification plugin takes as the date of a schedule: "2026-11-02T09:00:00.000Z".
 * The plugin reads it differently per platform (`TAURI_ENV_PLATFORM`):
 *
 * - iOS parses it with a formatter that has no time zone, so the digits are the local wall-clock
 *   time and the `Z` is only a letter (a `Date`'s JSON, which is UTC, would fire hours off).
 * - Android parses it with Jackson and that pattern, which reads it as UTC: there the text is the
 *   UTC instant of the local wall-clock time, a `Date`'s JSON.
 */
export function scheduleDateString(at: LocalTime, platform = 'ios'): string {
  if (platform.startsWith('android')) {
    return new Date(
      at.year,
      at.month - 1,
      at.day,
      at.hour,
      at.minute,
      at.second ?? 0
    ).toISOString();
  }
  return `${pad(at.year, 4)}-${pad(at.month)}-${pad(at.day)}T${pad(at.hour)}:${pad(at.minute)}:${pad(at.second ?? 0)}.000Z`;
}

/** The calendar date of a UTC day (unix seconds), at `hour` local. */
export function utcDayToLocalTime(day: number, hour: number): LocalTime {
  const date = new Date(day * 1000);
  return {
    year: date.getUTCFullYear(),
    month: date.getUTCMonth() + 1,
    day: date.getUTCDate(),
    hour,
    minute: 0,
  };
}

/** The day after `now`'s, at the notification hour. */
function tomorrow(now: LocalTime): LocalTime {
  // UTC arithmetic: only the calendar is wanted, so a daylight-saving change cannot get in
  return utcDayToLocalTime(Date.UTC(now.year, now.month - 1, now.day + 1) / 1000, NOTIFY_HOUR);
}

/**
 * The next upkeep reminder: `dayOfMonth` (1–28) at 09:00 this month while that is still ahead,
 * otherwise next month's.
 */
export function nextUpkeepDate(now: LocalTime, dayOfMonth: number): LocalTime {
  const thisMonth: LocalTime = {
    year: now.year,
    month: now.month,
    day: dayOfMonth,
    hour: NOTIFY_HOUR,
    minute: 0,
  };
  if (isAfter(thisMonth, now)) return thisMonth;
  return now.month === 12
    ? { ...thisMonth, year: now.year + 1, month: 1 }
    : { ...thisMonth, month: now.month + 1 };
}

/**
 * A stable identifier for one notification: the 32-bit FNV-1a hash of `kind:discriminator`, cut to
 * 31 bits (the plugin wants a signed 32-bit integer) and never 0. The same milestone gets the same
 * id on every plan, so the signature only changes when something about the plan does.
 */
export function notificationId(kind: PlannedKind, discriminator: string): number {
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(`${kind}:${discriminator}`)) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash & 0x7fffffff || 1;
}

/** The input of `buildNotificationPlan`. */
export interface NotificationPlanInput {
  /** The local time the plan is made at: nothing at or before it is planned. */
  now: LocalTime;
  /** The milestones the backend lists (its snoozed, done and muted ones are not in it). */
  milestones: readonly Milestone[];
  mutedKinds: readonly MilestoneKind[];
  prefs: NotificationPrefs;
  /**
   * Whether a backup can be made on this device (`exportsOnThisDevice`); without it no backup
   * reminder is planned, as it would lead to a button that cannot work. Default true.
   */
  canBackup?: boolean;
  /**
   * Sync is on (`syncIsOn`): the backup reminder names both ways out, under its own id. Default
   * false: the reminder is as before.
   */
  syncOn?: boolean;
  /**
   * The sync flag is on and sync is off: the backup reminder offers sync first, then a backup
   * (owner, 2026-11-17), under its own id. `syncOn` wins. Default false.
   */
  offerSync?: boolean;
}

/**
 * What to schedule now, earliest first and at most `MAX_PENDING`:
 * - for every unmuted deadline — one the person can still meet, not information — a notice 14 days
 *   and one the day before its last day to act (the notice deadline, else the event itself), at
 *   09:00 local, while "Termíny" is on;
 * - one monthly upkeep reminder on the chosen day, while "Měsíční údržba" is on;
 * - one backup reminder tomorrow, while "Záloha" is on and the backup milestone says the last
 *   backup is stale (or there is none) and is not muted.
 * Whatever is not after `now` is left out: the plugin refuses a time that has passed. Each item
 * says the screen its tap opens (`path`).
 */
export function buildNotificationPlan(input: NotificationPlanInput): PlannedNotification[] {
  const { now, milestones, mutedKinds, prefs, canBackup = true, syncOn = false } = input;
  const offerSync = !syncOn && input.offerSync === true;
  const items: PlannedNotification[] = [];

  if (prefs.deadlines) {
    for (const m of milestones) {
      const day = m.actionDay ?? m.dueDay;
      if (day === null || !isDeadline(m) || mutedKinds.includes(m.kind)) continue;
      const title: TextRef = { key: 'notifications.deadline.title', params: { title: m.title } };
      const whatKey = `notifications.deadline.kind.${m.kind}`;
      const path = milestonePath(m) ?? routes.agenda;
      items.push(
        {
          id: notificationId('deadline14', m.key),
          kind: 'deadline14',
          at: utcDayToLocalTime(day - 14 * DAY, NOTIFY_HOUR),
          title,
          body: { key: 'notifications.deadline.in14', params: { whatKey } },
          path,
        },
        {
          id: notificationId('deadline1', m.key),
          kind: 'deadline1',
          at: utcDayToLocalTime(day - DAY, NOTIFY_HOUR),
          title,
          body: { key: 'notifications.deadline.tomorrow', params: { whatKey } },
          path,
        }
      );
    }
  }

  if (prefs.upkeep) {
    items.push({
      id: notificationId('upkeep', 'upkeep'),
      kind: 'upkeep',
      at: nextUpkeepDate(now, prefs.upkeepDay),
      title: { key: 'notifications.upkeep.title' },
      body: { key: 'notifications.upkeep.body' },
      path: routes.overview,
    });
  }

  const staleBackup = milestones.find((m) => m.kind === 'backup_stale');
  if (canBackup && prefs.backup && staleBackup && !mutedKinds.includes('backup_stale')) {
    const never = staleBackup.sinceDay === null;
    const [variant, title, body] = syncOn
      ? ['backupSync', 'titleSync', never ? 'neverSync' : 'staleSync']
      : offerSync
        ? ['backupOffer', 'titleSync', never ? 'neverOffer' : 'staleOffer']
        : ['backup', 'title', never ? 'never' : 'stale'];
    items.push({
      // Its own id per wording (sync on, sync offered, backup only): the signature changes, so a
      // pending reminder with another text is cancelled and planned again
      id: notificationId('backup', variant),
      kind: 'backup',
      at: tomorrow(now),
      title: { key: `notifications.backup.${title}` },
      body: { key: `notifications.backup.${body}` },
      path: settingsPath('backup'),
    });
  }

  // Ties are broken by id so the cut (and the signature) do not depend on the milestone order
  return items
    .filter((item) => isAfter(item.at, now))
    .sort((a, b) => compare(a.at, b.at) || a.id - b.id)
    .slice(0, MAX_PENDING);
}

/**
 * Identifies a plan by what it schedules, ids and dates: two plans with the same signature are
 * the same schedule, so the scheduler leaves the pending notifications alone.
 */
export function planSignature(plan: PlannedNotification[]): string {
  return plan.map((item) => `${item.id}@${scheduleDateString(item.at)}`).join('|');
}
