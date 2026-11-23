/**
 * Settings page: a frame with sub-navigation and one section per route
 * `/settings/categorization-rules` is a separate page and is routed before
 * this one in `src/App.tsx`.
 */
import { useLocation } from 'wouter';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';
import { useAuth } from '@/hooks/use-auth';
import { SyncSection } from '@/components/settings/sync/SyncSection';
import { SettingsLayout } from '@/components/settings/SettingsLayout';
import {
  isLockedSection,
  resolveSettingsSection,
  type SettingsSectionId,
} from '@/components/settings/settings-sections';
import {
  DemoSettingsSection,
  LockedInDemoSection,
} from '@/components/settings/DemoSettingsSection';
import {
  AccountSection,
  CategorizationSection,
  DataSettingsSection,
  GeneralSection,
  IntegrationsSection,
  SecuritySection,
} from '@/components/settings/SettingsSectionPages';

const SECTION_PAGES: Record<SettingsSectionId, () => React.JSX.Element> = {
  demo: DemoSettingsSection,
  general: GeneralSection,
  security: SecuritySection,
  data: DataSettingsSection,
  integrations: IntegrationsSection,
  sync: SyncSection,
  categorization: CategorizationSection,
  account: AccountSection,
};

export default function SettingsPage() {
  const [location] = useLocation();
  const syncFeature = useDeviceSyncFeature();
  const { isDemo } = useAuth();
  const active = resolveSettingsSection(location, syncFeature, isDemo);
  const Section = SECTION_PAGES[active];

  return (
    <SettingsLayout active={active}>
      {/* The demo lists what needs a real profile, but shows only why it is locked (ADR 0037) */}
      {isLockedSection(active, isDemo) ? <LockedInDemoSection id={active} /> : <Section />}
    </SettingsLayout>
  );
}
