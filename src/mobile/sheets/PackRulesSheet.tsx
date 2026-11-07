import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { ScaledReportError } from '@/mobile/components/Scaled';
import { SearchField } from '@/mobile/components/SearchField';
import { Sheet } from '@/mobile/components/Sheet';
import { useCategorizationMutations, usePackRules } from '@/mobile/hooks/use-categorization-rules';

interface PackRulesSheetProps {
  /** The pack whose rules are shown; `null` closes the sheet. */
  pack: { packId: string; label: string; ruleCount: number } | null;
  onClose: () => void;
}

/** The rules of one pack, searchable by name or pattern, each with its own switch (the desktop's pack tab). */
export function PackRulesSheet({ pack, onClose }: PackRulesSheetProps) {
  const { t } = useTranslation('categorization');
  const { t: tm } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { t: ts } = useTranslation('settings');
  const categoryName = useCategoryName();
  const { categories } = useCategories();
  const [shown, setShown] = useState(pack);
  const [query, setQuery] = useState('');
  if (pack !== null && pack !== shown) {
    setShown(pack);
    setQuery('');
  }
  const { data: rules = [], isLoading, isError, refetch } = usePackRules(shown?.packId ?? null);
  const { setPackRuleDisabled } = useCategorizationMutations();

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return rules;
    return rules.filter(
      (r) => r.name.toLowerCase().includes(q) || r.pattern.toLowerCase().includes(q)
    );
  }, [rules, query]);

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);

  let body;
  if (isError) {
    body = <ScaledReportError onRetry={() => void refetch()} />;
  } else if (isLoading) {
    body = <div aria-hidden className="skeleton h-40 w-full" />;
  } else if (filtered.length === 0) {
    body = <p className="m-0 text-m-sub text-ink-3">{tm('settings.categorization.noMatching')}</p>;
  } else {
    body = (
      <ul className="m-0 list-none divide-y divide-line-soft p-0">
        {filtered.map((rule) => {
          const category = categoryById.get(rule.categoryId);
          return (
            <li key={rule.id} className="flex min-h-[52px] items-center gap-2 py-1.5">
              <span className="min-w-0 flex-1">
                <span className="block truncate text-m-body text-ink">{rule.name}</span>
                <span className="mt-0.5 block truncate text-m-sub text-ink-4">
                  {rule.pattern} → {category ? categoryName(category) : rule.categoryId}
                </span>
                {rule.disabled && (
                  <span className="block truncate text-m-caption text-ink-4">
                    {t('packTab.disabledNote')}
                  </span>
                )}
              </span>
              <span className="flex size-11 flex-none items-center justify-center">
                <Switch
                  checked={!rule.disabled}
                  disabled={setPackRuleDisabled.isPending}
                  onCheckedChange={(on) =>
                    setPackRuleDisabled.mutate({ ruleId: rule.id, disabled: !on })
                  }
                  aria-label={tm('settings.categorization.ruleSwitch', { name: rule.name })}
                />
              </span>
            </li>
          );
        })}
      </ul>
    );
  }

  return (
    <Sheet
      open={pack !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={shown?.label ?? ''}
      description={shown ? ts('rulePacks.rulesCount', { count: shown.ruleCount }) : undefined}
      full
      fill
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={onClose}
        >
          {tc('buttons.close')}
        </Button>
      }
    >
      <div className="flex min-h-0 flex-1 flex-col">
        <SearchField
          value={query}
          onChange={setQuery}
          placeholder={t('packTab.search')}
          clearLabel={tm('common:a11y.clearSearch')}
        />
        <div className="min-h-0 flex-1 overflow-y-auto">{body}</div>
      </div>
    </Sheet>
  );
}
