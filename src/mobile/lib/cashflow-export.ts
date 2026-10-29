import type { CashflowActuals, CashflowGroup } from '@shared/schema';
import { buildCashflowCsv } from '@/utils/report-currencies';

/**
 * The name the Cashflow report is staged under: a fixed one on Rust's allow-list
 * (`TEXT_EXPORT_FILE_NAMES` in `services/backup.rs`), like the class pages' exports, not the
 * desktop's dated `cashflow-2026-10.csv`. The person can rename it in the Files picker.
 */
export const CASHFLOW_EXPORT_FILE = 'moony-cashflow.csv';

/**
 * The CSV of the report on screen, as the desktop's Export writes it (`buildCashflowCsv`: the
 * twelve-month window, then the expense, income and transfer groups; the amounts are the report's
 * own decimal text) and the number it reports as saved: the months plus the groups of
 * the three sections. A period without a transaction counts 0, however many zero months its window
 * carries, so the export says there is nothing to export instead of saving a file of zeros.
 */
export function cashflowCsv(
  actuals: CashflowActuals,
  monthLabel: (month: number) => string,
  groupLabel: (group: CashflowGroup) => string
): { csv: string; count: number } {
  const count =
    actuals.transactionCount === 0
      ? 0
      : actuals.months.length +
        actuals.expenseCategories.length +
        actuals.incomeCategories.length +
        actuals.transfers.length;
  return { csv: buildCashflowCsv(actuals, monthLabel, groupLabel), count };
}
