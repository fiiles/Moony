/**
 * The phone's first run (spec §8.2, ruling R11): the desktop wizard's five steps in one column and
 * without the live preview, then Biometrie and Záloha, in the "explain first, then the system
 * prompt" pattern. `Lock.tsx` mounts it while the first run lasts (the language menu floats above
 * it); the flow is `use-first-run.ts`, the order `lib/first-run.ts`, one file per step in
 * `first-run/`. The frame is the lock page's: it scrolls inside itself (the document never does),
 * clears the status bar and the home indicator, and starts every step at its top.
 */
import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { MoonMark } from '@/components/common/MoonMark';
import { useAuth } from '@/hooks/use-auth';
import { StepProgress } from '@/mobile/components/StepProgress';
import { useFirstRun } from '@/mobile/hooks/use-first-run';
import { stepIndex, type FirstRunStep } from '@/mobile/lib/first-run';
import { AccountStep } from '@/mobile/pages/first-run/AccountStep';
import { AreasStep } from '@/mobile/pages/first-run/AreasStep';
import { BackupStep } from '@/mobile/pages/first-run/BackupStep';
import { BiometricsStep } from '@/mobile/pages/first-run/BiometricsStep';
import { FirstAccountStep } from '@/mobile/pages/first-run/FirstAccountStep';
import { PrivacyStep } from '@/mobile/pages/first-run/PrivacyStep';
import { RecoveryStep } from '@/mobile/pages/first-run/RecoveryStep';

interface FirstRunProps {
  /** The UI language (the country names follow it; it is saved with the profile). */
  language: string;
  /** Called once the last step is done; `Lock.tsx` then leaves for the overview. */
  onFinished: () => void;
}

export function FirstRun({ language, onFinished }: FirstRunProps) {
  const { t } = useTranslation('auth');
  const { t: tm } = useTranslation('mobile');
  const { recoveryKey } = useAuth();
  const flow = useFirstRun({ language, onFinished });
  const { step } = flow;
  const scroller = useRef<HTMLDivElement>(null);

  // A step opens at its top, not where the last one was scrolled to
  useEffect(() => {
    scroller.current?.scrollTo({ top: 0 });
  }, [step]);

  const stepName: Record<FirstRunStep, string> = {
    privacy: t('onboarding.steps.welcome'),
    account: t('onboarding.steps.account'),
    recovery: t('onboarding.steps.recovery'),
    areas: t('onboarding.steps.areas'),
    firstAccount: t('onboarding.steps.firstAccount'),
    biometrics: tm('firstRun.steps.biometrics'),
    backup: tm('firstRun.steps.backup'),
  };
  const { index, total } = stepIndex(flow.steps, step);

  return (
    <div
      ref={scroller}
      className="h-full overflow-y-auto overscroll-contain bg-canvas text-m-body text-ink-2"
    >
      {/* The top 44 pt belong to the language menu that `Lock.tsx` floats over the first run */}
      <div className="mx-auto w-full max-w-[480px] px-5 pb-[calc(2rem+var(--safe-bottom))] pt-[calc(var(--safe-top)+3.5rem)]">
        <div className="mb-6 flex items-center gap-[11px]">
          <MoonMark />
          <div>
            <b className="block text-m-title leading-tight text-ink">moony</b>
            <small className="block text-m-caption text-ink-4">
              {t('onboarding.brandTagline')}
            </small>
          </div>
        </div>
        <StepProgress
          index={index}
          total={total}
          label={tm('firstRun.stepLabel', { index, total, step: stepName[step] })}
        />

        {step === 'privacy' && <PrivacyStep flow={flow} />}
        {step === 'account' && <AccountStep flow={flow} />}
        {step === 'recovery' && recoveryKey && (
          <RecoveryStep flow={flow} recoveryKey={recoveryKey} />
        )}
        {step === 'areas' && <AreasStep flow={flow} />}
        {step === 'firstAccount' && <FirstAccountStep flow={flow} />}
        {step === 'biometrics' && <BiometricsStep flow={flow} />}
        {step === 'backup' && <BackupStep flow={flow} />}
      </div>
    </div>
  );
}
