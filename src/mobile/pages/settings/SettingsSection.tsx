import { Redirect, useParams } from 'wouter';
import { isSettingsSection, routes } from '@/mobile/shell/routes';
import CategoriesSection from './CategoriesSection';
import CategorizationSection from './CategorizationSection';
import BackupSection from './BackupSection';
import ImportSection from './ImportSection';
import NotificationsSection from './NotificationsSection';
import ProfileSection from './ProfileSection';
import RemindersSection from './RemindersSection';
import SecuritySection from './SecuritySection';
import TrackingSection from './TrackingSection';

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
    case 'tracking':
      return <TrackingSection />;
    case 'security':
      return <SecuritySection />;
    case 'reminders':
      return <RemindersSection />;
    case 'notifications':
      return <NotificationsSection />;
    case 'backup':
      return <BackupSection />;
    case 'categories':
      return <CategoriesSection />;
    case 'categorization':
      return <CategorizationSection />;
    case 'import':
      return <ImportSection />;
  }
}
