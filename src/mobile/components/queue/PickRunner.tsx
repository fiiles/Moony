import { useEffect, useRef } from 'react';
import type { BankTransaction, ChangedRow, TransactionCategory } from '@shared/schema';
import { useCategorize } from '@/mobile/hooks/use-categorize';

/** One accepted engine suggestion: the payment and the category its pill named. */
export interface PickRun {
  tx: BankTransaction;
  categoryId: string;
}

interface PickRunnerProps {
  run: PickRun;
  categories: readonly TransactionCategory[];
  /** What the remembered pick took along, once the learning has written. */
  onApplied: (changed: ChangedRow[]) => void;
  /** The category is saved (`true`) or the write failed and was reported (`false`). */
  onDone: (txId: string, saved: boolean) => void;
}

/**
 * Accepts one engine suggestion of the categorize queue through `useCategorize.pick`: the pick
 * the picker makes with its switches on — save, remember for this account and direction, also
 * categorize the other payments the rule covers — and no toast of its own, since the queue's row
 * turns over and offers the expansion. `useCategorize` is bound to one payment (it loads that
 * payment's preview), so the queue mounts one of these per accepted row instead of one hook per
 * row of a list of hundreds; it renders nothing and runs the pick once.
 */
export function PickRunner({ run, categories, onApplied, onDone }: PickRunnerProps) {
  const { pick } = useCategorize(run.tx, categories);
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void pick(run.categoryId, {
      remember: true,
      applyHistory: true,
      quiet: true,
      onApplied,
    }).then((saved) => onDone(run.tx.id, saved));
  }, [pick, run, onApplied, onDone]);

  return null;
}
