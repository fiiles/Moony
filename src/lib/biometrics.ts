/**
 * Biometric unlock on the phone and the desktop (spec §8.1, §8.2): the pure decisions behind the
 * lock screen's Face ID / Touch ID / Windows Hello path and the one-time enrolment offer. The
 * commands live in `biometricApi` (`src/lib/tauri-api.ts`); the master key never reaches the
 * webview.
 */
import type { BiometricStatus, BiometryType } from '@shared/schema';

/** Query key of `biometricApi.status()`; `queryClient.clear()` on lock drops it with the rest. */
export const BIOMETRIC_STATUS_KEY = ['biometric-status'] as const;

/** localStorage: the enrolment sheet was shown once, whatever the answer. */
export const BIOMETRICS_ASKED_KEY = 'moony-biometrics-asked';

/** Face, fingerprint or anything else: picks the wording and the icon. */
export type BiometryKind = 'face' | 'touch' | 'generic';

export function biometryKind(type: BiometryType): BiometryKind {
  if (type === 'faceId') return 'face';
  if (type === 'touchId') return 'touch';
  // Windows Hello is desktop-only; the phone has no copy of its own for it.
  return 'generic';
}

/** `mobile` namespace keys for one kind of biometry. */
export interface BiometricCopy {
  /** The lock screen's primary button. */
  unlock: string;
  /** The enrolment sheet's title and its one sentence. */
  title: string;
  lead: string;
}

const COPY: Record<BiometryKind, BiometricCopy> = {
  face: {
    unlock: 'lock.unlockWithFace',
    title: 'biometrics.titleFace',
    lead: 'biometrics.leadFace',
  },
  touch: {
    unlock: 'lock.unlockWithTouch',
    title: 'biometrics.titleTouch',
    lead: 'biometrics.leadTouch',
  },
  generic: {
    unlock: 'lock.unlockWithBiometrics',
    title: 'biometrics.titleGeneric',
    lead: 'biometrics.leadGeneric',
  },
};

export function biometricCopy(type: BiometryType): BiometricCopy {
  return COPY[biometryKind(type)];
}

/**
 * The lock screen offers "Odemknout obličejem" whenever biometric unlock is switched on, usable
 * or not: with nothing enrolled any more (or locked out) the keychain answers at once, without a
 * prompt, and the screen falls back to the password with the reason (invalidated or failed).
 */
export function canUnlockWithBiometrics(status: BiometricStatus | undefined): boolean {
  return status !== undefined && status.enrolled;
}

/** The enrolment sheet after a password unlock: usable, not yet on, and never asked before. */
export function shouldOfferEnrolment(status: BiometricStatus | undefined, asked: boolean): boolean {
  return status !== undefined && status.available && !status.enrolled && !asked;
}

export interface EnrolmentOfferState {
  /** The offer is due: a password unlock, and `shouldOfferEnrolment`. */
  offered: boolean;
  /** "Zapnout" is running (the system asks for Face ID / Touch ID). */
  enabling: boolean;
  /** The import sheet is open. */
  importOpen: boolean;
  /** A statement shared into Moony is on its way to the import sheet (`ReceivedQueue.pending`). */
  sharedPending: boolean;
}

/**
 * Whether the enrolment sheet is up. The import sheet and the offer never open together: after a
 * password unlock with a shared statement waiting, the import comes first — it is what the person
 * came for — and the offer opens once its sheet has closed. Until the drain the unlock starts has
 * answered, a shared file may be on its way, so the offer waits for that too. Once "Zapnout" runs
 * the sheet stays until the answer.
 */
export function enrolmentOfferOpen(state: EnrolmentOfferState): boolean {
  if (state.enabling) return true;
  return state.offered && !state.importOpen && !state.sharedPending;
}

/**
 * Whether the sheet was shown before. Storage that cannot be read counts as asked: a flag that
 * cannot be kept would bring the sheet back after every unlock.
 */
export function wasEnrolmentAsked(storage: Pick<Storage, 'getItem'> | null): boolean {
  if (!storage) return true;
  try {
    return storage.getItem(BIOMETRICS_ASKED_KEY) !== null;
  } catch {
    return true;
  }
}

