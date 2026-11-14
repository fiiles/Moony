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
