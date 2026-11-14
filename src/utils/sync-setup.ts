import type { PairingView, SyncServerCheck } from '@shared/schema';
import { scannedCodeKind, type ScannedCodeKind } from '@/utils/pairing-code';
import { isDefaultServer } from '@/utils/sync-server';

/** Where the data syncs: the Moony cloud, or a sync server the person runs. */
export type ServerKind = 'cloud' | 'own';
export type SetupStep = 'server' | 'account' | 'pay' | 'kit' | 'device';

/**
 * The steps of first-device setup. An own server has no separate account step (the connection
 * code names the account) and nothing to pay.
 */
export function setupSteps(kind: ServerKind): readonly SetupStep[] {
  return kind === 'own'
    ? ['server', 'kit', 'device']
    : ['server', 'account', 'pay', 'kit', 'device'];
}

/**
 * Whether the server step may continue: the server answered, and a plain http one only after the
 * person said they understand.
 */
export function serverContinue(check: SyncServerCheck | undefined, acknowledged: boolean): boolean {
  if (!check) return false;
  return !check.insecure || acknowledged;
}

/**
 * Whether setup may read the sync account now. An own server is not asked on the server step: a
 * resumed setup would otherwise contact a stored plain-http server before the warning is
 * acknowledged. The cloud's account is read throughout (the pay step polls it).
 */
export function accountQueryEnabled(step: SetupStep, kind: ServerKind): boolean {
  return kind === 'cloud' || step !== 'server';
}

/**
 * The plain-http acknowledgement belongs to the server that was shown: it holds only while the
 * input is the one it was given for, and typing clears it.
 */
export function acknowledgedAfterEdit(
  previousInput: string,
  nextInput: string,
  acknowledged: boolean
): boolean {
  return nextInput === previousInput && acknowledged;
}

/**
 * Whether joining with the received sync details must first be acknowledged: they name a server
 * reached over plain http, and it is not the one the person already acknowledged in the same
 * dialog (a desktop joiner who typed that http address).
 */
export function needsJoinAcknowledgement(
  view: Pick<PairingView, 'joinServer' | 'joinServerInsecure'>,
  acknowledgedHost: string | null
): boolean {
  return (
    view.joinServerInsecure && view.joinServer !== null && view.joinServer !== acknowledgedHost
  );
}

/**
 * What a Recovery Kit join asks of the server field, by what was typed or pasted: a typed recovery
 * code needs the server typed (`typed`); a Kit QR's text brings its own, which is checked unless it
 * is the Moony cloud (`qr`, with `qrServer` from `payloadServer` / `kitQrSummary`); anything else
 * needs nothing, the backend names what is wrong with it (`none`).
 */
export function kitServerNeed(
  kind: ScannedCodeKind,
  qrServer: string | null
): 'typed' | 'qr' | 'none' {
  if (kind === 'recoveryCode') return 'typed';
  if (kind === 'recover' && qrServer !== null && !isDefaultServer(qrServer)) return 'qr';
  return 'none';
}

/** `host[:port]` of an address, for showing which server is meant; anything else as it is. */
export function hostOf(address: string): string {
  try {
    return new URL(address).host || address;
  } catch {
    return address;
  }
}

/**
 * Whether the address the server answered for (`checked`) is the one this device already uses
 * (`stored`, the status's address): changing to it is no change, so the address step says so and
 * never asks for the password.
 */
export function isUnchangedAddress(checked: string | undefined, stored: string | null): boolean {
  return checked !== undefined && checked !== '' && checked === stored;
}

/**
 * A `deviceSync` key naming what is wrong when `code`, typed or pasted into a Recovery Kit field,
 * is another kind of code: a pairing code or a connection code. Anything else goes to the backend,
 * which says what is wrong with it.
 */
export function kitCodeProblem(code: string): string | null {
  switch (scannedCodeKind(code)) {
    case 'pair':
      return 'phone.notAKitCode';
    case 'connection':
      return 'codes.connectionNotHere';
    default:
      return null;
  }
}
