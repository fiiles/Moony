import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FileText, Plus } from 'lucide-react';
import type { Bond, InsertBond } from '@shared/schema';
import { exportApi } from '@/lib/tauri-api';
import { useBonds } from '@/hooks/use-bonds';
import { useBondMutations } from '@/hooks/use-bond-mutations';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import { BondsSummary } from '@/components/bonds/BondsSummary';
import { BondsLadderCard } from '@/components/bonds/BondsLadderCard';
import { BondsTable } from '@/components/bonds/BondsTable';
import { BondsFormDialog } from '@/components/bonds/BondsFormDialog';

type Modal = 'add' | 'edit' | 'delete' | null;

/**
 * Bonds list (design system §7 List, prototype bonds.html): four stats, the
 * maturity-and-coupon ladder, the issues table, one form dialog for add and
 * edit, a confirm dialog for delete.
 */
export default function Bonds() {
  const { t } = useTranslation('bonds');
  const { bonds, metrics, isLoading, today } = useBonds();
  const { createMutation, updateMutation, deleteMutation } = useBondMutations();
  const [modal, setModal] = useState<Modal>(null);
  const [selected, setSelected] = useState<Bond | null>(null);

  useShellPage({ status: { text: t('status'), tone: 'neutral' } });

  const close = () => {
    setModal(null);
    setSelected(null);
  };
  const openEdit = (bond: Bond) => {
    setSelected(bond);
    setModal('edit');
  };
  const openDelete = (bond: Bond) => {
    setSelected(bond);
    setModal('delete');
  };
  const submit = (data: InsertBond) => {
    if (modal === 'edit' && selected) {
      updateMutation.mutate({ id: selected.id, ...data }, { onSuccess: close });
    } else {
      createMutation.mutate(data, { onSuccess: close });
    }
  };

  const isEmpty = !isLoading && bonds.length === 0;

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          !isEmpty && (
            <>
              <ExportButton exportFn={exportApi.bonds} label={t('export')} />
              <Button onClick={() => setModal('add')}>
                <Plus />
                {t('addBond')}
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
          icon={<FileText />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <Button onClick={() => setModal('add')}>
              <Plus />
              {t('addBond')}
            </Button>
          }
        />
      ) : (
        <>
          <BondsSummary metrics={metrics} today={today} />
          <BondsLadderCard bonds={bonds} today={today} />
          <BondsTable bonds={bonds} today={today} onEdit={openEdit} onDelete={openDelete} />
        </>
      )}

      <BondsFormDialog
        open={modal === 'add' || modal === 'edit'}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        onSubmit={submit}
        bond={modal === 'edit' ? selected : null}
        isLoading={createMutation.isPending || updateMutation.isPending}
      />
      <ConfirmDeleteDialog
        open={modal === 'delete'}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: selected?.name ?? '' })}
        onConfirm={() => selected && deleteMutation.mutate(selected.id, { onSuccess: close })}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
