import { useTranslation } from 'react-i18next';
import type { SavingsAccountZone } from '@shared/schema';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { calculateZonedInterest, zoneUpperBound } from '@/utils/bank-account-zones';

interface InterestDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  account: {
    interestRate: string | null;
    hasZoneDesignation: boolean;
    interestRateValidUntil: number | null;
  };
  zones: readonly SavingsAccountZone[];
  /** Current balance in the account currency. */
  balance: number;
  currency: string;
}

/** Interest tiers (or the simple rate) with the effective rate on the current balance. */
export function InterestDialog({
  open,
  onOpenChange,
  account,
  zones,
  balance,
  currency,
}: InterestDialogProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const zoned = account.hasZoneDesignation && zones.length > 0;
  const simpleRate = parseFloat(account.interestRate ?? '0') || 0;
  const yearly = zoned ? calculateZonedInterest(balance, zones) : balance * (simpleRate / 100);
  const effective = balance > 0 ? yearly / balance : zoned ? 0 : simpleRate / 100;
  const money = (v: number) => fmt.money(v, currency, { decimals: 0 });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('interest.title')}</DialogTitle>
          <DialogDescription>
            {t(zoned ? 'interest.zonedSub' : 'interest.simpleSub')}
          </DialogDescription>
        </DialogHeader>
        <div>
          {zoned ? (
            zones.map((zone) => {
              const upper = zoneUpperBound(zone);
              return (
                <div
                  key={zone.id}
                  className="grid grid-cols-[1fr_auto] gap-x-4 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-0"
                >
                  <span className="num">
                    {money(parseFloat(zone.fromAmount || '0'))}
                    {' – '}
                    {upper !== null ? money(upper) : t('zones.unlimited')}
                  </span>
                  <b className="font-600 text-ink num">
                    {fmt.percent(parseFloat(zone.interestRate || '0') / 100, 2)}
                  </b>
                </div>
              );
            })
          ) : (
            <div className="grid grid-cols-[1fr_auto] gap-x-4 py-[9px] text-table text-ink-3">
              <span>{t('interest.rate')}</span>
              <b className="font-600 text-ink num">{fmt.percent(simpleRate / 100, 2)}</b>
            </div>
          )}
          <p className="mt-3 text-table font-600 text-ink">
            {t('interest.effectiveLine', {
              rate: fmt.percent(effective, 2),
              amount: money(yearly),
            })}
          </p>
          {account.interestRateValidUntil && (
            <p className="mt-1 text-micro font-500 text-ink-4">
              {t('interest.validUntil', { date: fmt.day(account.interestRateValidUntil) })}
            </p>
          )}
          <p className="mt-3 text-micro font-500 text-ink-4">{t('interest.caption')}</p>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
