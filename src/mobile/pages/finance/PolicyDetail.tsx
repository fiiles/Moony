import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import type { RealEstate } from '@shared/schema';
import { useAuth } from '@/hooks/use-auth';
import { useInsurance, useInsuranceMutations } from '@/hooks/use-insurance';
import { insuranceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import type { InsuranceRow } from '@/utils/insurance';
import { EndPolicyDialog } from '@/components/insurance/EndPolicyDialog';
import { InsuranceFormDialog } from '@/components/insurance/InsuranceFormDialog';
import { DocumentsSection } from '@/mobile/components/documents/DocumentsSection';
import { PolicyFacts } from '@/mobile/components/policy/PolicyFacts';
import { PolicyLimits } from '@/mobile/components/policy/PolicyLimits';
import { PolicyMenu } from '@/mobile/components/policy/PolicyMenu';
import { PolicyPremiums } from '@/mobile/components/policy/PolicyPremiums';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useBack } from '@/mobile/hooks/use-back';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { useRetained } from '@/mobile/hooks/use-retained';
import { policyFacts } from '@/mobile/lib/policy-detail';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { policyPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';

/** The types a document of a policy can have, in the order the add sheet offers them. */
const DOCUMENT_TYPES = ['contract', 'certificate', 'claim', 'other'] as const;

/** Route entry: another policy remounts the page, so its dialogs and sheets start over. */
export default function PolicyDetail() {
  const [, params] = useRoute(routes.policyDetail);
  return <PolicyDetailPage key={params?.id} id={params?.id} />;
}

/**
 * The policy detail (spec §8.8 "Pojistka", prototype frame 14): the name in the nav bar with
 * Upravit and `···` (Ukončit or Obnovit, Smazat), the "Kooperativa · Pojištění majetku · aktivní"
 * line, then facts first — the contract card, the limits, the documents, the notes — and the
 * cumulative-premiums line last. The numbers are the desktop detail's (`useInsurance`, the linked
 * property's query) in the policy's own currency, every amount through the eye; the policy form
 * and the end dialog are the desktop's.
 *
 * The page owns the delete (its confirm sheet and the mutation, `useDeleteAndLeave`): the policy
 * drops out of `useInsurance` while the delete is still finishing, which takes the `···` menu
 * away, and the page keeps showing the policy until it has left.
 */
function PolicyDetailPage({ id }: { id: string | undefined }) {
  const { t } = useTranslation('mobile');
  const { t: ti } = useTranslation('insurance');
  const back = useBack(routes.finance);
  const { user } = useAuth();
  const { rows, isLoading, today } = useInsurance();
  // Until the quotes load a null conversion would read as a missing rate: the limits' ranking and
  // the total of the coverage wait for them, as the desktop's do
  const { fxReady } = useCurrency();
  const { setStatus, remove: removePolicy } = useInsuranceMutations();
  const { data: property } = useQuery<RealEstate | null>({
    queryKey: ['insurance-real-estate', id],
    queryFn: () => insuranceApi.getRealEstate(id!),
    enabled: !!id,
  });

  const [editOpen, setEditOpen] = useState(false);
  const [endOpen, setEndOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);

  const current = fxReady ? (rows.find((r) => r.policy.id === id) ?? null) : null;
  const kept = useRetained(current);
  const exit = useDeleteAndLeave({
    home: routes.finance,
    pathOf: policyPath,
    keysOf: (policyId) => [
      ['insurance-documents', policyId],
      ['insurance-real-estate', policyId],
    ],
    remove: (policyId) => removePolicy.mutateAsync(policyId),
    closeSheet: () => setDeleteOpen(false),
  });
  // A policy being deleted stays on screen (as it was) until the screen has left
  const row = exit.deleting ? kept : current;
  const policy = row?.policy;
  const loading = isLoading || !fxReady;

  return (
    <Screen
      title={policy?.policyName ?? ''}
      leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />}
      trailing={
        row && (
          <>
            <NavTextButton label={ti('actions.edit')} onClick={() => setEditOpen(true)} />
            <PolicyMenu
              ended={row.ended}
              onEnd={() => setEndOpen(true)}
              onReactivate={() => setStatus.mutate({ policy: row.policy, status: 'active' })}
              onDelete={() => setDeleteOpen(true)}
            />
          </>
        )
      }
    >
      {row ? (
        <PolicyBody
          row={row}
          property={property}
          noticeDays={user?.insuranceNoticeDays}
          today={today}
        />
      ) : loading ? (
        <PolicySkeleton />
      ) : (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{ti('detail.notFound')}</p>
      )}

      {row && (
        <>
          <InsuranceFormDialog
            key={row.policy.updatedAt}
            policy={row.policy}
            open={editOpen}
            onOpenChange={setEditOpen}
          />
          <EndPolicyDialog
            open={endOpen}
            onOpenChange={setEndOpen}
            policy={row.policy}
            today={today}
            isPending={setStatus.isPending}
            onConfirm={(endDay) =>
              setStatus.mutate(
                { policy: row.policy, status: 'inactive', endDate: endDay },
                { onSuccess: () => setEndOpen(false) }
              )
            }
          />
          <ConfirmSheet
            open={deleteOpen}
            onOpenChange={setDeleteOpen}
            title={ti('confirmDelete.title')}
            description={ti('confirmDelete.description', { name: row.policy.policyName })}
            confirmLabel={ti('actions.delete')}
            isPending={removePolicy.isPending}
            onConfirm={() => exit.run(row.policy.id)}
          />
        </>
      )}
    </Screen>
  );
}

