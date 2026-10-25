import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useBack, useBackLabel } from '@/mobile/hooks/use-back';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

interface SectionPageProps {
  title: string;
  /** The one sentence under the nav bar, saying what the page is for. */
  lead?: string;
  children?: ReactNode;
}

/**
 * A page pushed on Nastavení (`/settings/:section`): the screen without the tab bar and a back
 * button to Nastavení, the lead as the first line of the body.
 */
export function SectionPage({ title, lead, children }: SectionPageProps) {
  const { t: tc } = useTranslation('common');
  const back = useBack(routes.settings);
  // The screen the push left when it came from another tab (a property's loan), else the parent
  const backText = useBackLabel(tc('nav.settings'));
  return (
    <Screen
      title={title}
      tabBar={false}
      leading={<NavTextButton label={backText} onClick={back} chevron />}
    >
      {lead && <p className="mx-0.5 mb-3.5 mt-1 text-m-lead text-ink-3">{lead}</p>}
      {children}
    </Screen>
  );
}
