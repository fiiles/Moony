import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { badgeVariants } from '@/components/ui/badge';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { DEMO_OUTLINE } from '@/components/common/DemoMark';
import { ResetDemoDialog } from '@/components/demo/ResetDemoDialog';
import { useAuth } from '@/hooks/use-auth';
import { cn } from '@/lib/utils';

/**
 * The top bar's "Ukázková data" (ADR 0037, prototype scene 2): a dashed badge button before the
 * rule; its popover says the data is made up and offers the reset and the way to an own profile.
 * Nothing renders outside the demo.
 */
export function DemoTopBarBadge() {
  const { t } = useTranslation('common');
  const { isDemo, closeDemoMutation } = useAuth();
  const [open, setOpen] = useState(false);
  const [resetOpen, setResetOpen] = useState(false);
  if (!isDemo) return null;

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            className={cn(
              badgeVariants({ variant: 'outline' }),
              DEMO_OUTLINE,
              'cursor-pointer text-ink-2 transition-colors duration-fast hover:border-ink-4 hover:text-ink focus-visible:outline-none focus-visible:shadow-focus'
            )}
            data-testid="demo-topbar-badge"
          >
            {t('demo.topBadge')}
          </button>
        </PopoverTrigger>
        <PopoverContent align="end" sideOffset={8} className="w-[320px] text-table text-ink-2">
          <b className="mb-1 block text-body font-650 text-ink">{t('demo.popoverTitle')}</b>
          <p className="m-0 mb-3 leading-[1.5] text-ink-3">{t('demo.popoverText')}</p>
          <div className="flex flex-wrap justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setOpen(false);
                setResetOpen(true);
              }}
            >
              {t('demo.reset.title')}
            </Button>
            <Button
              size="sm"
              disabled={closeDemoMutation.isPending}
              onClick={() => closeDemoMutation.mutate('ownProfile')}
            >
              {t('demo.createProfile')}
            </Button>
          </div>
        </PopoverContent>
      </Popover>
      <ResetDemoDialog open={resetOpen} onOpenChange={setResetOpen} />
    </>
  );
}
