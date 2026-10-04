import { useState } from 'react';
import { Link, useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import { ChevronDown, Info, ListFilter, Lock, Settings } from 'lucide-react';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { AboutModal } from '@/components/common/AboutModal';
import { useAuth } from '@/hooks/use-auth';
import { MoonMark } from '@/components/common/MoonMark';
import { cn } from '@/lib/utils';
import {
  NAV_GROUPS,
  OVERVIEW_ITEM,
  SETTINGS_ITEM,
  isGroupOpen,
  isNavItemActive,
  readGroupState,
  visibleItems,
  writeGroupState,
  type NavItem,
} from '@/components/common/nav-config';

/**
 * Sidebar (design system §5): continuous, no boxed groups. Rows 36 px,
 * 14/600, 16 px Lucide icons in ink-3; the active row is dark material.
 * Group titles are eyebrows with a chevron on hover; state persists per
 * group and Tools starts collapsed. The middle scrolls, Settings and the
 * account row stay pinned above a hairline.
 */
function NavRow({
  item,
  active,
  label,
  count,
}: {
  item: NavItem;
  active: boolean;
  label: string;
  count?: number;
}) {
  return (
    <Link
      href={item.url}
      aria-current={active ? 'page' : undefined}
      className={cn(
        'flex min-h-[var(--nav-row-h)] w-full items-center gap-[11px] rounded-r2 px-[11px] text-left text-nav text-ink-2 transition-[background,color] duration-fast hover:bg-ink-hover hover:text-ink focus-visible:outline-none focus-visible:shadow-focus [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-ink-3',
        active &&
          'bg-dark-grad text-ink-inverse shadow-dark hover:bg-dark-grad hover:text-ink-inverse [&_svg]:text-ink-inverse'
      )}
    >
      <item.icon strokeWidth={1.75} aria-hidden />
      <span className="truncate">{label}</span>
      {count !== undefined && (
        <span
          className={cn(
            'ml-auto text-micro font-700 text-ink-4 num',
            active && 'text-ink-inverse-2'
          )}
        >
          {count}
        </span>
      )}
    </Link>
  );
}

export function AppSidebar() {
  const [location] = useLocation();
  const { user, lockMutation } = useAuth();
  const { t } = useTranslation('common');
  const [aboutOpen, setAboutOpen] = useState(false);

  // Open/closed state per group, remembered across reloads (groups never
  // toggled follow the default: everything open except Tools).
  const [groupState, setGroupState] = useState<Record<string, boolean>>(() => readGroupState());
  const setGroupOpen = (id: string, open: boolean) => {
    const next = { ...groupState, [id]: open };
    setGroupState(next);
    writeGroupState(next);
  };

  const initials = `${user?.name?.charAt(0) ?? ''}${user?.surname?.charAt(0) ?? ''}`.toUpperCase();

  return (
    <aside className="sticky top-0 z-[2] flex h-screen flex-col border-r border-line-sidebar bg-sidebar-grad px-4 pb-4 pt-[25px] shadow-sidebar">
      <Link href="/" className="mb-[30px] flex items-center gap-[11px] rounded-r2 px-[10px] py-1">
        <MoonMark />
        <span className="block">
          <b className="block text-[18px] font-650 leading-tight tracking-[-0.08em] text-ink">
            moony
          </b>
          <small className="block text-micro font-500 text-ink-4">{t('app.tagline')}</small>
        </span>
      </Link>

      <nav className="-mx-1.5 min-h-0 flex-1 overflow-y-auto px-1.5" aria-label={t('nav.main')}>
        <div className="mb-[18px] grid gap-[3px]">
          <NavRow
            item={OVERVIEW_ITEM}
            active={isNavItemActive(location, OVERVIEW_ITEM.url)}
            label={t(OVERVIEW_ITEM.labelKey)}
          />
        </div>

        {NAV_GROUPS.map((group) => {
          const items = visibleItems(group, user?.menuPreferences);
          if (items.length === 0) return null;
          const containsActive = items.some((item) => isNavItemActive(location, item.url));
          const open = isGroupOpen(groupState, group.id, containsActive);
          return (
            <Collapsible
              key={group.id}
              open={open}
              onOpenChange={(next) => setGroupOpen(group.id, next)}
              className="group/nav mb-[18px]"
              data-nav-group={group.id}
            >
              <CollapsibleTrigger className="flex w-full items-center justify-between rounded-r1 px-[11px] pb-[7px] text-left text-eyebrow uppercase tracking-[0.1em] text-ink-4 focus-visible:outline-none focus-visible:shadow-focus">
                <span>{t(group.labelKey)}</span>
                <ChevronDown
                  aria-hidden
                  className={cn(
                    'size-3 opacity-0 transition-[opacity,transform] duration-fast group-hover/nav:opacity-100',
                    !open && '-rotate-90 opacity-100'
                  )}
                />
              </CollapsibleTrigger>
              <CollapsibleContent>
                <div className="grid gap-[3px]">
                  {items.map((item) => (
                    <NavRow
                      key={item.id}
                      item={item}
                      active={isNavItemActive(location, item.url)}
                      label={t(item.labelKey)}
                    />
                  ))}
                </div>
              </CollapsibleContent>
            </Collapsible>
          );
        })}
      </nav>

      <div className="mt-[10px] border-t border-line-sidebar pt-[10px]">
        <NavRow
          item={SETTINGS_ITEM}
          active={isNavItemActive(location, SETTINGS_ITEM.url)}
          label={t(SETTINGS_ITEM.labelKey)}
        />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="mt-1 flex w-full items-center gap-[9px] rounded-r2 p-[10px] text-left transition-colors duration-fast hover:bg-ink-hover focus-visible:outline-none focus-visible:shadow-focus"
            >
              <Avatar>
                <AvatarFallback>{initials}</AvatarFallback>
              </Avatar>
              <span className="min-w-0">
                <b className="block truncate text-table font-650 text-ink">
                  {user?.name} {user?.surname}
                </b>
                <small className="mt-0.5 block truncate text-micro font-500 text-ink-4">
                  {t('nav.accountRow')}
                </small>
              </span>
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="top" align="start" sideOffset={6} className="w-[232px]">
            <DropdownMenuItem asChild>
              <Link href="/settings/categorization-rules">
                <ListFilter />
                {t('nav.categorizationRules')}
              </Link>
            </DropdownMenuItem>
            <DropdownMenuItem asChild>
              <Link href="/settings">
                <Settings />
                {t('nav.settings')}
              </Link>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => setAboutOpen(true)}>
              <Info />
              {t('about.title')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onSelect={() => lockMutation.mutate()}
              disabled={lockMutation.isPending}
            >
              <Lock />
              {t('nav.lockApp')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <AboutModal open={aboutOpen} onOpenChange={setAboutOpen} />
    </aside>
  );
}
