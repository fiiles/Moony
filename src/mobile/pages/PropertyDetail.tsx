import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { ArrowUp } from 'lucide-react';
import type { RealEstate } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import type { PropertyRow, YearlyBalance } from '@/utils/real-estate';
import { Button } from '@/components/ui/button';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { PropertyFinance } from '@/mobile/components/property/PropertyFinance';
import { PropertyHeroCard } from '@/mobile/components/property/PropertyHeroCard';
import { PropertyMenu } from '@/mobile/components/property/PropertyMenu';
import { PropertyStats } from '@/mobile/components/property/PropertyStats';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useBack } from '@/mobile/hooks/use-back';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { useProperty, type PropertyData } from '@/mobile/hooks/use-property';
import { usePropertyMutations } from '@/mobile/hooks/use-property-mutations';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRetained } from '@/mobile/hooks/use-retained';
import { monthlyRent, propertyOwnKeys } from '@/mobile/lib/property-detail';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { propertyPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { RevalueSheet } from '@/mobile/sheets/RevalueSheet';

/** A property that has loaded: what the screen draws from. */
type LoadedProperty = PropertyData & {
  property: RealEstate;
  row: PropertyRow;
  balance: YearlyBalance;
};

/** Route entry: another property remounts the page, so its sheets and dialogs start over. */
export default function PropertyDetail() {
  const [, params] = useRoute(routes.propertyDetail);
  return <PropertyDetailPage key={params?.id} id={params?.id} />;
}

/**
 * The property detail (spec §8.8 "Nemovitost"): the name in the nav bar with `···` (Upravit,
 * Historie přecenění, Smazat nemovitost), the address and the "Investiční · pronajato" line, the
 * value with its trace and the change against the purchase, four stats, the primary **Přecenit**,
 * Finance (the linked loans and policies with Připojit, the yearly balance) and the notes. The
 * numbers are the desktop detail's (`useProperty`, the pure rules in `property-detail.ts`) in the
 * main currency, a missing rate shown as "—" and named in the note under the line; the property
 * form and the list of estimates are the desktop's dialogs.
 *
 * The page owns the delete (its confirm sheet and the mutation, `useDeleteAndLeave`): the property
 * drops out of the cache while the delete is still finishing, which takes the `···` menu away, and
 * the page keeps showing the property until it has left.
 */
function PropertyDetailPage({ id }: { id: string | undefined }) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('realEstate');
  const back = useBack(routes.wealth);
  const { currencyCode } = useCurrency();
  const data = useProperty(id);
  const { remove } = usePropertyMutations(id);

  const [revalueOpen, setRevalueOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);

  // Until the quotes load a null conversion would read as a missing rate: the screen waits for them
  const current = useMemo<LoadedProperty | null>(
    () =>
      !data.isLoading && data.property && data.row && data.balance
        ? { ...data, property: data.property, row: data.row, balance: data.balance }
        : null,
    [data]
  );
  const kept = useRetained(current);
  const exit = useDeleteAndLeave({
    home: routes.wealth,
    pathOf: propertyPath,
    keysOf: propertyOwnKeys,
    remove: () => remove.mutateAsync(),
    closeSheet: () => setDeleteOpen(false),
  });
  // A property being deleted stays on screen (as it was) until the screen has left
  const view = exit.deleting ? kept : current;

  return (
    <Screen
      title={view?.property.name ?? ''}
      leading={<NavTextButton label={t('tabs.wealth')} onClick={back} chevron />}
      trailing={
        view && (
          <PropertyMenu
            property={view.property}
            valuations={view.valuations}
            onDelete={() => setDeleteOpen(true)}
          />
        )
      }
    >
      {view ? (
        <PropertyBody view={view} currency={currencyCode} onRevalue={() => setRevalueOpen(true)} />
      ) : data.isLoading ? (
        <PropertySkeleton />
      ) : (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{t('property.notFound')}</p>
      )}

      {view && (
        <>
          <RevalueSheet open={revalueOpen} onOpenChange={setRevalueOpen} property={view.property} />
          <ConfirmSheet
            open={deleteOpen}
            onOpenChange={setDeleteOpen}
            title={tr('confirmDelete.title')}
            description={tr('confirmDelete.description', { name: view.property.name })}
            confirmLabel={tr('detail.menu.delete')}
            isPending={remove.isPending}
            onConfirm={() => exit.run(view.property.id)}
          />
        </>
      )}
    </Screen>
  );
}

