import type { PairingStage, PairingView } from '@shared/schema';
import { scannedCodeKind } from '@/utils/pairing-code';

/**
 * The pure parts of scanning on the phone (Nastavení → Sync): what a scanned or pasted text leads
 * to, what the Recovery Kit and the developer paste sheets refuse, the phone's default device name
 * and the step a pairing sheet shows.
 */

/** `join`: sync is off and this phone joins; `add`: sync is on and this phone adds another device. */
export type ScanMode = 'join' | 'add';

/**
 * Where a scanned QR goes: a pairing (either mode, the backend tells the role), a join with the
 * Recovery Kit's QR (sync off only), "Sync is already on" for a Kit with sync on, or nowhere. A bare
 * recovery code in a QR carries no account and is no Moony QR.
 */
export type ScanRoute = 'pair' | 'kitJoin' | 'alreadyOn' | 'unknown';

export function scanRoute(text: string, mode: ScanMode): ScanRoute {
  switch (scannedCodeKind(text)) {
    case 'pair':
      return 'pair';
    case 'recover':
      return mode === 'join' ? 'kitJoin' : 'alreadyOn';
    default:
      return 'unknown';
  }
}

/**
 * The Recovery Kit sheet reads recovery codes and the Kit QR's text only (review PF2): a pairing
 * code there is named (a `deviceSync` key). Anything else goes to the backend, which says what is
 * wrong with it.
 */
export function kitCodeProblem(code: string): string | null {
  return scannedCodeKind(code) === 'pair' ? 'phone.notAKitCode' : null;
}

/** The developer paste sheet takes a pairing code only (a `deviceSync` key otherwise). */
export function pairingCodeProblem(code: string): string | null {
  return scannedCodeKind(code) === 'pair' ? null : 'phone.notAPairingCode';
}

/** The `setup.defaultName` key of this phone: Android by its build platform, else an iPhone. */
export function phoneDeviceKey(platform: string): 'ios' | 'android' {
  return platform.startsWith('android') ? 'android' : 'ios';
}

/** Whether a rejected scan was cancelled (the plugin rejects with "cancelled" on both platforms). */
export function isScanCancelled(error: unknown): boolean {
  const message = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  return message === 'cancelled';
}

/** What a pairing sheet shows: a failed poll, an expired code, a contested scan, or the stage. */
export type PairingSheetStep = 'failed' | 'expired' | 'contested' | PairingStage;

/**
 * A failed poll comes first; an expired code only while something was still awaited (a sent or
 * received pairing has finished its part); a contested scan shows its own digits with a warning
 * and only Cancel (review PF4); otherwise the stage.
 */
export function pairingSheetStep(
  view: PairingView,
  { failed, expired }: { failed: boolean; expired: boolean }
): PairingSheetStep {
  if (failed) return 'failed';
  if (expired && view.stage !== 'sent' && view.stage !== 'received') return 'expired';
  if (view.contested) return 'contested';
  return view.stage;
}
