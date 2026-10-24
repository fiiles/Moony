/**
 * Dashboard "Getting started" checklist. Done-state comes from
 * `get_onboarding_progress`, the items follow the areas the user chose
 * (`gettingStartedItems`); the card hides when everything is done or the
 * user dismisses it. The empty overview shows it too.
 */
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'wouter';
import { Archive, Check, Landmark, PiggyBank, TrendingUp, X, type LucideIcon } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { useAuth } from '@/hooks/use-auth';
import { onboardingApi } from '@/lib/tauri-api';
import { cn } from '@/lib/utils';
import { gettingStartedItems, type GettingStartedKey } from '@/utils/getting-started';

const ONBOARDING_PROGRESS_KEY = ['onboarding-progress'] as const;

const ICONS: Record<GettingStartedKey, LucideIcon> = {
  importStatement: Landmark,
  addInvestment: TrendingUp,
  setBudget: PiggyBank,
  makeBackup: Archive,
};

export function GettingStartedCard({ className }: { className?: string }) {
  const { t } = useTranslation('dashboard');
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const { data: progress } = useQuery({
    queryKey: ONBOARDING_PROGRESS_KEY,
    queryFn: () => onboardingApi.getProgress(),
    staleTime: 60_000,
  });

  if (!progress || progress.checklistDismissed) return null;

  const items = gettingStartedItems(progress, user?.menuPreferences);
  if (items.every((i) => i.done)) return null;

  const dismiss = async () => {
    try {
      await onboardingApi.setFlag('onboarding.checklistDismissed', '1');
    } finally {
      queryClient.invalidateQueries({ queryKey: ONBOARDING_PROGRESS_KEY });
    }
  };

  const doneCount = items.filter((i) => i.done).length;

  return (
    <Card variant="flat" className={cn('mb-[17px]', className)} data-testid="getting-started-card">
      <CardHeader className="items-start pb-2">
        <div>
          <CardTitle className="text-h3">{t('gettingStarted.title')}</CardTitle>
          <CardDescription>
            {t('gettingStarted.subtitle')} · {doneCount}/{items.length}
          </CardDescription>
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={dismiss}
          title={t('gettingStarted.dismiss')}
          aria-label={t('gettingStarted.dismiss')}
        >
          <X />
        </Button>
      </CardHeader>
      <ul
        className={cn(
          'm-0 grid list-none grid-cols-2 gap-x-6 px-[21px] pb-4',
          // the last row (one or two cells) has no rule under it
          items.length % 2 === 0
            ? '[&>li:nth-last-child(-n+2)]:border-0'
            : '[&>li:last-child]:border-0'
        )}
      >
        {items.map((item) => {
          const Icon = ICONS[item.key];
          return (
            <li key={item.key} className="border-b border-line-soft">
              <Link
                href={item.href}
                className={cn(
                  'grid grid-cols-[18px_1fr] items-start gap-3 py-3 text-ink focus-visible:outline-none focus-visible:shadow-focus',
                  item.done && 'opacity-55'
                )}
              >
                <span
                  className={cn(
                    'mt-px grid size-[18px] place-items-center rounded-full border',
                    item.done
                      ? 'border-dark bg-dark text-ink-inverse'
                      : 'border-line-strong bg-paper'
                  )}
                  aria-hidden
                >
                  {item.done && <Check className="size-3" strokeWidth={2.5} />}
                </span>
                <span className="min-w-0">
                  <span className="flex flex-wrap items-center gap-2 text-table font-650">
                    <Icon className="size-3.5 text-ink-3" strokeWidth={1.75} aria-hidden />
                    <span className={cn(item.done && 'line-through')}>
                      {t(`gettingStarted.${item.key}`)}
                    </span>
                    {item.optional && (
                      <Badge variant="outline">{t('gettingStarted.optional')}</Badge>
                    )}
                  </span>
                  <span className="mt-0.5 block text-micro font-500 text-ink-4">
                    {t(`gettingStarted.${item.key}Hint`)}
                  </span>
                </span>
              </Link>
            </li>
          );
        })}
      </ul>
    </Card>
  );
}
