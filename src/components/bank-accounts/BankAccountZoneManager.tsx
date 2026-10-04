import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Plus, Trash2, Edit2, AlertCircle } from 'lucide-react';
import { useCurrency } from '@/lib/currency';
import { Alert, AlertDescription } from '@/components/ui/alert';

interface ZoneData {
  id?: string;
  fromAmount: string;
  toAmount?: string | null;
  interestRate: string;
}

interface BankAccountZoneManagerProps {
  zones: ZoneData[];
  onChange: (zones: ZoneData[]) => void;
}

export function BankAccountZoneManager({ zones, onChange }: BankAccountZoneManagerProps) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const { formatCurrency } = useCurrency();
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [formData, setFormData] = useState<ZoneData>({
    fromAmount: '',
    toAmount: '',
    interestRate: '',
  });
  const [validationError, setValidationError] = useState<string>('');

  const validateZone = (newZone: ZoneData, currentIndex: number | null): string => {
    // Basic validation
    if (!newZone.fromAmount || !newZone.interestRate) {
      return t('zones.validation.required');
    }

    const from = parseFloat(newZone.fromAmount);
    const to = newZone.toAmount ? parseFloat(newZone.toAmount) : null;

    // Check if toAmount is less than fromAmount
    if (to !== null && to <= from) {
      return t('zones.validation.toAmountGreater');
    }

    // Get all zones except the one being edited
    const otherZones = zones.filter((_, index) => index !== currentIndex);

    // Check if there's already an unlimited zone
    const hasUnlimitedZone = otherZones.some((z) => !z.toAmount || z.toAmount === '');
    if ((!newZone.toAmount || newZone.toAmount === '') && hasUnlimitedZone) {
      return t('zones.validation.oneUnlimited');
    }

    // Check for overlaps with existing zones
    for (const zone of otherZones) {
      const zoneFrom = parseFloat(zone.fromAmount);
      const zoneTo = zone.toAmount ? parseFloat(zone.toAmount) : null;

      // Check if ranges overlap
      if (to === null) {
        // New zone is unlimited, check if it starts before any existing zone ends
        if (zoneTo === null) {
          return t('zones.validation.cannotAddUnlimited');
        }
        if (from < zoneTo) {
          return t('zones.validation.overlapExisting', {
            from: formatCurrency(zoneFrom),
            to: formatCurrency(zoneTo),
          });
        }
      } else {
        // New zone has an upper limit
        if (zoneTo === null) {
          // Existing zone is unlimited
          if (to > zoneFrom) {
            return t('zones.validation.overlapUnlimited', { start: formatCurrency(zoneFrom) });
          }
        } else {
          // Both zones have limits - check for overlap
          if (
            (from >= zoneFrom && from < zoneTo) ||
            (to > zoneFrom && to <= zoneTo) ||
            (from <= zoneFrom && to >= zoneTo)
          ) {
            return t('zones.validation.overlapExisting', {
              from: formatCurrency(zoneFrom),
              to: formatCurrency(zoneTo),
            });
          }
        }
      }
    }

    return '';
  };

  const handleAdd = () => {
    setValidationError('');

    const error = validateZone(formData, null);
    if (error) {
      setValidationError(error);
      return;
    }

    const newZones = [...zones, formData];
    onChange(newZones.sort((a, b) => parseFloat(a.fromAmount) - parseFloat(b.fromAmount)));
    setFormData({ fromAmount: '', toAmount: '', interestRate: '' });
  };

  const handleEdit = (index: number) => {
    setEditingIndex(index);
    setFormData(zones[index]);
    setValidationError('');
  };

  const handleUpdate = () => {
    if (editingIndex === null) return;

    setValidationError('');

    const error = validateZone(formData, editingIndex);
    if (error) {
      setValidationError(error);
      return;
    }

    const newZones = [...zones];
    newZones[editingIndex] = formData;
    onChange(newZones.sort((a, b) => parseFloat(a.fromAmount) - parseFloat(b.fromAmount)));
    setEditingIndex(null);
    setFormData({ fromAmount: '', toAmount: '', interestRate: '' });
  };

  const handleDelete = (index: number) => {
    const newZones = zones.filter((_, i) => i !== index);
    onChange(newZones);
    setValidationError('');
  };

  const handleCancel = () => {
    setEditingIndex(null);
    setFormData({ fromAmount: '', toAmount: '', interestRate: '' });
    setValidationError('');
  };

  return (
    <div className="space-y-4">
      {/* Zone Input Form */}
      <div className="p-4 bg-well rounded-r2 border space-y-3">
        <p className="text-xs font-medium text-ink-3 uppercase tracking-wide">
          {editingIndex === null ? t('zones.addZoneTitle') : t('zones.editZoneTitle')}
        </p>
        <div className="grid grid-cols-7 gap-3">
          <div className="col-span-2 grid gap-1.5">
            <Label htmlFor="fromAmount" className="text-xs">
              {t('zones.fromAmount')}
            </Label>
            <Input
              id="fromAmount"
              type="number"
              step="0.01"
              value={formData.fromAmount}
              onChange={(e) => setFormData({ ...formData, fromAmount: e.target.value })}
              placeholder="0"
              className="h-9"
            />
          </div>
          <div className="col-span-2 grid gap-1.5">
            <Label htmlFor="toAmount" className="text-xs">
              {t('zones.toAmount')}
            </Label>
            <Input
              id="toAmount"
              type="number"
              step="0.01"
              value={formData.toAmount ?? ''}
              onChange={(e) => setFormData({ ...formData, toAmount: e.target.value })}
              placeholder={t('zones.unlimited')}
              className="h-9"
            />
          </div>
          <div className="col-span-2 grid gap-1.5">
            <Label htmlFor="zoneInterestRate" className="text-xs">
              {t('zones.ratePercent')}
            </Label>
            <InputWrap unit="%">
              <Input
                id="zoneInterestRate"
                type="number"
                step="0.01"
                value={formData.interestRate}
                onChange={(e) => setFormData({ ...formData, interestRate: e.target.value })}
                placeholder="0.00"
                className="h-9"
              />
            </InputWrap>
          </div>
          <div className="col-span-1 flex items-end">
            {editingIndex === null ? (
              <Button onClick={handleAdd} type="button" size="sm" className="h-9 w-full">
                <Plus className="h-4 w-4" />
              </Button>
            ) : (
              <div className="flex gap-1 w-full">
                <Button onClick={handleUpdate} type="button" size="sm" className="h-9 flex-1">
                  {t('zones.save')}
                </Button>
                <Button
                  onClick={handleCancel}
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-9 flex-1"
                  aria-label={tc('buttons.cancel')}
                >
                  ✕
                </Button>
              </div>
            )}
          </div>
        </div>

        {validationError && (
          <Alert variant="destructive">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>{validationError}</AlertDescription>
          </Alert>
        )}
      </div>
      {zones.length > 0 && (
        <div className="rounded-r2 border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('zones.fromAmount')}</TableHead>
                <TableHead>{t('zones.toAmount')}</TableHead>
                <TableHead>{t('zones.interestRate')}</TableHead>
                <TableHead className="w-[100px]">{tc('table.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {zones.map((zone, index) => (
                <TableRow key={index}>
                  <TableCell>{formatCurrency(parseFloat(zone.fromAmount))}</TableCell>
                  <TableCell>
                    {zone.toAmount
                      ? formatCurrency(parseFloat(zone.toAmount))
                      : t('zones.unlimited')}
                  </TableCell>
                  <TableCell>{parseFloat(zone.interestRate).toFixed(2)}%</TableCell>
                  <TableCell>
                    <div className="flex gap-2">
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => handleEdit(index)}
                        type="button"
                      >
                        <Edit2 className="h-4 w-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => handleDelete(index)}
                        type="button"
                        className="text-loss"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      {zones.length === 0 && (
        <div className="text-center text-sm text-ink-3 py-8 border border-dashed rounded-r2 bg-well">
          <p className="font-medium">{t('zones.emptyState')}</p>
          <p className="text-xs mt-1">{t('zones.emptyStateDescription')}</p>
        </div>
      )}
    </div>
  );
}
