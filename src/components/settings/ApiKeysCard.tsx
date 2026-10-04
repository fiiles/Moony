import { useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Eye, EyeOff } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { LabelHint } from '@/components/ui/label';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { priceApi } from '@/lib/tauri-api';
import { queryClient } from '@/lib/queryClient';

/** Settings → Integrations → API klíče: the optional CoinGecko key for crypto prices. */
export function ApiKeysCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const [showCoingecko, setShowCoingecko] = useState(false);
  // Track user edits separately - undefined means user hasn't edited yet
  const [editedCoingeckoKey, setEditedCoingeckoKey] = useState<string | undefined>(undefined);

  const { data: apiKeys } = useQuery({
    queryKey: ['api-keys'],
    queryFn: () => priceApi.getApiKeys(),
  });

  // Use edited value if user has made changes, otherwise use fetched value
  const coingeckoKey = editedCoingeckoKey ?? apiKeys?.coingecko ?? '';

  const saveApiKeysMutation = useMutation({
    mutationFn: async () => {
      await priceApi.setApiKeys({
        coingecko: coingeckoKey || undefined,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['api-keys'] });
      setEditedCoingeckoKey(undefined);
      toast(t('apiKeys.saved'));
    },
    onError: (error: Error) => {
      toast.error(t('toast.updateFailed'), { description: error.message });
    },
  });

  return (
    <SettingsCard title={t('apiKeys.title')} description={t('apiKeys.description')}>
      <SettingsRow
        htmlFor="coingecko-api-key"
        label={
          <>
            {t('apiKeys.coingecko.label')} <LabelHint>{t('apiKeys.optional')}</LabelHint>
          </>
        }
        hint={`${t('apiKeys.coingecko.hint')} ${t('apiKeys.coingecko.hint2')}`}
      >
        <a
          href="https://www.coingecko.com/en/api"
          target="_blank"
          rel="noopener noreferrer"
          className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
        >
          {t('apiKeys.coingecko.getDemoKey')} ↗
        </a>
      </SettingsRow>
      <div className="mt-3 flex items-center gap-2">
        <Input
          id="coingecko-api-key"
          type={showCoingecko ? 'text' : 'password'}
          value={coingeckoKey}
          onChange={(e) => setEditedCoingeckoKey(e.target.value)}
          placeholder={t('apiKeys.coingecko.placeholder')}
          autoComplete="off"
          className="max-w-[420px] font-mono"
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          aria-label={showCoingecko ? t('apiKeys.hide') : t('apiKeys.show')}
          onClick={() => setShowCoingecko(!showCoingecko)}
        >
          {showCoingecko ? <EyeOff /> : <Eye />}
        </Button>
        <Button
          size="sm"
          onClick={() => saveApiKeysMutation.mutate()}
          loading={saveApiKeysMutation.isPending}
          disabled={editedCoingeckoKey === undefined}
        >
          {tc('buttons.save')}
        </Button>
      </div>
    </SettingsCard>
  );
}
