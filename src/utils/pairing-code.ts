import type { PairingStage, PairingView } from '@shared/schema';

export type ScannedCodeKind = 'pair' | 'recover' | 'recoveryCode' | 'unknown';

const RECOVERY_CODE = /^[0-9A-HJKMNP-TV-Z]{80}$/;

/** The same normalisation as `decode_recovery_code` in Rust: separators go, ASCII upper case, I/L to 1, O to 0. */
function normalizeRecoveryCode(text: string): string {
  return text
    .replace(/[\p{White_Space}\-\u2010-\u2015]/gu, '')
    .replace(/[a-z]/g, (c) => c.toUpperCase())
    .replace(/[IL]/g, '1')
    .replace(/O/g, '0');
}

/**
 * What a scanned or typed text is: a pairing code, a Sync Recovery Kit QR, a typed recovery code
 * (80 Crockford characters once spaces and dashes go) or something else.
 */
export function scannedCodeKind(text: string): ScannedCodeKind {
  const trimmed = text.trim();
  if (trimmed.startsWith('moony-pair:')) return 'pair';
  if (trimmed.startsWith('moony-recover:')) return 'recover';
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
