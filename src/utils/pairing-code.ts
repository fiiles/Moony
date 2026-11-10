import type { PairingView } from '@shared/schema';

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

/** How often (ms) to ask the backend about a pairing, or `false` when nothing is waited for. */
export function pairingPollInterval(view: PairingView | undefined): number | false {
  switch (view?.stage) {
    case 'announcing':
    case 'waitingForScan':
    case 'waitingForOther':
      return 2000;
    default:
      return false;
  }
}
