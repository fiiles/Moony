import { Redirect, Route, Switch, useLocation } from 'wouter';
import { useAuth } from '@/hooks/use-auth';
import { ProtectedRoute } from '@/lib/protected-route';
import AuthPage from '@/pages/auth-page';
import Cashflow from '@/mobile/pages/Cashflow';
import Finance from '@/mobile/pages/Finance';
import Overview from '@/mobile/pages/Overview';
import Wealth from '@/mobile/pages/Wealth';
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

  return (
    <>
      <Switch>
        {/* The desktop lock screen and first run until Task 1.4 brings the phone one. */}
        <Route path={routes.auth} component={AuthPage} />
        <ProtectedRoute path={routes.overview} component={Overview} />
        <ProtectedRoute path={routes.finance} component={Finance} />
        <ProtectedRoute path={routes.cashflow} component={Cashflow} />
        <ProtectedRoute path={routes.wealth} component={Wealth} />
        <Route>
          <Redirect to={routes.overview} />
        </Route>
      </Switch>
      {tab && <TabBar active={tab} />}
    </>
  );
}
