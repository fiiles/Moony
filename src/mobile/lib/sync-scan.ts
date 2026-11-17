import type { PairingStage, PairingView } from '@shared/schema';
import { scannedCodeKind } from '@/utils/pairing-code';
import { decodeServerField, normalizeAddress } from '@/utils/sync-server';

/**
 * The pure parts of scanning on the phone (Nastavení → Sync): what a scanned or pasted text leads
 * to, what the Recovery Kit and the manual-entry sheets refuse, the phone's default device name
 * and the step a pairing sheet shows.
 */

/** `join`: sync is off and this phone joins; `add`: sync is on and this phone adds another device. */
export type ScanMode = 'join' | 'add';

/**
 * Where a scanned QR goes: a pairing (either mode, the backend tells the role), a join with the
 * Recovery Kit's QR (sync off only), "Sync is already on" for a Kit with sync on, a connection code
 * (a self-hosted server's: for setup, not here) or nowhere. A bare recovery code in a QR carries no
 * account and is no Moony QR.
 */
export type ScanRoute = 'pair' | 'kitJoin' | 'alreadyOn' | 'connection' | 'unknown';

export function scanRoute(text: string, mode: ScanMode): ScanRoute {
  switch (scannedCodeKind(text)) {
    case 'pair':
      return 'pair';
    case 'recover':
      return mode === 'join' ? 'kitJoin' : 'alreadyOn';
    case 'connection':
      return 'connection';
    default:
      return 'unknown';
  }
}

/**
 * Where a QR scanned by the setup camera goes: a connection code fills the server field; a pairing
 * code or a Recovery Kit QR is named as not for setup; anything else is no connection code.
 */
export type SetupScanRoute = 'connection' | 'pair' | 'recover' | 'unknown';

export function setupScanRoute(text: string): SetupScanRoute {
  switch (scannedCodeKind(text)) {
    case 'connection':
      return 'connection';
    case 'pair':
      return 'pair';
    case 'recover':
      return 'recover';
    default:
      return 'unknown';
  }
}

export { kitCodeProblem } from '@/utils/sync-setup';

/** The search parameter that opens the join choice on arrival (`?join=1`): "Připojit znovu" from the unlock sheet. */
export const JOIN_PARAM = 'join';

/**
 * The sync server a Recovery Kit QR without one names: `DEFAULT_RELAY_URL` in
 * `src-tauri/src/services/sync/relay/http.rs`, which `read_recovery_input` joins with.
 */
export const DEFAULT_SYNC_SERVER = 'https://sync.moonyfinance.com';

/**
 * What the phone shows before a scanned Recovery Kit QR joins: the sync server, or why the QR
 * cannot join (a `common` key, the one the backend would answer). The account is inside the code.
 */
export type KitQrSummary =
  | { ok: true; server: string }
  | {
      ok: false;
      problem: 'validation.syncRecoveryCodeInvalid' | 'validation.syncRelayUrlInvalid';
    };

const KIT_QR_PREFIX = 'moony-recover:2:';

/**
 * Reads the sync server of `moony-recover:2:<code>[:<base64url server>]` (`parse_kit_qr_payload`)
 * for the confirm sheet. It refuses no more than the backend does: the
 * code itself and the server's details are the backend's to check when the join runs.
 */
export function kitQrSummary(text: string): KitQrSummary {
  const invalid = { ok: false, problem: 'validation.syncRecoveryCodeInvalid' } as const;
  const trimmed = text.trim();
  if (!trimmed.startsWith(KIT_QR_PREFIX)) return invalid;
  const fields = trimmed.slice(KIT_QR_PREFIX.length).split(':');
  if (fields.length > 2 || fields[0] === '') return invalid;
  const [, encoded] = fields;
  if (encoded === undefined) return { ok: true, server: DEFAULT_SYNC_SERVER };
  const address = decodeServerField(encoded);
  if (address === null) return invalid;
  const server = normalizeAddress(address);
  if (server === null) return { ok: false, problem: 'validation.syncRelayUrlInvalid' };
  return { ok: true, server };
}

export { pairingCodeProblem } from '@/utils/pairing-code';

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
 * received pairing has finished its part); a contested scan shows a warning and only Cancel, and
 * no digits (review PF4); otherwise the stage.
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
