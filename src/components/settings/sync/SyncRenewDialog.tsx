import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { SyncPlan } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Segmented } from '@/components/ui/segmented';

/**
 * Renewing a lapsed subscription: the account does not tell which plan it had, so the plan is
 * chosen here. A self-hosted server answers without a payment page: nothing is opened then.
 */
export function SyncRenewDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { startCheckout } = useDeviceSyncMutations();
  const [plan, setPlan] = useState<SyncPlan>('monthly');
  const [selfHosted, setSelfHosted] = useState(false);

  const run = async () => {
    try {
      const target = await startCheckout.mutateAsync(plan);
      if (target.kind === 'open') {
        await openUrl(target.url);
        toast(t('renew.opened'));
        onClose();
      } else {
        setSelfHosted(true);
      }
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <Dialog open onOpenChange={(open) => !open && !startCheckout.isPending && onClose()}>
      <DialogContent className="max-w-[460px]">
        <DialogHeader>
          <DialogTitle>{t('renew.title')}</DialogTitle>
          <DialogDescription>
            {selfHosted ? t('alert.lapsed.selfHosted') : t('setup.pay.description')}
          </DialogDescription>
        </DialogHeader>
        {!selfHosted && (
          <Segmented<SyncPlan>
            aria-label={t('setup.pay.plan')}
            fullWidth
            value={plan}
            onValueChange={setPlan}
            options={[
              { value: 'monthly', label: t('setup.pay.monthly') },
              { value: 'yearly', label: t('setup.pay.yearly') },
            ]}
          />
        )}
        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={onClose}
            disabled={startCheckout.isPending}
          >
            {selfHosted ? tc('buttons.close') : t('setup.cancel')}
          </Button>
          {!selfHosted && (
            <Button type="button" onClick={run} loading={startCheckout.isPending}>
              {t('setup.pay.continue')}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
