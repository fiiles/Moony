import { useTranslation } from 'react-i18next';
import { Link } from 'wouter';
import { Info } from 'lucide-react';
import type { BudgetCounts } from '@/mobile/lib/cashflow-rows';

interface BudgetSummaryNoteProps {
  counts: BudgetCounts;
  /** Where the budgets are managed (`budgetsPath`). */
  href: string;
}

/**
 * The one-line budget summary under the bars (spec §8.7 item 3, README §13, `.m-note`): a quiet
 * row on `well`, radius 11, 12 / 500 `ink-2` with a 15 pt info glyph, at least 44 pt tall (the
 * text wraps instead of ending in an ellipsis). A tap opens the budgets.
 */
export function BudgetSummaryNote({ counts, href }: BudgetSummaryNoteProps) {
  const { t } = useTranslation('mobile');
  return (
    <Link
      href={href}
      className="mb-3.5 flex min-h-11 items-center gap-2 rounded-[11px] bg-well px-3 py-2 text-m-sub text-ink-2 [-webkit-touch-callout:none] active:bg-well-2"
    >
      <Info aria-hidden className="size-[15px] flex-none text-ink-3" strokeWidth={1.75} />
      <span className="min-w-0">
        {t('cashflow.budgetSummary', { ok: counts.ok, over: counts.over, near: counts.near })}
      </span>
    </Link>
  );
}
