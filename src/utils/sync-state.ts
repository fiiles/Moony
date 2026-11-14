import type { SyncProblem, SyncStatus } from '@shared/schema';
import { daysBetween } from '@/utils/milestones';

/** Whole UTC days without a success after which the person is asked to look (Rust: `SYNC_STALE_DAYS`). */
export const SYNC_STALE_DAYS = 7;

/** The engine retries these by itself: the server is out of reach for now (Rust: `TRANSIENT_PROBLEMS`). */
export const TRANSIENT_PROBLEMS: readonly SyncProblem[] = ['offline', 'relayError', 'rateLimited'];

export type SyncStateKind =
  'synced' | 'syncing' | 'unreachable' | 'unreachableLong' | 'waiting' | 'paused' | 'error';

export type SyncTone = 'fresh' | 'neutral' | 'stale';

export interface SyncStateView {
  kind: SyncStateKind;
  tone: SyncTone;
  /** `deviceSync` key of the label: `indicator.<kind>` (plural for `unreachableLong`, `count: days`). */
  labelKey: string;
  /** `deviceSync` key of the phone's Settings row: `phone.row.<kind>`. */
  rowKey: string;
  /** Whole UTC days without a success (`unreachableLong` only), else 0. */
  days: number;
  /** What the age after the label counts from: `lastSuccessAt`; `null` for `unreachableLong` or without a success. */
  ageFrom: number | null;
}

const TONES: Record<SyncStateKind, SyncTone> = {
  synced: 'fresh',
  syncing: 'neutral',
  unreachable: 'neutral',
  unreachableLong: 'stale',
  waiting: 'stale',
  paused: 'stale',
  error: 'stale',
};

function view(kind: SyncStateKind, status: SyncStatus, days = 0): SyncStateView {
  return {
    kind,
    tone: TONES[kind],
    labelKey: `indicator.${kind}`,
    rowKey: `phone.row.${kind}`,
    days,
    ageFrom: kind === 'unreachableLong' ? null : status.lastSuccessAt,
  };
}

/** `now` in unix seconds. `null` while sync is off or being set up (no indicator). */
export function syncState(status: SyncStatus | undefined, now: number): SyncStateView | null {
  if (!status || status.phase === 'off' || status.phase === 'settingUp') return null;
  // A running retry round shows as syncing even when the server has long been out of reach; the
  // Rust reminder (`sync_stalled`) stays raised then, so it does not flicker between rounds.
  if (status.phase === 'syncing') return view('syncing', status);
  if (status.problem && TRANSIENT_PROBLEMS.includes(status.problem)) {
    const anchor = status.lastSuccessAt ?? status.enabledAt;
    if (anchor !== null) {
      const days = Math.max(0, daysBetween(anchor, now));
      if (days >= SYNC_STALE_DAYS) return view('unreachableLong', status, days);
    }
    return view('unreachable', status);
  }
  switch (status.phase) {
    case 'idle':
      return view('synced', status);
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
