import { Info } from 'lucide-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

/** The two returns of the hero that have an explanation. */
export type ExplainedMetric = 'twr' | 'mwr';

const STEPS = ['step1', 'step2', 'step3', 'step4', 'step5'] as const;

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="m-0 text-table font-650 text-ink">{title}</h3>
      {children}
    </section>
  );
}

/**
 * What a return of the hero is and how the engine computes it: what it shows, the steps, the
 * formula, the period vs yearly rule and what to keep in mind. Read-only; closes with Zavřít.
 */
function MetricExplainDialog({
  metric,
  open,
  onOpenChange,
}: {
  metric: ExplainedMetric;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const key = (name: string) => `stocksAnalysis.performance.explain.${name}`;
  const own = (name: string) => t(key(`${metric}.${name}`));
  const prose = 'm-0 text-table font-500 leading-[1.6] text-ink-2';

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[600px]">
        <DialogHeader>
          <DialogTitle>{own('title')}</DialogTitle>
          <DialogDescription>{own('lead')}</DialogDescription>
        </DialogHeader>
        <div className="-mx-[27px] flex max-h-[62vh] flex-col gap-5 overflow-y-auto px-[27px] py-1">
          <Section title={t(key('whatTitle'))}>
            <p className={prose}>{own('what')}</p>
          </Section>
          <Section title={t(key('howTitle'))}>
            <ol className={`${prose} list-decimal space-y-1 pl-5`}>
              {STEPS.map((step) => (
                <li key={step}>{own(step)}</li>
              ))}
            </ol>
            <pre className="m-0 mt-1 whitespace-pre-wrap rounded-r2 bg-well px-3 py-2.5 font-mono text-table text-ink-2">
              {own('formula')}
            </pre>
          </Section>
          <Section title={t(key('yearTitle'))}>
            <p className={prose}>{own('year')}</p>
          </Section>
          <Section title={t(key('watchTitle'))}>
            <p className={prose}>{own('watch')}</p>
          </Section>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * The ⓘ beside a hero label: hovering shows the one-line summary, a click opens the full
 * explanation.
 */
export function MetricExplainButton({ metric }: { metric: ExplainedMetric }) {
  const { t } = useTranslation('reports');
  const [open, setOpen] = useState(false);
  const key = (name: string) => `stocksAnalysis.performance.explain.${name}`;
  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            aria-label={`${t(key(`${metric}.title`))} · ${t(key('open'))}`}
            onClick={() => setOpen(true)}
            className="inline-grid size-4 shrink-0 place-items-center rounded-full text-ink-4 transition-colors duration-fast hover:text-ink-2 focus-visible:outline-none focus-visible:shadow-focus"
          >
            <Info aria-hidden className="size-[13px]" />
          </button>
        </TooltipTrigger>
        <TooltipContent>
          <p>{t(key(`${metric}.lead`))}</p>
          <small>{t(key('tooltipMore'))}</small>
        </TooltipContent>
      </Tooltip>
      <MetricExplainDialog metric={metric} open={open} onOpenChange={setOpen} />
    </>
  );
}
