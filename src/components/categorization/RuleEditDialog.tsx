/**
 * Rule editor (design system prototype categorization-rules.html): name, what
 * to match on, the pattern, category and priority, the two switches, and a
 * live "matches N existing transactions" preview with the option to categorize
 * the open ones right away.
 */
import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { RefreshCw } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Segmented } from '@/components/ui/segmented';
import { Switch } from '@/components/ui/switch';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { categorizationApi } from '@/lib/tauri-api';
import type { CustomRule, CustomRuleInput, TransactionCategory } from '@shared/schema';
import { CategoryIcon } from '@/components/common/CategoryIcon';
import { useCategoryName } from '@/hooks/use-categories';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { HIT_DAYS } from '@/utils/rule-hits';
import { cn } from '@/lib/utils';

/** Starting values for a new rule, e.g. from the transaction it is created for (RUL-03). */
export interface RuleEditPrefill {
  name?: string;
  pattern?: string;
  categoryId?: string;
}

interface RuleEditDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  rule: CustomRule | null;
  /** Used only when `rule` is null (a new rule). */
  prefill?: RuleEditPrefill | null;
  categories: TransactionCategory[];
}

const RULE_TYPES = ['Contains', 'Word', 'Regex', 'StartsWith', 'EndsWith'] as const;
const RULE_TYPE_KEYS: Record<(typeof RULE_TYPES)[number], string> = {
  Contains: 'contains',
  Word: 'word',
  Regex: 'regex',
  StartsWith: 'starts_with',
  EndsWith: 'ends_with',
};

// Rule mode: either pattern-based (text matching) or IBAN-based (exact account matching)
type RuleMode = 'pattern' | 'iban';

