import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { Minus, Plus } from 'lucide-react';
import type { CryptoPositions, CryptoTransaction, TickerValueHistoryReport } from '@shared/schema';
import { useCryptoTransactionMutations } from '@/hooks/use-crypto-mutations';
import { useFormat } from '@/lib/use-format';
import { monthsBetween } from '@/utils/duration';
import type { ChartPeriod } from '@/utils/period';
import { amountOf, type CryptoRow } from '@/utils/positions';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { BuyCryptoModal } from '@/components/crypto/BuyCryptoModal';
import { SellCryptoModal } from '@/components/crypto/SellCryptoModal';
import { ButtonPair } from '@/mobile/components/ButtonPair';
import type { StatItem } from '@/mobile/components/StatRow';
import { ClassSkeleton } from '@/mobile/components/wealth/ClassSkeleton';
import { CryptoMenu } from '@/mobile/components/wealth/CryptoMenu';
import { PositionHeroCard } from '@/mobile/components/wealth/PositionHeroCard';
import { PositionLedger } from '@/mobile/components/wealth/PositionLedger';
import { PositionStats } from '@/mobile/components/wealth/PositionStats';
import { useBack } from '@/mobile/hooks/use-back';
import { useCryptoPosition, useDeleteCrypto } from '@/mobile/hooks/use-crypto';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { usePositionLedger } from '@/mobile/hooks/use-position-ledger';
import { useRetained } from '@/mobile/hooks/use-retained';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { cryptoPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';

/** What the screen shows of a loaded coin: the record and what is read from it. */
interface CoinView {
  holding: CryptoRow;
  positions: CryptoPositions | undefined;
  history: TickerValueHistoryReport | undefined;
}

/** A coin is held to eight decimals (a satoshi), where a share shows four. */
const COIN_DIGITS = 8;

/**
 * A coin of the Krypto list (spec §8.8, the position detail of the phone with the desktop's crypto
 * detail for its numbers): the ticker in the nav bar with the name and the currency under it, the
 * value with its line, the dashed "Vloženo" and a mark for every trade, Koupit · Prodat, six stats
 * (the sixth is how long the coin has been held), the transactions with their segment. There is
 * no company card (a coin has no company), and a trade has no Upravit (there is no update
 * command), only Smazat. The numbers are the desktop detail's (the same queries), in the report
 * currency; the buy, sell and rate dialogs are the desktop's.
 *
 * The page owns the delete (its confirm sheet and the mutation, `useDeleteAndLeave`): the coin
 * drops out of the list the page reads while the delete is still finishing, which would take the
 * `···` menu away, and the page keeps showing the coin until it has left.
 */
export default function CryptoDetail() {
  const { t: tcr } = useTranslation('crypto');
  const { t: td } = useTranslation('dashboard');
  const [, params] = useRoute(routes.cryptoDetail);
  const back = useBack(routes.cryptoList);
  const [period, setPeriod] = useState<ChartPeriod>('30D');
  const [trade, setTrade] = useState<'buy' | 'sell' | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const data = useCryptoPosition(params?.id, period);
  const ledger = usePositionLedger(data.transactions);
  const { deleteTransaction } = useCryptoTransactionMutations();
  const remove = useDeleteCrypto(params?.id);
  const { holding, positions, history, reportCurrency } = data;

  const current = useMemo<CoinView | null>(
    () => (holding ? { holding, positions, history } : null),
    [holding, positions, history]
  );
  const kept = useRetained(current);
  const exit = useDeleteAndLeave({
    home: routes.cryptoList,
    pathOf: cryptoPath,
    keysOf: (coinId) => [['crypto-transactions', coinId]],
    remove: () => remove.mutateAsync(),
    closeSheet: () => setDeleteOpen(false),
  });
  // A coin being deleted stays on screen (as it was) until the screen has left
  const view = exit.deleting ? kept : current;

  return (
    <Screen
      title={view?.holding.ticker ?? ''}
      leading={<NavTextButton label={td('cards.crypto')} onClick={back} chevron />}
      trailing={view && <CryptoMenu holding={view.holding} onDelete={() => setDeleteOpen(true)} />}
    >
      {view ? (
        <>
          {/* The nav bar keeps its one title, so the name and the currency are the first line */}
          <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
            {[view.holding.name, view.holding.originalCurrency].join(' · ')}
          </p>
          {/* The list's quality, shown when this coin is one of those not converted */}
          <ConversionQualityNote
            quality={
              view.holding.marketValue === null || view.holding.totalCost === null
                ? view.positions?.fxQuality
                : undefined
            }
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <PositionHeroCard
            holding={view.holding}
            history={view.history}
            transactions={data.transactions}
            unit={view.holding.ticker}
            quantityDigits={COIN_DIGITS}
            reportCurrency={reportCurrency}
            period={period}
            onPeriodChange={setPeriod}
            onMarkTap={ledger.flash}
          />
          <ButtonPair
            primary={{ label: tcr('detail.buy'), icon: Plus, onClick: () => setTrade('buy') }}
            secondary={{
              label: tcr('detail.sell'),
              icon: Minus,
              onClick: () => setTrade('sell'),
              disabled: view.holding.quantity <= 0,
            }}
          />
          <CoinStats
            holding={view.holding}
            reportCurrency={reportCurrency}
            transactions={data.transactions}
            classValue={amountOf(view.positions?.totals.value) ?? 0}
          />
          <PositionLedger
            ledger={ledger}
            count={data.transactions.length}
            currency={reportCurrency}
            convertAt={data.convertAt}
            ratesReady={data.ratesReady}
            unit={view.holding.ticker}
            quantityDigits={COIN_DIGITS}
            deleteTransaction={deleteTransaction}
          />
          <BuyCryptoModal
            open={trade === 'buy'}
            onOpenChange={(open) => !open && setTrade(null)}
            investment={view.holding}
          />
          <SellCryptoModal
            open={trade === 'sell'}
            onOpenChange={(open) => !open && setTrade(null)}
            investment={view.holding}
          />
          <ConfirmSheet
            open={deleteOpen}
            onOpenChange={setDeleteOpen}
            title={tcr('confirmDelete.title')}
            description={tcr('confirmDelete.description', {
              name: view.holding.name,
              ticker: view.holding.ticker,
            })}
            confirmLabel={tcr('actions.delete')}
            isPending={remove.isPending}
            onConfirm={() => exit.run(view.holding.id)}
          />
        </>
      ) : data.isLoading ? (
        // The outline of a class page is the detail's too: the hero, the stats and a few rows
        <ClassSkeleton />
      ) : (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{tcr('detail.notFound')}</p>
      )}
    </Screen>
  );
}

/** The six stats of a coin: how long it has been held is the sixth. */
function CoinStats({
  holding,
  reportCurrency,
  transactions,
  classValue,
}: {
  holding: CryptoRow;
  reportCurrency: string;
  transactions: readonly CryptoTransaction[];
  /** What all the coins are worth in the report currency: the base of the coin's share. */
  classValue: number;
}) {
  const { t } = useTranslation('mobile');
  const { t: tcr } = useTranslation('crypto');
  const fmt = useFormat();

  const buys = transactions.filter((tx) => tx.type === 'buy');
  const firstBuy = buys.reduce<CryptoTransaction | null>(
    (min, tx) => (min === null || tx.transactionDate < min.transactionDate ? tx : min),
    null
  );
  const sells = transactions.filter((tx) => tx.type === 'sell').length;

  // Whole months as the desktop counts them, but only the larger unit: "2 years 7 months" would
  // not fit a third of the row at 17 pt and would shrink the figures beside it. The day it began
  // is in the note, so nothing is lost. A coin bought this month is "less than a month", as there.
  const held = firstBuy
    ? monthsBetween(firstBuy.transactionDate, Math.floor(Date.now() / 1000))
    : null;
  const durationText =
    held === null
      ? null
      : held.totalMonths === 0
        ? tcr('detail.duration.lessThanMonth')
        : held.years > 0
          ? tcr('detail.duration.years', { count: held.years })
          : tcr('detail.duration.months', { count: held.months });

  // The share of the class is the open coin's value over all the coins'; a coin that is closed or
  // has no rate has none to say
  const share =
    holding.quantity > 0 && holding.marketValue !== null && classValue > 0
      ? holding.marketValue / classValue
      : null;
  const since = firstBuy
    ? fmt.day(firstBuy.transactionDate, shortDayOptions(firstBuy.transactionDate))
    : null;
  const sixth: StatItem = {
    label: tcr('detail.held'),
    value: durationText ?? '—',
    note:
      since === null
        ? tcr('detail.sinceFirst.none')
        : share === null
          ? tcr('detail.heldSince', { date: since })
          : t('wealth.crypto.heldNote', { date: since, share: fmt.percent(share, 0) }),
  };

  return (
    <PositionStats holding={holding} reportCurrency={reportCurrency} sells={sells} sixth={sixth} />
  );
}
