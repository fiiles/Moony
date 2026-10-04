/**
 * CoinGecko API key hint
 *
 * A dismissible inline alert on the Crypto page (design system §6 `.alert`).
 * Prices work without a key; a key only unlocks price history. Shown when the
 * user holds crypto, has no key and has not dismissed it (flag on the profile).
 */
import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Info } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { useAuth } from '@/hooks/use-auth';
import { authApi, priceApi } from '@/lib/tauri-api';

export function CoinGeckoApiKeyBanner() {
  const { t } = useTranslation('crypto');
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const [hidden, setHidden] = useState(false);

  // Same query key as Settings -> API keys, so saving a key there hides the banner
  const { data: apiKeys, isLoading } = useQuery({
    queryKey: ['api-keys'],
    queryFn: () => priceApi.getApiKeys(),
  });

  const hasKey = !!apiKeys?.coingecko?.trim();
  if (hidden || isLoading || hasKey || user?.coingeckoModalDismissed) return null;

  const dismiss = async () => {
    setHidden(true);
    try {
      await authApi.updateProfile({ coingeckoModalDismissed: true });
      queryClient.invalidateQueries({ queryKey: ['user-profile'] });
    } catch (error) {
      // The banner is already hidden for this visit; it may show again next time.
      console.error('Failed to store banner dismissal:', error);
    }
  };

  const goToSettings = async () => {
    await dismiss();
    setLocation('/settings/integrations');
  };

  return (
    <Alert className="mb-[17px]">
      <Info aria-hidden />
      <AlertTitle>{t('coingeckoBanner.title')}</AlertTitle>
      <AlertDescription>{t('coingeckoBanner.message')}</AlertDescription>
      <AlertActions>
        <Button variant="outline" size="sm" onClick={goToSettings}>
          {t('coingeckoBanner.goToSettings')}
        </Button>
        <Button variant="ghost" size="sm" onClick={dismiss}>
          {t('coingeckoBanner.dismiss')}
        </Button>
      </AlertActions>
    </Alert>
  );
}
