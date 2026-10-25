import type { ComponentProps } from 'react';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { EmptyState } from '@/components/common/EmptyState';
import {
  ConversionQualityNote,
  ReportError,
  ReportQualityNote,
} from '@/components/reports/ReportQualityNote';
import { cn } from '@/lib/utils';

// Phone wrappers for the desktop pieces that set a fixed size (`text-h3`, `text-table`, `text-micro`).
// Each hands the piece a `text-m-*` role through the piece's own class props, merged after the
// caller's classes (the same sizes and weights at the default text size, 130 % at the largest), so
// the empty states, legends and quality notes follow Dynamic Type like the rest of the shell.
// `src/mobile/type-roles.test.ts` forbids importing the three desktop modules anywhere else in
// src/mobile/. A role's own weight is 500, so the 10 px notes ask for `font-semibold` (600) again.

export type { LegendItem } from '@/components/charts/ChartLegend';

/** `EmptyState`: title in `text-m-section` (16/620), the sentence in `text-m-sub` (12, line height 1.5 as on the desktop). */
export function ScaledEmptyState({
  titleClassName,
  descriptionClassName,
  ...props
}: ComponentProps<typeof EmptyState>) {
  return (
    <EmptyState
      {...props}
      titleClassName={cn(titleClassName, 'text-m-section')}
      // A role carries its own line height (1.3), which tailwind-merge would let replace `leading-[1.5]`.
      descriptionClassName={cn(descriptionClassName, 'text-m-sub leading-[1.5]')}
    />
  );
}

/** `ChartLegend`: the items in `text-m-note` (10/600); swatches keep their sizes. */
export function ScaledChartLegend({ className, ...props }: ComponentProps<typeof ChartLegend>) {
  return <ChartLegend {...props} className={cn(className, 'text-m-note font-semibold')} />;
}

/** `ConversionQualityNote`: `text-m-note` (10/600); the desktop's `mb-5` stays when the caller sets no spacing. */
export function ScaledConversionNote({
  className = 'mb-5',
  ...props
}: ComponentProps<typeof ConversionQualityNote>) {
  return (
    <ConversionQualityNote {...props} className={cn(className, 'text-m-note font-semibold')} />
  );
}

/** `ReportQualityNote`: `text-m-note` (10/600). */
export function ScaledReportNote({
  className,
  ...props
}: ComponentProps<typeof ReportQualityNote>) {
  return <ReportQualityNote {...props} className={cn(className, 'text-m-note font-semibold')} />;
}

/** `ReportError`: `text-m-sub` (12). */
export function ScaledReportError({ className, ...props }: ComponentProps<typeof ReportError>) {
  return <ReportError {...props} className={cn(className, 'text-m-sub')} />;
}
