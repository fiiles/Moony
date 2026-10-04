import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { InsurancePolicy } from '@shared/schema';
import { isoDateFromUtcTimestamp } from '@/utils/period';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';

interface EndPolicyDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  policy: InsurancePolicy | null;
  today: number;
  isPending?: boolean;
  onConfirm: (endDay: number) => void;
}

const isoToUtcDaySec = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / 1000);
};

/**
 * "Ukončit pojistku" (prototype insurance-detail.html): the end date goes on
 * the contract and the policy leaves the premium, the calendar and the active
 * list — it stays in the history behind the toggle.
 */
export function EndPolicyDialog({
  open,
  onOpenChange,
  policy,
  today,
  isPending = false,
  onConfirm,
}: EndPolicyDialogProps) {
  const { t } = useTranslation('insurance');
  const { t: tc } = useTranslation('common');

  if (!policy) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('endDialog.title', { name: policy.policyName })}</DialogTitle>
          <DialogDescription>{t('endDialog.description')}</DialogDescription>
        </DialogHeader>
        {/* Mounted with the content, so the date starts at today on every open. */}
        <EndPolicyForm policy={policy} today={today} onConfirm={onConfirm} />
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button type="submit" form="end-policy-form" disabled={isPending} loading={isPending}>
            {isPending ? tc('status.saving') : t('endDialog.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EndPolicyForm({
  policy,
  today,
  onConfirm,
}: {
  policy: InsurancePolicy;
  today: number;
  onConfirm: (endDay: number) => void;
}) {
  const { t } = useTranslation('insurance');
  const [date, setDate] = useState(() => isoDateFromUtcTimestamp(today));
  return (
    <form
      id="end-policy-form"
      className="space-y-4"
      onSubmit={(e) => {
        e.preventDefault();
        if (date) onConfirm(isoToUtcDaySec(date));
      }}
    >
      <div className="space-y-1.5">
        <Label htmlFor="end-policy-date">{t('endDialog.date')}</Label>
        <Input
          id="end-policy-date"
          type="date"
          value={date}
          min={isoDateFromUtcTimestamp(policy.startDate)}
          onChange={(e) => setDate(e.target.value)}
          autoFocus
        />
      </div>
      <Alert>
        <Info aria-hidden />
        <AlertTitle>{t('endDialog.alertTitle')}</AlertTitle>
        <AlertDescription>{t('endDialog.alertNote')}</AlertDescription>
      </Alert>
    </form>
  );
}
