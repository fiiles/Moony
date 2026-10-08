import type { CashflowActuals, CashflowGroup, CategorySpendingSummary } from '@shared/schema';

type CsvReport = Pick<
  CashflowActuals,
  'reportCurrency' | 'months' | 'expenseCategories' | 'incomeSources'
>;

/** Backend money is already rounded decimal TEXT in the report currency. */
export function buildCashflowCsv(
  report: CsvReport,
  monthLabel: (month: number) => string,
  expenseLabel: (group: CashflowGroup) => string,
  incomeLabel: (group: CashflowGroup) => string
): string {
  const suffix = report.reportCurrency.toLowerCase();
  const cell = (value: string | number) => {
    const text = String(value);
    return /[;"\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
  };
  const line = (values: (string | number)[]) => values.map(cell).join(';');
  return [
    line(['month', `income_${suffix}`, `expenses_${suffix}`]),
    ...report.months.map((month) => line([monthLabel(month.month), month.income, month.expenses])),
    '',
    line(['expense_category', `amount_${suffix}`, `previous_${suffix}`, 'transactions']),
    ...report.expenseCategories.map((group) =>
      line([expenseLabel(group), group.amount, group.previousAmount, group.count])
    ),
    '',
    line(['income_source', `amount_${suffix}`, `previous_${suffix}`, 'transactions']),
    ...report.incomeSources.map((group) =>
      line([incomeLabel(group), group.amount, group.previousAmount, group.count])
    ),
  ].join('\n');
}

type BudgetStatusInput = Pick<
  CategorySpendingSummary,
  'budgetCurrency' | 'budgetSpent' | 'budgetLimitNative' | 'budgetPercentage'
>;

/** Counts are safe to aggregate when goal rows have different native currencies. */
export function summarizeBudgetStatus(rows: BudgetStatusInput[], targetExpenses: string) {
  const summary = { targetExpenses, onTrack: 0, near: 0, over: 0, unavailable: 0 };
  for (const row of rows) {
    const percentage = row.budgetPercentage;
    if (
      !row.budgetCurrency ||
      row.budgetSpent == null ||
      row.budgetLimitNative == null ||
      percentage == null ||
      !Number.isFinite(percentage)
    ) {
      summary.unavailable++;
    } else if (percentage > 100) {
      summary.over++;
    } else if (percentage >= 85) {
      summary.near++;
    } else {
      summary.onTrack++;
    }
  }
  return summary;
}
