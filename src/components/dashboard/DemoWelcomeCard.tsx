import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Chip } from '@/components/ui/chip';
import { DEMO_OUTLINE, DemoBadge } from '@/components/common/DemoMark';
import { useAuth } from '@/hooks/use-auth';
import { cn } from '@/lib/utils';

/**
 * Remembers the closed welcome per demo: the value is the demo profile's `createdAt`, so a reset
 * (a new demo, a new `createdAt`) brings the card back. Browser storage only: the demo's database
 * has no place for it, and losing it only shows the card once more.
 */
const DISMISSED_KEY = 'moony-demo-welcome-dismissed';

function readDismissed(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

function writeDismissed(value: string) {
  try {
    localStorage.setItem(DISMISSED_KEY, value);
  } catch {
    /* private mode: the card comes back next time */
  }
}

const LINKS = [
  { key: 'cashflow', href: '/reports/cashflow', navKey: 'nav.cashflow' },
  { key: 'stocks', href: '/reports/stocks-analysis', navKey: 'nav.stocksAnalysis' },
  { key: 'projection', href: '/reports/projection', navKey: 'nav.projection' },
] as const;

/**
 * The Overview's one-time welcome in the demo profile (ADR 0037, prototype scene 2): who Alex is,
 * that anything may change, and three places where the app is strongest. Closing it is remembered
 * for this demo; nothing renders outside the demo.
 */
export function DemoWelcomeCard() {
  const { t } = useTranslation('dashboard');
  const { t: tc } = useTranslation('common');
  const { isDemo, user } = useAuth();
  const [, setLocation] = useLocation();
  const [dismissed, setDismissed] = useState(readDismissed);

  if (!isDemo || !user) return null;
  const demoId = String(user.createdAt);
  if (dismissed === demoId) return null;

  const links = LINKS.filter(
    (link) => link.key !== 'stocks' || user.menuPreferences?.investments !== false
  );

  return (
    <section
      className={cn(
        'mb-[17px] grid grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-3.5 rounded-r4 bg-well px-[18px] py-4',
        DEMO_OUTLINE
      )}
      aria-labelledby="demo-welcome-title"
      data-testid="demo-welcome"
    >
      <DemoBadge />
      <div>
        <h3 id="demo-welcome-title" className="m-0 mb-1 text-h3 text-ink">
          {t('demoWelcome.title')}
        </h3>
        <p className="m-0 mb-3 max-w-[720px] text-body leading-[1.5] text-ink-3">
          {t('demoWelcome.text')}
        </p>
        <div className="flex flex-wrap gap-2">
          {links.map((link) => (
            <Chip key={link.key} role="link" onClick={() => setLocation(link.href)}>
              {t(`demoWelcome.links.${link.key}`)}
              <small className="text-micro font-500 text-ink-4">{tc(link.navKey)}</small>
            </Chip>
          ))}
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={tc('buttons.close')}
        onClick={() => {
          writeDismissed(demoId);
          setDismissed(demoId);
        }}
      >
        <X />
      </Button>
    </section>
  );
}
