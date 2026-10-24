import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FileText, Pencil, Trash2 } from 'lucide-react';
import type { Bond } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { RowMenu, RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { BondRowModel } from '@/mobile/lib/bond-rows';
import { RowBadge } from './RowBadge';

interface BondRowProps {
  row: BondRowModel;
  /** A tap, or Upravit in the menu: the page opens the form with this bond. */
  onEdit: (bond: Bond) => void;
  /** Smazat in the menu: the page asks in its confirm sheet. */
  onDelete: (bond: Bond) => void;
}

/**
 * An issue of the Dluhopisy list (spec §8.8): the bond icon in the chip, the name with its "do
 * roka" or "splaceno" badge over "ISIN · 10 × 1 000 Kč · 5 %" (no ISIN, no first part), the face
 * value over the maturity day or "bez splatnosti". Every amount is the bond's own (native is
 * authoritative, ADR 0012) and goes through the eye. A tap edits the bond (the desktop's form,
 * ruling 4); a long press opens the row's menu, Upravit and Smazat.
 */
export function BondRow({ row, onEdit, onDelete }: BondRowProps) {
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
      onClick={() => onEdit(bond)}
      onLongPress={() => setMenuOpen(true)}
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
