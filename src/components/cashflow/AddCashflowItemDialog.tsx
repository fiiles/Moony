import { currencyDecimals, type CurrencyCode } from '@shared/currencies';
import { preserveMoneyText } from '@/utils/money-flows';
/* eslint-disable react-hooks/set-state-in-effect */
import { useState, useEffect, useMemo, useRef } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useTranslation } from 'react-i18next';
import { useCurrency } from '@/lib/currency';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import type { CashflowReport, CashflowReportItem, InsertCashflowItem } from '@shared/schema';

interface AddCashflowItemDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: InsertCashflowItem) => void;
  /** Both sections: every category (grouped in the select) and the automatic sources for the replacement note. */
  report: CashflowReport;
  editItem?: CashflowReportItem | null;
  /** Category preselected for a new item (group row, empty-state link); an edited item keeps its own. */
  presetCategory?: string;
  /** Automatic source the new item replaces ("Nahradit vlastní částkou…"). */
  presetReplacementSourceId?: string | null;
  isLoading?: boolean;
}

/**
 * One dialog for a planned item: the category (grouped by section and
 * income / expense) decides where it counts; a replacement of an automatic
 * row is set only from the row menu and shown here as a note.
 */
export default function AddCashflowItemDialog({
  open,
  onOpenChange,
  onSubmit,
  report,
  editItem,
  presetCategory,
  presetReplacementSourceId = null,
  isLoading,
}: AddCashflowItemDialogProps) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { currencyCode: userCurrency } = useCurrency();

  const [replacementSourceId, setReplacementSourceId] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [amount, setAmount] = useState('');
  const [currency, setCurrency] = useState<string>(userCurrency);
  const [frequency, setFrequency] = useState<'monthly' | 'yearly'>('monthly');
  const [selectedCategory, setSelectedCategory] = useState<string>('');

  const isEditing = !!editItem;
  // The form is filled when the dialog opens, not on every refetch while it is open.
  const wasOpen = useRef(false);

  const groups = useMemo(
    () => [
      { labelKey: 'sections.personalIncome', categories: report.personal.income },
      { labelKey: 'sections.personalExpenses', categories: report.personal.expenses },
      { labelKey: 'sections.investmentIncome', categories: report.investments.income },
      { labelKey: 'sections.investmentExpenses', categories: report.investments.expenses },
    ],
    [report]
  );
  const incomeKeys = useMemo(
    () => new Set([...report.personal.income, ...report.investments.income].map((c) => c.key)),
    [report]
  );
  const firstCategory = groups.flatMap((g) => g.categories)[0]?.key ?? '';
  const replacedName = replacementSourceId
    ? (report.automaticSources.find((s) => s.sourceId === replacementSourceId)?.name ??
      t('readiness.retainedSource'))
    : null;

  useEffect(() => {
    const opening = open && !wasOpen.current;
    wasOpen.current = open;
    if (!opening) return;
    if (editItem) {
      setReplacementSourceId(editItem.replacementSourceId);
      setName(editItem.name);
      setAmount(editItem.originalAmount.toString());
      setCurrency(editItem.originalCurrency);
      setFrequency(editItem.originalFrequency);
      setSelectedCategory(editItem.category);
    } else {
      setReplacementSourceId(presetReplacementSourceId);
      setName('');
      setAmount('');
      setCurrency(userCurrency);
      setFrequency('monthly');
      setSelectedCategory(presetCategory ?? firstCategory);
    }
  }, [open, editItem, presetCategory, presetReplacementSourceId, firstCategory, userCurrency]);

  const valid =
    !!name.trim() &&
    !!amount.trim() &&
    Number.isFinite(Number(amount)) &&
    Number(amount) > 0 &&
    !!selectedCategory;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!valid) return;
    onSubmit({
      name: name.trim(),
      amount: preserveMoneyText(
        editItem?.originalAmount ?? null,
        editItem?.originalCurrency ?? null,
        Number(amount),
        currency
      ),
      replacementSourceId,
      currency,
      frequency,
      itemType: incomeKeys.has(selectedCategory) ? 'income' : 'expense',
      category: selectedCategory,
    });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>{isEditing ? t('addItem.editTitle') : t('addItem.title')}</DialogTitle>
            <DialogDescription>{t('addItem.description')}</DialogDescription>
          </DialogHeader>

          <div className="grid gap-4 py-4">
            <div className="grid gap-2">
              <Label htmlFor="category">{t('addItem.category')}</Label>
              <Select
                value={selectedCategory}
                onValueChange={setSelectedCategory}
                disabled={isEditing}
              >
                <SelectTrigger id="category">
                  <SelectValue placeholder={t('addItem.selectCategory')} />
                </SelectTrigger>
                <SelectContent>
                  {groups.map((group) => (
                    <SelectGroup key={group.labelKey}>
                      <SelectLabel>{t(group.labelKey)}</SelectLabel>
                      {group.categories.map((category) => (
                        <SelectItem key={category.key} value={category.key}>
                          {t(`categories.${category.key}`, { defaultValue: category.name })}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {replacedName && (
              <div className="rounded-r2 bg-well px-3 py-2.5">
                <b className="block text-caption font-650 text-ink">
                  {t('readiness.replacing', { name: replacedName })}
                </b>
                <p className="mt-0.5 text-micro text-ink-4">{t('readiness.replacementHelp')}</p>
              </div>
            )}

            <div className="grid gap-2">
              <Label htmlFor="name">{t('addItem.name')}</Label>
              <Input
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t('addItem.namePlaceholder')}
                required
              />
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="grid gap-2">
                <Label htmlFor="amount">{t('addItem.amount')}</Label>
                <Input
                  id="amount"
                  type="number"
                  step={
                    editItem &&
                    editItem.originalCurrency === currency &&
                    Number(editItem.originalAmount) === Number(amount)
                      ? 'any'
                      : 10 ** -currencyDecimals(currency as CurrencyCode)
                  }
                  min="0"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  placeholder={t('addItem.amountPlaceholder')}
                  required
                />
              </div>

              <div className="grid gap-2">
                <Label htmlFor="currency">{t('addItem.currency')}</Label>
                <CurrencyCombobox id="currency" value={currency} onChange={setCurrency} />
              </div>
            </div>

            <div className="grid gap-2">
              <Label htmlFor="frequency">{t('addItem.frequency')}</Label>
              <Select
                value={frequency}
                onValueChange={(v) => setFrequency(v as 'monthly' | 'yearly')}
              >
                <SelectTrigger id="frequency">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="monthly">{t('viewType.monthly')}</SelectItem>
                  <SelectItem value="yearly">{t('viewType.yearly')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" disabled={isLoading || !valid}>
              {isLoading
                ? tc('status.saving')
                : isEditing
                  ? tc('buttons.saveChanges')
                  : tc('buttons.add')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