export function markEnrolmentAsked(storage: Pick<Storage, 'setItem'> | null): void {
  try {
    storage?.setItem(BIOMETRICS_ASKED_KEY, '1');
  } catch {
    // Private mode: the sheet may come back once more.
  }
}

/**
 * Forget that the sheet was shown: a new account on the same phone (first run after the account
 * was deleted) is offered biometric unlock again.
 */
export function clearEnrolmentAsked(storage: Pick<Storage, 'removeItem'> | null): void {
  try {
    storage?.removeItem(BIOMETRICS_ASKED_KEY);
  } catch {
    // Storage that cannot be written cannot have kept the flag either.
  }
}

/**
 * What the lock screen does after a biometric unlock failed, by the backend's error key:
 * - `cancelled`: nothing; the button stays and nothing prompts again by itself;
 * - `invalidated` / `failed`: the message under the button and the password form;
 * - `other`: the password form (the provider toasts the error).
 */
export type BiometricFailure = 'cancelled' | 'invalidated' | 'failed' | 'other';

export function biometricFailure(errorKey: string | undefined): BiometricFailure {
  switch (errorKey) {
    case 'auth.biometricCancelled':
      return 'cancelled';
    case 'auth.biometricInvalidated':
      return 'invalidated';
    case 'auth.biometricFailed':
      return 'failed';
    default:
      return 'other';
  }
}

/**
 * Where the message under the primary comes from after a failed attempt: the backend's own words
 * for `invalidated` / `failed`, the generic `auth.biometricFailed` for an auth error that carries
 * no key (`Auth("No user profile found")`: the screen must never fall back to the password form
 * in silence), and none for a dismissed prompt or an error the provider toasts.
 */
export type BiometricNotice = 'backend' | 'generic' | null;

export function biometricNotice(failure: BiometricFailure, authError: boolean): BiometricNotice {
  if (failure === 'invalidated' || failure === 'failed') return 'backend';
  if (failure === 'other' && authError) return 'generic';
  return null;
}

/**
 * The lock screen opens the system prompt by itself once per lock event, not once per mount: the
 * lock page can mount again during the same lock (Android back from `/auth` pops into a protected
 * route, which redirects back), and a cancelled prompt must not come back with it. A lock event is
 * the lock mutation's `submittedAt` (0 for the first lock of a cold start, which no lock caused).
 */
export function shouldAutoPrompt(lockEvent: number, lastPrompted: number | null): boolean {
  return lastPrompted !== lockEvent;
}

/**
 * The desktop's security setting: `hidden` where the platform has no biometric unlock at all
 * (Linux, or the status has not arrived), `unavailable` when this machine has nothing usable and
 * the unlock is off (the row explains why), `ready` otherwise. An unlock that is on stays `ready`
 * even when biometrics stopped being usable, so the person can still switch it off.
 */
export type BiometricSettingState = 'hidden' | 'unavailable' | 'ready';

export function biometricSettingState(status: BiometricStatus | undefined): BiometricSettingState {
  if (status === undefined || !status.supported) return 'hidden';
  if (!status.available && !status.enrolled) return 'unavailable';
  return 'ready';
}

/** Which name the desktop's copy uses; Face ID and the rest share the generic wording. */
export type DesktopBiometryLabel = 'touchId' | 'windowsHello' | 'generic';

export function desktopBiometryLabel(type: BiometryType): DesktopBiometryLabel {
  if (type === 'touchId') return 'touchId';
  if (type === 'windowsHello') return 'windowsHello';
  return 'generic';
}

/**
 * The desktop lock screen prompts by itself once per lock event, like the phone, but only for a
 * window the person is looking at: a system prompt over a background window would steal focus
 * (and on a minimized or hidden one nobody could answer it).
 */
export function shouldAutoPromptDesktop(input: {
  lockEvent: number;
  lastPrompted: number | null;
  focused: boolean;
  enrolled: boolean;
}): boolean {
  return input.enrolled && input.focused && shouldAutoPrompt(input.lockEvent, input.lastPrompted);
}
