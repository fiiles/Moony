import { cn } from '@/lib/utils';

export interface WizardStep {
  id: string;
  label: string;
}

/**
 * Underline stepper (design system prototypes `.stepper` / `.wizard`): one
 * flexible cell per step, done steps in ink-3, the current one in ink, the
 * rest in ink-4. Numbered "1 · Soubor".
 */
export function WizardSteps({
  steps,
  current,
  className,
}: {
  steps: readonly WizardStep[];
  current: string;
  className?: string;
}) {
  const index = steps.findIndex((s) => s.id === current);
  return (
    <ol className={cn('flex gap-1', className)} aria-label="steps">
      {steps.map((step, i) => (
        <li
          key={step.id}
          aria-current={i === index ? 'step' : undefined}
          className={cn(
            'flex-1 border-b-2 pb-2 text-micro font-650 tracking-[0.02em]',
            i < index && 'border-ink-3 text-ink-3',
            i === index && 'border-ink text-ink',
            i > index && 'border-well-3 text-ink-4'
          )}
        >
          {i + 1} · {step.label}
        </li>
      ))}
    </ol>
  );
}
