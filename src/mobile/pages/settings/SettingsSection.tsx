import { useTranslation } from 'react-i18next';
import { Redirect, useParams } from 'wouter';
import { SectionPage } from '@/mobile/components/SectionPage';
import { isSettingsSection, routes, type SettingsSection as Section } from '@/mobile/shell/routes';
import ProfileSection from './ProfileSection';
import SecuritySection from './SecuritySection';

/**
 * A page pushed on Nastavení (`/settings/:section`, spec §8.9): the route's section picks the page,
 * an unknown one goes back to Nastavení.
 */
export default function SettingsSection() {
  const { section } = useParams<{ section: string }>();
  if (!isSettingsSection(section)) return <Redirect to={routes.settings} />;

  switch (section) {
    case 'profile':
      return <ProfileSection />;
    case 'security':
      return <SecuritySection />;
    // Reminders, notifications and the backup come with Tasks 2 and 3 of Phase 5
    case 'reminders':
    case 'notifications':
    case 'backup':
      return <UnbuiltSection section={section} />;
  }
}

function UnbuiltSection({ section }: { section: Section }) {
  const { t } = useTranslation('mobile');
  return <SectionPage title={t(`settings.rows.${section}`)} lead={t('shell.placeholder')} />;
}