interface PolicyBodyProps {
  row: InsuranceRow;
  property: RealEstate | null | undefined;
  noticeDays: number | null | undefined;
  today: number;
}

/** The loaded screen, top to bottom. */
function PolicyBody({ row, property, noticeDays, today }: PolicyBodyProps) {
  const { t: ti } = useTranslation('insurance');
  const { policy } = row;
  const facts = useMemo(
    () => policyFacts(policy, row, property, noticeDays, today),
    [policy, row, property, noticeDays, today]
  );
  // The nav bar keeps its one title, so the insurer, the type and the state are the first line
  const caption = [
    policy.provider.trim(),
    ti(`types.${policy.type}`, { defaultValue: policy.type }),
    ti(row.ended ? 'detail.statusEnded' : 'detail.statusActive'),
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <>
      <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
        {caption}
      </p>
      <PolicyFacts facts={facts} today={today} />
      <PolicyLimits row={row} />
      <DocumentsSection
        kind="insurance-document"
        ownerId={policy.id}
        fileTypes={DOCUMENT_TYPES}
        typeLabel={(type) => ti(`documents.types.${type}`)}
      />
      <SectionHead title={ti('detail.notes.title')} />
      {policy.notes ? (
        <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1">
          <p className="m-0 whitespace-pre-wrap break-words text-m-body text-ink-2">
            {policy.notes}
          </p>
        </div>
      ) : (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{ti('detail.notes.empty')}</p>
      )}
      <PolicyPremiums row={row} today={today} />
    </>
  );
}

/** While the policy loads: the contract card's rows and the head of the limits in outline. */
function PolicySkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-4 py-1 shadow-e1">
        {[0, 1, 2, 3, 4].map((i) => (
          <div key={i} className="flex min-h-11 items-center justify-between px-1 py-2">
            <div className="skeleton h-3 w-1/4" />
            <div className="skeleton h-3.5 w-2/5" />
          </div>
        ))}
      </div>
      <div className="skeleton mx-0.5 mb-2 mt-4 h-4 w-32" />
      <div className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1">
        {[0, 1, 2].map((i) => (
          <div key={i} className="flex min-h-[52px] items-center gap-3 px-3.5 py-2">
            <div className="skeleton h-3.5 w-2/5" />
          </div>
        ))}
      </div>
    </div>
  );
}
