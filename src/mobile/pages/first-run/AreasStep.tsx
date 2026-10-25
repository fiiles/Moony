import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { SwitchRow } from '@/mobile/components/SwitchRow';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { StepHead } from './StepHead';
import { caption, primaryButton, rowCard } from './styles';

/**
 * Co sledovat: the areas of the app, one switch row each. "Účty a cashflow" is always on; the
 * others can be left out and are changed later in Nastavení. With a banking country chosen, a
 * sentence says its categorization rules are switched on. There is no way back from here: the
 * database exists.
 */
export function AreasStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const { areas, setAreas, pending } = flow;

  return (
    <div data-testid="first-run-areas">
      <StepHead title={t('onboarding.areas.title')} lead={tm('firstRun.areasLead')} />

      <div className={rowCard}>
        <SwitchRow
          id="area-accounts"
          label={t('onboarding.areas.accounts.title')}
          sub={t('onboarding.areas.alwaysOn')}
          checked
          disabled
          onCheckedChange={() => undefined}
        />
        <SwitchRow
          id="area-investments"
          label={t('onboarding.areas.investments.title')}
          sub={t('onboarding.areas.investments.description')}
          checked={areas.investments}
          disabled={pending.areas}
          onCheckedChange={(on) => setAreas((current) => ({ ...current, investments: on }))}
        />
        <SwitchRow
          id="area-real-estate"
          label={t('onboarding.areas.realEstate.title')}
          sub={t('onboarding.areas.realEstate.description')}
          checked={areas.realEstate}
          disabled={pending.areas}
          onCheckedChange={(on) => setAreas((current) => ({ ...current, realEstate: on }))}
        />
        <SwitchRow
          id="area-liabilities"
          label={t('onboarding.areas.liabilities.title')}
          sub={t('onboarding.areas.liabilities.description')}
          checked={areas.liabilities}
          disabled={pending.areas}
          onCheckedChange={(on) => setAreas((current) => ({ ...current, liabilities: on }))}
        />
      </div>

      {flow.countryName && (
        <p className={`${caption} mb-4`}>
          {t('onboarding.areas.rulesTitle', { country: flow.countryName })}
        </p>
      )}

      <Button
        className={primaryButton}
        loading={pending.areas}
        disabled={pending.areas}
        onClick={() => void flow.saveAreas()}
      >
        {tc('buttons.continue')}
      </Button>
    </div>
  );
}
