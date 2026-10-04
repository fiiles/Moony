import { Switch, Route, useLocation } from 'wouter';
import { queryClient } from './lib/queryClient';
import { QueryClientProvider } from '@tanstack/react-query';
import { Toaster } from '@/components/ui/toaster';
import { TooltipProvider } from '@/components/ui/tooltip';
import { AppShell } from '@/components/shell/AppShell';
import NotFound from '@/pages/not-found';
import Dashboard from '@/pages/Dashboard';
import AuthPage from '@/pages/auth-page';
import Stocks from '@/pages/Stocks';
import StockDetail from '@/pages/StockDetail';
import StockMonitor from '@/pages/StockMonitor';
import StockMonitorDetail from '@/pages/StockMonitorDetail';
import RealEstate from '@/pages/RealEstate';
import RealEstateDetail from '@/pages/RealEstateDetail';
import Insurance from '@/pages/Insurance';
import InsuranceDetail from '@/pages/InsuranceDetail';
import Loans from '@/pages/Loans';
import LoanDetail from '@/pages/LoanDetail';
import Bonds from '@/pages/Bonds';
import Crypto from '@/pages/Crypto';
import CryptoDetail from '@/pages/CryptoDetail';
import Settings from '@/pages/Settings';
import OtherAssets from '@/pages/OtherAssets';
import Cashflow from '@/pages/Cashflow';
import CashflowPlanning from '@/pages/CashflowPlanning';
import Projection from '@/pages/Projection';
import AnnuityCalculator from '@/pages/AnnuityCalculator';
import EstateCalculator from '@/pages/EstateCalculator';
import StocksAnalysis from '@/pages/StocksAnalysis';
import Budgeting from '@/pages/Budgeting';
import BankAccounts from '@/pages/BankAccounts';
import BankAccountDetail from '@/pages/BankAccountDetail';
import CategorizationRules from '@/pages/CategorizationRules';
import { AuthProvider } from '@/hooks/use-auth';
import { ProtectedRoute } from '@/lib/protected-route';
import { CurrencyProvider } from '@/lib/currency-provider';
import { ErrorBoundary } from '@/components/common/ErrorBoundary';
import { I18nProvider } from '@/i18n/I18nProvider';
import { UpdateNotification } from '@/components/update-notification';
import { UpdaterProvider } from '@/hooks/use-updater';
import { useAutoLock } from '@/hooks/use-auto-lock';
import { SyncProvider } from '@/hooks/SyncProvider';

function Router() {
  return (
    <Switch>
      <ProtectedRoute path="/" component={Dashboard} />
      <ProtectedRoute path="/stocks" component={Stocks} />
      <ProtectedRoute path="/stocks/:id" component={StockDetail} />
      <ProtectedRoute path="/stock-monitor" component={StockMonitor} />
      <ProtectedRoute path="/stock-monitor/:ticker" component={StockMonitorDetail} />
      <ProtectedRoute path="/real-estate" component={RealEstate} />
      <ProtectedRoute path="/real-estate/:id" component={RealEstateDetail} />
      <ProtectedRoute path="/insurance" component={Insurance} />
      <ProtectedRoute path="/insurance/:id" component={InsuranceDetail} />
      <ProtectedRoute path="/loans" component={Loans} />
      <ProtectedRoute path="/loans/:id" component={LoanDetail} />
      <ProtectedRoute path="/bonds" component={Bonds} />
      <ProtectedRoute path="/settings" component={Settings} />
      <ProtectedRoute path="/crypto" component={Crypto} />
      <ProtectedRoute path="/crypto/:id" component={CryptoDetail} />
      <ProtectedRoute path="/other-assets" component={OtherAssets} />
      {/* Design system §11: actual flows from bank transactions, and the planned items */}
      <ProtectedRoute path="/reports/cashflow" component={Cashflow} />
      <ProtectedRoute path="/reports/cashflow-planning" component={CashflowPlanning} />
      <ProtectedRoute path="/reports/budgeting" component={Budgeting} />
      <ProtectedRoute path="/reports/projection" component={Projection} />
      <ProtectedRoute path="/reports/stocks-analysis" component={StocksAnalysis} />
      <ProtectedRoute path="/calculators/annuity" component={AnnuityCalculator} />
      <ProtectedRoute path="/calculators/estate" component={EstateCalculator} />
      <ProtectedRoute path="/bank-accounts" component={BankAccounts} />
      <ProtectedRoute path="/bank-accounts/:id" component={BankAccountDetail} />
      <ProtectedRoute path="/settings/categorization-rules" component={CategorizationRules} />
      {/* Settings sub-pages (general, security, ...); unknown segments fall back to General */}
      <ProtectedRoute path="/settings/*" component={Settings} />
      <Route component={NotFound} />
    </Switch>
  );
}

function AppLayout() {
  const [location] = useLocation();
  // Inert while locked (no user) and when the setting is off; see docs/architecture/overview.md.
  useAutoLock();

  if (location === '/auth') {
    return <AuthPage />;
  }

  return (
    <>
      <UpdateNotification />
      <AppShell>
        <Router />
      </AppShell>
    </>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <I18nProvider>
          <CurrencyProvider>
            <SyncProvider>
              <TooltipProvider delayDuration={200}>
                <ErrorBoundary>
                  <UpdaterProvider>
                    <AppLayout />
                  </UpdaterProvider>
                </ErrorBoundary>
                <Toaster />
              </TooltipProvider>
            </SyncProvider>
          </CurrencyProvider>
        </I18nProvider>
      </AuthProvider>
    </QueryClientProvider>
  );
}