interface PropertyBodyProps {
  view: LoadedProperty;
  /** The main currency. */
  currency: string;
  onRevalue: () => void;
}

/** The loaded screen, top to bottom. */
function PropertyBody({ view, currency, onRevalue }: PropertyBodyProps) {
  const { t: tr } = useTranslation('realEstate');
  const moneyText = useMoneyText();
  const { property, row, balance, trace, missing, loans, policies } = view;

  // The nav bar keeps its one title, so the address, the type and the rent are the first lines
  const rent = monthlyRent(property, balance);
  const state = balance.hasRent
    ? tr('detail.rented', {
        rent:
          rent.main !== null
            ? moneyText(rent.main, currency, { decimals: 0 })
            : moneyText(rent.native.amount, rent.native.currency, { decimals: 0 }),
      })
    : tr('detail.notRented');

  return (
    <>
      <div className="mx-0.5 -mt-0.5 mb-2.5 text-center text-m-caption text-ink-3">
        {property.address && <p className="m-0 truncate">{property.address}</p>}
        <p className="m-0">
          {[tr(`types.${property.type}`, { defaultValue: property.type }), state].join(' · ')}
        </p>
      </div>
      {/* Amounts that could not be converted into the main currency (left out of its totals) */}
      <ConversionQualityNote
        quality={{ missingCurrencies: missing }}
        currency={currency}
        className="mx-0.5 mb-3.5"
      />
      <PropertyHeroCard row={row} trace={trace} currency={currency} />
      <PropertyStats row={row} balance={balance} currency={currency} />
      <Button
        className="h-12 w-full whitespace-normal rounded-[10px] text-m-row [&_svg]:size-4"
        onClick={onRevalue}
      >
        <ArrowUp strokeWidth={1.75} aria-hidden />
        {tr('detail.revalue')}
      </Button>
      <PropertyFinance
        property={property}
        balance={balance}
        loans={loans}
        policies={policies}
        currency={currency}
      />
      {/* Náklady, Galerie and Dokumenty go here, between Finance and Poznámky (the next task) */}
      <SectionHead title={tr('detail.tabs.notes')} />
      {/* Read here; written in Upravit, where the desktop keeps them */}
      {property.notes ? (
        <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1">
          <p className="m-0 whitespace-pre-wrap break-words text-m-body text-ink-2">
            {property.notes}
          </p>
        </div>
      ) : (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{tr('detail.noNotes')}</p>
      )}
    </>
  );
}

/** While the property loads: the hero, the stats and a few rows in outline. */
function PropertySkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-32" />
        <div className="skeleton mt-3 h-9 w-56" />
        <div className="skeleton mt-3 h-3 w-48" />
        <div className="skeleton mt-4 h-[160px] w-full" />
      </div>
      <div className="mb-3.5 grid grid-cols-2 gap-2.5">
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className="rounded-r3 border border-line bg-paper px-3 py-2.5 shadow-e1">
            <div className="skeleton h-2.5 w-3/5" />
            <div className="skeleton mt-2.5 h-4 w-4/5" />
          </div>
        ))}
      </div>
      <div className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1">
        {[0, 1].map((i) => (
          <div key={i} className="flex min-h-[60px] items-center gap-3 px-3.5 py-2.5">
            <div className="skeleton size-9 flex-none rounded-full" />
            <div className="min-w-0 flex-1">
              <div className="skeleton h-3.5 w-2/5" />
              <div className="skeleton mt-2 h-3 w-3/5" />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
