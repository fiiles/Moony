import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Bitcoin,
  Briefcase,
  FileText,
  Gem,
  House,
  Landmark,
  Plus,
  TrendingUp,
  type LucideIcon,
} from 'lucide-react';
import { recordCurrency } from '@shared/currencies';
import { useAuth } from '@/hooks/use-auth';
import { useFormat } from '@/lib/use-format';
import { amountOf } from '@/utils/positions';
import { Button } from '@/components/ui/button';
import { ScaledConversionNote, ScaledEmptyState } from '@/mobile/components/Scaled';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { Hero } from '@/mobile/components/Hero';
import { Money } from '@/mobile/components/Money';
import { SectionHead } from '@/mobile/components/SectionHead';
import { AddAssetMenu, type AddKind } from '@/mobile/components/wealth/AddAssetMenu';
import { AllocationBar } from '@/mobile/components/wealth/AllocationBar';
import { ChangeLine } from '@/mobile/components/wealth/ChangeLine';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useWealth, type WealthData } from '@/mobile/hooks/use-wealth';
import { changeSinceDayOptions, periodChange } from '@/mobile/lib/change';
import { shortDayOptions } from '@/mobile/lib/format-day';
import {
  ALLOCATION_ORDER,
  allocationInput,
  classRowState,
  type InvestmentClass,
  type WealthClass,
} from '@/mobile/lib/wealth-rows';
import { NavIconButton } from '@/mobile/shell/NavBar';
import { propertyPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** Whole amounts: Majetek shows no decimals. */
const WHOLE = { decimals: 0 } as const;

/** What differs between the four investment classes: the chip, where a row leads, what its empty row adds. */
const INVESTMENT_ROWS: Record<
  InvestmentClass,
  { icon: LucideIcon; href: string; add: AddKind | null }
> = {
  investments: { icon: TrendingUp, href: routes.stocks, add: null },
  crypto: { icon: Bitcoin, href: routes.cryptoList, add: 'crypto' },
  bonds: { icon: FileText, href: routes.bondsList, add: 'bond' },
  otherAssets: { icon: Gem, href: routes.otherAssetsList, add: 'asset' },
};

/**
 * Majetek tab root (spec §8.8, prototype frame 10): the Aktiva hero with the allocation bar, then
 * what is owned as rows — Investice (a row per class the menu preference leaves in), Nemovitosti
 * and a last section Hotovost whose one row opens Finance — so the rows add up to the hero. Assets
 * only: loans and policies live on Finance. The trailing `+` is the typed add menu.
 */
export default function Wealth() {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const fmt = useFormat();
  // Every amount goes through the eye of Přehled: text where a string is needed, <Money> elsewhere
  const moneyText = useMoneyText();
  const wealth = useWealth();
  const [addKind, setAddKind] = useState<AddKind | null>(null);
  const { overview, reportCurrency, classes, isLoading } = wealth;
  const { display, unknown, hasHistory, oldest, shortHistoryBase } = overview;

  const money = (value: number) => moneyText(value, reportCurrency, WHOLE);

  // The 30-day change of the hero is fixed (no period segment on this page): "za 30 dní", or
  // "od 3. 10." when the history is shorter than that — as Přehled says it
  const assetsChange =
    hasHistory && !unknown.assets ? periodChange(display.assets, oldest?.assets) : null;
  const changeLabel =
    shortHistoryBase === null
      ? t('overview.change.30D')
      : td('changeSince', {
          date: fmt.day(shortHistoryBase, changeSinceDayOptions(shortHistoryBase)),
        });

  // The bar shows the hero's total, so every class counts, also one hidden from the rows
  const byClass = <T,>(pick: (cls: WealthClass) => T) =>
    Object.fromEntries(ALLOCATION_ORDER.map((cls) => [cls, pick(cls)])) as Record<WealthClass, T>;
  const segments = allocationInput(
    byClass((cls) => classes[cls].value),
    byClass((cls) => classes[cls].unknown),
    byClass((cls) => td(`cards.${cls}`))
  );

  return (
    <Screen
      title={t('tabs.wealth')}
      large
      subtitle={t('wealth.subtitle')}
      trailing={
        <AddAssetMenu
          trigger={
            <NavIconButton aria-label={t('wealth.add.label')}>
              <Plus strokeWidth={1.75} aria-hidden />
            </NavIconButton>
          }
          open={addKind}
          onOpenChange={setAddKind}
        />
      }
    >
      {isLoading ? (
        <WealthSkeleton />
      ) : overview.isEmptyPortfolio ? (
        <EmptyWealth />
      ) : (
        <>
          {/* Amounts the backend could not convert into the report currency (left out) */}
          <ScaledConversionNote
            quality={overview.fxQuality}
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <Hero
            label={t('overview.stats.assets')}
            value={unknown.assets ? null : display.assets}
            formatValue={money}
            change={
              assetsChange
                ? {
                    ratio: assetsChange.ratio ?? 0,
                    text: (
                      <>
                        <Money
                          value={assetsChange.delta}
                          currency={reportCurrency}
                          options={{ signed: true, ...WHOLE }}
                        />{' '}
                        {changeLabel}
                      </>
                    ),
                  }
                : undefined
            }
            note={unknown.assets ? undefined : t('overview.stats.noHistory')}
          >
            <AllocationBar
              segments={segments}
              otherLabel={td('cards.other')}
              formatPercent={(ratio) => fmt.percent(ratio, 0)}
              formatValue={money}
            />
          </Hero>
          <InvestmentSection wealth={wealth} onAdd={setAddKind} />
          <PropertySection wealth={wealth} />
          <CashSection wealth={wealth} />
        </>
      )}
    </Screen>
  );
}

/** Investice (spec §8.8): a row per visible class; the section's number is their sum. */
function InvestmentSection({
  wealth,
  onAdd,
}: {
  wealth: WealthData;
  onAdd: (kind: AddKind) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation('dashboard');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { reportCurrency, classes, visible } = wealth;
  if (visible.length === 0) return null;

  const money = (value: number) => moneyText(value, reportCurrency, WHOLE);
  const known = visible.filter((cls) => !classes[cls].unknown);
  const total = known.reduce((sum, cls) => sum + classes[cls].value, 0);

  const title = (cls: InvestmentClass) =>
    cls === 'investments' ? t('stocks:title') : td(`cards.${cls}`);
  const positions = (count: number, tickers: string) =>
    [t('stocks:summary.positions', { count }), tickers].filter(Boolean).join(' · ');

  const rowFor = (cls: InvestmentClass) => {
    const { icon: Icon, href, add } = INVESTMENT_ROWS[cls];
    const { value, unknown, change } = classes[cls];
    const { records } = wealth[cls === 'investments' ? 'stocks' : cls];
    const state = classRowState({ records, value, unknown });
    const common = {
      chip: (
        <RowChip>
          <Icon strokeWidth={1.75} />
        </RowChip>
      ),
      title: title(cls),
      chevron: true,
    };

    // No record of the class at all: a row that leads to adding the first one (stocks keep their
    // page, which has the empty state). A class with only closed records keeps its list.
    if (state === 'empty') {
      return (
        <EntityRow
          key={cls}
          {...common}
          sub={t('wealth.empty.class')}
          href={add === null ? href : undefined}
          onClick={add === null ? undefined : () => onAdd(add)}
        />
      );
    }

    // Held, but nothing converted: "—", and no change that would be measured against nothing
    const shown = state === 'unknown' ? '—' : money(value);
    const changeLine = change ? (
      <ChangeLine change={change} currency={reportCurrency} />
    ) : undefined;
    let sub: string;
    let valueSub: ReactNode;
    switch (cls) {
      case 'investments':
        sub = positions(wealth.stocks.count, wealth.stocks.tickers);
        valueSub = changeLine;
        break;
      case 'crypto':
        sub = positions(wealth.crypto.count, wealth.crypto.tickers);
        valueSub = changeLine;
        break;
      case 'bonds': {
        const { count, nearestMaturity, yearlyCoupon } = wealth.bonds;
        sub = [
          t('wealth.bonds.count', { count }),
          nearestMaturity !== null &&
            t('wealth.bonds.nearest', {
              month: fmt.monthInSentence(new Date(nearestMaturity * 1000)),
            }),
        ]
          .filter(Boolean)
          .join(' · ');
        valueSub =
          yearlyCoupon > 0 ? t('wealth.bonds.coupon', { amount: money(yearlyCoupon) }) : undefined;
        break;
      }
      case 'otherAssets': {
        const { count, names, valuedAt } = wealth.otherAssets;
        sub = [t('wealth.otherAssets.count', { count }), names].filter(Boolean).join(' · ');
        valueSub =
          valuedAt !== null
            ? t('wealth.otherAssets.valuedOn', {
                date: fmt.day(valuedAt, shortDayOptions(valuedAt)),
              })
            : undefined;
        break;
      }
    }
    return (
      <EntityRow
        key={cls}
        {...common}
        sub={sub}
        value={shown}
        valueSub={state === 'unknown' ? undefined : valueSub}
        href={href}
      />
    );
  };

  return (
    <section>
      <SectionHead
        title={t('wealth.sections.investments')}
        note={known.length > 0 ? money(total) : '—'}
      />
      <EntityRows>{visible.map(rowFor)}</EntityRows>
    </section>
  );
}

/** Nemovitosti: a row per property at its own market price; the number is the converted total. */
function PropertySection({ wealth }: { wealth: WealthData }) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { user } = useAuth();
  const { reportCurrency, classes, properties } = wealth;
  if (!(user?.menuPreferences?.realEstate ?? true) || properties.length === 0) return null;
  const excludePersonal = user?.excludePersonalRealEstate ?? false;

  return (
    <section>
      <SectionHead
        title={t('wealth.sections.realEstate')}
        note={
          classes.realEstate.unknown
            ? '—'
            : moneyText(classes.realEstate.value, reportCurrency, WHOLE)
        }
      />
      <EntityRows>
        {properties.map(({ property, change }) => {
          // A personal residence the profile leaves out of net worth is not in the hero nor in the
          // section's number: the row stays, and says so (the desktop's badge and wording)
          const excluded = excludePersonal && property.type === 'personal';
          const price = amountOf(property.marketPrice);
          const rent = amountOf(property.monthlyRent) ?? 0;
          const sub = [
            t(`wealth.realEstate.${property.type === 'personal' ? 'personal' : 'investment'}`),
            rent > 0 &&
              t('wealth.realEstate.rent', {
                amount: moneyText(rent, recordCurrency(property.monthlyRentCurrency), WHOLE),
              }),
          ]
            .filter(Boolean)
            .join(' · ');
          return (
            <EntityRow
              key={property.id}
              chip={
                <RowChip>
                  <House strokeWidth={1.75} />
                </RowChip>
              }
              title={
                excluded ? (
                  <span className="flex min-w-0 items-center">
                    <span className="min-w-0 truncate">{property.name}</span>
                    <span className="ml-1.5 inline-flex min-h-[18px] flex-none items-center rounded-r1 bg-well-2 px-1.5 py-px text-m-caption font-semibold leading-[1.3] text-ink-3">
                      {t('realEstate:table.excluded')}
                    </span>
                  </span>
                ) : (
                  property.name
                )
              }
              sub={sub}
              // The property's own price in its own currency; the section's number is the converted total
              value={
                price === null
                  ? '—'
                  : moneyText(price, recordCurrency(property.marketPriceCurrency), WHOLE)
              }
              valueSub={
                change ? (
                  <ChangeLine
                    change={{ delta: 0, ratio: change.ratio }}
                    currency={reportCurrency}
                    trail={fmt.day(change.day, shortDayOptions(change.day))}
                  />
                ) : undefined
              }
              href={propertyPath(property.id)}
            />
          );
        })}
      </EntityRows>
    </section>
  );
}

/** Hotovost: the one row of the cash on accounts, which opens the Finance tab. */
function CashSection({ wealth }: { wealth: WealthData }) {
  const { t } = useTranslation('mobile');
  const moneyText = useMoneyText();
  const { reportCurrency, classes, cash } = wealth;
  const { value, unknown, change } = classes.savings;
  const shown = unknown ? '—' : moneyText(value, reportCurrency, WHOLE);

  return (
    <section>
      <SectionHead title={t('wealth.sections.cash')} note={shown} />
      <EntityRows>
        <EntityRow
          chip={
            <RowChip>
              <Landmark strokeWidth={1.75} />
            </RowChip>
          }
          title={t('wealth.cash.title')}
          sub={t('upkeep.accounts', { count: cash.accounts })}
          value={shown}
          // Badge and amount like the other rows: "za 30 dní" is said once, in the hero
          valueSub={change ? <ChangeLine change={change} currency={reportCurrency} /> : undefined}
          href={routes.finance}
        />
      </EntityRows>
    </section>
  );
}

/** A brand-new profile: the empty state with the typed add menu as its one action. */
function EmptyWealth() {
  const { t } = useTranslation('mobile');
  return (
    <ScaledEmptyState
      icon={<Briefcase />}
      title={t('wealth.empty.title')}
      description={t('wealth.empty.description')}
      className="px-5 py-10"
      action={
        <AddAssetMenu
          trigger={
            <Button className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-3 text-m-row">
              <Plus aria-hidden />
              {t('wealth.add.label')}
            </Button>
          }
        />
      }
    />
  );
}

/** While the metrics settle: the hero and three rows in outline. */
function WealthSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3.5 pt-4 shadow-e1">
        <div className="skeleton h-3 w-24" />
        <div className="skeleton mt-3 h-9 w-56" />
        <div className="skeleton mt-3 h-3 w-40" />
        <div className="skeleton mt-4 h-2 w-full" />
      </div>
      <div className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1">
        {[0, 1, 2].map((i) => (
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
