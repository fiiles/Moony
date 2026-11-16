import { Briefcase, ChartColumn, Landmark, LayoutDashboard, type LucideIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'wouter';
import { cn } from '@/lib/utils';
import { tapTab } from '@/mobile/hooks/use-tab-memory';
import { routes, TABS, type TabId } from './routes';

const TAB_ICONS: Record<TabId, LucideIcon> = {
  overview: LayoutDashboard,
  finance: Landmark,
  cashflow: ChartColumn,
  wealth: Briefcase,
};

interface TabBarProps {
  active: TabId;
}

/**
 * Tab bar (spec §6 and §10.3, design system README §13, `.m-tabbar`): paper with a hairline on
 * top, 56 pt above the bottom safe area, Lucide 24 (stroke 1.75) and a 10 / 600 label; colour alone
 * marks the active tab (ink, the rest ink-4).
 * Switching tabs goes through the history, so Android back walks back through it. Each tab comes
 * back where it was left; a tap on the tab shown goes back to its root, or scrolls the root to
 * its top (`tapTab`, audit C3).
 */
export function TabBar({ active }: TabBarProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();

  return (
    <nav
      aria-label={tc('nav.main')}
      className="fixed inset-x-0 bottom-0 z-30 grid min-h-[calc(var(--m-tabbar-h)+var(--safe-bottom))] select-none grid-cols-4 border-t border-line bg-paper px-2.5 pb-[var(--safe-bottom)] pt-[7px] [-webkit-touch-callout:none]"
    >
      {TABS.map((tab) => {
        const Icon = TAB_ICONS[tab];
        const isActive = tab === active;
        return (
          <Link
            key={tab}
            href={routes[tab]}
            aria-current={isActive ? 'page' : undefined}
            // The link names the tab's root; the tap goes where the tab was left
            onClick={(event) => {
              event.preventDefault();
              tapTab(tab, (path) => setLocation(path));
            }}
            className={cn(
              'flex min-w-0 flex-col items-center gap-[3px] text-micro tracking-[-0.005em] [-webkit-touch-callout:none]',
              isActive ? 'text-ink' : 'text-ink-4'
            )}
          >
            <Icon aria-hidden className="size-6 flex-none" strokeWidth={1.75} />
            <span className="max-w-full text-center leading-[1.15] [overflow-wrap:anywhere]">
              {t(`tabs.${tab}`)}
            </span>
          </Link>
        );
      })}
    </nav>
  );
}
