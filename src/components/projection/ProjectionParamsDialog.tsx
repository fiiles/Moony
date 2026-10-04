import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ProjectionSettings } from '@shared/schema';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input, InputWrap } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useCurrency } from '@/lib/currency';
import type { ClassParams } from '@/components/projection/projection-classes';
import { contributionToStore, rateToStore } from '@/utils/projection-assumptions';

interface Draft {
  rate: string;
  contribution: string;
  enabled: boolean;
}

interface ProjectionParamsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  params: ClassParams[];
  onSave: (settings: ProjectionSettings[]) => void;
  isPending: boolean;
}

/**
 * "Parametry projekce" (prototype `projection.html`): one row per asset
 * class with the annual growth, the monthly contribution and an active
 * switch; a switched-off class stays at today's value.
 */
export function ProjectionParamsDialog({
  open,
  onOpenChange,
  params,
  onSave,
  isPending,
}: ProjectionParamsDialogProps) {
  const { t } = useTranslation('reports');
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[640px]">
        <DialogHeader>
          <DialogTitle>{t('projection.dialog.title')}</DialogTitle>
          <DialogDescription>{t('projection.dialog.description')}</DialogDescription>
        </DialogHeader>
        {/* Mounted per opening, so the draft starts from the current parameters */}
        {open && (
          <ParamsForm
            params={params}
            onSave={onSave}
            isPending={isPending}
            onCancel={() => onOpenChange(false)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

/** `toDisplay` converts a stored CZK amount into the display currency. */
function initialDraft(
  params: ClassParams[],
  toDisplay: (czk: number) => number
): Record<string, Draft> {
  const next: Record<string, Draft> = {};
  for (const p of params) {
    next[p.cls.key] = {
      // A default rate stays blank (the placeholder shows it), so the class keeps following its default
      rate: p.derived ? '' : (Math.round(p.rate * 100) / 100).toString(),
      // The stored contribution is CZK; the field shows the display currency
      contribution: Math.round(toDisplay(p.contribution)).toString(),
      enabled: p.enabled,
    };
  }
  return next;
}

function ParamsForm({
  params,
  onSave,
  isPending,
  onCancel,
}: {
  params: ClassParams[];
  onSave: (settings: ProjectionSettings[]) => void;
  isPending: boolean;
  onCancel: () => void;
}) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { currencyCode, convert } = useCurrency();
  // Kept so submit can tell an untouched contribution (stored CZK value kept as is) from an edited one
  const [initial] = useState<Record<string, Draft>>(() =>
    initialDraft(params, (czk) => convert(czk, 'CZK', currencyCode))
  );
  const [draft, setDraft] = useState<Record<string, Draft>>(initial);

  const update = (key: string, patch: Partial<Draft>) =>
    setDraft((d) => ({ ...d, [key]: { ...d[key], ...patch } }));

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    onSave(
      params.map((p) => ({
        id: '',
        assetType: p.cls.key,
        // '' = keep the class default, '0' = an explicit 0 %
        yearlyGrowthRate: rateToStore(draft[p.cls.key]?.rate ?? ''),
        monthlyContribution: p.cls.hasContribution
          ? contributionToStore(
              draft[p.cls.key]?.contribution ?? '',
              initial[p.cls.key]?.contribution ?? '',
              p.contribution,
              (v) => convert(v, currencyCode, 'CZK')
            )
          : '0',
        // Stored in CZK, the base currency (the backend sums it as CZK, see get_settings_map);
        // the dialog converts from the display currency above
        contributionCurrency: 'CZK',
        enabled: draft[p.cls.key]?.enabled ?? true,
      }))
    );
  };

  const cell = 'flex items-center';
  const head = 'text-thead uppercase text-ink-4';

  return (
    <form onSubmit={submit}>
      <div className="grid grid-cols-[1.4fr_1fr_1fr_60px] gap-x-3 border-b border-line-soft pb-2">
        <div className={head}>{t('projection.dialog.class')}</div>
        <div className={cn(head, 'text-right')}>{t('projection.dialog.growth')}</div>
        <div className={cn(head, 'text-right')}>{t('projection.dialog.contribution')}</div>
        <div className={cn(head, 'text-right')}>{t('projection.dialog.active')}</div>
      </div>
      {params.map((p) => {
        const d = draft[p.cls.key];
        const off = d ? !d.enabled : !p.enabled;
        return (
          <div
            key={p.cls.key}
            className={cn(
              'grid min-h-[46px] grid-cols-[1.4fr_1fr_1fr_60px] items-center gap-x-3 border-b border-line-soft py-1.5 last:border-b-0',
              off && 'text-ink-4'
            )}
          >
            <div className={cn(cell, 'text-body font-600', off ? 'text-ink-4' : 'text-ink')}>
              {t(`projection.categories.${p.cls.labelKey}`)}
            </div>
            <div className={cn(cell, 'justify-end')}>
              <InputWrap unit="%" className="w-[92px]">
                <Input
                  type="number"
                  step="0.1"
                  inputMode="decimal"
                  className="h-8 pr-[30px] text-right text-caption num"
                  aria-label={`${t(`projection.categories.${p.cls.labelKey}`)} · ${t('projection.dialog.growth')}`}
                  placeholder={(Math.round(p.rate * 100) / 100).toString()}
                  value={d?.rate ?? ''}
                  disabled={off}
                  onChange={(e) => update(p.cls.key, { rate: e.target.value })}
                />
              </InputWrap>
            </div>
            <div className={cn(cell, 'justify-end')}>
              {p.cls.hasContribution ? (
                <InputWrap unit={currencyCode} className="w-[128px]">
                  <Input
                    type="number"
                    step="any"
                    min="0"
                    inputMode="numeric"
                    className="h-8 pr-[42px] text-right text-caption num"
                    aria-label={`${t(`projection.categories.${p.cls.labelKey}`)} · ${t('projection.dialog.contribution')}`}
                    value={d?.contribution ?? ''}
                    disabled={off}
                    onChange={(e) => update(p.cls.key, { contribution: e.target.value })}
                  />
                </InputWrap>
              ) : (
                <span className="text-caption text-ink-5">—</span>
              )}
            </div>
            <div className={cn(cell, 'justify-end')}>
              <Switch
                checked={d?.enabled ?? p.enabled}
                aria-label={`${t(`projection.categories.${p.cls.labelKey}`)} · ${t('projection.dialog.active')}`}
                onCheckedChange={(checked) => update(p.cls.key, { enabled: checked })}
              />
            </div>
          </div>
        );
      })}
      <p className="mt-3 text-micro font-500 text-ink-4">
        {t('projection.dialog.hint')} {t('projection.dialog.blankHint')}
      </p>
      <DialogFooter className="mt-5">
        <Button type="button" variant="ghost" onClick={onCancel}>
          {tc('buttons.cancel')}
        </Button>
        <Button type="submit" loading={isPending}>
          {t('projection.dialog.save')}
        </Button>
      </DialogFooter>
    </form>
  );
}
