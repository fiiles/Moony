import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FileText, Pencil, Trash2 } from 'lucide-react';
import type { Bond } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { bondYearlyCoupon } from '@/utils/bonds';
import { Button } from '@/components/ui/button';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { RowMenu, RowMenuItem } from '@/mobile/components/RowMenu';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { BondRowModel } from '@/mobile/lib/bond-rows';
import type { BondPayment } from '@/mobile/lib/bond-schedule';
import { RowBadge } from '@/mobile/components/RowBadge';

interface BondRowProps {
  row: BondRowModel;
  /** The bond's next coupon or maturity, when one is due in the coming year. */
  nextPayment?: BondPayment;
  /** The details are unfolded under the row. */
  expanded: boolean;
  /** A tap folds or unfolds the details. */
  onToggle: () => void;
  /** Upravit in the details or the menu: the page opens the form with this bond. */
  onEdit: (bond: Bond) => void;
  /** Smazat in the menu: the page asks in its confirm sheet. */
  onDelete: (bond: Bond) => void;
}

/**
 * An issue of the Dluhopisy list (spec §8.8): the bond icon in the chip, the name with its "do
 * roka" or "splaceno" badge over "ISIN · 10 × 1 000 Kč · 5 %" (no ISIN, no first part), the face
 * value over the maturity day or "bez splatnosti". Every amount is the bond's own (native is
 * authoritative, ADR 0012). A tap unfolds everything the row has no
 * room for under it (`BondDetails`), with Upravit (the desktop's form); a long press opens the
 * row's menu, Upravit and Smazat.
 */
export function BondRow({ row, nextPayment, expanded, onToggle, onEdit, onDelete }: BondRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bonds');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [menuOpen, setMenuOpen] = useState(false);
  const { bond, face, currency, rate, quantity, unitFace, maturity, badge } = row;

  // A whole rate is written whole ("5 %"), a fractional one with its two decimals ("5,25 %")
  const rateText = fmt.percent(rate / 100, Number.isInteger(rate) ? 0 : 2);
  const sub = [bond.isin, `${fmt.number(quantity)} × ${moneyText(unitFace, currency)}`, rateText]
    .filter(Boolean)
    .join(' · ');

  return (
    <EntityRow
      chip={
        <RowChip>
          <FileText strokeWidth={1.75} />
        </RowChip>
      }
      title={
        <>
          {bond.name}
          {badge && <RowBadge>{tb(`table.${badge}`)}</RowBadge>}
        </>
      }
      sub={sub}
      value={moneyText(face, currency)}
      valueSub={maturity === null ? tb('table.noMaturity') : fmt.day(maturity)}
      onClick={onToggle}
      onLongPress={() => setMenuOpen(true)}
      below={
        expanded && <BondDetails row={row} nextPayment={nextPayment} onEdit={() => onEdit(bond)} />
      }
      accessory={
        <RowMenu
          open={menuOpen}
          onOpenChange={setMenuOpen}
          label={t('cashflow.rowMenu', { title: bond.name })}
        >
          <RowMenuItem onSelect={() => onEdit(bond)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {tb('actions.edit')}
          </RowMenuItem>
          <RowMenuItem variant="danger" onSelect={() => onDelete(bond)}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {tb('actions.delete')}
          </RowMenuItem>
        </RowMenu>
      }
    />
  );
}

/**
 * The unfolded details of an issue: what the row leaves out or shortens — ISIN, units and the
 * nominal of one, the face value, the rate and the yearly coupon it pays, the purchase and
 * maturity days and the next payment — as label-and-value rows, then Upravit. Amounts in the
 * bond's own currency.
 */
function BondDetails({
  row,
  nextPayment,
  onEdit,
}: {
  row: BondRowModel;
  nextPayment?: BondPayment;
  onEdit: () => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bonds');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { bond, face, currency, rate, quantity, unitFace, maturity } = row;
  const coupon = bondYearlyCoupon(bond);

  return (
    <div className="border-t border-line-soft px-3 pb-3 pt-1">
      {bond.isin && <KeyValueRow label={tb('form.isin')} value={bond.isin} />}
      <KeyValueRow label={tb('form.quantity')} value={fmt.number(quantity)} />
      <KeyValueRow label={tb('form.nominal')} value={moneyText(unitFace, currency)} />
      <KeyValueRow label={tb('summary.nominal')} value={moneyText(face, currency)} />
      <KeyValueRow
        label={tb('form.rate')}
        value={fmt.percent(rate / 100, Number.isInteger(rate) ? 0 : 2)}
      />
      <KeyValueRow
        label={tb('summary.yearlyCoupon')}
        value={moneyText(coupon, currency, { signed: coupon !== 0 })}
      />
      <KeyValueRow
        label={tb('form.purchaseDate')}
        value={bond.purchaseDate === null ? t('wealth.bonds.notGiven') : fmt.day(bond.purchaseDate)}
      />
      <KeyValueRow
        label={tb('form.maturity')}
        value={maturity === null ? tb('table.noMaturity') : fmt.day(maturity)}
      />
      {nextPayment && (
        <KeyValueRow
          label={t('wealth.bonds.nextPayment')}
          value={fmt.day(nextPayment.t)}
          sub={`${tb(`table.${nextPayment.kind}`)} · ${moneyText(nextPayment.amount, nextPayment.currency, { signed: true })}`}
        />
      )}
      <Button
        variant="secondary"
        className="mt-2 h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-2 text-m-row"
        onClick={onEdit}
      >
        <Pencil strokeWidth={1.75} aria-hidden />
        {tb('actions.edit')}
      </Button>
    </div>
  );
}
