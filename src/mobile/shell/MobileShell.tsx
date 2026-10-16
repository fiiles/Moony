import { Redirect, Route, Switch, useLocation } from 'wouter';
import { useAuth } from '@/hooks/use-auth';
import { ProtectedRoute } from '@/lib/protected-route';
import Agenda from '@/mobile/pages/Agenda';
import Cashflow from '@/mobile/pages/Cashflow';
import Finance from '@/mobile/pages/Finance';
import Lock from '@/mobile/pages/Lock';
import Overview from '@/mobile/pages/Overview';
import Settings from '@/mobile/pages/Settings';
import Wealth from '@/mobile/pages/Wealth';
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

  return (
    <>
      <Switch>
        {/* Lock screen and first run: outside `ProtectedRoute`, which redirects here while locked. */}
        <Route path={routes.auth} component={Lock} />
        <ProtectedRoute path={routes.overview} component={Overview} />
        <ProtectedRoute path={routes.agenda} component={Agenda} />
        <ProtectedRoute path={routes.finance} component={Finance} />
        <ProtectedRoute path={routes.cashflow} component={Cashflow} />
        <ProtectedRoute path={routes.wealth} component={Wealth} />
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
