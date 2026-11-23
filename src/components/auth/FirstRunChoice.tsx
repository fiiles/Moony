/**
 * The desktop first run's choice (ADR 0037, prototype `docs/specs/2026-11-23-demo-profile-prototype.html`
 * scenes 1 and 6): between the brand intro and the wizard, an own profile (the primary button) or
 * the demo, side by side at the same size. A stored demo turns its card into "Pokračovat v ukázce"
 * with when it was last opened, and offers to start again with the original data. The "already use
 * Moony" links lead where the wizard's own links lead.
 */
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, Lock, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { MoonMark } from '@/components/common/MoonMark';
import { RestoreBackup } from '@/components/common/RestoreBackup';
import { DemoAvatar } from '@/components/common/DemoMark';
import { useAuth } from '@/hooks/use-auth';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';
import { useFormat } from '@/lib/use-format';
import { DEMO_PERSONA, dayRelation } from '@/utils/demo-profile';

const TEXT_BUTTON =
  'font-650 text-ink underline underline-offset-[3px] hover:text-ink-2 disabled:pointer-events-none disabled:opacity-50';

const OWN_POINTS = ['password', 'currency', 'firstAccount'] as const;
const DEMO_POINTS = ['content', 'change', 'separate', 'prices'] as const;

export function FirstRunChoice({ language, meta }: { language: string; meta: string }) {
  const { t } = useTranslation('auth');
  const fmt = useFormat();
  const syncFeature = useDeviceSyncFeature();
  const { storedDemo, openDemoMutation, discardDemoMutation, enterWizard } = useAuth();
  const returning = storedDemo?.exists === true;
  const opening = openDemoMutation.isPending;
  const busy = opening || discardDemoMutation.isPending;

  const lastOpened = (() => {
    const at = storedDemo?.lastOpenedAt;
    if (at == null) return t('firstRun.demo.continueText');
    switch (dayRelation(at, new Date())) {
      case 'today':
        return t('firstRun.demo.lastOpenedToday', { time: fmt.time(at) });
      case 'yesterday':
        return t('firstRun.demo.lastOpenedYesterday', { time: fmt.time(at) });
      default:
        return t('firstRun.demo.lastOpenedOn', { date: fmt.dateTime(at) });
    }
  })();

  const ownPoints: string[] = OWN_POINTS.map((key) => t(`firstRun.own.points.${key}`));
  if (syncFeature) ownPoints.push(t('firstRun.own.points.devices'));

  return (
    <section
      className="relative grid min-h-svh place-items-center px-6 pb-16 pt-[76px]"
      data-testid="first-run-choice"
    >
      <div className="w-full max-w-[840px]">
        <div className="mb-[30px] flex items-center justify-center gap-[11px]">
          <MoonMark />
          <div>
            <b className="block text-[18px] font-650 leading-tight tracking-[-0.08em] text-ink">
              moony
            </b>
            <small className="block text-micro font-500 text-ink-4">
              {t('onboarding.brandTagline')}
            </small>
          </div>
        </div>
        <h1 className="m-0 text-center text-[30px] font-650 tracking-[-0.05em] text-ink">
          {t('firstRun.title')}
        </h1>
        <p className="mx-auto mb-8 mt-2.5 max-w-[540px] text-center text-[14px] leading-[1.5] text-ink-3">
          {t('firstRun.lead')}
        </p>

        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          {/* Own profile: the primary path */}
          <Card className="flex flex-col px-6 pb-[22px] pt-6">
            <h2 className="m-0 mb-1.5 text-[19px] font-650 tracking-[-0.045em] text-ink">
              {t('firstRun.own.title')}
            </h2>
            <p className="m-0 mb-4 text-body leading-[1.5] text-ink-3">{t('firstRun.own.text')}</p>
            <div className="mb-4 flex items-center gap-2.5 rounded-r3 bg-well px-3 py-[11px]">
              <span className="grid size-[29px] shrink-0 place-items-center rounded-full bg-well-3 text-ink-2">
                <Lock className="size-4" aria-hidden />
              </span>
              <div className="min-w-0">
                <b className="block text-table font-650 text-ink">{t('firstRun.own.peekTitle')}</b>
                <small className="mt-0.5 block text-micro font-500 text-ink-4">
                  {t('firstRun.own.peekText')}
                </small>
              </div>
            </div>
            <Points items={ownPoints} />
            <Button
              className="mt-auto h-10 text-body"
              disabled={busy}
              onClick={() => enterWizard({ from: 'choice', path: 'new' })}
              data-testid="first-run-own-profile"
            >
              {t('firstRun.own.button')}
            </Button>
            <p className="m-0 mt-2.5 min-h-[18px]" />
          </Card>

          {/* The demo: Alex Meyer's synthetic data, no password */}
          <Card className="flex flex-col px-6 pb-[22px] pt-6">
            <h2 className="m-0 mb-1.5 text-[19px] font-650 tracking-[-0.045em] text-ink">
              {returning ? t('firstRun.demo.continueTitle') : t('firstRun.demo.title')}
            </h2>
            <p className="m-0 mb-4 text-body leading-[1.5] text-ink-3">
              {returning ? lastOpened : t('firstRun.demo.text')}
            </p>
            <div className="mb-4 flex items-center gap-2.5 rounded-r3 bg-well px-3 py-[11px]">
              <DemoAvatar initials={DEMO_PERSONA.initials} />
              <div className="min-w-0">
                <b className="block text-table font-650 text-ink">{DEMO_PERSONA.name}</b>
                <small className="mt-0.5 block text-micro font-500 text-ink-4">
                  {t('firstRun.demo.peekText')}
                </small>
              </div>
            </div>
            <Points items={DEMO_POINTS.map((key) => t(`firstRun.demo.points.${key}`))} />
            <Button
              variant="outline"
              className="mt-auto h-10 text-body"
              disabled={busy}
              loading={opening}
              onClick={() => openDemoMutation.mutate(language)}
              data-testid="first-run-open-demo"
            >
              {opening && <RefreshCw aria-hidden />}
              {opening
                ? t('firstRun.demo.opening')
                : returning
                  ? t('firstRun.demo.continue')
                  : t('firstRun.demo.open')}
            </Button>
            <p className="m-0 mt-2.5 min-h-[18px] text-center text-caption">
              {returning && (
                <button
                  type="button"
                  className={TEXT_BUTTON}
                  disabled={busy}
                  onClick={() =>
                    discardDemoMutation.mutate(undefined, {
                      onSuccess: () => toast(t('firstRun.demo.restarted')),
                    })
                  }
                  data-testid="first-run-restart-demo"
                >
                  {t('firstRun.demo.restart')}
                </button>
              )}
            </p>
          </Card>
        </div>

        {/* Already using Moony: the wizard's own two ways in, as a shortcut */}
        <RestoreBackup fresh>
          {(pick, restoring) => (
            <p className="m-0 mt-[22px] flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-center text-caption font-500 text-ink-3">
              <span>
                {syncFeature
                  ? t('onboarding.welcome.otherDeviceLead')
                  : t('onboarding.welcome.restoreLead')}
              </span>
              {syncFeature && (
                <button
                  type="button"
                  className={TEXT_BUTTON}
                  disabled={busy || restoring}
                  onClick={() => enterWizard({ from: 'choice', path: 'join' })}
                  data-testid="first-run-join-sync"
                >
                  {t('onboarding.welcome.joinSync')}
                </button>
              )}
              <button
                type="button"
                className={TEXT_BUTTON}
                disabled={busy || restoring}
                onClick={pick}
                data-testid="first-run-restore-backup"
              >
                {t('onboarding.welcome.restoreBackup')}
              </button>
            </p>
          )}
        </RestoreBackup>
      </div>
      <div className="absolute inset-x-0 bottom-[18px] text-center text-micro font-500 text-ink-5">
        {meta}
      </div>
    </section>
  );
}

function Points({ items }: { items: string[] }) {
  return (
    <ul className="m-0 mb-[22px] grid list-none gap-2 p-0 text-table font-500 text-ink-2">
      {items.map((item) => (
        <li key={item} className="flex items-start gap-2">
          <Check className="mt-px size-3.5 shrink-0 text-ink-4" aria-hidden />
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}
