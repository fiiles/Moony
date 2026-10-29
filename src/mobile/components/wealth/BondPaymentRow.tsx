import { useTranslation } from 'react-i18next';
import { CalendarCheck, Coins } from 'lucide-react';
import { useFormat } from '@/lib/use-format';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { BondPayment } from '@/mobile/lib/bond-schedule';
import { shortDayOptions } from '@/mobile/lib/format-day';

/**
 * A coupon or a maturity to come, a row of "Nejbližší kupóny a splatnosti" (spec §8.8): a coin or
 * a calendar in the chip, the bond's name over "15. 3. 2027 · Kupón" (or "Splatnost"), and the
 * amount in the bond's own currency with its sign — a credit, so green. A maturity's amount is the principal with the last coupon. The row is read-only: the
 * bond is edited from the issues list under it.
 */
export function BondPaymentRow({ payment }: { payment: BondPayment }) {
  const { t } = useTranslation('bonds');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { bond, kind, amount, currency } = payment;
  const Glyph = kind === 'coupon' ? Coins : CalendarCheck;

  return (
    <EntityRow
      chip={
        <RowChip>
          <Glyph strokeWidth={1.75} />
        </RowChip>
      }
      title={bond.name}
      sub={`${fmt.day(payment.t, shortDayOptions(payment.t))} · ${t(`table.${kind}`)}`}
      value={moneyText(amount, currency, { signed: true })}
      valueTone="gain"
    />
  );
}
