import { describe, expect, it } from 'vitest';
import { onboardingSteps, stepAfterRecovery } from './onboarding-steps';

describe('onboardingSteps', () => {
  it('runs the five steps of a new profile', () => {
    expect(onboardingSteps('new')).toEqual([
      'welcome',
      'account',
      'recovery',
      'areas',
      'firstAccount',
    ]);
  });

  it('replaces the areas and the first account with Připojení on the join path', () => {
    expect(onboardingSteps('join')).toEqual(['welcome', 'account', 'recovery', 'connect']);
  });

  it('keeps Připojení and adds the normal steps once it was left without sync', () => {
    expect(onboardingSteps('declined')).toEqual([
      'welcome',
      'account',
      'recovery',
      'connect',
      'areas',
      'firstAccount',
    ]);
  });
});

describe('stepAfterRecovery', () => {
  it('goes to Připojení on the join path only', () => {
    expect(stepAfterRecovery('join')).toBe('connect');
    expect(stepAfterRecovery('new')).toBe('areas');
    expect(stepAfterRecovery('declined')).toBe('areas');
  });
});
