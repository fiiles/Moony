import { useTranslation } from 'react-i18next';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { PositionRowBase } from '@/utils/positions';
import { ChangeLine } from './ChangeLine';

/** The badge after a ticker ("ručně", "uzavřená"): 18 pt, 11 / 600 `ink-3` on `well-2`. */
const BADGE =
  'ml-1.5 inline-flex h-[18px] items-center rounded-r1 bg-well-2 px-1.5 align-[2px] text-m-caption font-semibold text-ink-3';

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
 * ticker with its "ručně" / "uzavřená" badge over the name, the value in the report currency over
 * the return with its amount. A tap opens the position. A value that could not be converted is
 * "—" with no return, never one measured against nothing.
 */
export function PositionRow({ row, name, href, currency }: PositionRowProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const moneyText = useMoneyText();
  const { marketValue, gainLoss, gainLossPercent } = row;
  const closed = row.quantity <= 0;

  return (
    <EntityRow
      chip={<RowChip>{row.ticker.slice(0, 2).toUpperCase()}</RowChip>}
      title={
        <>
          {row.ticker}
          {row.isManualPrice && <span className={BADGE}>{ts('detail.manual')}</span>}
          {closed && <span className={BADGE}>{t('wealth.stocks.closedBadge')}</span>}
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
