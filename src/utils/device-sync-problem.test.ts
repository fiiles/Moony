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
  secureStorage: ['none', 'none'],
  serverWentBack: ['none', 'none'],
  internal: ['none', 'none'],
};

describe('problemAction', () => {
  it.each(Object.entries(TABLE) as [SyncProblem, [SyncProblemAction, SyncProblemAction]][])(
    '%s',
    (problem, [normal, selfHosted]) => {
      expect(problemAction(problem, false, 0)).toBe(normal);
      expect(problemAction(problem, true, 0)).toBe(selfHosted);
    }
  );

  it('never offers a re-sync for a gap while changes wait to be sent', () => {
    expect(problemAction('gap', false, 3)).toBe('none');
    expect(problemAction('gap', true, 3)).toBe('none');
    expect(problemAction('gap', false, 0)).toBe('resync');
  });

  it('leaves the other problems as they are when changes wait', () => {
    for (const [problem, [normal, selfHosted]] of Object.entries(TABLE) as [
      SyncProblem,
      [SyncProblemAction, SyncProblemAction],
    ][]) {
      if (problem === 'gap') continue;
      expect(problemAction(problem, false, 3)).toBe(normal);
      expect(problemAction(problem, true, 3)).toBe(selfHosted);
    }
  });
});
