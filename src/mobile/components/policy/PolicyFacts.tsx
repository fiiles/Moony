import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Copy } from 'lucide-react';
import { toast } from 'sonner';
import { useDurationText } from '@/hooks/use-duration-text';
import { useFormat } from '@/lib/use-format';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { PolicyFact } from '@/mobile/lib/policy-detail';
import { backState } from '@/mobile/lib/back-label';
import { propertyPath } from '@/mobile/shell/routes';

/** The contract card's material: a list of key-value rows on paper. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-4 py-1 shadow-e1';

/**
 * A value that may take two lines: a row of the card ends in an ellipsis by the key-value row's
 * rule, which would cut "2 150 Kč · čtvrtletně" or a term of two dates short on a narrow phone.
 */
function Wrapping({ children }: { children: ReactNode }) {
  return <span className="whitespace-normal">{children}</span>;
}

/**
 * Číslo smlouvy: the row of the key-value card with the 44 pt copy button at its end. The button
 * has to sit outside the value cell of `KeyValueRow`, which clips what overflows it, so this row
 * is laid out the same way (`.m-kv`) by hand. The number is no amount: it is never masked.
 */
function NumberRow({ label, number }: { label: string; number: string }) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(number);
      toast(t('policy.copied'));
    } catch {
      // No clipboard (or no permission to write to it): say so, the number is still on screen
      toast.error(tc('status.error'));
    }
  };

  return (
    <div className="flex min-h-11 w-full items-center gap-3 px-1 py-2">
      <span className="max-w-[40%] flex-none hyphens-auto break-words text-m-sub text-ink-3">
        {label}
      </span>
      {/* Wraps at the number's spaces, never inside a group of digits that fits (`break-all` cut
          "8 540 221 937" as "…93 / 7"); only a number with no space that is wider than the cell
          still breaks, where it must */}
      <div className="min-w-0 flex-1 break-words text-right text-m-body text-ink num">{number}</div>
      {/* 44 pt: it reaches past the row's padding so the icon stays at the value's right edge */}
      <button
        type="button"
        onClick={copy}
        aria-label={t('policy.copy')}
        className="-my-2 -mr-3 inline-flex size-11 flex-none items-center justify-center rounded-[12px] text-ink-3 [-webkit-touch-callout:none] active:bg-well-2 focus-visible:outline-none focus-visible:shadow-focus"
      >
        <Copy className="size-4" strokeWidth={1.75} aria-hidden />
      </button>
    </div>
  );
}

interface PolicyFactsProps {
  facts: PolicyFact[];
  today: number;
  /** The policy's name: the property's back button shows it (the property is on Majetek). */
  from: string;
}

/**
 * The facts of the contract (spec §8.8 "Pojistka", prototype frame 14), first under the name: one
 * key-value card, a row per fact the policy has (`policyFacts`), the labels the desktop's contract
 * card has. The premium is an amount in the policy's own currency with its frequency — through the
 * eye; the term, the next payment, the cancellation deadline and the insured property are dates
 * and a name. The property is a row that opens it, its back button naming the policy.
 */
export function PolicyFacts({ facts, today, from }: PolicyFactsProps) {
  const { t } = useTranslation('mobile');
  const { t: ti } = useTranslation('insurance');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const duration = useDurationText('insurance');
  const [, setLocation] = useLocation();

  return (
    <div className={CARD}>
      {facts.map((fact) => {
        switch (fact.key) {
          case 'number':
            return (
              <NumberRow key={fact.key} label={ti('detail.contract.number')} number={fact.number} />
            );
          case 'provider':
            return (
              <KeyValueRow
                key={fact.key}
                label={ti('detail.contract.provider')}
                value={<Wrapping>{fact.provider}</Wrapping>}
              />
            );
          case 'type':
            return (
              <KeyValueRow
                key={fact.key}
                label={ti('detail.contract.type')}
                value={<Wrapping>{ti(`types.${fact.type}`, { defaultValue: fact.type })}</Wrapping>}
              />
            );
          case 'premium': {
            const amount = moneyText(fact.amount, fact.currency, { decimals: 0 });
            return (
              <KeyValueRow
                key={fact.key}
                label={ti('detail.contract.premium')}
                value={
                  <Wrapping>
                    {fact.frequency
                      ? t('policy.premium', {
                          amount,
                          frequency: ti(`frequencyShort.${fact.frequency}`),
                        })
                      : amount}
                  </Wrapping>
                }
              />
            );
          }
          case 'oneTime':
            return (
              <KeyValueRow
                key={fact.key}
                label={ti('detail.contract.oneTime')}
                value={moneyText(fact.amount, fact.currency, { decimals: 0 })}
              />
            );
          case 'nextPayment':
            return (
              <KeyValueRow
                key={fact.key}
                label={ti('detail.stats.nextPayment')}
                value={
                  <Wrapping>
                    {fmt.day(fact.day)} ·{' '}
                    {ti('table.in', { duration: duration.ahead(today, fact.day) })}
                  </Wrapping>
                }
              />
            );
          case 'term': {
            const start = fmt.day(fact.start);
            return (
              <KeyValueRow
                key={fact.key}
                label={t('policy.term')}
                value={
                  <Wrapping>
                    {fact.end === null
                      ? t('policy.termOpen', { start })
                      : t('policy.termFixed', { start, end: fmt.day(fact.end) })}
                  </Wrapping>
                }
                sub={
                  fact.anniversary !== null
                    ? t('policy.anniversary', { date: fmt.day(fact.anniversary) })
                    : undefined
                }
              />
            );
          }
          case 'notice':
            return (
              <KeyValueRow
                key={fact.key}
                label={t('policy.notice')}
                value={fmt.day(fact.deadline)}
              />
            );
          case 'property':
            return (
              <KeyValueRow
                key={fact.key}
                label={ti('detail.contract.property')}
                value={<Wrapping>{fact.name}</Wrapping>}
                onClick={() => setLocation(propertyPath(fact.id), { state: backState(from) })}
              />
            );
        }
      })}
    </div>
  );
}
