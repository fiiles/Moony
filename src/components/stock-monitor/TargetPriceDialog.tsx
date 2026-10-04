import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { formatNativePrice } from '@/utils/stock-monitor';

export interface TargetPriceTarget {
  ticker: string;
  currency: string | null;
  currentPrice: string | null;
  fiftyTwoWeekHigh: string | null;
  targetPrice: string | null;
}

interface TargetPriceDialogProps {
  target: TargetPriceTarget | null;
  onClose: () => void;
  onSave: (ticker: string, targetPrice: string | null) => Promise<unknown>;
  saving: boolean;
}

/**
 * "Cílová cena" (prototype `stock-monitor-detail.html`, modal--sm): one
 * field with the native currency as unit, the current price and the 52-week
 * high as the hint, "Smazat cíl" on the left when a target exists.
 */
export function TargetPriceDialog({ target, onClose, onSave, saving }: TargetPriceDialogProps) {
  const { t } = useTranslation('stockMonitor');
  return (
    <Dialog open={target !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('target.title')}</DialogTitle>
          <DialogDescription>{t('target.description')}</DialogDescription>
        </DialogHeader>
        {target && <TargetForm target={target} onClose={onClose} onSave={onSave} saving={saving} />}
      </DialogContent>
    </Dialog>
  );
}

function TargetForm({
  target,
  onClose,
  onSave,
  saving,
}: {
  target: TargetPriceTarget;
  onClose: () => void;
  onSave: (ticker: string, targetPrice: string | null) => Promise<unknown>;
  saving: boolean;
}) {
  const { t } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [draft, setDraft] = useState(target.targetPrice ?? '');
  const current = parseFloat(target.currentPrice ?? '');
  const high = parseFloat(target.fiftyTwoWeekHigh ?? '');
  const hasTarget = isFinite(parseFloat(target.targetPrice ?? ''));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const normalized = draft.trim().replace(',', '.');
    const value = normalized === '' ? null : normalized;
    if (value !== null && !(parseFloat(value) > 0)) return;
    try {
      await onSave(target.ticker, value);
      onClose();
    } catch {
      // the mutation hook already toasted the error; keep the draft
    }
  };

  const clear = async () => {
    try {
      await onSave(target.ticker, null);
      onClose();
    } catch {
      // already toasted
    }
  };

  const hintParts = [
    isFinite(current)
      ? t('target.hintCurrent', { price: formatNativePrice(current, target.currency, fmt.locale) })
      : null,
    isFinite(high)
      ? t('target.hintHigh', { price: formatNativePrice(high, target.currency, fmt.locale) })
      : null,
  ].filter(Boolean);

  return (
    <form onSubmit={submit}>
      <div className="grid gap-1.5">
        <Label htmlFor="target-price">{t('target.field')}</Label>
        <InputWrap unit={target.currency ?? ''}>
          <Input
            id="target-price"
            inputMode="decimal"
            autoFocus
            placeholder="0"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            className="num"
          />
        </InputWrap>
        {hintParts.length > 0 && (
          <p className="text-micro font-500 text-ink-4">{hintParts.join(' · ')}.</p>
        )}
      </div>
      <DialogFooter className="mt-5">
        {hasTarget ? (
          <Button type="button" variant="danger" onClick={clear} disabled={saving}>
            {t('target.clear')}
          </Button>
        ) : (
          <span />
        )}
        <Button type="button" variant="ghost" onClick={onClose}>
          {tc('buttons.cancel')}
        </Button>
        <Button type="submit" loading={saving}>
          {t('target.save')}
        </Button>
      </DialogFooter>
    </form>
  );
}
