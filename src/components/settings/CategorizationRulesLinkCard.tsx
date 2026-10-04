import { useTranslation } from 'react-i18next';
import { Link } from 'wouter';
import { Button } from '@/components/ui/button';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';

/** Entry point to the full rules page (`/settings/categorization-rules`). */
export function CategorizationRulesLinkCard() {
  const { t } = useTranslation('settings');

  return (
    <SettingsCard
      title={t('categorizationRules.title')}
      description={t('categorizationRules.description')}
    >
      <SettingsRow
        label={t('categorizationRules.rowLabel')}
        hint={t('categorizationRules.rowHint')}
      >
        <Button asChild variant="outline" size="sm">
          <Link href="/settings/categorization-rules">{t('categorizationRules.open')} →</Link>
        </Button>
      </SettingsRow>
    </SettingsCard>
  );
}
