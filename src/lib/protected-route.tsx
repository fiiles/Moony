import { useAuth } from '../hooks/use-auth';
import { Loader2 } from 'lucide-react';
import { Redirect, Route } from 'wouter';

export function ProtectedRoute({
  path,
  component: Component,
}: {
  path: string;
  component: () => React.JSX.Element;
}) {
  const { user, appStatus, isLoading } = useAuth();

  if (isLoading) {
    return (
      <Route path={path}>
        <div className="flex items-center justify-center min-h-screen bg-canvas">
          <Loader2 className="h-8 w-8 animate-spin text-ink-5" />
        </div>
      </Route>
    );
  }

  // If app needs setup or is locked (not unlocked), redirect to auth
  if (appStatus !== 'unlocked' || !user) {
    return (
      <Route path={path}>
        <Redirect to="/auth" />
      </Route>
    );
  }

  return <Route path={path} component={Component} />;
}
