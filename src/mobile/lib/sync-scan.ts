import type { PairingStage, PairingView } from '@shared/schema';
import { scannedCodeKind } from '@/utils/pairing-code';
import { groupAccountNumber } from '@/utils/recovery-kit';

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

/**
 * The sync server a Recovery Kit QR without one names: `DEFAULT_RELAY_URL` in
 * `src-tauri/src/services/sync/relay/http.rs`, which `read_recovery_input` joins with.
 */
export const DEFAULT_SYNC_SERVER = 'https://sync.moonyfinance.com';

/**
 * What the phone shows before a scanned Recovery Kit QR joins: the account number in groups of four
 * and the sync server, or why the QR cannot join (a `common` key, the one the backend would answer).
 */
export type KitQrSummary =
  | { ok: true; account: string; server: string }
  | {
      ok: false;
      problem: 'validation.syncRecoveryCodeInvalid' | 'validation.syncRelayUrlInvalid';
    };

const KIT_QR_PREFIX = 'moony-recover:1:';

/**
 * Reads the account and the sync server of `moony-recover:1:<account>:<code>[:<base64url server>]`
 * (`parse_recovery_qr_payload`) for the confirm sheet. It refuses no more than the backend does: the
 * code itself and the server's details are the backend's to check when the join runs.
 */
export function kitQrSummary(text: string): KitQrSummary {
  const invalid = { ok: false, problem: 'validation.syncRecoveryCodeInvalid' } as const;
  const trimmed = text.trim();
  if (!trimmed.startsWith(KIT_QR_PREFIX)) return invalid;
  const fields = trimmed.slice(KIT_QR_PREFIX.length).split(':');
  if (fields.length < 2 || fields.length > 3) return invalid;
  const [account, , encoded] = fields;
  if (!/^\d{16}$/.test(account)) return invalid;
  if (encoded === undefined) {
    return { ok: true, account: groupAccountNumber(account), server: DEFAULT_SYNC_SERVER };
  }
  if (!/^[A-Za-z0-9_-]+$/.test(encoded)) return invalid;
  let address: string;
  try {
    const binary = atob(encoded.replace(/-/g, '+').replace(/_/g, '/'));
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    address = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return invalid;
  }
  try {
    const server = new URL(address).href.replace(/\/+$/, '');
    return { ok: true, account: groupAccountNumber(account), server };
  } catch {
    return { ok: false, problem: 'validation.syncRelayUrlInvalid' };
  }
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
