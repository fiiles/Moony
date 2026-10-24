import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Download, Ellipsis, Gem, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { priceDecimals } from '@shared/currencies';
import { useFormat } from '@/lib/use-format';
import type { OtherAssetRow } from '@/utils/other-assets';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { ChangeLine } from '@/mobile/components/wealth/ChangeLine';
import { ClassEmpty } from '@/mobile/components/wealth/ClassEmpty';
import { ClassHero } from '@/mobile/components/wealth/ClassHero';
import { ClassSkeleton } from '@/mobile/components/wealth/ClassSkeleton';
import { useBack } from '@/mobile/hooks/use-back';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useOtherAssets } from '@/mobile/hooks/use-other-asset';
import { OtherAssetAddSheet } from '@/mobile/sheets/OtherAssetAddSheet';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { otherAssetPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** A quantity of an asset is held to eight decimals, as the desktop's transactions list writes it. */
const QUANTITY_DIGITS = 8;

/**
 * Ostatní aktiva, the class page of Majetek (spec §8.8): the value of the assets in the report
 * currency with its line, then the assets as rows, the highest value first — what is held at
 * which price, the value and the gain against what it cost. The desktop's stats are not repeated:
 * the hero carries the value and what is invested, the rows the gains. The numbers are the
 * desktop's (`useOtherAssets`, the same queries); every amount goes through the eye of Přehled.
 * Nothing owned is the class's empty state with the button that adds the first asset, which is
 * also the `+` of the page: the phone's add sheet.
 */
export default function OtherAssets() {
  const { t } = useTranslation('mobile');
  const { t: to } = useTranslation('otherAssets');
  const { t: td } = useTranslation('dashboard');
  const { t: tc } = useTranslation('common');
  const back = useBack(routes.wealth);
  const { assets, rows, totals, costReady, reportCurrency, isLoading, isError } = useOtherAssets();
  const [addOpen, setAddOpen] = useState(false);

  const later = () => toast(t('wealth.placeholder.later'));
  // The currencies left out of the values, and — once the costs are in — of the costs
  const missing = costReady
    ? [...new Set([...totals.missing, ...totals.missingCost])]
    : totals.missing;
  // Every asset without a rate is "—", not a total of 0
  const total = rows.every((row) => row.value === null) ? null : totals.value;
  // What was paid is said only when it is known for every asset
  const invested =
    costReady && totals.missingCost.length === 0 && totals.cost > 0 ? totals.cost : null;

  return (
    <Screen
      title={td('cards.otherAssets')}
      leading={<NavTextButton label={t('tabs.wealth')} onClick={back} chevron />}
      trailing={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <NavIconButton aria-label={tc('labels.moreActions')}>
                <Ellipsis strokeWidth={1.75} aria-hidden />
              </NavIconButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[240px]">
              <RowMenuItem onSelect={later}>
                <Download strokeWidth={1.75} aria-hidden />
                {tc('buttons.export')}
              </RowMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <NavIconButton aria-label={to('addAsset')} onClick={() => setAddOpen(true)}>
            <Plus strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </>
      }
    >
      {isLoading ? (
        <ClassSkeleton />
      ) : assets.length === 0 ? (
        // A failed load is not "no assets yet": said as such, with no empty state to add the first one
        isError ? (
          <LoadFailed />
        ) : (
          <ClassEmpty
            icon={<Gem />}
            title={to('empty.title')}
            description={to('empty.description')}
            addLabel={to('addAsset')}
            onAdd={() => setAddOpen(true)}
          />
        )
      ) : (
        <>
          {/* Amounts the rates could not convert into the report currency (left out) */}
          <ConversionQualityNote
            quality={{ missingCurrencies: missing }}
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <ClassHero
            trendClass="otherAssets"
            label={to('summary.value')}
            totalValue={total}
            invested={invested}
            currency={reportCurrency}
            rememberKey="otherAssets"
          />
          <EntityRows>
            {rows.map((row) => (
              <AssetRow key={row.asset.id} row={row} currency={reportCurrency} />
            ))}
          </EntityRows>
        </>
      )}
      <OtherAssetAddSheet open={addOpen} onOpenChange={setAddOpen} />
    </Screen>
  );
}

/**
 * An asset of the list (spec §8.8): the gem in the chip, the name over "12 × 1 850 Kč" — what is
 * held at its own price in its own currency, or the day it was last valued when nothing is held —
 * the value in the report currency over the gain against the purchase cost. A value that could not
 * be converted is "—" with no gain; an asset that cost nothing has no gain to measure either.
 */
function AssetRow({ row, currency }: { row: OtherAssetRow; currency: string }) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { asset, quantity, price, priceCurrency, value, cost, gain, gainPct } = row;

  const sub =
    quantity > 0
      ? `${fmt.number(quantity, { maximumFractionDigits: QUANTITY_DIGITS })} × ${moneyText(price, priceCurrency, { decimals: priceDecimals(price, priceCurrency) })}`
      : t('wealth.otherAssets.valuedOn', { date: fmt.day(asset.updatedAt) });

  return (
    <EntityRow
      chip={
        <RowChip>
          <Gem strokeWidth={1.75} />
        </RowChip>
      }
      title={asset.name}
      sub={sub}
      value={value === null ? '—' : moneyText(value, currency, { decimals: 0 })}
      valueSub={
        value !== null && gain !== null && cost !== null && cost > 0 ? (
          <ChangeLine change={{ delta: gain, ratio: gainPct }} currency={currency} />
        ) : undefined
      }
      href={otherAssetPath(asset.id)}
    />
  );
}

/** The assets could not be loaded: said as such, not shown as "no assets yet". */
function LoadFailed() {
  const { t } = useTranslation('mobile');
  return (
    <p role="alert" className="mx-0.5 py-6 text-center text-m-body text-ink-3">
      {t('wealth.otherAssets.loadFailed')}
    </p>
  );
}
