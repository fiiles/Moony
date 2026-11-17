/**
 * The order of the phone's first run (spec §8.2): the desktop wizard's five steps in one column,
 * then the two the phone adds in the "explain first, then the system prompt" pattern. With sync
 * built in, the join path (spec 2026-11-14 §5.2) puts Připojení after the key. Pure: the flow's
 * state lives in `use-first-run.ts`, the steps in `src/mobile/pages/first-run/`.
 */
import type { OnboardingPath } from '@/utils/onboarding-steps';

/** The steps in the order they come: the encrypted database is created at the end of `recovery`. */
export const FIRST_RUN_STEPS = [
  'privacy',
  'account',
  'recovery',
  'connect',
  'areas',
  'firstAccount',
  'biometrics',
  'backup',
] as const;

export type FirstRunStep = (typeof FIRST_RUN_STEPS)[number];

/** Where the run ends: the overview, or Settings → Synchronizace ("Nastavte synchronizaci"). */
export type FirstRunExit = 'overview' | 'sync';

/** The position of a step in the full order, which the shorter lists keep. */
const position = (step: FirstRunStep) => FIRST_RUN_STEPS.indexOf(step);

/** The steps a join leaves out: the synced profile brings the areas and the accounts, and the backup. */
const NOT_AFTER_A_JOIN: readonly FirstRunStep[] = ['areas', 'firstAccount', 'backup'];

/**
 * The steps this phone goes through: "Biometrie" is left out where biometry cannot be used, and
 * "Záloha" where a backup cannot be made (`canBackup` false: Android for now, audit C7) unless the
 * sync flag is on (`syncFeature`): then the step leads with "Nastavit synchronizaci", which works
 * without backups. By path
 * (default `new`): `new` has no Připojení; `join` ends with Připojení and Biometrie; `declined`
 * (Připojení left without sync) has every step.
 */
export function firstRunSteps(input: {
  biometryAvailable: boolean;
  canBackup?: boolean;
  syncFeature?: boolean;
  path?: OnboardingPath;
}): FirstRunStep[] {
  const path = input.path ?? 'new';
  return FIRST_RUN_STEPS.filter(
    (step) =>
      (step !== 'connect' || path !== 'new') &&
      (path !== 'join' || !NOT_AFTER_A_JOIN.includes(step)) &&
      (step !== 'biometrics' || input.biometryAvailable) &&
      (step !== 'backup' || input.canBackup !== false || input.syncFeature === true)
  );
}

/**
 * Whether leaving this step still leaves nothing behind: until the recovery key has been
 * confirmed there is no database, only the form and a key that can be discarded. From Připojení
 * (or, without it, the areas step) on the account exists and so does its data, and there is no
 * way back.
 */
export function canLeaveWithoutAccount(step: FirstRunStep): boolean {
  return position(step) <= position('recovery');
}

/**
 * The step after `current`, or `null` after the last. `current` need not be in the list: the list
 * is recomputed when the biometry status is read again, and the person on "Biometrie" must still
 * move on when it no longer lists that step.
 */
export function nextStep(
  steps: readonly FirstRunStep[],
  current: FirstRunStep
): FirstRunStep | null {
  return steps.find((step) => position(step) > position(current)) ?? null;
}

/** The step before `current`, or `null` on the first one and once the account exists (Připojení on). */
export function previousStep(
  steps: readonly FirstRunStep[],
  current: FirstRunStep
): FirstRunStep | null {
  if (!canLeaveWithoutAccount(current)) return null;
  return [...steps].reverse().find((step) => position(step) < position(current)) ?? null;
}

/** "2 / 7": the 1-based place of `current` in the list that is shown, and the list's length. */
export function stepIndex(
  steps: readonly FirstRunStep[],
  current: FirstRunStep
): { index: number; total: number } {
  const before = steps.filter((step) => position(step) < position(current)).length;
  return { index: before + 1, total: steps.length };
}
