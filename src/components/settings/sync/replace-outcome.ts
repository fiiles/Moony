import { toast } from 'sonner';
import { outcomeLines } from '@/utils/replace-impact';
import type { ReplaceOutcomeView } from '@shared/schema';

/** The key the backend answers when what the confirmation showed no longer holds. */
export const IMPACT_CHANGED_KEY = 'validation.syncReplaceImpactChanged';

const OUTCOME_KEYS: Record<string, string> = {
  attachmentsInBackupOnly: 'attachments',
  unsentItemsInBackupOnly: 'items',
  unsentBatchesInBackupOnly: 'batches',
};

/** The toast after a replace: `title` names the backup file; the lines say what only the backup holds and that it stays. */
export function replaceOutcomeToast(
  t: (key: string, options?: Record<string, unknown>) => string,
  title: string,
  outcome: ReplaceOutcomeView
) {
  const lines = outcomeLines(outcome).map((line) =>
    t(`replace.outcome.${OUTCOME_KEYS[line.key]}`, { count: line.count })
  );
  // Items and batches are separate units: a failed push is often "0 items, 1 batch".
  if (outcome.unsentItemsInBackupOnly + outcome.unsentBatchesInBackupOnly > 0) {
    lines.push(t('replace.outcome.someUnsent'));
  }
  if (outcome.kept) lines.push(t('replace.outcome.kept'));
  toast(title, {
    description: lines.length > 0 ? lines.join('\n') : undefined,
    duration: lines.length > 0 ? 15000 : undefined,
  });
}
