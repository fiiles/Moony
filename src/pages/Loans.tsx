import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CreditCard, Plus } from 'lucide-react';
import type { InsertLoan, Loan } from '@shared/schema';
import { exportApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { useLoans } from '@/hooks/use-loans';
import { useLoanMutations } from '@/hooks/use-loan-mutations';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import { LoanFormDialog } from '@/components/loans/LoanFormDialog';
import { LoansSummary } from '@/components/loans/LoansSummary';
import { DebtTrajectoryCard } from '@/components/loans/DebtTrajectoryCard';
import { LoansTable } from '@/components/loans/LoansTable';

type Modal = 'add' | 'edit' | 'delete' | null;

/**
 * Loans (design system §7 List, prototype loans.html): how much I owe and when
 * it ends — four stats, the debt trajectory with milestones, and the table with
 * the repaid share and payoff month per loan.
 */
export default function Loans() {
  const { t } = useTranslation('loans');
  const fmt = useFormat();
  const { rows, metrics, isLoading, today } = useLoans();
  const { createMutation, updateMutation, deleteMutation } = useLoanMutations();
  const [modal, setModal] = useState<Modal>(null);
  const [selected, setSelected] = useState<Loan | null>(null);

  useShellPage({ status: { text: t('status', { date: fmt.day(today) }), tone: 'neutral' } });

  const close = () => {
    setModal(null);
    setSelected(null);
  };
  const closeIf = (next: boolean) => {
    if (!next) close();
  };
  const open = (which: Modal) => (loan: Loan) => {
    setSelected(loan);
    setModal(which);
  };

  const handleAddSubmit = (data: InsertLoan | (Partial<Loan> & { id: string })) => {
    if ('id' in data) return;
    createMutation.mutate(data, { onSuccess: close });
  };
  const handleEditSubmit = (data: InsertLoan | (Partial<Loan> & { id: string })) => {
    if (!('id' in data)) return;
    updateMutation.mutate(data as { id: string } & Partial<InsertLoan>, { onSuccess: close });
  };

  const isEmpty = !isLoading && rows.length === 0;

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          !isEmpty && (
            <>
              <ExportButton exportFn={exportApi.loans} label={t('export')} />
              <Button onClick={() => setModal('add')}>
                <Plus />
                {t('addLoan')}
              </Button>
            </>
          )
        }
      />

      {isLoading ? (
        <Stats>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      ) : isEmpty ? (
        <EmptyState
          icon={<CreditCard />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <Button onClick={() => setModal('add')}>
              <Plus />
              {t('addLoan')}
            </Button>
          }
        />
      ) : (
        <>
          <LoansSummary metrics={metrics} today={today} />
          <DebtTrajectoryCard rows={rows} metrics={metrics} today={today} />
          <LoansTable rows={rows} today={today} onEdit={open('edit')} onDelete={open('delete')} />
        </>
      )}

      <LoanFormDialog
        open={modal === 'add'}
        onOpenChange={closeIf}
        onSubmit={handleAddSubmit}
        isLoading={createMutation.isPending}
      />
      <LoanFormDialog
        open={modal === 'edit'}
        onOpenChange={closeIf}
        onSubmit={handleEditSubmit}
        loan={selected}
        isLoading={updateMutation.isPending}
      />
      <ConfirmDeleteDialog
        open={modal === 'delete'}
        onOpenChange={closeIf}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: selected?.name ?? '' })}
        onConfirm={() => selected && deleteMutation.mutate(selected.id, { onSuccess: close })}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
