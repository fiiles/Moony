import { ReportTransactionsDialog } from '@/components/reports/ReportTransactionsDialog';
export interface CategoryTransactionsTarget {
  categoryId: string;
  name: string;
}
export function CategoryTransactionsDialog({
  target,
  onClose,
  start,
  end,
  accountIds,
}: {
  target: CategoryTransactionsTarget | null;
  onClose: () => void;
  start: number;
  end: number;
  periodLabel: string;
  accountIds?: string[];
}) {
  return (
    <ReportTransactionsDialog
      key={target?.categoryId}
      target={target ? { ...target, flowKind: 'expense' } : null}
      start={start}
      end={end}
      accountIds={accountIds}
      onClose={onClose}
    />
  );
}
