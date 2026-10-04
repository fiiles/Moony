import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { categorizationApi } from '@/lib/tauri-api';
import { queryClient } from '@/lib/queryClient';
import { useLanguage } from '@/i18n/I18nProvider';

/** Settings → Categorization → Balíčky pravidel: one row per country pack with a switch. */
export function RulePacksCard() {
  const { t } = useTranslation('settings');
  const { language } = useLanguage();

  const { data: packs = [] } = useQuery({
    queryKey: ['rulePacks'],
    queryFn: () => categorizationApi.getRulePacks(),
  });

  const toggleMutation = useMutation({
    mutationFn: (packIds: string[]) => categorizationApi.setRulePacksEnabled(packIds),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rulePacks'] });
      queryClient.invalidateQueries({ queryKey: ['packRules'] });
    },
    onError: (error: Error) => {
      toast.error(t('rulePacks.updateFailed'), { description: error.message });
    },
  });

  // Localized country names from the ISO code (pack ids are ISO 3166-1
  // alpha-2, except the synthetic "global" pack).
  const regionNames = new Intl.DisplayNames([language], { type: 'region' });
  const packLabel = (packId: string, fallback: string) => {
    if (packId === 'global') return t('rulePacks.globalPack');
    try {
      return regionNames.of(packId.toUpperCase()) ?? fallback;
    } catch {
      return fallback;
    }
  };

  const handleToggle = (packId: string, enabled: boolean) => {
    const enabledIds = packs.filter((p) => p.enabled).map((p) => p.packId);
    const next = enabled ? [...enabledIds, packId] : enabledIds.filter((id) => id !== packId);
    toggleMutation.mutate(next);
  };

  // Enabled packs first, then alphabetically by localized label
  const sorted = [...packs].sort((a, b) => {
    if (a.enabled !== b.enabled) return a.enabled ? -1 : 1;
    if (a.packId === 'global') return -1;
    if (b.packId === 'global') return 1;
    return packLabel(a.packId, a.country).localeCompare(packLabel(b.packId, b.country), language);
  });

  return (
    <SettingsCard title={t('rulePacks.title')} description={t('rulePacks.description')}>
      <div className="-mr-2 max-h-[360px] overflow-y-auto pr-2">
        {sorted.map((pack) => (
          <SettingsRow
            key={pack.packId}
            htmlFor={`pack-${pack.packId}`}
            label={packLabel(pack.packId, pack.country)}
            hint={t('rulePacks.rulesCount', { count: pack.ruleCount })}
            className="py-2.5"
          >
            <Switch
              id={`pack-${pack.packId}`}
              checked={pack.enabled}
              disabled={toggleMutation.isPending}
              onCheckedChange={(checked) => handleToggle(pack.packId, checked)}
            />
          </SettingsRow>
        ))}
      </div>
    </SettingsCard>
  );
}
