import type { SyncDevice, SyncPhase, SyncStatus } from '@shared/schema';
import { syncAge, type SyncAgeKey } from '@/utils/sync-state';

/** Whether a phase means sync is running. A `Record`, so a new phase must be decided here. */
const PHASE_IS_ON: Record<SyncPhase, boolean> = {
  off: false,
  settingUp: false,
  idle: true,
  syncing: true,
  waiting: true,
  paused: true,
  error: true,
};

/** Sync is on for this profile in a build that runs it: the flag is on and the phase is neither `off` nor `settingUp`. Everything part C changes for "sync on" reads as today without it. */
export function syncIsOn(featureOn: boolean, status: SyncStatus | undefined): boolean {
  return featureOn && status !== undefined && PHASE_IS_ON[status.phase];
}

export interface ProtectionLine {
  devices: number;
  age: { key: SyncAgeKey; count: number };
}

/** The §7.3 line: only with the flag on, `protectsData` and a `lastCompleteAt`, once the device list is in; `devices` = its length (at least 1), `age` = `syncAge(lastCompleteAt, now)`. `null` otherwise. */
export function protectionLine(
  featureOn: boolean,
  status: SyncStatus | undefined,
  devices: readonly SyncDevice[] | undefined,
  now: number
): ProtectionLine | null {
  if (!featureOn || !status || !status.protectsData || status.lastCompleteAt == null) return null;
  if (devices === undefined) return null;
  return {
    devices: Math.max(1, devices.length),
    age: syncAge(status.lastCompleteAt, now),
  };
}
