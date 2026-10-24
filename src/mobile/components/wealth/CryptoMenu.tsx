import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Pencil, Trash2 } from 'lucide-react';
import type { CryptoRow } from '@/utils/positions';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { UpdateCryptoPriceModal } from '@/components/crypto/UpdateCryptoPriceModal';
import { RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { useDeleteCrypto } from '@/mobile/hooks/use-crypto';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { NavIconButton } from '@/mobile/shell/NavBar';

/**
 * The `···` of a coin (spec §8.8): Upravit cenu opens the desktop's rate dialog, Smazat pozici
 * asks in a confirm sheet and, once the coin is gone and the sheet's history entry with it, tells
 * the page to leave (`onDeleted`). There is no rename, dividend or follow for a coin, so these are
 * all. Each item is wired here with its dialog, so the page only places the button.
 */
export function CryptoMenu({
  holding,
  onDeleted,
}: {
  holding: CryptoRow;
  /** The coin is deleted and its confirm sheet has closed: leave the screen. */
  onDeleted: () => void;
}) {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const remove = useDeleteCrypto(holding.id);
  const [priceOpen, setPriceOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <NavIconButton aria-label={tc('labels.moreActions')}>
            <Ellipsis strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[240px]">
          <RowMenuItem onSelect={() => setPriceOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {t('detail.menu.editPrice')}
          </RowMenuItem>
          <DropdownMenuSeparator />
          <RowMenuItem variant="danger" onSelect={() => setDeleteOpen(true)}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {t('detail.menu.delete')}
          </RowMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <UpdateCryptoPriceModal open={priceOpen} onOpenChange={setPriceOpen} investment={holding} />
      <ConfirmSheet
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', {
          name: holding.name,
          ticker: holding.ticker,
        })}
        confirmLabel={t('actions.delete')}
        isPending={remove.isPending}
        onConfirm={() =>
          remove.mutate(undefined, {
            // The sheet takes its own history entry off first, then the page goes back
            onSuccess: () => leaveAfterSheet(() => setDeleteOpen(false), onDeleted),
          })
        }
      />
    </>
  );
}
