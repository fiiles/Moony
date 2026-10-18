import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { useLocation, useRoute } from 'wouter';
import { Minus, Plus } from 'lucide-react';
import type { ChartPeriod } from '@/utils/period';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { BuyInvestmentModal } from '@/components/stocks/BuyInvestmentModal';
import { SellInvestmentModal } from '@/components/stocks/SellInvestmentModal';
import { ButtonPair } from '@/mobile/components/ButtonPair';
import { SectionHead } from '@/mobile/components/SectionHead';
import { CompanyCard } from '@/mobile/components/wealth/CompanyCard';
import { PositionHeroCard } from '@/mobile/components/wealth/PositionHeroCard';
import { PositionLedger } from '@/mobile/components/wealth/PositionLedger';
import { PositionMenu } from '@/mobile/components/wealth/PositionMenu';
import { PositionStats } from '@/mobile/components/wealth/PositionStats';
import { useBack } from '@/mobile/hooks/use-back';
import { usePosition } from '@/mobile/hooks/use-position';
import { usePositionLedger } from '@/mobile/hooks/use-position-ledger';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { positionPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** How long after leaving a deleted position the screen may still be it (a restored deep link). */
const DELETED_GUARD_MS = 250;

/**
 * A position of the Akcie list (spec §8.8, prototype frame 12): the ticker in the nav bar with the
 * name and the currency under it, the value with its line, the dashed "Vloženo" and a mark for
 * every trade, Koupit · Prodat, six stats, the transactions with their segment and "O společnosti"
 * as a key-value card. The numbers are the desktop detail's (the same queries), in the report
 * currency; the buy and sell dialogs are the desktop's.
 */
export default function PositionDetail() {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const [, params] = useRoute(routes.positionDetail);
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const back = useBack(routes.stocks);
  const [period, setPeriod] = useState<ChartPeriod>('30D');
  const [trade, setTrade] = useState<'buy' | 'sell' | null>(null);
  const data = usePosition(params?.id, period);
  const ledger = usePositionLedger(data.transactions);
  const { holding, reportCurrency } = data;

  // The position is gone: back to Akcie as Back does, so the history keeps no ghost of it. Its own
  // queries are dropped only now that the screen is leaving: removed while it is still up, a
  // re-render would rebuild them empty, unmount the menu and lose this very callback. On a
  // restored deep link the detail is the first entry and back has nowhere to go; the screen is
  // then replaced.
  const leaveDeleted = (id: string) => {
    back();
    queryClient.removeQueries({ queryKey: ['investment', id] });
    queryClient.removeQueries({ queryKey: ['investment-transactions', id] });
    window.setTimeout(() => {
      if (window.location.pathname === positionPath(id)) {
        setLocation(routes.stocks, { replace: true });
      }
    }, DELETED_GUARD_MS);
  };

  return (
    <Screen
      title={holding?.ticker ?? ''}
      leading={<NavTextButton label={t('wealth.stocks.back')} onClick={back} chevron />}
      trailing={
        holding && <PositionMenu holding={holding} onDeleted={() => leaveDeleted(holding.id)} />
      }
    >
      {data.isLoading ? (
        <PositionSkeleton />
      ) : data.notFound || !holding ? (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{ts('detail.notFound')}</p>
      ) : (
        <>
          {/* The nav bar keeps its one title, so the name and the currency are the first line */}
          <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
            {[holding.companyName, holding.originalCurrency].join(' · ')}
          </p>
          {/* Amounts the backend could not convert into the report currency (left out) */}
          <ConversionQualityNote
            quality={data.position?.fxQuality}
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <PositionHeroCard
            holding={holding}
            history={data.history}
            transactions={data.transactions}
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
              disabled: holding.quantity <= 0,
            }}
          />
          <PositionStats
            holding={holding}
            reportCurrency={reportCurrency}
            sells={data.transactions.filter((tx) => tx.type === 'sell').length}
          />
          <PositionLedger
            ledger={ledger}
            count={data.transactions.length}
            currency={reportCurrency}
            convertAt={data.convertAt}
            ratesReady={data.ratesReady}
          />
          <SectionHead title={ts('detail.about')} note={ts('detail.source')} />
          <CompanyCard
            name={holding.companyName}
            info={data.companyInfo}
            stockCurrency={holding.originalCurrency}
            pending={data.companyPending}
            onRetry={data.refetchCompany}
          />
          <BuyInvestmentModal
            open={trade === 'buy'}
            onOpenChange={(open) => !open && setTrade(null)}
            investment={holding}
          />
          <SellInvestmentModal
            open={trade === 'sell'}
            onOpenChange={(open) => !open && setTrade(null)}
            investment={holding}
          />
        </>
      )}
    </Screen>
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
