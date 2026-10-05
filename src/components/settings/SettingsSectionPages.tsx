import { DataSection } from '@/components/settings/DataSection';
import { CategoriesCard } from '@/components/settings/CategoriesCard';
import { ProfileCard } from '@/components/settings/ProfileCard';
import { LanguageCard } from '@/components/settings/LanguageCard';
import { DashboardPreferencesCard } from '@/components/settings/DashboardPreferencesCard';
import { MilestoneSettingsCard } from '@/components/settings/MilestoneSettingsCard';
import { MenuVisibilityCard } from '@/components/settings/MenuVisibilityCard';
import { ChangePasswordCard } from '@/components/settings/ChangePasswordCard';
import { AutoLockSlot } from '@/components/settings/AutoLockSlot';
import { UpdatesCard } from '@/components/settings/UpdatesCard';
import { ApiKeysCard } from '@/components/settings/ApiKeysCard';
import { McpServerCard } from '@/components/settings/McpServerCard';
import { RulePacksCard } from '@/components/settings/RulePacksCard';
import { CategorizationRulesLinkCard } from '@/components/settings/CategorizationRulesLinkCard';
import { DangerZoneCard } from '@/components/settings/DangerZoneCard';

/** /settings */
export function GeneralSection() {
  return (
    <>
      <ProfileCard />
      <LanguageCard />
      <DashboardPreferencesCard />
      <MilestoneSettingsCard />
      <MenuVisibilityCard />
    </>
  );
}

/** /settings/security */
export function SecuritySection() {
  return (
    <>
      <ChangePasswordCard />
      <AutoLockSlot />
      <UpdatesCard />
    </>
  );
}

/** /settings/data */
export function DataSettingsSection() {
  return <DataSection />;
}

/** /settings/integrations */
export function IntegrationsSection() {
  return (
    <>
      <ApiKeysCard />
      <McpServerCard />
    </>
  );
}

/** /settings/categorization */
export function CategorizationSection() {
  return (
    <>
      <RulePacksCard />
      <CategoriesCard />
      <CategorizationRulesLinkCard />
    </>
  );
}

/** /settings/account */
export function AccountSection() {
  return <DangerZoneCard />;
}
