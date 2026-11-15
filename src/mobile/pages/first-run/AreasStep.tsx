import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { SwitchRow } from '@/mobile/components/SwitchRow';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { StepHead } from './StepHead';
import { caption, field48, fieldLabel, primaryButton, rowCard } from './styles';

/**
 * Co sledovat: the areas of the app, one switch row each. "Účty a cashflow" is always on; the
 * others can be left out and are changed later in Nastavení. With a banking country chosen, a
 * sentence says its categorization rules are switched on. There is no way back from here: the
 * database exists. After Připojení left without sync (the declined path) the profile has no name
 * yet: name, surname and an optional e-mail are asked above the areas, and "Pokračovat" checks them.
 */
export function AreasStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const { areas, setAreas, pending } = flow;
  const fieldId = useId();
  const names = flow.profileName;
  const setName = (key: keyof typeof names) => (value: string) =>
    flow.setProfileName({ ...names, [key]: value });

  return (
    <div data-testid="first-run-areas">
      <StepHead title={t('onboarding.areas.title')} lead={tm('firstRun.areasLead')} />

      {flow.path === 'declined' && (
        <div className="mb-5 space-y-3" data-testid="first-run-areas-name">
          <p className={caption}>{t('onboarding.areas.nameLead')}</p>
          <div>
            <label htmlFor={`${fieldId}-name`} className={fieldLabel}>
              {t('onboarding.account.name')}
            </label>
            <Input
              id={`${fieldId}-name`}
              value={names.name}
              onChange={(event) => setName('name')(event.target.value)}
              autoComplete="given-name"
              autoCapitalize="words"
              autoCorrect="off"
              enterKeyHint="next"
              className={field48}
              readOnly={pending.areas}
            />
          </div>
          <div>
            <label htmlFor={`${fieldId}-surname`} className={fieldLabel}>
              {t('onboarding.account.surname')}
            </label>
            <Input
              id={`${fieldId}-surname`}
              value={names.surname}
              onChange={(event) => setName('surname')(event.target.value)}
              autoComplete="family-name"
              autoCapitalize="words"
              autoCorrect="off"
              enterKeyHint="next"
              className={field48}
              readOnly={pending.areas}
            />
          </div>
          <div>
            <label htmlFor={`${fieldId}-email`} className={cn(fieldLabel, 'flex justify-between')}>
              <span>{t('onboarding.account.email')}</span>
              <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
            </label>
            {/* Text, not `email`: the schema validates, and the browser's own message would not be translated */}
            <Input
              id={`${fieldId}-email`}
              type="text"
              inputMode="email"
              value={names.email}
              onChange={(event) => setName('email')(event.target.value)}
              autoComplete="email"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              placeholder={t('onboarding.account.emailPlaceholder')}
              className={field48}
              readOnly={pending.areas}
            />
          </div>
        </div>
      )}

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
