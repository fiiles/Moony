import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { Pencil } from 'lucide-react';
import type { RealEstate } from '@shared/schema';
import { useAuth } from '@/hooks/use-auth';
import { useDurationText } from '@/hooks/use-duration-text';
import { useInsurance, useInsuranceMutations } from '@/hooks/use-insurance';
import { insuranceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import type { InsuranceRow } from '@/utils/insurance';
import { Button } from '@/components/ui/button';
import { EndPolicyDialog } from '@/components/insurance/EndPolicyDialog';
import { InsuranceFormDialog } from '@/components/insurance/InsuranceFormDialog';
import { DetailSub } from '@/mobile/components/DetailSub';
import { Hero } from '@/mobile/components/Hero';
import { MissingOrFailed } from '@/mobile/components/LoadError';
import { DocumentsSection } from '@/mobile/components/documents/DocumentsSection';
import { PolicyFacts } from '@/mobile/components/policy/PolicyFacts';
import { PolicyLimits } from '@/mobile/components/policy/PolicyLimits';
import { PolicyMenu } from '@/mobile/components/policy/PolicyMenu';
import { PolicyPremiums } from '@/mobile/components/policy/PolicyPremiums';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useBack, useBackLabel } from '@/mobile/hooks/use-back';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { useRetained } from '@/mobile/hooks/use-retained';
import { useRowIntent } from '@/mobile/hooks/use-row-intent';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { exactMoneyOptions } from '@/mobile/lib/money-rule';
import { policyFacts, policyHero } from '@/mobile/lib/policy-detail';
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
 * The policy detail (spec §8.8 "Pojistka", prototype frame 14): the name in the nav bar with `···`
 * (Ukončit or Obnovit, Smazat), the "Kooperativa · Pojištění majetku · aktivní" line, then facts
 * first — the contract card with the full-width Upravit button under it, the limits, the
 * documents, the notes — and the cumulative-premiums line last. The numbers are the desktop
 * detail's (`useInsurance`, the linked
 * property's query) in the policy's own currency; the policy form
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
  // The page the push came from (another tab, a sheet: `backLabel`), else the parent
  const backText = useBackLabel(t('tabs.finance'));
  const { user } = useAuth();
  const { rows, isLoading, today } = useInsurance();
  // Until the quotes load a null conversion would read as a missing rate: the limits' ranking and
  // the total of the coverage wait for them, as the desktop's do
  const { fxReady } = useCurrency();
  const { setStatus, remove: removePolicy } = useInsuranceMutations();
  // The property a policy is linked to changes from places this page does not see (Připojit
  // pojistku and Odpojit on the property detail, the desktop dialogs, a deleted property), so it is
  // read fresh on every open, whatever the global `staleTime` says (the pattern of `FRESH_ON_OPEN`
  // in `use-property.ts`)
  const { data: property } = useQuery<RealEstate | null>({
    queryKey: ['insurance-real-estate', id],
    queryFn: () => insuranceApi.getRealEstate(id!),
    enabled: !!id,
    staleTime: 0,
    refetchOnMount: 'always',
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
  // A policy row's menu (README §13, Gestures) opens the form or the end dialog here
  useRowIntent(
    {
      edit: () => setEditOpen(true),
      end: () => {
        if (row && !row.ended) setEndOpen(true);
      },
    },
    !!row
  );

  return (
    <Screen
      title={policy?.policyName ?? ''}
      large
      leading={<NavTextButton label={backText} onClick={back} chevron />}
      trailing={
        row && (
          <PolicyMenu
            ended={row.ended}
            onEnd={() => setEndOpen(true)}
            onReactivate={() => setStatus.mutate({ policy: row.policy, status: 'active' })}
            onDelete={() => setDeleteOpen(true)}
          />
        )
      }
    >
      {row ? (
        <PolicyBody
          row={row}
          property={property}
          noticeDays={user?.insuranceNoticeDays}
          today={today}
          onEdit={() => setEditOpen(true)}
        />
      ) : loading ? (
        <PolicySkeleton />
      ) : (
        <MissingOrFailed>{ti('detail.notFound')}</MissingOrFailed>
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
  onEdit: () => void;
}

/** The loaded screen, top to bottom. */
function PolicyBody({ row, property, noticeDays, today, onEdit }: PolicyBodyProps) {
  const { t: ti } = useTranslation('insurance');
  const { policy } = row;
  // The premium and the next payment lead in the hero (D15), so the card leaves them out
  const facts = useMemo(
    () =>
      policyFacts(policy, row, property, noticeDays, today).filter(
        (fact) => fact.key !== 'premium' && fact.key !== 'nextPayment'
      ),
    [policy, row, property, noticeDays, today]
  );
  // The insurer, the type and the state under the large title
  const sub = [
    policy.provider.trim(),
    ti(`types.${policy.type}`, { defaultValue: policy.type }),
    ti(row.ended ? 'detail.statusEnded' : 'detail.statusActive'),
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <>
      <DetailSub>{sub}</DetailSub>
      <PremiumHero row={row} today={today} />
      <PolicyFacts facts={facts} today={today} from={policy.policyName} />
      <Button
        variant="secondary"
        className="mb-3.5 h-auto min-h-12 w-full whitespace-normal py-2 text-center rounded-[10px] text-m-row [&_svg]:size-4"
        onClick={onEdit}
      >
        <Pencil strokeWidth={1.75} aria-hidden />
        {ti('actions.editPolicy')}
      </Button>
      <PolicyLimits row={row} />
      <DocumentsSection
        kind="insurance-document"
        ownerId={policy.id}
        fileTypes={DOCUMENT_TYPES}
        typeLabel={(type) => ti(`documents.types.${type}`)}
      />
      <SectionHead title={ti('detail.notes.title')} />
      {/* A card like every other section, also when it is empty (D16) */}
      <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1">
        {policy.notes ? (
          <p className="m-0 whitespace-pre-wrap break-words text-m-body text-ink-2">
            {policy.notes}
          </p>
        ) : (
          <p className="m-0 text-m-sub text-ink-3">{ti('detail.notes.empty')}</p>
        )}
      </div>
      <PolicyPremiums row={row} today={today} />
    </>
  );
}

/**
 * The policy's hero (D15), as the account and the loan lead with theirs: the premium with its
 * frequency, exact as a single payment is, in the policy's own currency, and the next payment
 * under it — or the one-time amount and its day, or that the policy has ended.
 */
function PremiumHero({ row, today }: { row: InsuranceRow; today: number }) {
  const { t } = useTranslation('mobile');
  const { t: ti } = useTranslation('insurance');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const duration = useDurationText('insurance');
  const hero = policyHero(row.policy, row);

  if (hero.kind === 'none') {
    return (
      <Hero
        label={ti('detail.contract.premium')}
        value={null}
        formatValue={String}
        note={t('policy.hero.noPremium')}
      />
    );
  }
  const exact = (value: number) =>
    moneyText(value, hero.currency, exactMoneyOptions(value, hero.currency));
  if (hero.kind === 'oneTime') {
    return (
      <Hero
        label={ti('detail.contract.oneTime')}
        value={hero.amount}
        formatValue={exact}
        note={t('policy.hero.paidOn', { date: fmt.day(hero.paidOn) })}
      />
    );
  }
  const note = hero.ended
    ? hero.endDay !== null
      ? t('policy.hero.ended', { date: fmt.day(hero.endDay) })
      : t('policy.hero.endedPlain')
    : hero.nextPayment !== null
      ? t('policy.hero.next', {
          date: fmt.day(hero.nextPayment),
          distance: ti('table.in', { duration: duration.ahead(today, hero.nextPayment) }),
        })
      : undefined;
  return (
    <Hero
      label={t('policy.hero.label', { frequency: ti(`frequencyShort.${hero.frequency}`) })}
      value={hero.amount}
      formatValue={exact}
      note={note}
    />
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
