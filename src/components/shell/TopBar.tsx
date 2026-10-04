import { Fragment } from 'react';
import { Link, useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import { findNavLocation, SETTINGS_ITEM } from '@/components/common/nav-config';
import { resolveSettingsSection } from '@/components/settings/settings-sections';
import { useLanguage } from '@/i18n/I18nProvider';
import { DataStatus } from '@/components/shell/DataStatus';
import { UpdateStatus } from '@/components/shell/UpdateStatus';
import { HistoryRecalculationBadge } from '@/components/common/HistoryRecalculationBadge';
import { useShell } from '@/components/shell/shell-context';

interface Crumb {
  label: string;
  href?: string;
  /** `strong` is the current page; `tag` a lowercase descriptor after it. */
  kind: 'link' | 'strong' | 'tag';
}

/**
 * Top bar (design system §5): breadcrumb left, data status right, 40 px
 * below. List pages read "Peníze / bankovní účty", details
 * "Investice / Akcie / Běžný účet", settings "Nastavení / obecné".
 */
export function TopBar() {
  const [location] = useLocation();
  const { t } = useTranslation('common');
  const { t: ts } = useTranslation('settings');
  const { getLocale } = useLanguage();
  const { page } = useShell();
  const lower = (s: string) => s.toLocaleLowerCase(getLocale());

  const nav = findNavLocation(location);
  const crumbs: Crumb[] = [];
  if (!nav) {
    crumbs.push({ label: t('app.name'), kind: 'strong' });
  } else if (nav.item.id === 'overview') {
    crumbs.push({ label: t(nav.item.labelKey), kind: 'strong' });
    crumbs.push({ label: t('nav.overviewTagline'), kind: 'tag' });
  } else if (nav.item.id === SETTINGS_ITEM.id) {
    crumbs.push({ label: t(nav.item.labelKey), kind: 'strong', href: nav.item.url });
    if (page.crumb) crumbs.push({ label: page.crumb, kind: 'strong' });
    else
      crumbs.push({
        label: lower(ts(`sections.${resolveSettingsSection(location)}`)),
        kind: 'tag',
      });
  } else if (nav.isDetail && page.crumb) {
    if (nav.group) crumbs.push({ label: t(nav.group.labelKey), kind: 'link', href: nav.item.url });
    crumbs.push({ label: t(nav.item.labelKey), kind: 'link', href: nav.item.url });
    crumbs.push({ label: page.crumb, kind: 'strong' });
  } else {
    if (nav.group) crumbs.push({ label: t(nav.group.labelKey), kind: 'link', href: nav.item.url });
    crumbs.push({ label: lower(t(nav.item.labelKey)), kind: 'tag' });
  }

  return (
    <div className="mb-10 flex items-center justify-between gap-6 text-table text-ink-3">
      <nav aria-label={t('nav.breadcrumb')} className="flex min-w-0 items-center gap-2">
        {crumbs.map((crumb, i) => (
          <Fragment key={`${crumb.kind}-${i}`}>
            {i > 0 && (
              <i aria-hidden className="not-italic text-ink-5">
                /
              </i>
            )}
            {crumb.kind === 'strong' ? (
              crumb.href ? (
                <Link href={crumb.href} className="truncate font-650 text-ink-2 hover:text-ink">
                  {crumb.label}
                </Link>
              ) : (
                <b className="truncate font-650 text-ink-2">{crumb.label}</b>
              )
            ) : crumb.href ? (
              <Link href={crumb.href} className="truncate hover:text-ink">
                {crumb.label}
              </Link>
            ) : (
              <span className="truncate">{crumb.label}</span>
            )}
          </Fragment>
        ))}
      </nav>
      <div className="flex shrink-0 items-center gap-4">
        <HistoryRecalculationBadge />
        <UpdateStatus />
        <DataStatus override={page.status} />
      </div>
    </div>
  );
}
