import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Download, Ellipsis, FileText, Plus } from 'lucide-react';
import type { Bond, InsertBond } from '@shared/schema';
import { useBondMutations } from '@/hooks/use-bond-mutations';
import { useBonds, type BondsMetrics } from '@/hooks/use-bonds';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { bondCurrency, bondFaceValue } from '@/utils/bonds';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { BondsFormDialog } from '@/components/bonds/BondsFormDialog';
import { ScaledConversionNote } from '@/mobile/components/Scaled';
import { RowMenuCheckboxItem, RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { BondPaymentRow } from '@/mobile/components/wealth/BondPaymentRow';
import { BondRow } from '@/mobile/components/wealth/BondRow';
import { ClassEmpty } from '@/mobile/components/wealth/ClassEmpty';
import { ClassSkeleton } from '@/mobile/components/wealth/ClassSkeleton';
import { MoreRow } from '@/mobile/components/wealth/MoreRow';
import { LoadFailed } from '@/mobile/components/wealth/LoadFailed';
import { useBack, useBackLabel } from '@/mobile/hooks/use-back';
import { useBondsLoadFailed } from '@/mobile/hooks/use-bonds-failed';
import { useClassExport } from '@/mobile/hooks/use-class-export';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import { bondRows } from '@/mobile/lib/bond-rows';
import { upcomingBondPayments, type BondPayment } from '@/mobile/lib/bond-schedule';
import { foldRows } from '@/mobile/lib/position-rows';
import { statMoneyOptions, statTone } from '@/mobile/lib/stat-money';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/**
 * Dluhopisy, the class page of Majetek (spec §8.8): three stats (the face value of the live
 * issues, the yearly coupon, the nearest maturity), "Nejbližší kupóny a splatnosti" — what is
 * paid in the next twelve months, in each bond's own currency — and the issues as rows. The stats
 * are the desktop's own (`useBonds`), in the main currency; the rows and the dated list are native
 * (ADR 0012). Every amount goes through the eye of Přehled. A tap on an issue opens the desktop's
 * form (ruling 4), a long press its menu; adding is the same form from the `+`. Repaid issues sit
 * behind "Zobrazit splacené" in the `···` menu, remembered for the session.
 */
export default function Bonds() {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bonds');
  const { t: td } = useTranslation('dashboard');
  const { t: tc } = useTranslation('common');
  const back = useBack(routes.wealth);
  // The page the push came from (another tab, a sheet: `backLabel`), else the parent
  const backText = useBackLabel(t('tabs.wealth'));
  const { bonds, metrics, isLoading, today } = useBonds();
  const loadFailed = useBondsLoadFailed();
  const { exportCsv, isPending: exporting } = useClassExport('bonds');
  const { currencyCode, fxReady } = useCurrency();
  const { createMutation, updateMutation, deleteMutation } = useBondMutations();
  const [showMatured, setShowMatured] = useRemembered('bonds.showMatured', false);
  // The form's bond stays after it closes, so the title does not flip while it fades out
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Bond | null>(null);
  const [deleting, setDeleting] = useState<Bond | null>(null);

  const openAdd = () => {
    setEditing(null);
    setFormOpen(true);
  };
  const openEdit = (bond: Bond) => {
    setEditing(bond);
    setFormOpen(true);
  };
  const closeForm = () => setFormOpen(false);
  const submit = (data: InsertBond) => {
    if (editing) updateMutation.mutate({ id: editing.id, ...data }, { onSuccess: closeForm });
    else createMutation.mutate(data, { onSuccess: closeForm });
  };

  const rows = bondRows(bonds, today, showMatured);
  const payments = upcomingBondPayments(bonds, today);

  return (
    <Screen
      title={td('cards.bonds')}
      leading={<NavTextButton label={backText} onClick={back} chevron />}
      trailing={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <NavIconButton aria-label={tc('labels.moreActions')}>
                <Ellipsis strokeWidth={1.75} aria-hidden />
              </NavIconButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[240px]">
              <RowMenuCheckboxItem checked={showMatured} onCheckedChange={setShowMatured}>
                {tb('table.showMatured')}
              </RowMenuCheckboxItem>
              <DropdownMenuSeparator />
              <RowMenuItem disabled={exporting} onSelect={() => void exportCsv()}>
                <Download strokeWidth={1.75} aria-hidden />
                {tc('buttons.export')}
              </RowMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <NavIconButton aria-label={tb('addBond')} onClick={openAdd}>
            <Plus strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </>
      }
    >
      {isLoading || !fxReady ? (
        <ClassSkeleton />
      ) : bonds.length === 0 ? (
        // A failed load is not "no bonds yet": said as such, with no empty state to add the first one
        loadFailed ? (
          <LoadFailed>{t('wealth.bonds.loadFailed')}</LoadFailed>
        ) : (
          <ClassEmpty
            icon={<FileText />}
            title={tb('empty.title')}
            description={tb('empty.description')}
            addLabel={tb('addBond')}
            onAdd={openAdd}
          />
        )
      ) : (
        <>
          {/* Live issues in a currency without a rate are left out of the stats (named here) */}
          <ScaledConversionNote
            quality={{ missingCurrencies: metrics.missing }}
            currency={currencyCode}
            className="mx-0.5 mb-3.5"
          />
          <BondStats metrics={metrics} currency={currencyCode} />
          <UpcomingPayments payments={payments} />
          <SectionHead title={tb('table.title')} />
          {rows.length === 0 ? (
            <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
              {t('wealth.bonds.allRepaid', { action: tb('table.showMatured') })}
            </p>
          ) : (
            <EntityRows>
              {rows.map((row) => (
                <BondRow key={row.bond.id} row={row} onEdit={openEdit} onDelete={setDeleting} />
              ))}
            </EntityRows>
          )}
        </>
      )}
      <BondsFormDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        onSubmit={submit}
        bond={editing}
        isLoading={createMutation.isPending || updateMutation.isPending}
      />
      <ConfirmSheet
        open={deleting !== null}
        onOpenChange={(open) => {
          if (!open) setDeleting(null);
        }}
        title={tb('confirmDelete.title')}
        description={tb('confirmDelete.description', { name: deleting?.name ?? '' })}
        confirmLabel={tb('actions.delete')}
        isPending={deleteMutation.isPending}
        onConfirm={() => {
          if (!deleting) return;
          deleteMutation.mutate(deleting.id, { onSuccess: () => setDeleting(null) });
        }}
      />
    </Screen>
  );
}

/**
 * Three stats (spec §8.8, the desktop's `BondsSummary`): the face value of the live issues with
 * their number, the yearly coupon with the weighted rate (a credit, so green), and the nearest
 * maturity with what it returns. The stats are in the main currency at the current rates; the
 * nearest maturity's amount, when its currency has no rate, stands in its own. When every live
 * issue lacks a rate the face value and the coupon are "—" (ADR 0012), not a 0 that is no sum.
 */
function BondStats({ metrics, currency }: { metrics: BondsMetrics; currency: string }) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bonds');
  const fmt = useFormat();
  const moneyText = useMoneyText();

  const income = metrics.projectedYearlyIncome;
  // Live issues, none of them in a currency with a rate: the totals are not 0, they are unknown
  const unconverted =
    metrics.liveCount > 0 && metrics.currencies.every((code) => metrics.missing.includes(code));
  const { nearest } = metrics;
  const nearestDay = nearest?.maturityDate ?? null;
  const returns =
    nearest === null
      ? null
      : metrics.nearestValue !== null
        ? moneyText(metrics.nearestValue, currency, { decimals: 0 })
        : moneyText(bondFaceValue(nearest), bondCurrency(nearest));

  const stats: StatItem[] = [
    {
      label: tb('summary.nominal'),
      value: unconverted
        ? '—'
        : moneyText(metrics.totalValue, currency, statMoneyOptions(metrics.totalValue)),
      note: tb('summary.issues', { count: metrics.liveCount }),
    },
    {
      label: tb('summary.yearlyCoupon'),
      value: unconverted
        ? '—'
        : moneyText(income, currency, { ...statMoneyOptions(income), signed: true }),
      tone: statTone(income),
      // The figure first, since the note is cut at the third of a row; no rate for what has none
      note: unconverted
        ? undefined
        : t('wealth.bonds.rateNote', { rate: fmt.percent(metrics.averageYield / 100, 2) }),
    },
    {
      label: tb('summary.nearestMaturity'),
      value: nearestDay !== null ? fmt.day(nearestDay) : '—',
      note:
        nearestDay !== null && returns !== null
          ? tb('summary.returns', { amount: returns })
          : metrics.liveCount > 0
            ? tb('summary.noMaturity')
            : tb('summary.noneLive'),
    },
  ];
  return <StatRow stats={stats} />;
}

/**
 * "Nejbližší kupóny a splatnosti" (spec §8.8, ruling 5): the next twelve months of payments, the
 * first eight until "Zobrazit všech N", or the line that says there is none. Bonds without a
 * maturity have no day and are not here.
 */
function UpcomingPayments({ payments }: { payments: readonly BondPayment[] }) {
  const { t } = useTranslation('mobile');
  const [expanded, setExpanded] = useState(false);
  const { shown, hidden } = foldRows(payments, expanded);

  return (
    <>
      <SectionHead title={t('wealth.bonds.upcoming')} />
      {payments.length === 0 ? (
        <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
          {t('wealth.bonds.noUpcoming')}
        </p>
      ) : (
        <EntityRows>
          {shown.map((payment) => (
            <BondPaymentRow key={`${payment.bond.id}-${payment.t}`} payment={payment} />
          ))}
          {hidden > 0 && (
            <MoreRow
              label={t('wealth.position.tx.showAll', { count: payments.length })}
              onClick={() => setExpanded(true)}
            />
          )}
        </EntityRows>
      )}
    </>
  );
}
