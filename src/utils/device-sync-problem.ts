import type { SyncProblem } from '@shared/schema';

/** The action the problem alert offers. */
export type SyncProblemAction =
  'renew' | 'askOperator' | 'turnOffOrRetry' | 'repair' | 'resync' | 'updateApp' | 'none';

/** Which action the alert shows for a problem; a lapsed subscription on a self-hosted server has no Renew. */
export function problemAction(problem: SyncProblem, selfHosted: boolean): SyncProblemAction {
  switch (problem) {
    case 'lapsed':
      return selfHosted ? 'askOperator' : 'renew';
    case 'vaultGone':
      return 'turnOffOrRetry';
    case 'captureBroken':
      return 'repair';
    case 'gap':
      return 'resync';
    case 'newerSchema':
      return 'updateApp';
    default:
      return 'none';
  }
}
