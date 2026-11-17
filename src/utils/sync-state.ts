import type { SyncProblem, SyncStatus } from '@shared/schema';
import { daysBetween } from '@/utils/milestones';
import { uploadsKeepUnsynced } from '@/utils/sync-attachments';

/** Whole UTC days without a success after which the person is asked to look (Rust: `SYNC_STALE_DAYS`). */
export const SYNC_STALE_DAYS = 7;

/**
 * The engine retries these by itself: the server is out of reach for now (Rust: `TRANSIENT_PROBLEMS`).
 * Every surface shows it as a problem from the first failed round (`unreachable`, red), and what
 * needs the server waits (`serverOutOfReach`).
 */
export const TRANSIENT_PROBLEMS: readonly SyncProblem[] = ['offline', 'relayError', 'rateLimited'];

export type SyncStateKind =
  | 'synced'
  | 'uploading'
  | 'syncing'
  | 'syncingLong'
  | 'firstUpload'
  | 'removing'
  | 'unreachable'
  | 'unreachableLong'
  | 'waiting'
  | 'paused'
  | 'error';

/** A round that runs longer than this (seconds) reads "taking longer than usual", with a retry. */
export const SYNC_SLOW_AFTER_SECS = 60;

export type SyncTone = 'fresh' | 'neutral' | 'stale';

export interface SyncStateView {
  kind: SyncStateKind;
  tone: SyncTone;
  /** `deviceSync` key of the label: `indicator.<kind>` (plural for `unreachableLong`, `count: days`); `attachments.uploading` for `uploading`. */
  labelKey: string;
  /** `deviceSync` key of the phone's Settings row: `phone.row.<kind>`; `attachments.uploading` for `uploading`. */
  rowKey: string;
  /** Whole UTC days without a success (`unreachableLong` only), else 0. */
  days: number;
  /** What the age after the label counts from: `lastSuccessAt`; `null` for `unreachableLong` or without a success. */
  ageFrom: number | null;
}

const TONES: Record<SyncStateKind, SyncTone> = {
  synced: 'fresh',
  uploading: 'neutral',
  syncing: 'neutral',
  syncingLong: 'neutral',
  firstUpload: 'neutral',
  removing: 'neutral',
  // A problem from the first failed round (owner, 2026-11-17): red and named on every surface
  unreachable: 'stale',
  unreachableLong: 'stale',
  waiting: 'stale',
  paused: 'stale',
  error: 'stale',
};

function view(kind: SyncStateKind, status: SyncStatus, days = 0): SyncStateView {
  return {
    kind,
    tone: TONES[kind],
    // "Uploading documents and photos" has one wording on every surface
    labelKey: kind === 'uploading' ? 'attachments.uploading' : `indicator.${kind}`,
    rowKey: kind === 'uploading' ? 'attachments.uploading' : `phone.row.${kind}`,
    days,
    ageFrom: kind === 'unreachableLong' ? null : status.lastSuccessAt,
  };
}

/**
 * Whole UTC days the server has been out of reach, counted from the last success (or when sync was
 * turned on); `null` unless the status carries a transient problem. Phase does not matter.
 */
function outOfReachDays(status: SyncStatus, now: number): number | null {
  if (!status.problem || !TRANSIENT_PROBLEMS.includes(status.problem)) return null;
  const anchor = status.lastSuccessAt ?? status.enabledAt;
  return anchor === null ? 0 : Math.max(0, daysBetween(anchor, now));
}

/**
 * The server is out of reach under 7 days: the status line itself says so (red) with the line that
 * sync resumes on its own, and no alert box comes with it (that waits for `unreachableLong`, with
 * the problem's text). Also while a retry round runs. `now` in unix seconds.
 */
export function isRecentlyOutOfReach(status: SyncStatus, now: number): boolean {
  const days = outOfReachDays(status, now);
  return days !== null && days < SYNC_STALE_DAYS;
}

/**
 * The sync server is out of reach (a transient problem, whatever the phase and however long):
 * actions that only the server can carry out (adding or removing a device, a new recovery kit,
 * deleting the synced data, a repair or re-sync, a checkout, downloading everything) wait, each
 * with the line that says why. `false` while sync is off or being set up.
 */
export function serverOutOfReach(status: SyncStatus | undefined): boolean {
  if (!status || status.phase === 'off' || status.phase === 'settingUp') return false;
  return status.problem !== null && TRANSIENT_PROBLEMS.includes(status.problem);
}

/** What a running round reads as: finishing a removal, the first upload, one that is slow, or plain syncing. */
function syncingKind(status: SyncStatus, now: number): SyncStateKind {
  if (status.removalPending) return 'removing';
  if (status.firstUpload) return 'firstUpload';
  if (status.syncingSince !== null && now - status.syncingSince > SYNC_SLOW_AFTER_SECS) {
    return 'syncingLong';
  }
  return 'syncing';
}

/**
 * `now` in unix seconds. `null` while sync is off or being set up (no indicator). Precedence: the
 * server out of reach (stale from the first failed round, `unreachableLong` after 7 days; a retry
 * round keeps it, so the label never flickers back to "syncing"), then a round running, then a
 * problem phase, then uploading (idle while documents and photos wait), then synchronised. An
 * attachments problem never changes the label (`shownProblem` adds its own line elsewhere).
 */
