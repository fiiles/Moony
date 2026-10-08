import { ReportTransactionsDialog } from '@/components/reports/ReportTransactionsDialog';
export interface CategoryTransactionsTarget {
  categoryId: string;
  name: string;
  budgetCurrency?: string;
}
export function CategoryTransactionsDialog({
  target,
  onClose,
  start,
  end,
  accountIds,
  reportCurrency,
  ratesTimestamp,
}: {
  target: CategoryTransactionsTarget | null;
  onClose: () => void;
  start: number;
  end: number;
  periodLabel: string;
  accountIds?: string[];
  reportCurrency: string;
  ratesTimestamp: number;
}) {
  return (
    <ReportTransactionsDialog
      key={target?.categoryId}
      target={target ? { ...target, flowKind: 'expense' } : null}
      start={start}
      end={end}
      accountIds={accountIds}
      reportCurrency={reportCurrency}
      ratesTimestamp={ratesTimestamp}
      onClose={onClose}
    />
  );
}
