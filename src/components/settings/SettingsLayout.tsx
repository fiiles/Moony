import { useEffect, useRef, type ReactNode } from 'react';
import { Link } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Lock } from 'lucide-react';
import { dataApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { formatBytes } from '@/utils/format-bytes';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { cn } from '@/lib/utils';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';
import { useAuth } from '@/hooks/use-auth';
import { DemoBadge } from '@/components/common/DemoMark';
import {
  isLockedSection,
  visibleSettingsSections,
  type SettingsSectionId,
} from '@/components/settings/settings-sections';

/**
 * Settings frame (design system, prototype `settings.html`): page head with
 * the section's one-sentence description, a sticky 200 px sub-navigation on
 * the left and the section's stacked flat cards on the right. In the demo profile the list starts
 * with "Ukázka" (its badge) and the sections that need a real profile carry a lock (ADR 0037).
 */
export function SettingsLayout({
  active,
  children,
}: {
  active: SettingsSectionId;
  children: ReactNode;
}) {
  const { t } = useTranslation('settings');
  const fmt = useFormat();
  const syncFeature = useDeviceSyncFeature();
  const { isDemo } = useAuth();
  const contentRef = useRef<HTMLDivElement>(null);

  // The scroll container is <main>; start every sub-page at the top unless a
  // link targets an anchor on it (e.g. #mcp).
  useEffect(() => {
    if (window.location.hash) return;
    contentRef.current?.closest('main')?.scrollTo({ top: 0 });
  }, [active]);

  const { data: location } = useQuery({
    queryKey: ['data-location'],
    queryFn: () => dataApi.getDataLocation(),
  });
  useShellPage({
    status: location
      ? { text: t('status', { size: formatBytes(location.totalBytes, fmt.locale) }), tone: 'fresh' }
      : undefined,
  });

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={
          isLockedSection(active, isDemo)
            ? t('sections.description.lockedInDemo')
            : t(`sections.description.${active}`)
        }
        className="mb-[30px]"
      />
      <div className="grid grid-cols-[200px_minmax(0,1fr)] items-start gap-8">
        <nav aria-label={t('sections.navLabel')} className="sticky top-8 grid gap-0.5">
          {visibleSettingsSections(syncFeature, isDemo).map((section) => {
            const isActive = section.id === active;
            return (
              <Link
                key={section.id}
                href={section.path}
                aria-current={isActive ? 'page' : undefined}
                className={cn(
                  'flex items-center gap-2 rounded-r2 px-[11px] py-2 text-body font-600 transition-colors duration-fast focus-visible:outline-none focus-visible:shadow-focus',
                  isActive ? 'bg-well-2 text-ink' : 'text-ink-2 hover:bg-well hover:text-ink',
                  section.locked && !isActive && 'text-ink-4'
                )}
              >
                {section.id === 'demo' ? <DemoBadge /> : t(section.labelKey)}
                {section.locked && (
                  <Lock aria-hidden className="ml-auto size-[13px] shrink-0 text-ink-5" />
                )}
              </Link>
            );
          })}
        </nav>
        <div ref={contentRef} className="min-w-0">
          {children}
        </div>
      </div>
    </>
  );
}
