import { useCallback, useEffect, useRef, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import type { StockReportCostMethod, StockTransactionsReportSettings } from '@shared/schema';
import { parseHoldingMonths } from '@/utils/stock-transactions-report';
import { Input, InputWrap } from '@/components/ui/input';
import { Segmented } from '@/components/ui/segmented';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { ExplainEm, ExplainText, ExplainTip, ExplainTitle } from './ExplainTip';
import { holdingLength } from './holding-length';

/** The preset lengths of the holding test, in months (spec D7). */
const PRESETS = [6, 12, 24, 36, 60, 120] as const;
const CUSTOM = 'custom';
/** A typed length is saved this long after the last valid keystroke (or at once on blur). */
const SAVE_DELAY_MS = 400;

type Pending = { months: number; timer: ReturnType<typeof setTimeout> };

/** Cancels the pending save and returns its length, `null` when none was pending. */
function takePending(pending: { current: Pending | null }): number | null {
  const taken = pending.current;
  if (taken === null) return null;
  clearTimeout(taken.timer);
  pending.current = null;
  return taken.months;
}

/**
 * The settings strip of the hero (spec D6, D7): the cost method and the holding test with its
 * length. Every valid change is saved at once, except the custom months field: it keeps what is
 * typed, marks it invalid until it reads as 1–600 whole months while the last valid length
 * stays, and saves the last valid length once typing pauses (or on blur), so one value is saved
 * and requested. Any other change takes a pending length along in the same save.
 */
export function ReportSettings({
  settings,
  onChange,
}: {
  settings: StockTransactionsReportSettings;
  onChange: (next: StockTransactionsReportSettings) => void;
}) {
  const { t } = useTranslation('stockTransactions');
  /**
   * The months field is on screen because of the person (picked "Vlastní…" or typed into it): it
   * stays while they type, even through a value that equals a preset, until a preset is picked
   * or the test is switched off.
   */
  const [customPicked, setCustomPicked] = useState(false);
  /** What the months field holds; `null` shows the stored length. */
  const [draft, setDraft] = useState<string | null>(null);

  /** The typed length waiting to be saved. */
  const pending = useRef<Pending | null>(null);
  /** The settings and the callback of the last render, for a save that fires later. */
  const latest = useRef({ settings, onChange });
  useEffect(() => {
    latest.current = { settings, onChange };
  });
  const savePending = useCallback(() => {
    const typed = takePending(pending);
    const { settings: current, onChange: save } = latest.current;
    if (typed !== null && typed !== current.holdingTestMonths) {
      save({ ...current, holdingTestMonths: typed });
    }
  }, []);
  // Leaving the page while a length waits still saves it
  useEffect(() => savePending, [savePending]);

  const months = settings.holdingTestMonths;
  const isPreset = (PRESETS as readonly number[]).includes(months);
  const custom = customPicked || !isPreset;
  const text = draft ?? String(months);
  const invalid = parseHoldingMonths(text) === null;

  /** `settings` with a typed length that has not been saved yet. */
  const withPending = (): StockTransactionsReportSettings => {
    const typed = takePending(pending);
    return typed === null ? settings : { ...settings, holdingTestMonths: typed };
  };

  const pickLength = (value: string) => {
    setDraft(null);
    if (value === CUSTOM) {
      setCustomPicked(true);
      return;
    }
    setCustomPicked(false);
    takePending(pending);
    const next = Number(value);
    if (next !== months) onChange({ ...settings, holdingTestMonths: next });
  };
  const typeMonths = (value: string) => {
    setCustomPicked(true);
    setDraft(value);
    const next = parseHoldingMonths(value);
    // An invalid keystroke keeps the last valid length waiting
    if (next === null) return;
    takePending(pending);
    pending.current = { months: next, timer: setTimeout(savePending, SAVE_DELAY_MS) };
  };

  return (
    <div className="mb-[10px] flex min-h-[46px] flex-wrap items-center justify-between gap-x-6 gap-y-[10px] rounded-r2 bg-well px-3 py-2">
      <div className="inline-flex items-center gap-[9px] text-caption font-650 text-ink-2">
        <span>{t('settings.costMethod')}</span>
        <ExplainTip label={t('tips.costMethod.label')}>
          <ExplainTitle>{t('tips.costMethod.title')}</ExplainTitle>
          <ExplainText>
            <Trans t={t} i18nKey="tips.costMethod.fifo" components={{ em: <ExplainEm /> }} />
          </ExplainText>
          <ExplainText>
            <Trans t={t} i18nKey="tips.costMethod.average" components={{ em: <ExplainEm /> }} />
          </ExplainText>
          <ExplainText>{t('tips.costMethod.choose')}</ExplainText>
        </ExplainTip>
        <Segmented<StockReportCostMethod>
          value={settings.costMethod}
          onValueChange={(costMethod) => onChange({ ...withPending(), costMethod })}
          aria-label={t('settings.costMethod')}
          options={[
            { value: 'fifo', label: t('settings.fifo') },
            { value: 'average', label: t('settings.average') },
          ]}
        />
      </div>

      <div className="inline-flex flex-wrap items-center gap-[9px] text-caption font-650 text-ink-2">
        <label htmlFor="stock-report-holding-test">{t('settings.holdingTest')}</label>
        <ExplainTip label={t('tips.holdingTest.label')}>
          <ExplainTitle>{t('tips.holdingTest.title')}</ExplainTitle>
          <ExplainText>{t('tips.holdingTest.what')}</ExplainText>
          <ExplainText>{t('tips.holdingTest.when')}</ExplainText>
          <ExplainText>{t('tips.holdingTest.tax')}</ExplainText>
        </ExplainTip>
        <Switch
          id="stock-report-holding-test"
          className="ml-0.5"
          checked={settings.holdingTestEnabled}
          onCheckedChange={(holdingTestEnabled) => {
            setCustomPicked(false);
            setDraft(null);
            onChange({ ...withPending(), holdingTestEnabled });
          }}
        />
        {settings.holdingTestEnabled && (
          <>
            <Select value={custom ? CUSTOM : String(months)} onValueChange={pickLength}>
              <SelectTrigger
                aria-label={t('settings.length')}
                className="h-[30px] w-[112px] text-caption font-600"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PRESETS.map((value) => (
                  <SelectItem key={value} value={String(value)}>
                    {holdingLength(t, value)}
                  </SelectItem>
                ))}
                <SelectItem value={CUSTOM}>{t('settings.custom')}</SelectItem>
              </SelectContent>
            </Select>
            {custom && (
              <InputWrap unit={t('settings.monthsUnit')}>
                <Input
                  inputMode="numeric"
                  value={text}
                  onChange={(event) => typeMonths(event.target.value)}
                  onBlur={savePending}
                  aria-label={t('settings.customMonths')}
                  aria-invalid={invalid}
                  title={invalid ? t('settings.invalidMonths') : undefined}
                  className="h-[30px] w-[92px] text-caption font-600"
                />
              </InputWrap>
            )}
          </>
        )}
      </div>
    </div>
  );
}
