import { useTranslation } from 'react-i18next';
import type { RealEstate } from '@shared/schema';
import type { YearlyBalance } from '@/utils/real-estate';
import { cn } from '@/lib/utils';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import {
  balanceNote,
  balanceRows,
  monthlyRent,
  type BalanceRow,
} from '@/mobile/lib/property-detail';

/** Whole amounts: the card shows no decimals. */
const WHOLE = { decimals: 0 } as const;

/** The card's material: a list of key-value rows on paper. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-4 py-1 shadow-e1';

const TONES = { gain: 'text-gain', loss: 'text-loss', total: 'font-semibold' } as const;

interface YearlyBalanceCardProps {
  property: RealEstate;
  balance: YearlyBalance;
  /** The main currency. */
  currency: string;
}

/**
 * "Roční bilance" as a key-value card (spec §8.8 "Nemovitost"): the rent of a year less the
 * recurring costs, the insurance and the interest is the net income, less the principal paid is
 * the cashflow on the account — the desktop's lines in its order, signs and colours
 * (`balanceRows`), costs and the principal negative, a line whose amount has no rate "—". Under
 * it the desktop's sentence: nothing to balance, what is missing, or what the property earns or
 * costs a month. Every amount goes through the eye.
 */
export function YearlyBalanceCard({ property, balance, currency }: YearlyBalanceCardProps) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const signed = (value: number) => moneyText(value, currency, { signed: true, ...WHOLE });

  /** The monthly rent in the main currency, or in its own without a rate. */
  const rent = monthlyRent(property, balance);
  const rentText =
    rent.main !== null
      ? moneyText(rent.main, currency, WHOLE)
      : moneyText(rent.native.amount, rent.native.currency, WHOLE);
  const label = ({ key }: BalanceRow) =>
    key === 'rent' ? t('detail.balance.rent', { amount: rentText }) : t(`detail.balance.${key}`);

  const note = balanceNote(balance);
  const sentence =
    note.kind === 'empty'
      ? t('detail.balance.empty')
      : note.kind === 'missing'
        ? tc('fxQuality.missing', { currency, currencies: note.currencies.join(', ') })
        : t(note.kind === 'costs' ? 'detail.balance.costs' : 'detail.balance.earns', {
            amount: moneyText(note.monthly, currency, WHOLE),
            income: signed(note.income),
          });

  return (
    <>
      <div className={CARD}>
        {balanceRows(balance).map((row) => (
          <KeyValueRow
            key={row.key}
            label={label(row)}
            value={
              <span className={cn(row.tone && TONES[row.tone], row.tone === 'total' && 'text-ink')}>
                {row.amount !== null ? signed(row.amount) : '—'}
              </span>
            }
          />
        ))}
      </div>
      <p className="mx-0.5 mb-3.5 mt-0 text-m-caption text-ink-3">{sentence}</p>
    </>
  );
}
