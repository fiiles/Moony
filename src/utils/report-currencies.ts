import { isoMinorUnits } from '@shared/currencies';
import type { CashflowActuals, CashflowGroup, CategorySpendingSummary } from '@shared/schema';
import type { Formatters } from '@/lib/format';

/** Report precision is the ISO minor unit of the wire contract, not the cash style. */
export function formatReportMoney(
  fmt: Pick<Formatters, 'money'>,
  amount: number,
  currency: string,
  opts: { signed?: boolean } = {}
): string {
  return fmt.money(amount, currency, { ...opts, decimals: isoMinorUnits(currency) });
}

type CsvReport = Pick<
  CashflowActuals,
  'reportCurrency' | 'months' | 'expenseCategories' | 'incomeCategories' | 'transfers'
>;

/** Backend money is already rounded decimal TEXT in the report currency. */
export function buildCashflowCsv(
  report: CsvReport,
  monthLabel: (month: number) => string,
  groupLabel: (group: CashflowGroup) => string
): string {
  const suffix = report.reportCurrency.toLowerCase();
  const cell = (value: string | number) => {
    const text = String(value);
    return /[;"\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
  };
  const line = (values: (string | number)[]) => values.map(cell).join(';');
  const section = (head: string, groups: CashflowGroup[]) => [
    '',
    line([head, `amount_${suffix}`, 'transactions']),
    ...groups.map((group) => line([groupLabel(group), group.amount, group.count])),
  ];
  return [
    line(['month', `income_${suffix}`, `expenses_${suffix}`]),
    ...report.months.map((month) => line([monthLabel(month.month), month.income, month.expenses])),
    ...section('expense_category', report.expenseCategories),
    ...section('income_category', report.incomeCategories),
    ...section('transfer', report.transfers),
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
