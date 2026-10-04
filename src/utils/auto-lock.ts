/**
 * Auto-lock after inactivity: the device-level setting and the pure
 * deadline logic behind `useAutoLock()` (src/hooks/use-auto-lock.ts). Times are
 * epoch milliseconds passed in by the caller, so nothing here touches the DOM
 * or the clock and all of it is unit-testable.
 *
 * The setting lives in `localStorage` (`moony-auto-lock-minutes`): it applies
 * to this machine, and the lock screen must honour it before any profile is read.
 */
import {
  readSetting,
  writeSetting,
  type ReadableStorage,
  type WritableStorage,
} from '@/utils/device-storage';

export const AUTO_LOCK_STORAGE_KEY = 'moony-auto-lock-minutes';

/** Minutes of inactivity before locking; 0 turns auto-lock off. */
export const AUTO_LOCK_OPTIONS = [0, 5, 15, 30, 60] as const;
export type AutoLockMinutes = (typeof AUTO_LOCK_OPTIONS)[number];

export const DEFAULT_AUTO_LOCK_MINUTES: AutoLockMinutes = 15;

/** Input events closer together than this are not recorded again. */
export const ACTIVITY_THROTTLE_MS = 1000;

const MS_PER_MINUTE = 60_000;

/** Unknown, missing or out-of-list values fall back to the default. */
export function parseAutoLockMinutes(raw: string | null | undefined): AutoLockMinutes {
  if (raw === null || raw === undefined || raw.trim() === '') return DEFAULT_AUTO_LOCK_MINUTES;
  const value = Number(raw);
  return AUTO_LOCK_OPTIONS.find((option) => option === value) ?? DEFAULT_AUTO_LOCK_MINUTES;
}

export function readAutoLockMinutes(storage?: ReadableStorage): AutoLockMinutes {
  return parseAutoLockMinutes(readSetting(AUTO_LOCK_STORAGE_KEY, storage));
}

export function writeAutoLockMinutes(minutes: AutoLockMinutes, storage?: WritableStorage): void {
  writeSetting(AUTO_LOCK_STORAGE_KEY, String(minutes), storage);
}

/** When the app must lock if no further activity happens; `null` while auto-lock is off. */
export function autoLockDeadline(lastActivityMs: number, minutes: AutoLockMinutes): number | null {
  return minutes > 0 ? lastActivityMs + minutes * MS_PER_MINUTE : null;
}

/**
 * True once the idle time has reached the limit. A clock that jumped backwards
 * (now < last activity) is never "due"; a clock that jumped forwards, such as
 * after the computer slept, is.
 */
export function isAutoLockDue(
  nowMs: number,
  lastActivityMs: number,
  minutes: AutoLockMinutes
): boolean {
  const deadline = autoLockDeadline(lastActivityMs, minutes);
  return deadline !== null && nowMs >= deadline && nowMs >= lastActivityMs;
}

/**
 * Milliseconds to wait before the next check, never negative and never longer
 * than one full interval (so a backwards clock jump cannot stall the lock).
 * `null` while auto-lock is off.
 */
export function msUntilAutoLock(
  nowMs: number,
  lastActivityMs: number,
  minutes: AutoLockMinutes
): number | null {
  const deadline = autoLockDeadline(lastActivityMs, minutes);
  if (deadline === null) return null;
  return Math.min(minutes * MS_PER_MINUTE, Math.max(0, deadline - nowMs));
}

/** Throttle for input events: record at most once per `throttleMs` (and after a backwards jump). */
export function shouldRecordActivity(
  nowMs: number,
  lastRecordedMs: number,
  throttleMs: number = ACTIVITY_THROTTLE_MS
): boolean {
  return nowMs < lastRecordedMs || nowMs - lastRecordedMs >= throttleMs;
}
