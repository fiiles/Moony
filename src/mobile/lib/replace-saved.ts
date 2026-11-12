import { outcomeLines } from '@/utils/replace-impact';
import type { ReplaceOutcomeView } from '@shared/schema';

/** A replace that ended with a backup the person should know where to find: what the sheet says. */
export interface ReplaceSaved {
  /** The headline (what was done and the backup's file name). */
  headline: string;
  outcome: ReplaceOutcomeView;
}

const OUTCOME_KEYS: Record<string, string> = {
  attachmentsInBackupOnly: 'attachments',
  unsentItemsInBackupOnly: 'items',
  unsentBatchesInBackupOnly: 'batches',
};

/**
 * The sentences saying what only the backup holds after a replace (`deviceSync` keys
 * `replace.outcome.*`, with `count`), the "some changes stay only in the backup" line when anything
 * unsent is in it. Whether it is kept and where it is listed is said by the caller (the phone's
 * sentence differs from the desktop's).
 */
export function outcomeSentences(outcome: ReplaceOutcomeView): { key: string; count?: number }[] {
  const lines: { key: string; count?: number }[] = outcomeLines(outcome).map((line) => ({
    key: `replace.outcome.${OUTCOME_KEYS[line.key]}`,
    count: line.count,
  }));
  if (outcome.unsentItemsInBackupOnly + outcome.unsentBatchesInBackupOnly > 0) {
    lines.push({ key: 'replace.outcome.someUnsent' });
  }
  return lines;
}
