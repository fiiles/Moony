import { describe, expect, it } from 'vitest';
import type { ReplaceImpactView, ReplaceOutcomeView } from '@shared/schema';
import { consentOf, impactLines, outcomeLines } from '@/utils/replace-impact';

const impact: ReplaceImpactView = {
  pendingItems: 4,
  pendingBatches: 2,
  documents: 3,
  photos: 5,
  accounts: 7,
  transactions: 120,
  changesThrough: '9007199254740993',
};

const outcome: ReplaceOutcomeView = {
  backupPath: '/x/backup-kept.zip',
  attachmentsKept: 1,
  attachmentsInBackupOnly: 2,
  unsentItemsInBackupOnly: 0,
  unsentBatchesInBackupOnly: 1,
  kept: true,
};

describe('impactLines', () => {
  it('join lists accounts and transactions, then documents and photos', () => {
    expect(impactLines('join', impact)).toEqual([
      { key: 'accounts', count: 7 },
      { key: 'transactions', count: 120 },
      { key: 'documents', count: 3 },
      { key: 'photos', count: 5 },
    ]);
  });

  it('resync lists items and batches apart', () => {
    expect(impactLines('resync', impact)).toEqual([
      { key: 'items', count: 4 },
      { key: 'batches', count: 2 },
      { key: 'documents', count: 3 },
      { key: 'photos', count: 5 },
    ]);
  });

  it('repair lists only the attachments', () => {
    expect(impactLines('repair', impact).map((l) => l.key)).toEqual(['documents', 'photos']);
  });

  it('zero counts are left out', () => {
    const lines = impactLines('resync', { ...impact, pendingItems: 0, documents: 0 });
    expect(lines).toEqual([
      { key: 'batches', count: 2 },
      { key: 'photos', count: 5 },
    ]);
  });
});

describe('consentOf', () => {
  it('consent repeats the watermark', () => {
    expect(consentOf('join', impact)).toEqual({
      changesThrough: '9007199254740993',
      attachments: 8,
    });
    expect(consentOf('resync', impact)).toEqual({
      changesThrough: '9007199254740993',
      attachments: 0,
    });
  });
});

describe('outcomeLines', () => {
  it('outcome lines name what stayed in the backup', () => {
    expect(outcomeLines(outcome)).toEqual([
      { key: 'attachmentsInBackupOnly', count: 2 },
      { key: 'unsentBatchesInBackupOnly', count: 1 },
    ]);
  });
});
