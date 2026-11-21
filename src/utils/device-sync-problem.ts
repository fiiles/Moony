import type { HeldGapKind, HeldGapView, SyncProblem, SyncStatus } from '@shared/schema';

/** The action the problem alert offers. */
export type SyncProblemAction =
  'renew' | 'askOperator' | 'turnOffOrRetry' | 'repair' | 'skipMissing' | 'updateApp' | 'none';

const DAY_SECONDS = 86_400;

/** A translation function, as `useTranslation('deviceSync').t` gives it. */
type Translate = (key: string, options?: Record<string, unknown>) => string;

/**
 * The `deviceSync` key of the status text for `problem`: a key change this device made itself
 * that is still finishing (`ownChangeFinishing`) has its own text under `keyChangeWaiting`.
 */
export function problemTextKey(
  problem: SyncProblem,
  status: Pick<SyncStatus, 'ownChangeFinishing'>
): string {
  return problem === 'keyChangeWaiting' && status.ownChangeFinishing
    ? 'status.problem.keyChangeWaitingOwn'
    : `status.problem.${problem}`;
}

/**
 * The held changes an alert for `problem` is about: a gap's are the missing changes and those
 * waiting for a parent; `clockAhead`'s are the changes dated more than ten years ahead; none for
 * any other problem.
 */
export function heldFor(
  problem: SyncProblem,
  gaps: readonly HeldGapView[] | undefined
): HeldGapView[] {
  const kinds: HeldGapKind[] =
    problem === 'gap' ? ['missing', 'parent'] : problem === 'clockAhead' ? ['ahead'] : [];
  return (gaps ?? []).filter((gap) => kinds.includes(gap.kind));
}

/** The oldest held change dated more than ten years ahead, or `null`. */
export function aheadGap(gaps: readonly HeldGapView[] | undefined): HeldGapView | null {
  const ahead = heldFor('clockAhead', gaps);
  if (ahead.length === 0) return null;
  return ahead.reduce((best, gap) => (gap.since < best.since ? gap : best));
}

/**
 * The alert's text for a held change dated more than ten years ahead: its device and the date it
 * carries (`dateText` formats unix seconds), and that the wait goes on after that device's clock
 * is corrected.
 */
export function aheadText(
  gap: HeldGapView,
  t: Translate,
  dateText: (seconds: number) => string
): string {
  const device = heldGapDevice(gap);
  const date = gap.datedAt === null ? '' : dateText(gap.datedAt);
  return date
    ? t('alert.clockAhead.text', { device, date })
    : t('alert.clockAhead.textFar', { device });
}

/** The skip confirmation's texts for `gap`, on desktop and phone alike. */
export function skipGapTexts(
  gap: HeldGapView,
  t: Translate,
  dateText: (seconds: number) => string
): { title: string; description: string; skipped: string; unchanged: string } {
  const device = heldGapDevice(gap);
  const count = gap.missingChanges;
  if (gap.kind === 'ahead') {
    const date = gap.datedAt === null ? '' : dateText(gap.datedAt);
    return {
      title: t('skipGap.titleAhead'),
      description: date
        ? t('skipGap.descriptionAhead', { device, date })
        : t('skipGap.descriptionAheadFar', { device }),
      skipped: t('skipGap.successAhead'),
      unchanged: t('skipGap.appliedAhead'),
    };
  }
  return {
    title: t('skipGap.title', { count }),
    description:
      gap.kind === 'parent'
        ? t('skipGap.descriptionParent', { device })
        : t('skipGap.description', { device, count }),
    skipped: t('skipGap.success', { count }),
    unchanged: t('skipGap.arrived'),
  };
}

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
 * The problems whose alert also offers "Synchronizovat toto zařízení znovu" (it replaces this
 * device's data with the synchronised ones, after a backup): the changes of other devices cannot
 * be used here and nothing else fixes it. Not the gap (the answer is to wait or skip the change),
 * not the problems of the server or the subscription, and not the ones with their own card.
 */
export function offersResync(problem: SyncProblem): boolean {
  switch (problem) {
    case 'undecryptable':
    case 'invalidChain':
    case 'missingParent':
    case 'quarantined':
    case 'internal':
      return true;
    default:
      return false;
  }
}

/**
 * Which action the alert shows for a problem; a lapsed account on an own server has no
 * Renew. A gap (a change from another device that has not arrived) offers nothing destructive while
 * it is younger than 14 days: this device keeps syncing and sending its own changes meanwhile. Once
 * a held change may be skipped (`skippable`, from `skippableGap`), the alert offers to skip it;
 * Re-sync is never the gap's answer (it would replace this device's data, and the gap would
 * come back with the same missing change); the alert offers it only after a repair that could
 * not send every change. A change dated more than ten years ahead
 * (`clockAhead`) is offered for skipping the same way, from its own alert.
 */
export function problemAction(
  problem: SyncProblem,
  ownServer: boolean,
  skippable: HeldGapView | null = null
): SyncProblemAction {
  switch (problem) {
    case 'lapsed':
      return ownServer ? 'askOperator' : 'renew';
    case 'vaultGone':
      return 'turnOffOrRetry';
    case 'captureBroken':
      return 'repair';
    case 'gap':
    case 'clockAhead':
      // A change dated more than ten years ahead waits like a gap: skippable after 14 days.
      return skippable ? 'skipMissing' : 'none';
    case 'newerSchema':
      return 'updateApp';
    case 'serverWentBack':
      // No action yet: the text says how to bring the devices back together (turn sync off,
      // set it up again on the device with the most complete data, join the others) and not to
      // re-sync, which would replace this device's data with the server's older copy.
      return 'none';
    // `keyNotGiven`, `keyChangeUnverified`, `unauthorized` and a long `keyChangeWaiting` are
    // answered by the rejoin and waiting states (`syncProblemState`), which the alert shows
    // before it asks for an action.
    case 'quotaExceeded':
    case 'snapshotTooLarge':
    case 'undecryptable':
    case 'invalidChain':
    case 'missingParent':
    case 'quarantined':
    case 'offline':
    case 'relayError':
    case 'rateLimited':
    case 'unauthorized':
    case 'keyRotated':
    case 'keyNotGiven':
    case 'keyChangeUnverified':
    case 'keyChangeWaiting':
    case 'secureStorage':
    case 'internal':
      return 'none';
    default: {
      // A new SyncProblem must be placed above: this fails to compile until it is
      const unhandled: never = problem;
      return unhandled;
    }
  }
}
