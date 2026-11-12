import { useTranslation } from 'react-i18next';
import { usePairingExpired } from '@/hooks/use-pairing-expired';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { PairingView } from '@shared/schema';

/**
 * Under a join confirmation: until when the code is valid, so reading the numbers too long does not
 * end in a surprise; once it has passed, that it has expired.
 */
export function PairingExpiryNote({ view, className }: { view: PairingView; className?: string }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const expired = usePairingExpired(view);
  return (
    <p
      className={cn('m-0', expired ? 'font-500 text-loss' : 'text-ink-3', className)}
      role={expired ? 'alert' : undefined}
      data-testid="pairing-expiry"
    >
      {expired
        ? tc('validation.syncPairingExpired')
        : t('pairing.validUntil', { time: fmt.time(view.expiresAt) })}
    </p>
  );
}
