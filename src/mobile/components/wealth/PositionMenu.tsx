import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Ellipsis, Eye, Pencil, Receipt, Tag, Trash2 } from 'lucide-react';
import { useWatchedStocks } from '@/hooks/use-stock-monitor';
import type { StockRow } from '@/utils/positions';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ManualDividendModal } from '@/components/stocks/ManualDividendModal';
import { ManualPriceModal } from '@/components/stocks/ManualPriceModal';
import { usePositionMutations } from '@/mobile/hooks/use-position';
import { PositionRenameSheet } from '@/mobile/sheets/PositionRenameSheet';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { NavIconButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';

/** The classes of a menu item on the phone: 44 pt tall, body type, 18 pt icons. */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

/**
 * The `···` of a position (spec §8.8): Upravit cenu and Odhad dividendy open the desktop's dialogs,
 * Upravit název a sheet, Sledovat adds the ticker to the watched ones (it says "Sledováno" and
 * waits when it already is), Smazat pozici asks in a confirm sheet and, once the position is gone,
 * replaces the screen with the Akcie list. Each of them is wired here with its dialog, so the page
 * only places the button.
 */
export function PositionMenu({ holding }: { holding: StockRow }) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const { remove, rename, follow } = usePositionMutations(holding.id, holding.ticker);
  const { data: watched = [] } = useWatchedStocks();
  const [priceOpen, setPriceOpen] = useState(false);
  const [dividendOpen, setDividendOpen] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);

  const following = watched.some(
    (row) => row.ticker.toUpperCase() === holding.ticker.toUpperCase()
  );

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <NavIconButton aria-label={tc('labels.moreActions')}>
            <Ellipsis strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[240px]">
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => setPriceOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {ts('detail.menu.editPrice')}
          </DropdownMenuItem>
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => setDividendOpen(true)}>
            <Receipt strokeWidth={1.75} aria-hidden />
            {ts('detail.menu.editDividend')}
          </DropdownMenuItem>
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => setRenameOpen(true)}>
            <Tag strokeWidth={1.75} aria-hidden />
            {ts('detail.menu.editName')}
          </DropdownMenuItem>
          <DropdownMenuItem
            className={MENU_ITEM}
            disabled={following || follow.isPending}
            onSelect={() => follow.mutate()}
          >
            <Eye strokeWidth={1.75} aria-hidden />
            {following ? t('wealth.position.following') : t('wealth.position.follow')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            variant="danger"
            className={MENU_ITEM}
            onSelect={() => setDeleteOpen(true)}
          >
            <Trash2 strokeWidth={1.75} aria-hidden />
            {ts('actions.deletePosition')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <ManualPriceModal open={priceOpen} onOpenChange={setPriceOpen} investment={holding} />
      <ManualDividendModal
        open={dividendOpen}
        onOpenChange={setDividendOpen}
        investment={holding}
      />
      <PositionRenameSheet
        open={renameOpen}
        onOpenChange={setRenameOpen}
        name={holding.companyName}
        isPending={rename.isPending}
        onSave={(name) => rename.mutate(name, { onSuccess: () => setRenameOpen(false) })}
      />
      <ConfirmSheet
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={ts('detail.confirmDelete.title')}
        description={ts('detail.confirmDelete.description', {
          name: holding.companyName,
          ticker: holding.ticker,
        })}
        confirmLabel={ts('actions.deletePosition')}
        isPending={remove.isPending}
        onConfirm={() =>
          remove.mutate(undefined, {
            onSuccess: () => {
              setDeleteOpen(false);
              setLocation(routes.stocks, { replace: true });
            },
          })
        }
      />
    </>
  );
}
