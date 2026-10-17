import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import type { DatedFact } from '@shared/schema';
import type { Formatters } from '@/lib/format';
import { useFormat } from '@/lib/use-format';
import type { LineTip } from '@/components/charts/MoonyLineChart';
import type { EventCluster } from '@/utils/chart-scale';
import type { JumpEvent, JumpPart } from '@/utils/net-worth-jumps';
import { amountOf } from '@/utils/positions';

/** Facts named on one class line; the rest is an ellipsis. */
const FACTS_SHOWN = 3;
/** Tickers or assets a summarised day of trades names (`MAX_NAMED_TRADES` in `dated_facts.rs`). */
const TRADE_NAMES_SHOWN = 3;

/**
 * Tooltip of a net-worth jump mark: "Velká změna +1,15 mil. Kč", the day, then one line per class
 * that moved ("Nemovitosti +3,12 mil. Kč · nákup Chlumec"). A class without a dated fact that day
 * gets its change alone (a market move, a payment that arrived). Amounts are compact, in the
 * report currency the line is drawn in; the native amounts of a manual loan balance keep their
 * own currency. `formatMoney` replaces `fmt.money` (the phone passes its masking formatter while
 * amounts are hidden).
 */
export function useNetWorthJumpTip(currency: string, formatMoney?: Formatters['money']) {
  const { t } = useTranslation('dashboard');
  const fmt = useFormat();
  const money = formatMoney ?? fmt.money;

  return useCallback(
    (cluster: EventCluster<JumpEvent>): LineTip => {
      // Marks fold only above 24 of them and a period holds at most 12; the largest speaks
      // for a cluster that ever forms.
      const { jump } = cluster.events.reduce((a, b) =>
        Math.abs(b.jump.delta) > Math.abs(a.jump.delta) ? b : a
      );
      const signed = (value: number) =>
        money(value, currency, { signed: true, compact: true, decimals: 2 });
      const native = (value: string | null, code: string | null): string | null => {
        const parsed = amountOf(value);
        return parsed === null
          ? null
          : money(parsed, code ?? currency, { compact: true, decimals: 2 });
      };

      const factText = (fact: DatedFact): string => {
        switch (fact.kind) {
          case 'buy':
          case 'sell': {
            const count = fact.count ?? 1;
            if (count <= 1) return t(`jumps.fact.${fact.kind}`, { name: fact.name });
            const names = count > TRADE_NAMES_SHOWN ? `${fact.name}…` : fact.name;
            return t(`jumps.fact.${fact.kind}Many`, {
              positions: t('jumps.positions', { count }),
              names,
            });
          }
          case 'balance': {
            if (fact.class !== 'loans') return t('jumps.fact.balance', { name: fact.name });
            const to = native(fact.amount, fact.currency);
            const from = native(fact.previousAmount, fact.currency);
            if (to === null) return t('jumps.fact.balance', { name: fact.name });
            return from === null
              ? t('jumps.fact.loanBalanceTo', { name: fact.name, amount: to })
              : t('jumps.fact.loanBalance', { name: fact.name, from, to });
          }
          case 'start':
          case 'extraPayment':
          case 'valuation':
          case 'firstValuation':
            return t(`jumps.fact.${fact.kind}`, { name: fact.name });
          default:
            // A kind a newer backend knows: its name still says something
            return fact.name;
        }
      };

      const partLine = (part: JumpPart): string => {
        const head = `${t(`jumps.classes.${part.cls}`)} ${signed(part.delta)}`;
        if (part.facts.length === 0) return head;
        const shown = part.facts.slice(0, FACTS_SHOWN).map(factText);
        const facts = part.facts.length > FACTS_SHOWN ? `${shown.join(', ')}…` : shown.join(', ');
        return `${head} · ${facts}`;
      };

      return {
        title: t(jump.kind === 'market' ? 'jumps.marketTitle' : 'jumps.title', {
          amount: signed(jump.delta),
        }),
        lines: [fmt.day(jump.t), ...jump.parts.map(partLine)],
      };
    },
    [t, fmt, currency, money]
  );
}
