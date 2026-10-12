import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowRight } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { MoonMark } from '@/components/common/MoonMark';
import './onboarding.css';

/** A first-run brand moment. It never delays users who request reduced motion. */
export function OnboardingWelcome({ onContinue }: { onContinue: () => void }) {
  const { t } = useTranslation('auth');

  useEffect(() => {
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    if (motion.matches) {
      onContinue();
      return;
    }

    const timer = window.setTimeout(onContinue, 2600);
    const handleMotionChange = () => {
      if (motion.matches) onContinue();
    };
    motion.addEventListener('change', handleMotionChange);
    return () => {
      window.clearTimeout(timer);
      motion.removeEventListener('change', handleMotionChange);
    };
  }, [onContinue]);

  return (
    <section
      className="onboarding-intro relative flex min-h-[calc(100svh-64px)] flex-col items-center justify-center overflow-hidden px-6 pb-24 text-center"
      data-testid="onboarding-intro"
    >
      <div className="onboarding-intro-content relative flex flex-col items-center">
        <div className="relative mb-10 grid size-36 place-items-center" aria-hidden>
          <span className="onboarding-intro-orbit absolute inset-0 rounded-full border border-line-strong" />
          <span className="onboarding-intro-halo absolute inset-4 rounded-full bg-canvas-glow" />
          <div className="onboarding-intro-moon">
            <MoonMark size={36} className="scale-[2]" />
          </div>
        </div>
        <span className="onboarding-intro-brand text-[20px] font-650 tracking-[-0.08em] text-ink">
          moony
        </span>
        <h1 className="onboarding-intro-title mb-0 mt-5 text-[clamp(28px,4vw,42px)] font-650 leading-tight tracking-[-0.05em] text-ink">
          {t('onboarding.intro.title')}
        </h1>
        <p className="onboarding-intro-tagline mb-0 mt-3 text-body text-ink-3">
          {t('onboarding.intro.tagline')}
        </p>
      </div>
      <Button
        variant="ghost"
        onClick={onContinue}
        className="absolute bottom-10"
        data-testid="onboarding-intro-skip"
      >
        {t('onboarding.intro.skip')}
        <ArrowRight aria-hidden />
      </Button>
    </section>
  );
}
