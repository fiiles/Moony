import { describe, expect, it } from 'vitest';
import {
  FIRST_RUN_STEPS,
  canLeaveWithoutAccount,
  firstRunSteps,
  nextStep,
  previousStep,
  stepIndex,
} from './first-run';

const withBiometry = firstRunSteps({ biometryAvailable: true });
const withoutBiometry = firstRunSteps({ biometryAvailable: false });

describe('firstRunSteps', () => {
  it('lists seven steps with biometry and six without', () => {
    expect(withBiometry).toEqual([...FIRST_RUN_STEPS]);
    expect(withBiometry).toHaveLength(7);
    expect(withoutBiometry).toEqual([
      'privacy',
      'account',
      'recovery',
      'areas',
      'firstAccount',
      'backup',
    ]);
  });
});

describe('nextStep and previousStep', () => {
  it('walks forward and back; no way back once the database exists', () => {
    const steps = firstRunSteps({ biometryAvailable: true });
    expect(nextStep(steps, 'privacy')).toBe('account');
    expect(previousStep(steps, 'account')).toBe('privacy');
    expect(previousStep(steps, 'areas')).toBeNull();
    expect(nextStep(steps, 'backup')).toBeNull();
  });

  it('has nothing before the first step', () => {
    expect(previousStep(withBiometry, 'privacy')).toBeNull();
  });

  it('goes back from the recovery key to the account form: the database is not there yet', () => {
    expect(previousStep(withBiometry, 'recovery')).toBe('account');
  });

  it('has no way back from any step once the account exists', () => {
    for (const step of ['areas', 'firstAccount', 'biometrics', 'backup'] as const) {
      expect(previousStep(withBiometry, step)).toBeNull();
    }
  });

  it('skips the biometrics step when biometry is unavailable', () => {
    expect(nextStep(withoutBiometry, 'firstAccount')).toBe('backup');
    expect(nextStep(withBiometry, 'firstAccount')).toBe('biometrics');
    expect(nextStep(withBiometry, 'biometrics')).toBe('backup');
  });

  it('still moves on when the list shrinks under the step that is showing', () => {
    // The status is re-read after "Zapnout"; a step that is no longer listed must not strand the person
    expect(nextStep(withoutBiometry, 'biometrics')).toBe('backup');
  });
});

describe('stepIndex', () => {
  it('numbers the steps from one', () =>
    expect(stepIndex(withoutBiometry, 'backup')).toEqual({ index: 6, total: 6 }));

  it('counts seven with biometry', () => {
    expect(stepIndex(withBiometry, 'privacy')).toEqual({ index: 1, total: 7 });
    expect(stepIndex(withBiometry, 'biometrics')).toEqual({ index: 6, total: 7 });
    expect(stepIndex(withBiometry, 'backup')).toEqual({ index: 7, total: 7 });
  });

  it('places a step that is no longer listed where it stood', () => {
    expect(stepIndex(withoutBiometry, 'biometrics')).toEqual({ index: 6, total: 6 });
  });
});

describe('canLeaveWithoutAccount', () => {
  it('is true while there is no database and false from the areas step on', () => {
    expect(FIRST_RUN_STEPS.map((step) => [step, canLeaveWithoutAccount(step)] as const)).toEqual([
      ['privacy', true],
      ['account', true],
      ['recovery', true],
      ['areas', false],
      ['firstAccount', false],
      ['biometrics', false],
      ['backup', false],
    ]);
  });
});
