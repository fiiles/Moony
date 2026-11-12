import type { SyncProblem } from '@shared/schema';

/** The action the problem alert offers. */
export type SyncProblemAction =
  'renew' | 'askOperator' | 'turnOffOrRetry' | 'repair' | 'resync' | 'updateApp' | 'none';

/**
 * Which action the alert shows for a problem; a lapsed subscription on a self-hosted server has no
 * Renew. A gap (a change from another device that has not arrived) never offers Re-sync while this
 * device still has changes waiting to be sent: it keeps syncing and sending them meanwhile, and a
 * re-sync would leave them only in the backup.
 */
export function problemAction(
  problem: SyncProblem,
  selfHosted: boolean,
  pendingChanges: number
): SyncProblemAction {
  switch (problem) {
    case 'lapsed':
      return selfHosted ? 'askOperator' : 'renew';
    case 'vaultGone':
      return 'turnOffOrRetry';
    case 'captureBroken':
      return 'repair';
    case 'gap':
      return pendingChanges > 0 ? 'none' : 'resync';
    case 'newerSchema':
      return 'updateApp';
    default:
      return 'none';
  }
}
