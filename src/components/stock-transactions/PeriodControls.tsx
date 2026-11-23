import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { dayFromIso, isoDay } from '@/utils/stock-performance';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Segmented } from '@/components/ui/segmented';

export type PeriodMode = 'year' | 'custom';

export interface PeriodControlsProps {
  mode: PeriodMode;
  onModeChange: (mode: PeriodMode) => void;
  year: number;
  minYear: number;
  maxYear: number;
  onYearChange: (year: number) => void;
  /** The custom range, both days included (UTC midnight, unix seconds). */
  fromDay: number;
  toDay: number;
  /** Bounds of the date inputs. */
  minDay: number;
  maxDay: number;
  onRangeChange: (range: { fromDay: number; toDay: number }) => void;
}

/**
 * The period of the report in the page head (spec D2): the year stepper (‹ 2026 ›) or, for
 * "Vlastní období", the Od and Do date inputs, then the Rok · Vlastní období segment. A range the
 * inputs hold with Od after Do (or a day missing) stays a draft marked invalid; the last valid
 * range stays in effect.
 */
export function PeriodControls({
  mode,
  onModeChange,
  year,
  minYear,
  maxYear,
  onYearChange,
  fromDay,
  toDay,
  minDay,
  maxDay,
  onRangeChange,
}: PeriodControlsProps) {
  const { t } = useTranslation('stockTransactions');
  /** What the inputs hold while it is not a valid range. */
  const [draft, setDraft] = useState<{ from: string; to: string } | null>(null);

  const changeInput = (side: 'from' | 'to', value: string) => {
    const next = {
      from: draft?.from ?? isoDay(fromDay),
      to: draft?.to ?? isoDay(toDay),
      [side]: value,
    };
    const from = dayFromIso(next.from);
    const to = dayFromIso(next.to);
    if (from !== null && to !== null && from <= to) {
      setDraft(null);
      onRangeChange({ fromDay: from, toDay: to });
    } else {
      setDraft(next);
    }
  };
  const changeMode = (next: PeriodMode) => {
    setDraft(null);
    onModeChange(next);
  };
  const invalid = draft !== null;

  return (
    <>
      {mode === 'year' ? (
        <div
          className="mx-1 inline-flex items-center gap-1"
          role="group"
          aria-label={t('period.year')}
        >
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('period.prevYear')}
            disabled={year <= minYear}
            onClick={() => onYearChange(year - 1)}
          >
            <ChevronLeft />
          </Button>
          <b className="min-w-[48px] text-center text-body font-650 text-ink num">{year}</b>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('period.nextYear')}
            disabled={year >= maxYear}
            onClick={() => onYearChange(year + 1)}
          >
            <ChevronRight />
          </Button>
        </div>
      ) : (
        <div className="inline-flex items-center gap-1.5 text-caption text-ink-4">
          {(['from', 'to'] as const).map((side, i) => (
            <span key={side} className="contents">
              {i > 0 && <span aria-hidden>–</span>}
              <Input
                type="date"
                value={draft?.[side] ?? isoDay(side === 'from' ? fromDay : toDay)}
                min={isoDay(minDay)}
                max={isoDay(maxDay)}
                aria-label={t(`period.${side}`)}
                aria-invalid={invalid}
                title={invalid ? t('period.rangeOrder') : undefined}
                onChange={(event) => changeInput(side, event.target.value)}
                className="h-control w-[132px] text-table"
              />
            </span>
          ))}
        </div>
      )}
      <Segmented
        size="lg"
        value={mode}
        onValueChange={changeMode}
        aria-label={t('period.label')}
        options={[
          { value: 'year', label: t('period.year') },
          { value: 'custom', label: t('period.custom') },
        ]}
      />
    </>
  );
}
