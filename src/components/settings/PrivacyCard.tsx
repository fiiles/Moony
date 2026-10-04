import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { getConsent, setConsent, useAnalyticsAvailable } from '@/lib/analytics';

/** Settings → Account → Soukromí a analytika: the anonymous-usage consent. */
export function PrivacyCard() {
  const { t } = useTranslation('settings');
  const [analyticsEnabled, setAnalyticsEnabled] = useState(getConsent() ?? false);
  const available = useAnalyticsAvailable();
  // A build without an analytics key cannot send anything, so the switch is off and disabled.
  const unavailable = available === false;

  return (
    <SettingsCard title={t('privacy.title')} description={t('privacy.description')}>
      <SettingsRow
        htmlFor="analytics-consent"
        label={t('privacy.analytics')}
        hint={unavailable ? t('privacy.analyticsUnavailable') : t('privacy.analyticsHint')}
      >
        <Switch
          id="analytics-consent"
          checked={unavailable ? false : analyticsEnabled}
          disabled={unavailable}
          onCheckedChange={(checked) => {
            setConsent(checked);
            setAnalyticsEnabled(checked);
          }}
        />
      </SettingsRow>
    </SettingsCard>
  );
}
