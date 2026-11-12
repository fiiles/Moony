import type { HeldGapView, SyncProblem } from '@shared/schema';

/** The action the problem alert offers. */
export type SyncProblemAction =
  | 'renew'
  | 'askOperator'
  | 'turnOffOrRetry'
  | 'repair'
  | 'resync'
  | 'skipMissing'
  | 'updateApp'
  | 'none';

const DAY_SECONDS = 86_400;

/** Whether a held change may be skipped at `now` (unix seconds): 14 days after it was first held. */
export function gapSkippable(gap: HeldGapView, now: number): boolean {
  return now >= gap.skippableAt;
}

/**
 * The held change the alert offers to skip at `now` (unix seconds): a missing change before one
 * waiting for a parent (skipping the missing one may bring the parent), then the oldest.
 */
export function skippableGap(
  gaps: readonly HeldGapView[] | undefined,
  now: number
): HeldGapView | null {
  const skippable = (gaps ?? []).filter((gap) => gapSkippable(gap, now));
  if (skippable.length === 0) return null;
  const rank = (gap: HeldGapView) => (gap.kind === 'missing' ? 0 : 1);
  return skippable.reduce((best, gap) =>
    rank(gap) < rank(best) || (rank(gap) === rank(best) && gap.since < best.since) ? gap : best
  );
}

/** When the first held change becomes skippable (unix seconds), while none is; else `null`. */
export function firstSkippableAt(
  gaps: readonly HeldGapView[] | undefined,
  now: number
): number | null {
  const list = gaps ?? [];
  if (list.length === 0 || list.some((gap) => gapSkippable(gap, now))) return null;
  return Math.min(...list.map((gap) => gap.skippableAt));
}

/**
 * How the alert and its confirmation name a held change's author: its name from the device list,
 * else the start of its device ID (a device whose row never arrived has no name here).
 */
export function heldGapDevice(gap: HeldGapView): string {
  return gap.deviceName ?? gap.deviceId.slice(0, 8);
}

/** Whole days a held change has waited at `now` (unix seconds). */
export function heldDays(gap: HeldGapView, now: number): number {
  return Math.max(0, Math.floor((now - gap.since) / DAY_SECONDS));
}

/**
 * Which action the alert shows for a problem; a lapsed subscription on a self-hosted server has no
 * Renew. A gap (a change from another device that has not arrived) offers nothing destructive while
 * it is younger than 14 days: this device keeps syncing and sending its own changes meanwhile. Once
 * a held change may be skipped (`skippable`, from `skippableGap`), the alert offers to skip it;
 * Re-sync is never the gap's answer (it would replace this device's data, and the gap would
 * come back with the same missing change).
 */
export function problemAction(
  problem: SyncProblem,
  selfHosted: boolean,
  skippable: HeldGapView | null = null
): SyncProblemAction {
  switch (problem) {
    case 'lapsed':
      return selfHosted ? 'askOperator' : 'renew';
    case 'vaultGone':
      return 'turnOffOrRetry';
    case 'captureBroken':
      return 'repair';
    case 'gap':
      return skippable ? 'skipMissing' : 'none';
    case 'newerSchema':
      return 'updateApp';
    case 'clockAhead':
      // No action: the change applies by itself once its time comes; the text says to check the
      // devices' date and time. It is never offered as a missing change to skip.
      return 'none';
    case 'serverWentBack':
      // No action yet: the text says how to bring the devices back together (turn sync off,
      // set it up again on the device with the most complete data, join the others).
      return 'none';
    default:
      return 'none';
  }
}
