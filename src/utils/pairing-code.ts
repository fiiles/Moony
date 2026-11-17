import type { PairingStage, PairingView } from '@shared/schema';

export type ScannedCodeKind = 'pair' | 'recover' | 'recoveryCode' | 'connection' | 'unknown';

const CONNECTION_CODE = /^moony-sync\d+:/;
/**
 * `text` the way `connection_code::cleaned` reads a connection code in Rust: trimmed, every
 * White_Space and zero-width character removed (a code wrapped by a chat app or copied with a
 * zero-width space still reads).
 */
export function stripInvisible(text: string): string {
  return text.trim().replace(/[\p{White_Space}\u200B-\u200D\uFEFF]/gu, '');
}

/** The kit code (format v2, the account inside): 95 Crockford characters, shown as 19 groups of five. */
const RECOVERY_CODE = /^[0-9A-HJKMNP-TV-Z]{95}$/;

/** The same normalisation as `decode_kit_code` in Rust: separators go, ASCII upper case, I/L to 1, O to 0. */
function normalizeRecoveryCode(text: string): string {
  return text
    .replace(/[\p{White_Space}\-\u2010-\u2015]/gu, '')
    .replace(/[a-z]/g, (c) => c.toUpperCase())
    .replace(/[IL]/g, '1')
    .replace(/O/g, '0');
}

/**
 * What a scanned or typed text is: a pairing code (white space inside it ignored), a Sync Recovery Kit QR, a typed recovery code
 * (95 Crockford characters once spaces and dashes go), a connection code of a self-hosted sync
 * server (`moony-sync<n>:`, any version) or something else.
 */
export function scannedCodeKind(text: string): ScannedCodeKind {
  const trimmed = text.trim();
  // The pairing and connection code parsers remove inner white space and zero-width characters
  // (owner decision O-3 for the pairing code); the Kit parser just trims, and a recovery code
  // keeps zero-width characters invalid
  const compact = stripInvisible(trimmed);
  if (compact.startsWith('moony-pair:')) return 'pair';
  if (trimmed.startsWith('moony-recover:')) return 'recover';
  if (CONNECTION_CODE.test(compact)) return 'connection';
  if (RECOVERY_CODE.test(normalizeRecoveryCode(trimmed))) return 'recoveryCode';
  return 'unknown';
}

/** The six comparison digits in two groups of three: `482686` becomes `482 686`. Anything else stays. */
export function formatComparisonCode(code: string): string {
  return /^\d{6}$/.test(code) ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
}

/** The answer to a typed code that was wrong while attempts are left: the pairing goes on. */
export const CODE_MISMATCH_KEY = 'validation.syncPairingCodeMismatch';

/** Digits in a comparison code. */
export const CODE_LENGTH = 6;

/**
 * What the holder's person typed, as the digits it can mean: Unicode compatibility forms are
 * folded first (a full-width `４８２` is `482`), then everything but ASCII digits goes (spaces,
 * dashes), and at most six digits stay. `482 686` becomes `482686`.
 */
export function normalizeTypedCode(input: string): string {
  return input
    .normalize('NFKC')
    .replace(/[^0-9]/g, '')
    .slice(0, CODE_LENGTH);
}

/** Whether `input` holds a full code: exactly six digits once normalised, nothing else typed. */
export function isCompleteCode(input: string): boolean {
  return normalizeTypedCode(input).length === CODE_LENGTH;
}

/**
 * How often (ms) to ask the backend about a pairing, or `false` when nothing is waited for. The
 * scanner reads the shower's acknowledgement while `connecting`; the shower retries its put.
 */
export function pairingPollInterval(view: PairingView | undefined): number | false {
  switch (view?.stage) {
    case 'announcing':
    case 'connecting':
    case 'waitingForScan':
    case 'waitingForOther':
      return 2000;
    default:
      return false;
  }
}

/**
 * What the backend answered when asked about the pairing right after a failed join
 * (`deviceSyncApi.pairingPoll()`): the session's stage, or the error key it refused with.
 */
export type JoinFailurePoll = { ok: true; stage: PairingStage } | { ok: false; key?: string };

/** What a failed join's dialog offers: the same join again, and whether a setup blocks it. */
export interface JoinFailureAction {
  /** The backend kept the received session: "Try again" joins again; otherwise "Start again". */
  retryable: boolean;
  /** A half-finished setup on this device refused the join (`syncJoinSetupPending`): "Cancel setup". */
  setupPending: boolean;
}

const PAIRING_ENDED = new Set([
  'validation.syncPairingNotStarted',
  'validation.syncPairingExpired',
]);

/**
 * After a failed pairing join: whether the backend kept the received session (`after_join_error`
 * keeps it for a momentary relay failure, a rate limit, another sync change running and a
 * half-finished setup, and ends it for anything else). The join's own error key cannot tell
 * (`syncRelayError` covers both), so the poll's answer decides: stage `received` is kept, "not
 * started" or "expired" ended. A poll that failed any other way counts as kept, since the next
 * join's failure is asked about again. `errorKey` (the join's) tells only a pending setup apart.
 */
export function joinFailureAction(
  poll: JoinFailurePoll,
  errorKey: string | undefined
): JoinFailureAction {
  const retryable = poll.ok
    ? poll.stage === 'received'
    : poll.key === undefined || !PAIRING_ENDED.has(poll.key);
  return { retryable, setupPending: retryable && errorKey === 'validation.syncJoinSetupPending' };
}

/**
 * A typed or pasted pairing code (the manual-entry field on the phone and on the desktop) takes a
 * pairing code only: `null` when it is one, else the `deviceSync` key of what to tell the person.
 */
export function pairingCodeProblem(code: string): string | null {
  switch (scannedCodeKind(code)) {
    case 'pair':
      return null;
    case 'connection':
      return 'codes.connectionNotHere';
    default:
      return 'phone.notAPairingCode';
  }
}
