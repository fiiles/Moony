import { describe, expect, it } from 'vitest';
import type { SyncProblem } from '@shared/schema';
import { problemAction, type SyncProblemAction } from './device-sync-problem';

const TABLE: Record<SyncProblem, [SyncProblemAction, SyncProblemAction]> = {
  // [normal server, self-hosted]
  captureBroken: ['repair', 'repair'],
  vaultGone: ['turnOffOrRetry', 'turnOffOrRetry'],
  lapsed: ['renew', 'askOperator'],
  quotaExceeded: ['none', 'none'],
  snapshotTooLarge: ['none', 'none'],
  newerSchema: ['updateApp', 'updateApp'],
  gap: ['resync', 'resync'],
  undecryptable: ['none', 'none'],
  invalidChain: ['none', 'none'],
  missingParent: ['none', 'none'],
  quarantined: ['none', 'none'],
  offline: ['none', 'none'],
  relayError: ['none', 'none'],
  rateLimited: ['none', 'none'],
  unauthorized: ['none', 'none'],
  keyRotated: ['none', 'none'],
  internal: ['none', 'none'],
};

describe('problemAction', () => {
  it.each(Object.entries(TABLE) as [SyncProblem, [SyncProblemAction, SyncProblemAction]][])(
    '%s',
    (problem, [normal, selfHosted]) => {
      expect(problemAction(problem, false)).toBe(normal);
      expect(problemAction(problem, true)).toBe(selfHosted);
    }
  );
});
