/** The desktop first run's steps (spec 2026-11-14 §5.1). */
export type OnboardingStep =
  'welcome' | 'account' | 'recovery' | 'connect' | 'areas' | 'firstAccount';

/** `new`: today's run; `join`: Připojení after the key; `declined`: Připojení was left without sync, the normal steps follow. */
export type OnboardingPath = 'new' | 'join' | 'declined';

const STEPS: Record<OnboardingPath, OnboardingStep[]> = {
  new: ['welcome', 'account', 'recovery', 'areas', 'firstAccount'],
  join: ['welcome', 'account', 'recovery', 'connect'],
  declined: ['welcome', 'account', 'recovery', 'connect', 'areas', 'firstAccount'],
};

/** The steps the wizard shows for a path, in order. */
export function onboardingSteps(path: OnboardingPath): OnboardingStep[] {
  return [...STEPS[path]];
}

/** Where the run goes once the database exists: `connect` on the join path, else `areas`. */
export function stepAfterRecovery(path: OnboardingPath): 'connect' | 'areas' {
  return path === 'join' ? 'connect' : 'areas';
}
