/**
 * "Check for updates automatically" (Settings → Security → Updates): a
 * device-level switch stored in `localStorage`, on by default.
 */
import {
  readSetting,
  writeSetting,
  type ReadableStorage,
  type WritableStorage,
} from '@/utils/device-storage';

export const AUTO_UPDATE_CHECK_STORAGE_KEY = 'moony-auto-update-check';

/** Anything but an explicit "false" keeps the default (on). */
export function parseAutoUpdateCheck(raw: string | null | undefined): boolean {
  return raw !== 'false';
}

export function readAutoUpdateCheck(storage?: ReadableStorage): boolean {
  return parseAutoUpdateCheck(readSetting(AUTO_UPDATE_CHECK_STORAGE_KEY, storage));
}

export function writeAutoUpdateCheck(enabled: boolean, storage?: WritableStorage): void {
  writeSetting(AUTO_UPDATE_CHECK_STORAGE_KEY, String(enabled), storage);
}
