import { describe, expect, it } from 'vitest';
import type { BiometricStatus } from '@shared/schema';
import {
  BIOMETRICS_ASKED_KEY,
  biometricCopy,
  biometricFailure,
  biometricNotice,
  biometricSettingState,
  biometryKind,
  canUnlockWithBiometrics,
  clearEnrolmentAsked,
  desktopBiometryLabel,
  enrolmentOfferOpen,
  markEnrolmentAsked,
  shouldAutoPrompt,
  shouldAutoPromptDesktop,
  shouldOfferEnrolment,
  wasEnrolmentAsked,
} from './biometrics';

function status(patch: Partial<BiometricStatus> = {}): BiometricStatus {
  return {
    available: true,
    enrolled: false,
    biometryType: 'faceId',
    reason: null,
    supported: true,
    ...patch,
  };
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
    expect(biometryKind('windowsHello')).toBe('generic');
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

describe('enrolmentOfferOpen', () => {
  const quiet = { offered: true, enabling: false, importOpen: false, sharedPending: false };

  it('opens the offer when it is due and nothing else is on the way', () => {
    expect(enrolmentOfferOpen(quiet)).toBe(true);
    expect(enrolmentOfferOpen({ ...quiet, offered: false })).toBe(false);
  });

  it('lets a shared statement come first: waits while one is on its way', () => {
    // The drain after the unlock has not answered yet, or a file waits for the sheet
    expect(enrolmentOfferOpen({ ...quiet, sharedPending: true })).toBe(false);
  });

  it('waits while the import sheet is open, and opens once it has closed', () => {
    expect(enrolmentOfferOpen({ ...quiet, importOpen: true })).toBe(false);
    expect(enrolmentOfferOpen({ ...quiet, importOpen: true, sharedPending: true })).toBe(false);
    expect(enrolmentOfferOpen(quiet)).toBe(true);
  });

  it('stays up while Zapnout runs, whatever else happens', () => {
    expect(
      enrolmentOfferOpen({ offered: false, enabling: true, importOpen: true, sharedPending: true })
    ).toBe(true);
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

describe('biometricNotice', () => {
  it('says why under the button, in the backend words for the two keyed failures', () => {
    expect(biometricNotice('invalidated', true)).toBe('backend');
    expect(biometricNotice('failed', true)).toBe('backend');
  });

  it('names an auth error without a key generically, never in silence (R21c)', () => {
    // "No user profile found" arrives as kind `auth` with no key
    expect(biometricNotice('other', true)).toBe('generic');
  });

  it('leaves a dismissed prompt and a toasted error without a message', () => {
    expect(biometricNotice('cancelled', true)).toBeNull();
    // the provider toasts everything that is not an auth error
    expect(biometricNotice('other', false)).toBeNull();
  });
});

describe('shouldAutoPrompt', () => {
  it('prompts once per lock event, however often the lock screen mounts (R21a)', () => {
    expect(shouldAutoPrompt(0, null)).toBe(true);
    expect(shouldAutoPrompt(1_000, null)).toBe(true);
    // a remount during the same lock (Android back from /auth)
    expect(shouldAutoPrompt(1_000, 1_000)).toBe(false);
    // the next lock
    expect(shouldAutoPrompt(2_000, 1_000)).toBe(true);
    // the first lock of a cold start has no lock submission behind it: key 0
    expect(shouldAutoPrompt(0, 0)).toBe(false);
  });
});

describe('biometricSettingState', () => {
  it('hides the setting without a status or on a platform without the feature', () => {
    expect(biometricSettingState(undefined)).toBe('hidden');
    expect(biometricSettingState(status({ supported: false }))).toBe('hidden');
    expect(biometricSettingState(status({ supported: false, enrolled: true }))).toBe('hidden');
  });

  it('is unavailable when this machine has no usable biometrics and the unlock is off', () => {
    expect(biometricSettingState(status({ available: false, enrolled: false }))).toBe(
      'unavailable'
    );
  });

  it('stays ready when it is on but no longer usable, so the person can switch it off', () => {
    expect(biometricSettingState(status({ available: false, enrolled: true }))).toBe('ready');
  });

  it('is ready when biometrics are usable', () => {
    expect(biometricSettingState(status())).toBe('ready');
    expect(biometricSettingState(status({ enrolled: true }))).toBe('ready');
  });
});

describe('desktopBiometryLabel', () => {
  it('names Touch ID and Windows Hello, and falls back to the generic wording', () => {
    expect(desktopBiometryLabel('touchId')).toBe('touchId');
    expect(desktopBiometryLabel('windowsHello')).toBe('windowsHello');
    expect(desktopBiometryLabel('faceId')).toBe('generic');
    expect(desktopBiometryLabel('biometric')).toBe('generic');
    expect(desktopBiometryLabel('none')).toBe('generic');
  });
});

describe('shouldAutoPromptDesktop', () => {
  const base = { lockEvent: 1_000, lastPrompted: null, focused: true, enrolled: true };

  it('prompts once per lock event', () => {
    expect(shouldAutoPromptDesktop(base)).toBe(true);
    expect(shouldAutoPromptDesktop({ ...base, lastPrompted: 1_000 })).toBe(false);
    expect(shouldAutoPromptDesktop({ ...base, lastPrompted: 1_000, lockEvent: 2_000 })).toBe(true);
  });

  it('never prompts a window that is not in front', () => {
    expect(shouldAutoPromptDesktop({ ...base, focused: false })).toBe(false);
  });

  it('never prompts when biometric unlock is off', () => {
    expect(shouldAutoPromptDesktop({ ...base, enrolled: false })).toBe(false);
  });
});

describe('an invalidated key never re-offers the enrolment', () => {
  it('stays quiet once the offer was answered: the desktop clears the flag only after delete_account', () => {
    expect(shouldOfferEnrolment(status({ available: true, enrolled: false }), true)).toBe(false);
  });
});
