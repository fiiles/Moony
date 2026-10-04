import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Info } from 'lucide-react';
import { bankAccountsApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { calculateEffectiveRate, zoneUpperBound } from '@/utils/bank-account-zones';
import { useTranslation } from 'react-i18next';

interface ZonesInfoModalProps {
  accountId: string;
  accountName: string;
  balance: number;
  currency: string;
  trigger?: React.ReactNode;
}

export function ZonesInfoModal({
  accountId,
  accountName,
  balance,
  currency,
  trigger,
}: ZonesInfoModalProps) {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const accountCurrency = currency || 'CZK';
  const [open, setOpen] = useState(false);

  const { data: zones } = useQuery({
    queryKey: ['bank-account-zones', accountId],
    queryFn: () => bankAccountsApi.getZones(accountId),
    enabled: open, // Only fetch when modal opens
  });

  const effectiveRate = zones ? calculateEffectiveRate(balance, zones) : 0;

  return (
    <>
      {trigger ? (
        <span onClick={() => setOpen(true)} className="cursor-pointer">
          {trigger}
        </span>
      ) : (
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="ml-1"
          aria-label={t('zones.title', 'Interest Rate Zones')}
          onClick={(e) => {
            e.stopPropagation();
            setOpen(true);
          }}
        >
          <Info />
        </Button>
      )}

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-w-md" onClick={(e) => e.stopPropagation()}>
          <DialogHeader>
            <DialogTitle>{accountName}</DialogTitle>
            <DialogDescription>{t('zones.title', 'Interest Rate Zones')}</DialogDescription>
          </DialogHeader>

          <div className="space-y-4">
            {/* Current Balance & Effective Rate */}
            <div className="flex justify-between items-center p-3 bg-well rounded-r2">
              <div>
                <p className="text-sm text-ink-3">{t('fields.balance')}</p>
                <p className="font-semibold">{fmt.money(balance, accountCurrency)}</p>
              </div>
              <div className="text-right">
                <p className="text-sm text-ink-3">{t('zones.effectiveRate', 'Effective Rate')}</p>
                <p className="num text-h3 text-gain">{effectiveRate.toFixed(2)}%</p>
              </div>
            </div>

            {/* Zones Table */}
            {zones && zones.length > 0 ? (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('zones.fromAmount', 'From')}</TableHead>
                    <TableHead>{t('zones.toAmount', 'To')}</TableHead>
                    <TableHead className="text-right">{t('zones.interestRate', 'Rate')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {zones.map((zone) => {
                    const zoneFrom = parseFloat(zone.fromAmount || '0');
                    const zoneTo = zoneUpperBound(zone); // null = unlimited
                    const isActive = balance >= zoneFrom && (zoneTo === null || balance <= zoneTo);

                    return (
                      <TableRow key={zone.id} className={isActive ? 'bg-gain-soft' : ''}>
                        <TableCell>{fmt.money(zoneFrom, accountCurrency)}</TableCell>
                        <TableCell>
                          {zoneTo === null
                            ? t('zones.unlimited')
                            : fmt.money(zoneTo, accountCurrency)}
                        </TableCell>
                        <TableCell className="text-right font-semibold text-gain">
                          {parseFloat(zone.interestRate || '0').toFixed(2)}%
                          {isActive && (
                            <Badge variant="gain" className="ml-2">
                              {t('zones.active', 'Active')}
                            </Badge>
                          )}
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            ) : (
              <p className="text-ink-3 text-center py-4">
                {t('zones.noZones', 'No interest rate zones configured')}
              </p>
            )}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
