import type { SyncServerCheck } from '@shared/schema';

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
 * Whether a stored server address is plain http to a host other than localhost / 127.0.0.1
 * (`is_insecure_address` in Rust): a resumed setup shows the warning again for it.
 */
export function isInsecureAddress(address: string): boolean {
  try {
    const url = new URL(address);
    return url.protocol === 'http:' && url.hostname !== 'localhost' && url.hostname !== '127.0.0.1';
  } catch {
    return false;
  }
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
