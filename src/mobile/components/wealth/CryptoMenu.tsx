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
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { NavIconButton } from '@/mobile/shell/NavBar';

/**
 * The `···` of a coin (spec §8.8): Upravit cenu opens the desktop's rate dialog, Smazat pozici
 * asks the page to confirm and delete (`onDelete`). The delete is the page's, not this menu's: the
 * menu goes away as soon as the coin drops out of the list the page reads, taking its callbacks
 * with it. There is no rename, dividend or follow for a coin, so these are all. The rate dialog is
 * wired here, so the page only places the button.
 */
export function CryptoMenu({
  holding,
  onDelete,
}: {
  holding: CryptoRow;
  /** Smazat pozici: the screen asks in its confirm sheet and deletes (`useDeleteAndLeave`). */
  onDelete: () => void;
}) {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const [priceOpen, setPriceOpen] = useState(false);

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
          <RowMenuItem variant="danger" onSelect={onDelete}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {t('detail.menu.delete')}
          </RowMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <UpdateCryptoPriceModal open={priceOpen} onOpenChange={setPriceOpen} investment={holding} />
    </>
  );
}
