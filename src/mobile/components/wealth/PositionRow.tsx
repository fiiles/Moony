import { useTranslation } from 'react-i18next';
import { Minus, Plus } from 'lucide-react';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useOpenWithIntent } from '@/mobile/hooks/use-row-intent';
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
 * with its amount. A tap opens the position; the long press's menu opens it with Koupit or Prodat
 * (README §13, Gestures). A value that could not be converted is "—" with no
 * return, never one measured against nothing. Closed positions are not listed here (`positionRows`).
 */
export function PositionRow({ row, name, href, currency }: PositionRowProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const moneyText = useMoneyText();
  const openWith = useOpenWithIntent();
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
            // The caption of the list says it once; the row says it to VoiceOver
            srLabel={t('wealth.sincePurchase')}
          />
        ) : undefined
      }
      href={href}
      menu={{
        label: `${tc('labels.actions')}: ${row.ticker}`,
        children: (
          <>
            <RowMenuItem onSelect={() => openWith(href, 'buy')}>
              <Plus strokeWidth={1.75} aria-hidden />
              {tc('buttons.buy')}
            </RowMenuItem>
            {row.quantity > 0 && (
              <RowMenuItem onSelect={() => openWith(href, 'sell')}>
                <Minus strokeWidth={1.75} aria-hidden />
                {tc('buttons.sell')}
              </RowMenuItem>
            )}
          </>
        ),
      }}
    />
  );
}
