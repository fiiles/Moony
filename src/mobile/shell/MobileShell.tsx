import { useEffect } from 'react';
import { Redirect, Route, Switch, useLocation } from 'wouter';
import { useAuth } from '@/hooks/use-auth';
import { ProtectedRoute } from '@/lib/protected-route';
import { clearEnrolmentAsked } from '@/mobile/lib/biometrics';
import Agenda from '@/mobile/pages/Agenda';
import Budgets from '@/mobile/pages/Budgets';
import Cashflow from '@/mobile/pages/Cashflow';
import CashflowCategory from '@/mobile/pages/CashflowCategory';
import Finance from '@/mobile/pages/Finance';
import AccountDetail from '@/mobile/pages/finance/AccountDetail';
import {
  LoanPlaceholder,
  PolicyPlaceholder,
  QueuePlaceholder,
} from '@/mobile/pages/finance/FinancePlaceholder';
import Lock from '@/mobile/pages/Lock';
import Overview from '@/mobile/pages/Overview';
import PositionDetail from '@/mobile/pages/PositionDetail';
import Settings from '@/mobile/pages/Settings';
import Stocks from '@/mobile/pages/Stocks';
import Wealth from '@/mobile/pages/Wealth';
import {
  BondsPlaceholder,
  CryptoPlaceholder,
  OtherAssetsPlaceholder,
  PropertyPlaceholder,
  RealEstatePlaceholder,
} from '@/mobile/pages/WealthPlaceholder';
import { EnableBiometricsSheet } from '@/mobile/sheets/EnableBiometricsSheet';
import { routes, tabForPath } from './routes';
import { TabBar } from './TabBar';

/**
 * Phone shell (spec §6): the four tab roots behind the lock, the lock screen outside it, and the
 * tab bar over the unlocked tab screens. Navigation is the browser history (wouter), so Android
 * back pops whatever was pushed.
 */
export function MobileShell() {
  const [location] = useLocation();
  const { user, appStatus } = useAuth();
  const tab = appStatus === 'unlocked' && user ? tabForPath(location) : null;

  // No account on this phone (first launch, or after the account was deleted): the next one is
  // offered biometric unlock again, so the "asked" flag of the old account goes.
  useEffect(() => {
    if (appStatus !== 'needs_setup') return;
    try {
      clearEnrolmentAsked(localStorage);
    } catch {
      // Storage that cannot be reached cannot have kept the flag either.
    }
  }, [appStatus]);

  return (
    <>
      <Switch>
        {/* Lock screen and first run: outside `ProtectedRoute`, which redirects here while locked. */}
        <Route path={routes.auth} component={Lock} />
        <ProtectedRoute path={routes.overview} component={Overview} />
        <ProtectedRoute path={routes.agenda} component={Agenda} />
        <ProtectedRoute path={routes.finance} component={Finance} />
        {/* Pushed on Finance; the screens come in Phase 2a (account), 2b (queue) and 4 (loan, policy). */}
        <ProtectedRoute path={routes.accountDetail} component={AccountDetail} />
        <ProtectedRoute path={routes.loanDetail} component={LoanPlaceholder} />
        <ProtectedRoute path={routes.policyDetail} component={PolicyPlaceholder} />
        <ProtectedRoute path={routes.uncategorized} component={QueuePlaceholder} />
        <ProtectedRoute path={routes.cashflow} component={Cashflow} />
        <ProtectedRoute path={routes.cashflowCategory} component={CashflowCategory} />
        <ProtectedRoute path={routes.budgets} component={Budgets} />
        <ProtectedRoute path={routes.wealth} component={Wealth} />
        {/* Pushed on Majetek: the Akcie class page and a position of it. */}
        <ProtectedRoute path={routes.stocks} component={Stocks} />
        <ProtectedRoute path={routes.positionDetail} component={PositionDetail} />
        <ProtectedRoute path={routes.cryptoList} component={CryptoPlaceholder} />
        <ProtectedRoute path={routes.bondsList} component={BondsPlaceholder} />
        <ProtectedRoute path={routes.otherAssetsList} component={OtherAssetsPlaceholder} />
        <ProtectedRoute path={routes.realEstateList} component={RealEstatePlaceholder} />
        <ProtectedRoute path={routes.propertyDetail} component={PropertyPlaceholder} />
        {/* Placeholder page until the Settings sheet of Phase 5 (spec §8.9). */}
        <ProtectedRoute path={routes.settings} component={Settings} />
        <Route>
          <Redirect to={routes.overview} />
        </Route>
      </Switch>
      {tab && <TabBar active={tab} />}
      {/* Once, after the first password unlock: "Odemykat Moony obličejem?" (spec §8.2). */}
      <EnableBiometricsSheet />
    </>
  );
}
