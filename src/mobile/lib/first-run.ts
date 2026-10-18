/**
 * The order of the phone's first run (spec §8.2): the desktop wizard's five steps in one column,
 * then the two the phone adds in the "explain first, then the system prompt" pattern. Pure: the
 * flow's state lives in `use-first-run.ts`, the steps in `src/mobile/pages/first-run/`.
 */

/** The steps in the order they come: the encrypted database is created at the end of `recovery`. */
export const FIRST_RUN_STEPS = [
  'privacy',
  'account',
  'recovery',
  'areas',
  'firstAccount',
  'biometrics',
  'backup',
] as const;

export type FirstRunStep = (typeof FIRST_RUN_STEPS)[number];

/** The position of a step in the full order, which the shorter lists keep. */
const position = (step: FirstRunStep) => FIRST_RUN_STEPS.indexOf(step);

/** The steps this phone goes through: "Biometrie" is left out where biometry cannot be used. */
export function firstRunSteps(input: { biometryAvailable: boolean }): FirstRunStep[] {
  return FIRST_RUN_STEPS.filter((step) => step !== 'biometrics' || input.biometryAvailable);
}

/**
 * Whether leaving this step still leaves nothing behind: until the recovery key has been
 * confirmed there is no database, only the form and a key that can be discarded. From the areas
 * step on the account exists and so does its data, and there is no way back.
 */
export function canLeaveWithoutAccount(step: FirstRunStep): boolean {
  return position(step) < position('areas');
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

/** The step before `current`, or `null` on the first one and from the areas step on (the account exists). */
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
