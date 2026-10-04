import { useTranslation } from 'react-i18next';
import { Segmented } from '@/components/ui/segmented';
import { ALL_CHART_PERIODS, type ChartPeriod } from '@/utils/period';

export type Period = ChartPeriod;

interface TimePeriodSelectorProps {
  value: Period;
  onChange: (period: Period) => void;
  /** Subset and order of the segments (default: all six). */
  options?: readonly Period[];
  size?: 'sm' | 'lg';
  /** `long` reads "30 dní · 3 měsíce · Letos · Rok · Vše" (page heads); `short` "30D · 90D · YTD · 1R · Vše". */
  labels?: 'short' | 'long';
  className?: string;
}

/** Period segmented control (design system §6 `.seg`); values stay language-neutral. */
export default function TimePeriodSelector({
  value,
  onChange,
  options = ALL_CHART_PERIODS,
  size = 'sm',
  labels = 'short',
  className,
}: TimePeriodSelectorProps) {
  const { t } = useTranslation('common');
  const ns = labels === 'long' ? 'periodsLong' : 'periods';
  return (
    <Segmented
      value={value}
      onValueChange={onChange}
      size={size}
      className={className}
      aria-label={t('periods.label')}
      options={options.map((period) => ({ value: period, label: t(`${ns}.${period}`) }))}
    />
  );
}