export function syncState(status: SyncStatus | undefined, now: number): SyncStateView | null {
  if (!status || status.phase === 'off' || status.phase === 'settingUp') return null;
  const days = outOfReachDays(status, now);
  if (days !== null) {
    return days >= SYNC_STALE_DAYS
      ? view('unreachableLong', status, days)
      : view('unreachable', status);
  }
  if (status.phase === 'syncing') return view(syncingKind(status, now), status);
  // The first copy goes up while the rest of the sync is idle: say so instead of "synchronised"
  if (status.phase === 'idle' && status.firstUpload) return view('firstUpload', status);
  // A key change another device made, not yet confirmed: the ordinary "syncing", not a problem
  if (status.problem === 'keyRotated' && status.waitingForRemover === null) {
    return view('syncing', status);
  }
  switch (status.phase) {
    case 'idle':
      return view(uploadsKeepUnsynced(status) ? 'uploading' : 'synced', status);
    // Held (stale) here, while the Rust reminder is not raised for `waiting` without a problem:
    // a held change with no named problem gives the person nothing to do.
    case 'waiting':
      return view('waiting', status);
    case 'paused':
      return view('paused', status);
    case 'error':
      return view('error', status);
    default: {
      const unhandled: never = status.phase;
      return unhandled;
    }
  }
}

export type SyncAgeKey = 'justNow' | 'minutes' | 'hours' | 'days';

/** "před 4 min": whole minutes/hours/days since `from`; under a minute or in the future is `justNow` (count 0). */
export function syncAge(from: number, now: number): { key: SyncAgeKey; count: number } {
  const elapsed = now - from;
  if (elapsed < 60) return { key: 'justNow', count: 0 };
  if (elapsed < 3_600) return { key: 'minutes', count: Math.floor(elapsed / 60) };
  if (elapsed < 86_400) return { key: 'hours', count: Math.floor(elapsed / 3_600) };
  return { key: 'days', count: Math.floor(elapsed / 86_400) };
}

/** The problems that all mean "this device must connect again" (one state, one button). */
export const REJOIN_PROBLEMS: readonly SyncProblem[] = [
  'keyNotGiven',
  'keyChangeUnverified',
  'unauthorized',
];

export type SyncProblemStateKind = 'rejoin' | 'waitingForRemover';

export interface SyncProblemState {
  kind: SyncProblemStateKind;
  /** `rejoin`: the device that made the key change, which this device does not know (`rejoinBy`); `waitingForRemover`: the device awaited. */
  by: string | null;
  /** `waitingForRemover`: since when (unix seconds). */
  since: number | null;
}

/**
 * The state the status card shows for a key-change problem: `keyNotGiven`, `keyChangeUnverified`
 * and `unauthorized` are one "connect this device again" state; a confirmation awaited for more
 * than a day (`waitingForRemover`, beside `keyChangeWaiting` or `keyRotated`) is the waiting state.
 * `null` for any other problem. A rejoin wins over waiting.
 */
export function syncProblemState(status: SyncStatus | undefined): SyncProblemState | null {
  if (!status || status.phase === 'off' || status.phase === 'settingUp') return null;
  const problem = status.problem;
  if (problem !== null && REJOIN_PROBLEMS.includes(problem)) {
    return { kind: 'rejoin', by: problem === 'unauthorized' ? null : status.rejoinBy, since: null };
  }
  if (
    status.waitingForRemover !== null &&
    (problem === 'keyChangeWaiting' || problem === 'keyRotated')
  ) {
    return {
      kind: 'waitingForRemover',
      by: status.waitingForRemover.by,
      since: status.waitingForRemover.since,
    };
  }
  return null;
}

export type PopoverLineKind =
  'removed' | 'afterKitJoin' | 'removalNotice' | 'newDevices' | 'kitUsed';

/** One extra line of the top-bar "Stav dat" popover: its `deviceSync` key, text values and unix-second dates to format. */
export interface PopoverLine {
  kind: PopoverLineKind;
  key: string;
  values: Record<string, string | number>;
  /** Names of values that are unix seconds the view formats as dates. */
  dates: string[];
}

/**
 * The lines the popover adds for notices: this device was removed (readable with sync off), the
 * Kit joiner's decision, another device's key change, new devices and a used recovery kit; one
 * line each, in that order.
 */
export function statusPopoverLines(status: SyncStatus | undefined): PopoverLine[] {
  if (!status) return [];
  const lines: PopoverLine[] = [];
  if (status.removedNotice) {
    lines.push({
      kind: 'removed',
      key: status.removedNotice.left ? 'popover.line.left' : 'popover.line.removed',
      values: {},
      dates: [],
    });
  }
  if (status.phase === 'off' || status.phase === 'settingUp') return lines;
  if (status.afterKitJoin) {
    lines.push({ kind: 'afterKitJoin', key: 'popover.line.afterKitJoin', values: {}, dates: [] });
  }
  const notice = status.removalNotice;
  if (notice) {
    const removedNothing = notice.removed.length === 0;
    lines.push({
      kind: 'removalNotice',
      key:
        removedNothing && notice.kitReplaced ? 'popover.line.newKit' : 'popover.line.removalNotice',
      values: { by: notice.by, names: notice.removed.join(', ') },
      dates: [],
    });
  }
  if (status.newDevices.length > 0) {
    lines.push({
      kind: 'newDevices',
      key: 'popover.line.newDevices',
      values: { names: status.newDevices.map((device) => device.name).join(', ') },
      dates: [],
    });
  }
  if (status.kitUsed !== null) {
    lines.push({
      kind: 'kitUsed',
      key: 'popover.line.kitUsed',
      values: { at: status.kitUsed },
      dates: ['at'],
    });
  }
  return lines;
}
