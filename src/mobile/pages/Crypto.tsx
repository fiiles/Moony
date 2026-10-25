import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Archive, Bitcoin, Download, Ellipsis, Plus, RefreshCw } from 'lucide-react';
import type { CryptoPositions } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { amountOf, totalOf, type CryptoRow } from '@/utils/positions';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ScaledConversionNote } from '@/mobile/components/Scaled';
import { AddCryptoModal } from '@/components/crypto/AddCryptoModal';
import { RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { ClassEmpty } from '@/mobile/components/wealth/ClassEmpty';
import { ClassHero } from '@/mobile/components/wealth/ClassHero';
import { ClassSkeleton } from '@/mobile/components/wealth/ClassSkeleton';
import { PositionList } from '@/mobile/components/wealth/PositionList';
import { PricesLine } from '@/mobile/components/wealth/PricesLine';
import { useBack } from '@/mobile/hooks/use-back';
import { useClassExport } from '@/mobile/hooks/use-class-export';
import {
  useCryptoPositions,
  useCryptoTrades,
  useRefreshCryptoPrices,
} from '@/mobile/hooks/use-crypto';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { statMoneyOptions, statTone } from '@/mobile/lib/stat-money';
import { ClosedPositionsSheet } from '@/mobile/sheets/ClosedPositionsSheet';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { cryptoPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/**
 * Krypto, the class page of Majetek (spec §8.8: "Krypto is the same page"): the value of the coins
 * with its line and the buy and sell days, three stats (the unrealized result, the realized one,
 * the largest position), then the coins as rows with sort, search and the first eight until
 * "Zobrazit všech N". Coins pay no dividends and have no performance report, so the stats are the
 * desktop's own (`CryptoSummary`) and there is no Pozice · Sledované segment (the watchlist is
 * Yahoo's stocks). The numbers are the desktop's (the same queries), in the report currency; every
 * amount goes through the eye of Přehled. The list holds the open coins; the closed ones are the
 * sheet Uzavřené pozice in the `···` menu, and Export there hands the transactions' CSV to the
 * Files picker. The sort and the period are remembered for the session, so a visit to a coin does
 * not reset them.
 */
export default function Crypto() {
  const { t } = useTranslation('mobile');
  const { t: tcr } = useTranslation('crypto');
  const { t: td } = useTranslation('dashboard');
  const { t: tc } = useTranslation('common');
  const back = useBack(routes.wealth);
  const { positions, rows, reportCurrency, isLoading, isError } = useCryptoPositions();
  const { transactions: trades, convertAt } = useCryptoTrades();
  const refresh = useRefreshCryptoPrices();
  const { exportCsv, isPending: exporting } = useClassExport('crypto');
  // The add dialog and the closed sheet are not kept for the session (a coin remounts the page), so
  // they do not reopen on return.
  const [addOpen, setAddOpen] = useState(false);
  const [closedOpen, setClosedOpen] = useState(false);
  const hasPositions = rows.length > 0;

  const open = rows.filter((row) => row.quantity > 0);

  return (
    <Screen
      title={td('cards.crypto')}
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
              <RowMenuItem disabled={refresh.isPending} onSelect={() => refresh.mutate()}>
                <RefreshCw strokeWidth={1.75} aria-hidden />
                {tcr('refreshPrices')}
              </RowMenuItem>
              <RowMenuItem onSelect={() => setClosedOpen(true)}>
                <Archive strokeWidth={1.75} aria-hidden />
                {t('wealth.stocks.menu.closed')}
              </RowMenuItem>
              <DropdownMenuSeparator />
              <RowMenuItem disabled={exporting} onSelect={() => void exportCsv()}>
                <Download strokeWidth={1.75} aria-hidden />
                {tc('buttons.export')}
              </RowMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <NavIconButton aria-label={tcr('addCrypto')} onClick={() => setAddOpen(true)}>
            <Plus strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </>
      }
    >
      {isLoading ? (
        <ClassSkeleton />
      ) : !hasPositions ? (
        isError ? (
          <LoadFailed />
        ) : (
          <NoCoins onAdd={() => setAddOpen(true)} />
        )
      ) : (
        <>
          {/* The nav bar keeps its one title, so the count and the age of the prices are the first line */}
          <PricesLine count={tcr('summary.positions', { count: open.length })} rows={rows} />
          {/* Amounts the backend could not convert into the report currency (left out) */}
          <ScaledConversionNote
            quality={positions?.fxQuality}
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <ClassHero
            trendClass="crypto"
            label={tcr('summary.value')}
            totalValue={totalOf(positions?.totals.value)}
            invested={totalOf(positions?.totals.costBasis)}
            currency={reportCurrency}
            trades={trades}
            convertAt={convertAt}
            rememberKey="crypto"
          />
          <CryptoStats
            positions={positions}
            rows={open}
            currency={reportCurrency}
            sells={trades.filter((tx) => tx.type === 'sell').length}
          />
          <PositionList
            rows={rows}
            hrefOf={(row) => cryptoPath(row.id)}
            currency={reportCurrency}
            dimmed={refresh.isPending}
            rememberKey="crypto"
          />
        </>
      )}
      <AddCryptoModal open={addOpen} onOpenChange={setAddOpen} />
      <ClosedPositionsSheet
        open={closedOpen}
        onOpenChange={setClosedOpen}
        rows={rows}
        trades={trades}
        currency={reportCurrency}
        hrefOf={(row) => cryptoPath(row.id)}
      />
    </Screen>
  );
}

/**
 * Three stats (spec §8.8, the desktop's `CryptoSummary`): the unrealized result against what is
 * invested, the realized result with the number of sells, and the largest open position with its
 * share of the class. A coin without a rate has no value to be the largest and no share.
 */
function CryptoStats({
  positions,
  rows,
  currency,
  sells,
}: {
  positions: CryptoPositions | undefined;
  /** The open coins. */
  rows: readonly CryptoRow[];
  currency: string;
  sells: number;
}) {
  const { t } = useTranslation('mobile');
  const { t: tcr } = useTranslation('crypto');
  const fmt = useFormat();
  const moneyText = useMoneyText();

  const totals = positions?.totals;
  const unrealized = totalOf(totals?.unrealizedGain);
  const realized = amountOf(totals?.realizedGain);
  const classValue = amountOf(totals?.value) ?? 0;
  const largest = rows.reduce<CryptoRow | null>(
    (max, row) =>
      row.marketValue !== null && (max === null || row.marketValue > (max.marketValue ?? 0))
        ? row
        : max,
    null
  );
  const share =
    largest !== null && largest.marketValue !== null && classValue > 0
      ? largest.marketValue / classValue
      : 0;

  const statMoney = (amount: number) =>
    moneyText(amount, currency, { ...statMoneyOptions(amount), signed: true });

  const stats: StatItem[] = [
    {
      label: (unrealized ?? 0) >= 0 ? tcr('summary.unrealizedGain') : tcr('summary.unrealizedLoss'),
      value: unrealized === null ? '—' : statMoney(unrealized),
      tone: statTone(unrealized),
      note:
        unrealized !== null && totals !== undefined
          ? fmt.percent(totals.unrealizedGainPercent / 100, 1, { signed: true })
          : undefined,
    },
    {
      label: tcr('summary.realized'),
      value: realized === null ? '—' : statMoney(realized),
      tone: statTone(realized),
      note: sells > 0 ? tcr('detail.sells', { count: sells }) : tcr('detail.noSells'),
    },
    {
      label: tcr('summary.largest'),
      value: largest?.ticker ?? '—',
      note: largest
        ? t('wealth.crypto.largestNote', { share: fmt.percent(share, 0) })
        : tcr('summary.noPositions'),
    },
  ];
  return <StatRow stats={stats} />;
}

/** Nothing owned: the empty state of the class, with the button that adds the first coin. */
function NoCoins({ onAdd }: { onAdd: () => void }) {
  const { t: tcr } = useTranslation('crypto');
  return (
    <ClassEmpty
      icon={<Bitcoin />}
      title={tcr('empty.title')}
      description={tcr('empty.description')}
      addLabel={tcr('addCrypto')}
      onAdd={onAdd}
    />
  );
}

/** The coins could not be loaded: said as such, not shown as "no coins yet". */
function LoadFailed() {
  const { t } = useTranslation('mobile');
  return (
    <p role="alert" className="mx-0.5 py-6 text-center text-m-body text-ink-3">
      {t('wealth.stocks.loadFailed')}
    </p>
  );
}
