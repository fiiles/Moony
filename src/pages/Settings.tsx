/**
 * Settings page: a frame with sub-navigation and one section per route
 * `/settings/categorization-rules` is a separate page and is routed before
 * this one in `src/App.tsx`.
 */
import { useLocation } from 'wouter';
import { SettingsLayout } from '@/components/settings/SettingsLayout';
import {
  resolveSettingsSection,
  type SettingsSectionId,
} from '@/components/settings/settings-sections';
import {
  AccountSection,
  CategorizationSection,
  DataSettingsSection,
  GeneralSection,
  IntegrationsSection,
  SecuritySection,
} from '@/components/settings/SettingsSectionPages';

const SECTION_PAGES: Record<SettingsSectionId, () => React.JSX.Element> = {
  general: GeneralSection,
  security: SecuritySection,
  data: DataSettingsSection,
  integrations: IntegrationsSection,
  categorization: CategorizationSection,
  account: AccountSection,
};

export default function SettingsPage() {
  const [location] = useLocation();
  const active = resolveSettingsSection(location);
  const Section = SECTION_PAGES[active];

  return (
    <SettingsLayout active={active}>
      <Section />
    </SettingsLayout>
  );
}
