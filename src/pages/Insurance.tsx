import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Plus, Shield } from 'lucide-react';
import { exportApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useInsurance, useInsuranceMutations } from '@/hooks/use-insurance';
import type { InsuranceRow } from '@/utils/insurance';
import { MCP_INSURANCE_TIP_KEY, MCP_INSURANCE_TIP_LEGACY_KEY } from '@/utils/tips';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { TipBanner } from '@/components/common/TipBanner';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import { InsuranceFormDialog } from '@/components/insurance/InsuranceFormDialog';
import { InsuranceSummary } from '@/components/insurance/InsuranceSummary';
import { PaymentCalendarCard } from '@/components/insurance/PaymentCalendarCard';
import { InsuranceTable } from '@/components/insurance/InsuranceTable';
import { EndPolicyDialog } from '@/components/insurance/EndPolicyDialog';

type Modal = 'add' | 'edit' | 'end' | 'delete' | null;

/**
 * Insurance (design system §7 List, prototype insurance.html): am I covered
 * and what is due — four stats, the twelve-month payment calendar and the
 * policies table with next payment and anniversary per policy.
 */
export default function Insurance() {
  const { t } = useTranslation('insurance');
  const [, navigate] = useLocation();
  const { formatCurrency } = useCurrency();
  const fmt = useFormat();
  const { rows, metrics, isLoading, today } = useInsurance();
  const { setStatus, remove } = useInsuranceMutations();
  const [modal, setModal] = useState<Modal>(null);
  const [selected, setSelected] = useState<InsuranceRow | null>(null);

  useShellPage({
    status: metrics.nextPayment
      ? {
          text: t('status.nextPayment', {
            date: fmt.day(metrics.nextPayment.day),
            amount: formatCurrency(metrics.nextPayment.amountCzk),
          }),
          tone: 'neutral',
        }
      : rows.length > 0
        ? { text: t('status.none'), tone: 'neutral' }
        : undefined,
  });

  const close = () => {
    setModal(null);
    setSelected(null);
  };
  const closeIf = (next: boolean) => {
    if (!next) close();
  };
  const open = (which: Modal) => (row: InsuranceRow) => {
    setSelected(row);
    setModal(which);
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
              <ExportButton exportFn={exportApi.insurancePolicies} label={t('export')} />
              <Button onClick={() => setModal('add')}>
                <Plus />
                {t('addPolicy')}
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
          icon={<Shield />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <div className="flex flex-col items-center gap-4">
              <Button onClick={() => setModal('add')}>
                <Plus />
                {t('addPolicy')}
              </Button>
              {/* the AI route is a quiet secondary line here, not a banner above the list */}
              <TipBanner
                className="w-full max-w-md text-left"
                storageKey={MCP_INSURANCE_TIP_KEY}
                legacyStorageKey={MCP_INSURANCE_TIP_LEGACY_KEY}
                summary={t('mcp.tipSummary')}
                details={
                  <>
                    <p>{t('mcp.bannerBody')}</p>
                    <button
                      type="button"
                      onClick={() => navigate('/settings/integrations#mcp')}
                      className="underline underline-offset-[3px] text-ink hover:text-ink-2"
                    >
                      {t('mcp.setupLink')}
                    </button>
                  </>
                }
              />
            </div>
          }
        />
      ) : (
        <>
          <InsuranceSummary metrics={metrics} today={today} />
          <PaymentCalendarCard rows={rows} today={today} />
          <InsuranceTable
            rows={rows}
            today={today}
            onEdit={open('edit')}
            onEnd={open('end')}
            onReactivate={(row) => setStatus.mutate({ policy: row.policy, status: 'active' })}
            onDelete={open('delete')}
          />
        </>
      )}

      <InsuranceFormDialog open={modal === 'add'} onOpenChange={closeIf} />
      {selected && (
        <InsuranceFormDialog
          key={selected.policy.id}
          policy={selected.policy}
          open={modal === 'edit'}
          onOpenChange={closeIf}
        />
      )}
      <EndPolicyDialog
        open={modal === 'end'}
        onOpenChange={closeIf}
        policy={selected?.policy ?? null}
        today={today}
        isPending={setStatus.isPending}
        onConfirm={(endDay) =>
          selected &&
          setStatus.mutate(
            { policy: selected.policy, status: 'inactive', endDate: endDay },
            { onSuccess: close }
          )
        }
      />
      <ConfirmDeleteDialog
        open={modal === 'delete'}
        onOpenChange={closeIf}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: selected?.policy.policyName ?? '' })}
        onConfirm={() => selected && remove.mutate(selected.policy.id, { onSuccess: close })}
        isPending={remove.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
