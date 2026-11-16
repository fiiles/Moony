import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Check, X } from 'lucide-react';
import { useAuth } from '@/hooks/use-auth';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { onboardingApi } from '@/lib/tauri-api';
import { cn } from '@/lib/utils';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { exportsOnThisDevice } from '@/mobile/lib/export-file';
import { offerSyncLink } from '@/utils/getting-started';
import { phoneGettingStartedItems } from '@/mobile/lib/getting-started';
import { settingsPath } from '@/mobile/shell/routes';
import { syncIsOn } from '@/utils/safe-copy';

/** The progress the desktop card, Nastavení and the backup read: one cache entry. */
const PROGRESS_KEY = ['onboarding-progress'] as const;

/**
 * Začínáme (spec §8.3 item 7, the desktop's `GettingStartedCard`): the checklist of the first steps,
 * done ones ticked and struck through, each row opening the phone page where it is done. It shows
 * while something is left and until the person hides it (the desktop's flag, so hiding it here
 * hides it there too). On an empty Přehled it is the way to the first account.
 */
export function GettingStartedCard() {
  const { t } = useTranslation('dashboard');
  const { t: tm } = useTranslation('mobile');
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const featureOn = useDeviceSyncFeature();
  const { data: syncStatus } = useDeviceSyncStatus();
  const { data: progress } = useQuery({
    queryKey: PROGRESS_KEY,
    queryFn: () => onboardingApi.getProgress(),
    staleTime: 60_000,
  });

  if (!progress || progress.checklistDismissed) return null;
  const sync = { featureOn, syncOn: syncIsOn(featureOn, syncStatus) };
  const items = phoneGettingStartedItems(
    progress,
    user?.menuPreferences,
    exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? ''),
    sync
  );
  if (items.every((item) => item.done)) return null;
  const doneCount = items.filter((item) => item.done).length;

  const dismiss = async () => {
    try {
      await onboardingApi.setFlag('onboarding.checklistDismissed', '1');
    } finally {
      void queryClient.invalidateQueries({ queryKey: PROGRESS_KEY });
    }
  };

  return (
    <section aria-label={t('gettingStarted.title')}>
      <div className="mb-2 mt-1 flex items-start gap-2">
        <div className="min-w-0 flex-1 pl-0.5">
          <h2 className="m-0 text-m-section text-ink">
            {t('gettingStarted.title')}{' '}
            <span className="text-m-sub font-semibold text-ink-3">
              {doneCount}/{items.length}
            </span>
          </h2>
          <p className="m-0 mt-0.5 text-m-sub text-ink-3">{t('gettingStarted.subtitle')}</p>
        </div>
        <button
          type="button"
          aria-label={t('gettingStarted.dismiss')}
          onClick={() => void dismiss()}
          className="-mr-1.5 -mt-1.5 inline-flex size-11 flex-none items-center justify-center rounded-[12px] text-ink-3 active:bg-well-2 [&_svg]:size-5"
        >
          <X aria-hidden strokeWidth={1.75} />
        </button>
      </div>
      <EntityRows>
        {items.flatMap((item) => [
          <EntityRow
            key={item.key}
            chip={
              <span
                aria-hidden
                className={cn(
                  'flex size-9 flex-none items-center justify-center rounded-full border [&_svg]:size-4',
                  item.done
                    ? 'border-dark bg-dark text-ink-inverse'
                    : 'border-line-strong bg-paper text-transparent'
                )}
              >
                <Check strokeWidth={2.5} />
              </span>
            }
            title={
              <span className={cn(item.done && 'text-ink-3 line-through')}>
                {t(`gettingStarted.${item.key}`)}
              </span>
            }
            sub={[
              item.key === 'makeBackup' || item.key === 'protectData'
                ? tm(`gettingStarted.${item.key}Hint`)
                : t(`gettingStarted.${item.key}Hint`),
              item.optional && !item.done ? t('gettingStarted.optional') : null,
            ]
              .filter(Boolean)
              .join(' · ')}
            href={item.href}
            from={tm('tabs.overview')}
            wrap
            chevron
          />,
          ...(offerSyncLink(item, sync)
            ? [
                <EntityRow
                  key={`${item.key}-sync`}
                  title={t('gettingStarted.setUpSync')}
                  href={settingsPath('sync')}
                  from={tm('tabs.overview')}
                  wrap
                  chevron
                />,
              ]
            : []),
        ])}
      </EntityRows>
    </section>
  );
}
