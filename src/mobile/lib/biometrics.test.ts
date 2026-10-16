import { describe, expect, it } from 'vitest';
import type { BiometricStatus } from '@shared/schema';
import {
  BIOMETRICS_ASKED_KEY,
  biometricCopy,
  biometricFailure,
  biometryKind,
  canUnlockWithBiometrics,
  clearEnrolmentAsked,
  markEnrolmentAsked,
  shouldOfferEnrolment,
  wasEnrolmentAsked,
} from './biometrics';

function status(patch: Partial<BiometricStatus> = {}): BiometricStatus {
  return { available: true, enrolled: false, biometryType: 'faceId', reason: null, ...patch };
}

/** A Map behind the Storage methods the helpers use. */
function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    removeItem: (key: string) => void values.delete(key),
  };
}

const throwing = {
  getItem: (): string | null => {
    throw new Error('blocked');
  },
  setItem: (): void => {
    throw new Error('blocked');
  },
  removeItem: (): void => {
    throw new Error('blocked');
  },
};

describe('biometryKind and biometricCopy', () => {
  it('words Face ID as the face, Touch ID as the fingerprint, anything else generically', () => {
    expect(biometryKind('faceId')).toBe('face');
    expect(biometryKind('touchId')).toBe('touch');
    expect(biometryKind('biometric')).toBe('generic');
    expect(biometryKind('none')).toBe('generic');
  });

  it('picks the lock button and the sheet copy of the same kind', () => {
    expect(biometricCopy('faceId')).toEqual({
      unlock: 'lock.unlockWithFace',
      title: 'biometrics.titleFace',
      lead: 'biometrics.leadFace',
    });
    expect(biometricCopy('touchId').unlock).toBe('lock.unlockWithTouch');
    expect(biometricCopy('biometric').title).toBe('biometrics.titleGeneric');
  });
});

describe('canUnlockWithBiometrics', () => {
  it('follows the switch; an unusable biometry is the keychain answer to show', () => {
    expect(canUnlockWithBiometrics(status({ enrolled: true }))).toBe(true);
    // Enrolment removed or locked out: the prompt-free answer says invalidated or failed.
    expect(canUnlockWithBiometrics(status({ enrolled: true, available: false }))).toBe(true);
    expect(canUnlockWithBiometrics(status({ enrolled: false }))).toBe(false);
    expect(canUnlockWithBiometrics(undefined)).toBe(false);
  });
});

describe('shouldOfferEnrolment', () => {
  it('offers once on a phone that can but does not yet unlock with biometrics', () => {
    expect(shouldOfferEnrolment(status(), false)).toBe(true);
    expect(shouldOfferEnrolment(status(), true)).toBe(false);
    expect(shouldOfferEnrolment(status({ enrolled: true }), false)).toBe(false);
    expect(shouldOfferEnrolment(status({ available: false }), false)).toBe(false);
    expect(shouldOfferEnrolment(undefined, false)).toBe(false);
  });
});

describe('the asked flag', () => {
  it('is unset until marked, and marking keeps it', () => {
    const storage = memoryStorage();
    expect(wasEnrolmentAsked(storage)).toBe(false);
    markEnrolmentAsked(storage);
    expect(wasEnrolmentAsked(storage)).toBe(true);
    expect(storage.getItem(BIOMETRICS_ASKED_KEY)).toBe('1');
  });

  it('counts unreadable or missing storage as asked, so the sheet never nags', () => {
    expect(wasEnrolmentAsked(throwing)).toBe(true);
    expect(wasEnrolmentAsked(null)).toBe(true);
    expect(() => markEnrolmentAsked(throwing)).not.toThrow();
    expect(() => markEnrolmentAsked(null)).not.toThrow();
  });

  it('is forgotten by clearing, so a new account on the same phone is asked again', () => {
    const storage = memoryStorage();
    markEnrolmentAsked(storage);
    expect(wasEnrolmentAsked(storage)).toBe(true);
    clearEnrolmentAsked(storage);
    expect(wasEnrolmentAsked(storage)).toBe(false);
    expect(storage.getItem(BIOMETRICS_ASKED_KEY)).toBeNull();
    // Nothing to clear is fine, and so is storage that cannot be reached.
    expect(() => clearEnrolmentAsked(storage)).not.toThrow();
    expect(() => clearEnrolmentAsked(throwing)).not.toThrow();
    expect(() => clearEnrolmentAsked(null)).not.toThrow();
  });
});

describe('biometricFailure', () => {
  it('sorts the backend keys into what the lock screen does', () => {
    expect(biometricFailure('auth.biometricCancelled')).toBe('cancelled');
    expect(biometricFailure('auth.biometricInvalidated')).toBe('invalidated');
    expect(biometricFailure('auth.biometricFailed')).toBe('failed');
    expect(biometricFailure('auth.keyFilesCorrupted')).toBe('other');
    expect(biometricFailure(undefined)).toBe('other');
  });
});