export default function RuleEditDialog({
  open,
  onOpenChange,
  rule,
  prefill,
  categories,
}: RuleEditDialogProps) {
  const { t } = useTranslation('categorization');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  const fmt = useFormat();
  const queryClient = useQueryClient();

  const getInitialMode = (r: CustomRule | null): RuleMode => {
    if (!r) return 'pattern';
    if (r.ibanPattern && (!r.pattern || r.pattern === '*')) return 'iban';
    return 'pattern';
  };

  const newRulePrefill = rule ? null : (prefill ?? null);
  const prefilledCategoryId = categories.some((c) => c.id === newRulePrefill?.categoryId)
    ? (newRulePrefill?.categoryId ?? '')
    : '';
  const defaultCategoryId = newRulePrefill ? prefilledCategoryId : (categories[0]?.id ?? '');

  // Form state resets on rule / prefill change via the `key` prop at the render site.
  const [ruleMode, setRuleMode] = useState<RuleMode>(getInitialMode(rule));
  const [name, setName] = useState(rule?.name ?? newRulePrefill?.name ?? '');
  const [ruleType, setRuleType] = useState<string>(rule?.ruleType ?? 'Contains');
  const [pattern, setPattern] = useState(rule?.pattern ?? newRulePrefill?.pattern ?? '');
  const [chosenCategoryId, setChosenCategoryId] = useState<string | null>(rule?.categoryId ?? null);
  const categoryId = chosenCategoryId ?? defaultCategoryId;
  const [priority, setPriority] = useState(rule?.priority ?? 50);
  const [isActive, setIsActive] = useState(rule?.isActive ?? true);
  const [stopProcessing, setStopProcessing] = useState(rule?.stopProcessing ?? false);
  const [ibanPattern, setIbanPattern] = useState(rule?.ibanPattern ?? '');
  const [applyNow, setApplyNow] = useState(true);

  const input = useMemo<CustomRuleInput>(
    () => ({
      name,
      ruleType: ruleMode === 'iban' ? 'Contains' : ruleType,
      pattern: ruleMode === 'iban' ? '*' : pattern,
      categoryId,
      priority,
      isActive,
      stopProcessing,
      ibanPattern: ruleMode === 'iban' ? ibanPattern || undefined : undefined,
    }),
    [name, ruleMode, ruleType, pattern, categoryId, priority, isActive, stopProcessing, ibanPattern]
  );
  const hasPattern = ruleMode === 'iban' ? ibanPattern.trim() !== '' : pattern.trim() !== '';
  // Only what decides the matches goes into the key: a name change must not re-run the preview.
  const previewInput = useDebouncedValue(
    {
      ruleType: input.ruleType,
      pattern: input.pattern,
      ibanPattern: input.ibanPattern ?? null,
    },
    300
  );
  const preview = useQuery({
    queryKey: ['rulePreview', previewInput],
    queryFn: () =>
      categorizationApi.previewRuleMatches(
        {
          ...input,
          ruleType: previewInput.ruleType,
          pattern: previewInput.pattern,
          ibanPattern: previewInput.ibanPattern ?? undefined,
        },
        HIT_DAYS
      ),
    enabled: open && hasPattern,
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });
  const previewPending =
    hasPattern &&
    (preview.isFetching ||
      previewInput.pattern !== input.pattern ||
      (previewInput.ibanPattern ?? undefined) !== input.ibanPattern);

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['customRules'] });
    queryClient.invalidateQueries({ queryKey: ['ruleHits'] });
    queryClient.invalidateQueries({ queryKey: ['categorizationOverview'] });
    queryClient.invalidateQueries({ queryKey: ['bank-transactions'] });
  };

  const saveMutation = useMutation({
    mutationFn: async (data: CustomRuleInput) => {
      const saved = rule
        ? await categorizationApi.updateCustomRule(rule.id, data)
        : await categorizationApi.createCustomRule(data);
      const open = preview.data?.uncategorized ?? 0;
      let applied = 0;
      if (applyNow && open > 0 && data.isActive) {
        applied = await categorizationApi.applyRuleToUncategorized(saved.id, HIT_DAYS);
      }
      return { saved, applied };
    },
    onSuccess: ({ applied }) => {
      invalidate();
      toast(
        applied > 0
          ? t('editor.savedApplied', { count: applied })
          : rule
            ? t('customRules.updateSuccess')
            : t('customRules.createSuccess')
      );
      onOpenChange(false);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const canSave =
    !saveMutation.isPending &&
    name.trim() !== '' &&
    categoryId !== '' &&
    hasPattern &&
    !preview.isError;

  const openCount = preview.data?.uncategorized ?? 0;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{rule ? t('editor.editTitle') : t('editor.newTitle')}</DialogTitle>
          <DialogDescription>
            {newRulePrefill
              ? `${t('editor.lead')} ${t('customRules.form.prefillHint')}`
              : t('editor.lead')}
          </DialogDescription>
        </DialogHeader>
        <form
          id="rule-edit-form"
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (canSave) saveMutation.mutate(input);
          }}
        >
          <div className="space-y-1.5">
            <Label htmlFor="rule-name">{t('editor.name')}</Label>
            <Input
              id="rule-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('customRules.form.namePlaceholder')}
              autoFocus
            />
          </div>
          <div className="space-y-1.5">
            <Label>{t('editor.mode')}</Label>
            <Segmented
              fullWidth
              value={ruleMode}
              onValueChange={setRuleMode}
              options={[
                { value: 'pattern', label: t('editor.modeText') },
                { value: 'iban', label: t('editor.modeIban') },
              ]}
            />
          </div>
          {ruleMode === 'pattern' ? (
            <div className="grid grid-cols-[150px_1fr] gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="rule-type">{t('editor.matchType')}</Label>
                <Select value={ruleType} onValueChange={setRuleType}>
                  <SelectTrigger id="rule-type">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {RULE_TYPES.map((type) => (
                      <SelectItem key={type} value={type}>
                        {t(`ruleTypes.${RULE_TYPE_KEYS[type]}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="rule-pattern">{t('editor.pattern')}</Label>
                <Input
                  id="rule-pattern"
                  value={pattern}
                  onChange={(e) => setPattern(e.target.value)}
                  placeholder={t('customRules.form.patternPlaceholder')}
                  className="font-mono"
                />
                <p className="text-micro font-500 text-ink-4">
                  {preview.isError ? t('editor.invalidRegex') : t('editor.patternHint')}
                </p>
              </div>
            </div>
          ) : (
            <div className="space-y-1.5">
              <Label htmlFor="rule-iban">{t('editor.modeIban')}</Label>
              <Input
                id="rule-iban"
                value={ibanPattern}
                onChange={(e) => setIbanPattern(e.target.value)}
                placeholder={t('customRules.form.ibanPatternPlaceholder')}
                className="font-mono"
              />
              <p className="text-micro font-500 text-ink-4">{t('editor.ibanHint')}</p>
            </div>
          )}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="rule-category">{t('editor.category')}</Label>
              <Select
                value={categoryId}
                onValueChange={(value) => setChosenCategoryId(value || null)}
              >
                <SelectTrigger id="rule-category">
                  <SelectValue placeholder={t('customRules.form.categoryPlaceholder')} />
                </SelectTrigger>
                <SelectContent>
                  {categories.map((category) => (
                    <SelectItem key={category.id} value={category.id}>
                      <span className="flex items-center gap-2">
                        <CategoryIcon iconName={category.icon || 'tag'} className="size-3.5" />
                        {categoryName(category)}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="rule-priority" className="flex justify-between">
                <span>{t('editor.priority')}</span>
                <span className="font-500 text-ink-5">{t('editor.priorityHint')}</span>
              </Label>
              <Input
                id="rule-priority"
                type="number"
                value={priority}
                onChange={(e) =>
                  setPriority(Math.min(100, Math.max(1, parseInt(e.target.value) || 50)))
                }
                min={1}
                max={100}
              />
            </div>
          </div>
          <div className="flex flex-wrap gap-6 text-table font-500 text-ink-2">
            <label className="flex cursor-pointer items-center gap-2">
              <Switch checked={isActive} onCheckedChange={setIsActive} />
              {t('editor.active')}
            </label>
            <label className="flex cursor-pointer items-center gap-2">
              <Switch checked={stopProcessing} onCheckedChange={setStopProcessing} />
              {t('editor.stop')}
              <span className="font-500 text-ink-4">{t('editor.stopHint')}</span>
            </label>
          </div>

          <div
            className={cn(
              'rounded-r3 bg-well px-3.5 py-3 text-caption text-ink-2 transition-opacity duration-fast',
              previewPending && 'opacity-60'
            )}
            aria-live="polite"
          >
            {!hasPattern ? (
              <span className="text-ink-3">{t('editor.matchesEmpty')}</span>
            ) : preview.isError ? (
              <span className="text-loss">{t('editor.invalidRegex')}</span>
            ) : !preview.data ? (
              <span className="flex items-center gap-2 text-ink-3">
                <RefreshCw className="size-3 animate-spin" aria-hidden />
                {t('editor.matchesChecking')}
              </span>
            ) : preview.data.matches === 0 ? (
              <span>{t('editor.matchesNone', { days: HIT_DAYS })}</span>
            ) : (
              <>
                <b className="font-650 text-ink">
                  {t('editor.matchesTitle', { count: preview.data.matches })}
                </b>{' '}
                {t('editor.matchesNote', { days: HIT_DAYS, open: preview.data.uncategorized })}
                <ul className="mb-0 mt-2 list-disc pl-4 text-micro text-ink-3">
                  {preview.data.samples.slice(0, 3).map((s, i) => (
                    <li key={i}>
                      {fmt.day(s.bookingDate)} ·{' '}
                      {[s.description, s.counterparty].filter(Boolean).join(' · ')} ·{' '}
                      {fmt.money(Number(s.amount), s.currency, { signed: true, decimals: 0 })}
                    </li>
                  ))}
                </ul>
                {openCount > 0 && (
                  <label className="mt-2.5 flex cursor-pointer items-center gap-[9px] text-table font-500 text-ink-2">
                    <Checkbox checked={applyNow} onCheckedChange={(v) => setApplyNow(v === true)} />
                    {t('editor.applyNow', { count: openCount })}
                  </label>
                )}
              </>
            )}
          </div>
        </form>
        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={() => onOpenChange(false)}
            disabled={saveMutation.isPending}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="rule-edit-form"
            disabled={!canSave}
            loading={saveMutation.isPending}
          >
            {saveMutation.isPending ? tc('status.saving') : t('editor.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
