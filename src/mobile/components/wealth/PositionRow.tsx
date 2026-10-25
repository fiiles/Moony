import { useTranslation } from 'react-i18next';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { PositionRowBase } from '@/utils/positions';
import { ChangeLine } from './ChangeLine';
import { RowBadge } from '@/mobile/components/RowBadge';

interface PositionRowProps {
  row: PositionRowBase;
  /** The company name or the coin name, under the ticker. */
  name: string;
  /** Where a tap goes: the position's detail. */
  href: string;
  /** The report currency: the value and the gain are the backend's, converted into it. */
  currency: string;
}

/**
 * A position of a class list, Akcie or Krypto (spec §8.8, prototype frame 11): the first two
 * letters of the ticker in the chip (no logo fetch: it would tell the network what is held), the
 * ticker with its "ručně" badge over the name, the value in the report currency over the return
 * with its amount. A tap opens the position. A value that could not be converted is "—" with no
 * return, never one measured against nothing. Closed positions are not listed here (`positionRows`).
 */
export function PositionRow({ row, name, href, currency }: PositionRowProps) {
  const { t: ts } = useTranslation('stocks');
  const moneyText = useMoneyText();
  const { marketValue, gainLoss, gainLossPercent } = row;

  return (
    <EntityRow
      chip={<RowChip>{row.ticker.slice(0, 2).toUpperCase()}</RowChip>}
      title={
        <>
          {row.ticker}
          {row.isManualPrice && <RowBadge>{ts('detail.manual')}</RowBadge>}
        </>
      }
      sub={name}
      value={marketValue === null ? '—' : moneyText(marketValue, currency, { decimals: 0 })}
      valueSub={
        marketValue !== null && gainLoss !== null ? (
          <ChangeLine
            change={{
              delta: gainLoss,
              ratio: gainLossPercent === null ? null : gainLossPercent / 100,
            }}
            currency={currency}
          />
        ) : undefined
      }
      href={href}
    />
  );
}
