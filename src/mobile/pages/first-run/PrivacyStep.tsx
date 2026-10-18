import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check } from 'lucide-react';
import { FX_SUPPORTED_CODES, currencyName } from '@shared/currencies';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import type { FirstRunFlow } from '@/mobile/hooks/use-first-run';
import { CountryPickerSheet } from '@/mobile/sheets/CountryPickerSheet';
import { CurrencyPickerSheet } from '@/mobile/sheets/CurrencyPickerSheet';
import { StepHead } from './StepHead';
import { caption, primaryButton } from './styles';

const PRIVACY_POINTS = ['encryption', 'localOnly', 'marketData'] as const;

/**
 * Soukromí: what the app does with the data, and the two choices that shape the rest: the main
 * currency (only one with a rate source) and the banking country (rule packs, the import's bank
 * order). Each opens a picker sheet; the hints under the rows are the desktop's.
 */
export function PrivacyStep({ flow }: { flow: FirstRunFlow }) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();
  const [currencyOpen, setCurrencyOpen] = useState(false);
  const [countryOpen, setCountryOpen] = useState(false);

  return (
    <div data-testid="first-run-privacy">
      <StepHead title={t('onboarding.welcome.title')} lead={tm('firstRun.privacyLead')} />

      <div className="mb-4 grid gap-2 rounded-r3 bg-well px-4 py-3.5 text-m-sub text-ink-2">
        {PRIVACY_POINTS.map((key) => (
          <div key={key} className="flex items-start gap-2">
            <Check className="mt-px size-3.5 shrink-0 text-gain" aria-hidden />
            <span>
              <b className="font-650 text-ink">{t(`onboarding.welcome.points.${key}.title`)}</b>{' '}
              {t(`onboarding.welcome.points.${key}.text`)}
            </span>
          </div>
        ))}
      </div>

      <div className="mb-5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1">
        <div className="pb-2.5">
          <KeyValueRow
            label={t('onboarding.welcome.currency')}
            value={`${flow.currency} · ${currencyName(flow.currency, fmt.locale)}`}
            onClick={() => setCurrencyOpen(true)}
          />
          <p className={caption}>{t('onboarding.welcome.currencyHint')}</p>
        </div>
        <div className="pb-2.5">
          <KeyValueRow
            label={t('onboarding.welcome.country')}
            value={flow.countryName ?? t('onboarding.welcome.countryNone')}
            onClick={() => setCountryOpen(true)}
          />
          <p className={caption}>{t('onboarding.welcome.countryHint')}</p>
        </div>
      </div>

      <Button className={primaryButton} onClick={flow.goNext}>
        {tc('buttons.continue')}
      </Button>

      <CurrencyPickerSheet
        open={currencyOpen}
        onOpenChange={setCurrencyOpen}
        value={flow.currency}
        onPick={flow.setCurrency}
        options={FX_SUPPORTED_CODES}
      />
      <CountryPickerSheet
        open={countryOpen}
        onOpenChange={setCountryOpen}
        value={flow.country}
        onPick={flow.setCountry}
      />
    </div>
  );
}
