import { Redirect, useParams } from 'wouter';
import { isSettingsSection, routes } from '@/mobile/shell/routes';
import BackupSection from './BackupSection';
import NotificationsSection from './NotificationsSection';
import ProfileSection from './ProfileSection';
import RemindersSection from './RemindersSection';
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
    case 'reminders':
      return <RemindersSection />;
    case 'notifications':
      return <NotificationsSection />;
    case 'backup':
      return <BackupSection />;
  }
}
