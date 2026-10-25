import { useEffect, useState } from 'react';
import { Redirect, Route, Switch, useLocation } from 'wouter';
import { useAuth } from '@/hooks/use-auth';
import { ProtectedRoute } from '@/lib/protected-route';
import { ReceivedFileHost } from '@/mobile/components/import/ReceivedFileHost';
import { ImportSheetProvider } from '@/mobile/hooks/use-import-sheet';
import { useNotificationSchedule } from '@/mobile/hooks/use-notification-schedule';
import { clearEnrolmentAsked } from '@/mobile/lib/biometrics';
import Agenda from '@/mobile/pages/Agenda';
import Bonds from '@/mobile/pages/Bonds';
import Budgets from '@/mobile/pages/Budgets';
import Cashflow from '@/mobile/pages/Cashflow';
import CashflowCategory from '@/mobile/pages/CashflowCategory';
import Crypto from '@/mobile/pages/Crypto';
import CryptoDetail from '@/mobile/pages/CryptoDetail';
import Finance from '@/mobile/pages/Finance';
import AccountDetail from '@/mobile/pages/finance/AccountDetail';
import CategorizeQueue from '@/mobile/pages/finance/CategorizeQueue';
import LoanDetail from '@/mobile/pages/finance/LoanDetail';
import PolicyDetail from '@/mobile/pages/finance/PolicyDetail';
import Lock from '@/mobile/pages/Lock';
import OtherAssetDetail from '@/mobile/pages/OtherAssetDetail';
import OtherAssets from '@/mobile/pages/OtherAssets';
import Overview from '@/mobile/pages/Overview';
import PositionDetail from '@/mobile/pages/PositionDetail';
import PropertyDetail from '@/mobile/pages/PropertyDetail';
import Settings from '@/mobile/pages/Settings';
import SettingsSection from '@/mobile/pages/settings/SettingsSection';
import Stocks from '@/mobile/pages/Stocks';
import Wealth from '@/mobile/pages/Wealth';
import WatchDetail from '@/mobile/pages/WatchDetail';
import { EnableBiometricsSheet } from '@/mobile/sheets/EnableBiometricsSheet';
import { routes, tabForPath } from './routes';
import { TabBar } from './TabBar';

/** Plans the local notifications (spec §8.9) and draws nothing. */
function NotificationScheduler() {
  useNotificationSchedule();
  return null;
}

/**
 * Phone shell (spec §6): the four tab roots behind the lock, the lock screen outside it, and the
 * tab bar over the unlocked tab screens. Navigation is the browser history (wouter), so Android
 * back pops whatever was pushed. The import sheet lives above the routes (`ImportSheetProvider`),
 * so every entry point can open it and it outlives the screen that did; `ReceivedFileHost` feeds
 * it the statements the system opened in Moony.
 */
export function MobileShell() {
  const [location] = useLocation();
  const { user, appStatus } = useAuth();
  const tab = appStatus === 'unlocked' && user ? tabForPath(location) : null;
  // A statement shared into Moony is on its way to the import sheet: the biometric offer waits
  const [sharedPending, setSharedPending] = useState(false);

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
    <ImportSheetProvider>
      <Switch>
        {/* Lock screen and first run: outside `ProtectedRoute`, which redirects here while locked. */}
        <Route path={routes.auth} component={Lock} />
        <ProtectedRoute path={routes.overview} component={Overview} />
        <ProtectedRoute path={routes.agenda} component={Agenda} />
        <ProtectedRoute path={routes.finance} component={Finance} />
        {/* Pushed on Finance: an account, a loan, a policy and the categorize queue. */}
        <ProtectedRoute path={routes.accountDetail} component={AccountDetail} />
        <ProtectedRoute path={routes.loanDetail} component={LoanDetail} />
        <ProtectedRoute path={routes.policyDetail} component={PolicyDetail} />
        <ProtectedRoute path={routes.uncategorized} component={CategorizeQueue} />
        <ProtectedRoute path={routes.cashflow} component={Cashflow} />
        <ProtectedRoute path={routes.cashflowCategory} component={CashflowCategory} />
        <ProtectedRoute path={routes.budgets} component={Budgets} />
        <ProtectedRoute path={routes.wealth} component={Wealth} />
        {/* Pushed on Majetek: the Akcie class page and a position of it. */}
        <ProtectedRoute path={routes.stocks} component={Stocks} />
        <ProtectedRoute path={routes.positionDetail} component={PositionDetail} />
        {/* A watched ticker, followed or not: from a Sledované row or a search result. */}
        <ProtectedRoute path={routes.watchDetail} component={WatchDetail} />
        {/* The Krypto class page and a coin of it. */}
        <ProtectedRoute path={routes.cryptoList} component={Crypto} />
        <ProtectedRoute path={routes.cryptoDetail} component={CryptoDetail} />
        {/* The Dluhopisy class page */}
        <ProtectedRoute path={routes.bondsList} component={Bonds} />
        {/* The Ostatní aktiva class page and an asset of it */}
        <ProtectedRoute path={routes.otherAssetsList} component={OtherAssets} />
        <ProtectedRoute path={routes.otherAssetDetail} component={OtherAssetDetail} />
        <ProtectedRoute path={routes.propertyDetail} component={PropertyDetail} />
        {/* Nastavení and the pages pushed on it (spec §8.9) */}
        <ProtectedRoute path={routes.settings} component={Settings} />
        <ProtectedRoute path={routes.settingsSection} component={SettingsSection} />
        <Route>
          <Redirect to={routes.overview} />
        </Route>
      </Switch>
      {tab && <TabBar active={tab} />}
      {/* Once, after the first password unlock: "Odemykat Moony obličejem?" (spec §8.2). A shared
          statement's import comes first; the offer opens once that sheet has closed. */}
      <EnableBiometricsSheet sharedPending={sharedPending} />
      {/* Only while unlocked: it reads the milestones, which the locked database cannot answer. */}
      {appStatus === 'unlocked' && <NotificationScheduler />}
      {/* A statement shared into the app opens the import sheet after the unlock, once. It stays
          mounted through a lock (and drains only while unlocked), so a path it has taken but not
          yet shown survives every unlock. */}
      <ReceivedFileHost onPendingChange={setSharedPending} />
    </ImportSheetProvider>
  );
}
