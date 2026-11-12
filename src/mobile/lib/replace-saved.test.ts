import { describe, expect, it } from 'vitest';
import { outcomeSentences } from './replace-saved';
import type { ReplaceOutcomeView } from '@shared/schema';

const outcome = (patch: Partial<ReplaceOutcomeView>): ReplaceOutcomeView => ({
  backupPath: '/data/backups/sync/x.zip',
  attachmentsKept: 0,
  attachmentsInBackupOnly: 0,
  unsentItemsInBackupOnly: 0,
  unsentBatchesInBackupOnly: 0,
  kept: true,
  ...patch,
});

describe('outcomeSentences', () => {
  it('says nothing when the backup holds nothing the data lost', () => {
    expect(outcomeSentences(outcome({}))).toEqual([]);
  });

  it('names each count that is only in the backup, and that some changes stay only there', () => {
    expect(
      outcomeSentences(
        outcome({
          attachmentsInBackupOnly: 2,
          unsentItemsInBackupOnly: 0,
          unsentBatchesInBackupOnly: 1,
        })
      )
    ).toEqual([
      { key: 'replace.outcome.attachments', count: 2 },
      { key: 'replace.outcome.batches', count: 1 },
      { key: 'replace.outcome.someUnsent' },
    ]);
  });
});
