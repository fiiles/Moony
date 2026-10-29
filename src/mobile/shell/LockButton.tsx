import { useTranslation } from 'react-i18next';
import { Lock } from 'lucide-react';
import { useAuth } from '@/hooks/use-auth';
import { clearQuietLock, requestQuietLock } from '@/mobile/lib/quiet-lock';
import { NavIconButton } from './NavBar';

/**
 * The lock at the left of every tab root's nav bar (a pushed screen has its back button there):
 * locks the whole app at once, as Nastavení's "Zamknout aplikaci" does, and the lock screen stays
 * quiet afterwards (Face ID only from its button).
 */
export function LockButton() {
  const { t } = useTranslation('common');
  const { lockMutation } = useAuth();
  return (
    <NavIconButton
      aria-label={t('nav.lockApp')}
      onClick={() => {
        if (lockMutation.isPending) return;
        requestQuietLock();
        lockMutation.mutate(undefined, { onError: clearQuietLock });
      }}
    >
      <Lock strokeWidth={1.75} aria-hidden />
    </NavIconButton>
  );
}
