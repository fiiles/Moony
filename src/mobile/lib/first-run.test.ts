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
    expect(withBiometry).toEqual(FIRST_RUN_STEPS.filter((step) => step !== 'connect'));
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

describe('firstRunSteps by path (spec 2026-11-14 §5.2)', () => {
  it('the join path goes from the key to Připojení and ends with biometrics, never Záloha', () => {
    expect(firstRunSteps({ biometryAvailable: true, canBackup: true, path: 'join' })).toEqual([
      'privacy',
      'account',
      'recovery',
      'connect',
      'biometrics',
    ]);
    expect(firstRunSteps({ biometryAvailable: false, canBackup: true, path: 'join' })).toEqual([
      'privacy',
      'account',
      'recovery',
      'connect',
    ]);
  });

  it('a declined join continues with every normal step', () => {
    expect(firstRunSteps({ biometryAvailable: true, canBackup: true, path: 'declined' })).toEqual([
      ...FIRST_RUN_STEPS,
    ]);
    expect(FIRST_RUN_STEPS).toHaveLength(8);
  });

  it('the new path has no Připojení', () => {
    expect(firstRunSteps({ biometryAvailable: true, canBackup: true })).toEqual([
      'privacy',
      'account',
      'recovery',
      'areas',
      'firstAccount',
      'biometrics',
      'backup',
    ]);
    expect(firstRunSteps({ biometryAvailable: true, canBackup: true, path: 'new' })).not.toContain(
      'connect'
    );
  });

  it('has no way back from Připojení', () => {
    const joinSteps = firstRunSteps({ biometryAvailable: true, path: 'join' });
    expect(canLeaveWithoutAccount('connect')).toBe(false);
    expect(previousStep(joinSteps, 'connect')).toBeNull();
    expect(canLeaveWithoutAccount('recovery')).toBe(true);
  });

  it('ends the join path after Připojení without biometry', () => {
    expect(nextStep(['privacy', 'account', 'recovery', 'connect'], 'connect')).toBeNull();
    expect(nextStep(firstRunSteps({ biometryAvailable: true, path: 'join' }), 'connect')).toBe(
      'biometrics'
    );
    expect(nextStep(firstRunSteps({ biometryAvailable: true, path: 'join' }), 'recovery')).toBe(
      'connect'
    );
  });

  it('goes from the key to Co sledovat on the new path', () => {
    expect(nextStep(withBiometry, 'recovery')).toBe('areas');
  });
});

describe('firstRunSteps without backups', () => {
  it('leaves the backup step out where a backup cannot be made (Android for now)', () => {
    const steps = firstRunSteps({ biometryAvailable: true, canBackup: false });
    expect(steps).not.toContain('backup');
    expect(steps.at(-1)).toBe('biometrics');
    expect(nextStep(steps, 'biometrics')).toBeNull();
  });

  it('keeps the last step with the sync flag on: its first way is sync, which needs no backup', () => {
    const steps = firstRunSteps({ biometryAvailable: true, canBackup: false, syncFeature: true });
    expect(steps.at(-1)).toBe('backup');
    // …and still never after a join
    expect(
      firstRunSteps({ biometryAvailable: true, canBackup: false, syncFeature: true, path: 'join' })
    ).not.toContain('backup');
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
  it('is true while there is no database and false from Připojení on', () => {
    expect(FIRST_RUN_STEPS.map((step) => [step, canLeaveWithoutAccount(step)] as const)).toEqual([
      ['privacy', true],
      ['account', true],
      ['recovery', true],
      ['connect', false],
      ['areas', false],
      ['firstAccount', false],
      ['biometrics', false],
      ['backup', false],
    ]);
  });
});
