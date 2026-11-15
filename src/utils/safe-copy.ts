import type { SyncPhase, SyncStatus } from '@shared/schema';

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
