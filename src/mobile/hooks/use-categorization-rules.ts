import { useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { CustomRule } from '@shared/schema';
import { useLanguage } from '@/i18n/I18nProvider';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { categorizationApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { CATEGORIZE_QUEUE_KEY } from '@/mobile/hooks/use-categorize-queue';
import { customRuleInput, nextEnabledPacks } from '@/mobile/lib/categorization-rows';
import { HIT_DAYS, hitMap } from '@/utils/rule-hits';

/** Label of a pack: "Globální značky" for `global`, else the country in the UI language. */
export function usePackLabel() {
  const { t } = useTranslation('settings');
  const { language } = useLanguage();
  return useMemo(() => {
    const regions = new Intl.DisplayNames([language], { type: 'region' });
    return (packId: string, fallback: string) => {
      if (packId === 'global') return t('rulePacks.globalPack');
      try {
        return regions.of(packId.toUpperCase()) ?? fallback;
      } catch {
        return fallback;
      }
    };
  }, [language, t]);
}

/** The desktop's rule queries (same keys, one shared cache) for the phone's Kategorizace page. */
export function useCategorizationRules() {
  const packs = useQuery({
    queryKey: ['rulePacks'],
    queryFn: () => categorizationApi.getRulePacks(),
  });
  const custom = useQuery({
    queryKey: ['customRules'],
    queryFn: () => categorizationApi.getCustomRules(),
  });
  const learned = useQuery({
    queryKey: ['learnedPayees'],
    queryFn: () => categorizationApi.getLearnedPayees(),
  });
  const hitCounts = useQuery({
    queryKey: ['ruleHits', HIT_DAYS],
    queryFn: () => categorizationApi.getRuleHitCounts(HIT_DAYS),
  });
  const hits = useMemo(() => hitMap(hitCounts.data ?? []), [hitCounts.data]);
  return { packs, custom, learned, hits };
}

/** The rules of one pack, for its sheet. */
export function usePackRules(packId: string | null) {
  return useQuery({
    queryKey: ['packRules', packId],
    queryFn: () => categorizationApi.getPackRules(packId ?? ''),
    enabled: packId !== null,
  });
}

/**
 * The page's writes, as the desktop makes them (`RulePacksCard`, `CategorizationRules`). Beyond
 * their own keys they refresh the queue and the pickers' counts (`categorizationOverview`, the
 * categorize queue), which read the same rules.
 */
export function useCategorizationMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('categorization');
  const { t: ts } = useTranslation('settings');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };
  const refreshRules = async () => {
    await Promise.all(
      [
        'rulePacks',
        'packRules',
        'customRules',
        'learnedPayees',
        'ruleHits',
        'categorizationOverview',
      ]
        .map((key) => queryClient.invalidateQueries({ queryKey: [key] }))
        .concat(queryClient.invalidateQueries({ queryKey: CATEGORIZE_QUEUE_KEY }))
    );
  };

  const setPackEnabled = useMutation({
    mutationFn: ({
      packs,
      packId,
      enabled,
    }: {
      packs: Parameters<typeof nextEnabledPacks>[0];
      packId: string;
      enabled: boolean;
    }) => categorizationApi.setRulePacksEnabled(nextEnabledPacks(packs, packId, enabled)),
    onSuccess: refreshRules,
    onError: (error: Error) => {
      toast.error(ts('rulePacks.updateFailed'), { description: translateApiError(error, tc) });
    },
  });
  const setPackRuleDisabled = useMutation({
    mutationFn: ({ ruleId, disabled }: { ruleId: string; disabled: boolean }) =>
      categorizationApi.setPackRuleDisabled(ruleId, disabled),
    onSuccess: async (_r, v) => {
      await refreshMoneyFlows(queryClient);
      toast(t(v.disabled ? 'packTab.ruleOff' : 'packTab.ruleOn'));
    },
    onError,
  });
  const setCustomRuleActive = useMutation({
    mutationFn: ({ rule, isActive }: { rule: CustomRule; isActive: boolean }) =>
      categorizationApi.updateCustomRule(rule.id, customRuleInput(rule, isActive)),
    onSuccess: async (_r, v) => {
      await refreshMoneyFlows(queryClient);
      toast(t(v.isActive ? 'customTab.activated' : 'customTab.deactivated'));
    },
    onError,
  });
  const deleteCustomRule = useMutation({
    mutationFn: (id: string) => categorizationApi.deleteCustomRule(id),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('customRules.deleteSuccess'));
    },
    onError,
  });
  const deleteLearnedPayee = useMutation({
    mutationFn: (id: string) => categorizationApi.deleteLearnedPayee(id),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('learnedRules.deleteSuccess'));
    },
    onError,
  });

  return {
    setPackEnabled,
    setPackRuleDisabled,
    setCustomRuleActive,
    deleteCustomRule,
    deleteLearnedPayee,
  };
}
