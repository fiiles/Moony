import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Info, RefreshCw } from 'lucide-react';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { useAuth } from '@/hooks/use-auth';

/**
 * "Obnovit původní data" (ADR 0037, prototype scene 2): the only confirmation in the demo, because
 * it is the only thing there that throws work away. The demo is generated again in the profile's
 * language, then the Overview shows it (with the welcome card back). One component for the profile
 * menu, the top-bar popover and Settings.
 */
export function ResetDemoDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation('common');
  const { resetDemoMutation } = useAuth();
  const [, setLocation] = useLocation();
  const pending = resetDemoMutation.isPending;

  const confirm = () => {
    resetDemoMutation.mutate(undefined, {
      // The provider shows "back to its original state"
      onSuccess: () => {
        onOpenChange(false);
        setLocation('/');
      },
      // The provider says why and returns to the choice screen
      onError: () => onOpenChange(false),
    });
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!pending) onOpenChange(next);
      }}
    >
      <DialogContent className="max-w-[440px]" hideCloseButton={pending}>
        <DialogHeader>
          <DialogTitle>{t('demo.reset.title')}</DialogTitle>
          <DialogDescription>{t('demo.reset.lead')}</DialogDescription>
        </DialogHeader>
        <Alert>
          <Info aria-hidden />
          <AlertTitle>{t('demo.reset.noticeTitle')}</AlertTitle>
          <AlertDescription>{t('demo.reset.noticeText')}</AlertDescription>
        </Alert>
        <DialogFooter>
          <Button variant="ghost" disabled={pending} onClick={() => onOpenChange(false)}>
            {t('buttons.cancel')}
          </Button>
          <Button
            onClick={confirm}
            disabled={pending}
            loading={pending}
            data-testid="demo-reset-confirm"
          >
            {pending && <RefreshCw aria-hidden />}
            {t('demo.reset.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
