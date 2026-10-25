import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { Minus, Plus } from 'lucide-react';
import type {
  InvestmentTransaction,
  StockCompanyInfo,
  StockPosition,
  TickerValueHistoryReport,
} from '@shared/schema';
import { useInvestmentTransactionMutations } from '@/hooks/use-investment-mutations';
import type { ChartPeriod } from '@/utils/period';
import type { StockRow } from '@/utils/positions';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { BuyInvestmentModal } from '@/components/stocks/BuyInvestmentModal';
import { EditTransactionModal } from '@/components/stocks/EditTransactionModal';
import { SellInvestmentModal } from '@/components/stocks/SellInvestmentModal';
import { ButtonPair } from '@/mobile/components/ButtonPair';
import { SectionHead } from '@/mobile/components/SectionHead';
import { CompanyCard } from '@/mobile/components/wealth/CompanyCard';
import { PositionHeroCard } from '@/mobile/components/wealth/PositionHeroCard';
import { PositionLedger } from '@/mobile/components/wealth/PositionLedger';
import { PositionMenu } from '@/mobile/components/wealth/PositionMenu';
import { PositionStats, useDividendStat } from '@/mobile/components/wealth/PositionStats';
import { useBack } from '@/mobile/hooks/use-back';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { usePosition, usePositionMutations } from '@/mobile/hooks/use-position';
import { usePositionLedger } from '@/mobile/hooks/use-position-ledger';
import { useRetained } from '@/mobile/hooks/use-retained';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { positionPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';

/** What the screen shows of a loaded position: the record and what is read from it. */
interface PositionView {
  holding: StockRow;
  position: StockPosition | undefined;
  history: TickerValueHistoryReport | undefined;
  companyInfo: StockCompanyInfo | undefined;
  companyPending: boolean;
}

/**
 * A position of the Akcie list (spec §8.8, prototype frame 12): the ticker in the nav bar with the
 * name and the currency under it, the value with its line, the dashed "Vloženo" and a mark for
 * every trade, Koupit · Prodat, six stats, the transactions with their segment and "O společnosti"
 * as a key-value card. The numbers are the desktop detail's (the same queries), in the report
 * currency; the buy and sell dialogs are the desktop's.
 *
 * The page owns the delete (its confirm sheet and the mutation, `useDeleteAndLeave`): the position
 * can drop out of what the page reads while the delete is still finishing, which would take the
 * `···` menu away, and the page keeps showing the position until it has left.
 */
export default function PositionDetail() {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const [, params] = useRoute(routes.positionDetail);
  const back = useBack(routes.stocks);
  const [period, setPeriod] = useState<ChartPeriod>('30D');
  const [trade, setTrade] = useState<'buy' | 'sell' | null>(null);
  const [editing, setEditing] = useState<InvestmentTransaction | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const data = usePosition(params?.id, period);
  const ledger = usePositionLedger(data.transactions);
  const { deleteTransaction } = useInvestmentTransactionMutations();
  const { holding, position, history, companyInfo, companyPending, reportCurrency } = data;
  const { remove } = usePositionMutations(params?.id, holding?.ticker);

  const current = useMemo<PositionView | null>(
    () => (holding ? { holding, position, history, companyInfo, companyPending } : null),
    [holding, position, history, companyInfo, companyPending]
  );
  const kept = useRetained(current);
  const exit = useDeleteAndLeave({
    home: routes.stocks,
    pathOf: positionPath,
    keysOf: (positionId) => [
      ['investment', positionId],
      ['investment-transactions', positionId],
    ],
    remove: () => remove.mutateAsync(),
    closeSheet: () => setDeleteOpen(false),
  });
  // A position being deleted stays on screen (as it was) until the screen has left
  const view = exit.deleting ? kept : current;

  return (
    <Screen
      title={view?.holding.ticker ?? ''}
      leading={<NavTextButton label={t('wealth.stocks.back')} onClick={back} chevron />}
      trailing={
        view && <PositionMenu holding={view.holding} onDelete={() => setDeleteOpen(true)} />
      }
    >
      {view ? (
        <>
          {/* The nav bar keeps its one title, so the name and the currency are the first line */}
          <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
            {[view.holding.companyName, view.holding.originalCurrency].join(' · ')}
          </p>
          {/* Amounts the backend could not convert into the report currency (left out) */}
          <ConversionQualityNote
            quality={view.position?.fxQuality}
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <PositionHeroCard
            holding={view.holding}
            history={view.history}
            transactions={data.transactions}
            unit={ts('table.unit')}
            reportCurrency={reportCurrency}
            period={period}
            onPeriodChange={setPeriod}
            onMarkTap={ledger.flash}
          />
          <ButtonPair
            primary={{ label: ts('detail.buy'), icon: Plus, onClick: () => setTrade('buy') }}
            secondary={{
              label: ts('detail.sell'),
              icon: Minus,
              onClick: () => setTrade('sell'),
              disabled: view.holding.quantity <= 0,
            }}
          />
          <StockStats
            holding={view.holding}
            reportCurrency={reportCurrency}
            sells={data.transactions.filter((tx) => tx.type === 'sell').length}
          />
          <PositionLedger
            ledger={ledger}
            count={data.transactions.length}
            currency={reportCurrency}
            convertAt={data.convertAt}
            ratesReady={data.ratesReady}
            unit={ts('table.unit')}
            deleteTransaction={deleteTransaction}
            onEdit={setEditing}
          />
          <EditTransactionModal
            transaction={editing}
            open={editing !== null}
            onOpenChange={(open) => {
              if (!open) setEditing(null);
            }}
          />
          <SectionHead title={ts('detail.about')} note={ts('detail.source')} />
          <CompanyCard
            name={view.holding.companyName}
            info={view.companyInfo}
            stockCurrency={view.holding.originalCurrency}
            pending={view.companyPending}
            onRetry={data.refetchCompany}
          />
          <BuyInvestmentModal
            open={trade === 'buy'}
            onOpenChange={(open) => !open && setTrade(null)}
            investment={view.holding}
          />
          <SellInvestmentModal
            open={trade === 'sell'}
            onOpenChange={(open) => !open && setTrade(null)}
            investment={view.holding}
          />
          <ConfirmSheet
            open={deleteOpen}
            onOpenChange={setDeleteOpen}
            title={ts('detail.confirmDelete.title')}
            description={ts('detail.confirmDelete.description', {
              name: view.holding.companyName,
              ticker: view.holding.ticker,
            })}
            confirmLabel={ts('actions.deletePosition')}
            isPending={remove.isPending}
            onConfirm={() => exit.run(view.holding.id)}
          />
        </>
      ) : data.isLoading ? (
        <PositionSkeleton />
      ) : (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{ts('detail.notFound')}</p>
      )}
    </Screen>
  );
}

/** The six stats of a stock: the dividends of a year are the sixth. */
function StockStats({
  holding,
  reportCurrency,
  sells,
}: {
  holding: StockRow;
  reportCurrency: string;
  sells: number;
}) {
  const sixth = useDividendStat(holding, reportCurrency);
  return (
    <PositionStats holding={holding} reportCurrency={reportCurrency} sells={sells} sixth={sixth} />
  );
}

/** While the position loads: the hero, the stats and a few rows in outline. */
function PositionSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-32" />
        <div className="skeleton mt-3 h-9 w-56" />
        <div className="skeleton mt-3 h-3 w-48" />
        <div className="skeleton mt-4 h-[160px] w-full" />
      </div>
      <div className="mb-3.5 grid grid-cols-3 gap-2.5">
        {[0, 1, 2].map((i) => (
          <div key={i} className="rounded-r3 border border-line bg-paper px-3 py-2.5 shadow-e1">
            <div className="skeleton h-2.5 w-3/5" />
            <div className="skeleton mt-2.5 h-4 w-4/5" />
          </div>
        ))}
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
