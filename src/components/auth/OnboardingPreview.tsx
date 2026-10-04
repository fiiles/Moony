import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';

export interface PreviewAreas {
  investments: boolean;
  realEstate: boolean;
  liabilities: boolean;
}

interface OnboardingPreviewProps {
  areas: PreviewAreas;
  /** First name typed in the account step, for the greeting. */
  name?: string;
  /** Net worth shown once the first account has a balance ("128 400 Kč"). */
  netWorth?: string;
  /** Caption above the miniature: changes on the last step. */
  caption: string;
}

/**
 * Live miniature of the future overview (prototype onboarding.html): the
 * sidebar groups and stat cards follow the chosen areas, so the choice in
 * step 4 is concrete rather than abstract.
 */
export function OnboardingPreview({ areas, name, netWorth, caption }: OnboardingPreviewProps) {
  const { t } = useTranslation('common');
  const { t: ta } = useTranslation('auth');
  const count = 1 + [areas.investments, areas.realEstate, areas.liabilities].filter(Boolean).length;

  const group = (label: string, items: string[], on = true) =>
    on && (
      <>
        <div className="mb-1 ml-2 mt-3 text-[6.5px] font-700 uppercase tracking-[0.1em] text-ink-4">
          {label}
        </div>
        {items.map((item) => (
          <div
            key={item}
            className="flex items-center gap-1.5 rounded-[5px] px-2 py-[5px] font-600 text-ink-2"
          >
            <i className="block size-[7px] rounded-[2px] bg-current opacity-50" />
            {item}
          </div>
        ))}
      </>
    );

  const stat = (label: string, value?: string, on = true) =>
    on && (
      <div className="rounded-[7px] border border-line bg-paper px-[9px] py-2">
        <small className="block text-[6.5px] font-600 text-ink-3">{label}</small>
        {value ? (
          <b className="mt-[3px] block text-[10px] font-700 tracking-[-0.04em] text-ink">{value}</b>
        ) : (
          <span className="mt-1 block h-[7px] w-3/5 rounded-[3px] bg-well-2" />
        )}
      </div>
    );

  return (
    <section className="relative flex items-center justify-center overflow-hidden border-l border-line bg-[linear-gradient(160deg,var(--well),var(--canvas))] p-12">
      <div className="pointer-events-none absolute -right-[200px] -top-[220px] size-[520px] rounded-full bg-canvas-glow" />
      <div className="absolute left-12 right-12 top-7 flex justify-between text-eyebrow uppercase text-ink-4">
        <span>{caption}</span>
        <span>{ta('onboarding.preview.areas', { count })}</span>
      </div>
      <div
        aria-hidden
        className="relative grid aspect-[16/10.2] w-full max-w-[620px] grid-cols-[150px_1fr] overflow-hidden rounded-[14px] border border-line bg-canvas text-[9px] shadow-modal"
      >
        <div className="border-r border-line-sidebar bg-sidebar-2 px-2.5 py-3.5">
          <div className="flex items-center gap-1.5 rounded-[5px] bg-dark px-2 py-[5px] font-600 text-ink-inverse">
            <i className="block size-[7px] rounded-[2px] bg-current opacity-50" />
            {t('nav.overview')}
          </div>
          {group(t('nav.groupMoney'), [t('nav.bankAccounts'), t('nav.budgets'), t('nav.cashflow')])}
          {group(
            t('nav.groupInvestments'),
            [t('nav.stocks'), t('nav.crypto'), t('nav.bonds')],
            areas.investments
          )}
          {group(t('nav.realEstate'), [t('nav.realEstate')], areas.realEstate)}
          {group(
            t('nav.groupLiabilities'),
            [t('nav.loans'), t('nav.insurance')],
            areas.liabilities
          )}
        </div>
        <div className="px-[18px] py-4">
          <div className="mb-3.5 text-[7px] text-ink-4">{ta('onboarding.preview.crumb')}</div>
          <div className="text-[15px] font-700 tracking-[-0.05em] text-ink">
            {name
              ? ta('onboarding.preview.greetingNamed', { name })
              : ta('onboarding.preview.greeting')}
          </div>
          <div className="my-2.5 rounded-[9px] border border-line bg-paper px-3.5 py-3 shadow-e1">
            <small className="text-[7.5px] font-600 text-ink-3">
              {ta('onboarding.preview.netWorth')}
            </small>
            <b className="my-1 block text-[20px] font-750 tracking-[-0.06em] text-ink">
              {netWorth ?? ta('onboarding.preview.empty')}
            </b>
            <small className="text-[7.5px] font-600 text-ink-4">
              {netWorth ? ta('onboarding.preview.afterAccount') : ta('onboarding.preview.fills')}
            </small>
            <svg
              viewBox="0 0 300 40"
              preserveAspectRatio="none"
              className="mt-1.5 block h-10 w-full"
            >
              <path
                d="M0 30 C 40 28, 80 26, 120 22 S 200 16, 240 12 S 280 8, 300 6"
                fill="none"
                stroke="var(--chart-line)"
                strokeWidth="1.5"
                strokeDasharray="3 3"
              />
            </svg>
          </div>
          <div className="grid grid-cols-3 gap-1.5">
            {stat(ta('onboarding.preview.statAccounts'), netWorth)}
            {stat(t('nav.groupInvestments'), undefined, areas.investments)}
            {stat(t('nav.realEstate'), undefined, areas.realEstate)}
            {stat(t('nav.groupLiabilities'), undefined, areas.liabilities)}
          </div>
          <div className="mt-1.5 grid grid-cols-2 gap-1.5">
            {[ta('onboarding.preview.allocation'), ta('onboarding.preview.recent')].map((label) => (
              <div
                key={label}
                className={cn(
                  'min-h-[56px] rounded-[7px] border border-line bg-paper px-[9px] py-2 text-[7px] font-600 text-ink-3'
                )}
              >
                {label}
                <span className="mt-[5px] block h-[5px] w-[70%] rounded-[3px] bg-well-2" />
                <span className="mt-[5px] block h-[5px] w-[45%] rounded-[3px] bg-well-2" />
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
