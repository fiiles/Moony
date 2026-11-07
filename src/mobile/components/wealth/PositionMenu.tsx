import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Coins, Ellipsis, Eye, Pencil, Receipt, RefreshCw, Split, Tag, Trash2 } from 'lucide-react';
import { useWatchedStocks } from '@/hooks/use-stock-monitor';
import type { StockRow } from '@/utils/positions';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { useRefreshStockPrices } from '@/mobile/hooks/use-stock-positions';
import { ManualDividendModal } from '@/components/stocks/ManualDividendModal';
import { ManualPriceModal } from '@/components/stocks/ManualPriceModal';
import { usePositionMutations } from '@/mobile/hooks/use-position';
import { PositionRenameSheet } from '@/mobile/sheets/PositionRenameSheet';
import { NavIconButton } from '@/mobile/shell/NavBar';

/**
 * The `···` of a position (spec §8.8): Zapsat dividendu asks the page for its sheet
 * (`onAddDividend`), Zapsat split for the splits dialog (`onSplits`), Upravit cenu and Odhad
 * dividendy open the desktop's dialogs, Upravit název a sheet, Sledovat adds the ticker to the
 * watched ones (it says "Sledováno" and waits when it already is), Smazat pozici asks the page to confirm and delete (`onDelete`). The delete is the
 * page's, not this menu's: the menu goes away as soon as the position drops out of what the page
 * reads, taking its callbacks with it. The other items are wired here with their dialogs, so the
 * page only places the button.
 */
export function PositionMenu({
  holding,
  onDelete,
  onAddDividend,
  onSplits,
}: {
  holding: StockRow;
  /** Zapsat dividendu: the page owns the sheet (it also opens it to edit a row's dividend). */
  onAddDividend: () => void;
  /** Zapsat split: the page owns the splits dialog (the applied splits open it too). */
  onSplits: () => void;
  /** Smazat pozici: the screen asks in its confirm sheet and deletes (`useDeleteAndLeave`). */
  onDelete: () => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const { rename, follow } = usePositionMutations(holding.id, holding.ticker);
  // Akcie's "Aktualizovat ceny", here too (spec §8.8, C11): the detail reads the same prices
  const refresh = useRefreshStockPrices();
  const { data: watched = [] } = useWatchedStocks();
  const [priceOpen, setPriceOpen] = useState(false);
  const [dividendOpen, setDividendOpen] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);

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
          <RowMenuItem disabled={refresh.isPending} onSelect={() => refresh.mutate()}>
            <RefreshCw strokeWidth={1.75} aria-hidden />
            {ts('refreshPrices')}
          </RowMenuItem>
          <RowMenuItem onSelect={onAddDividend}>
            <Coins strokeWidth={1.75} aria-hidden />
            {ts('detail.dividendRecords.add')}
          </RowMenuItem>
          <DropdownMenuSeparator />
          <RowMenuItem onSelect={() => setPriceOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {ts('detail.menu.editPrice')}
          </RowMenuItem>
          <RowMenuItem onSelect={() => setDividendOpen(true)}>
            <Receipt strokeWidth={1.75} aria-hidden />
            {ts('detail.menu.editDividend')}
          </RowMenuItem>
          <RowMenuItem onSelect={() => setRenameOpen(true)}>
            <Tag strokeWidth={1.75} aria-hidden />
            {ts('detail.menu.editName')}
          </RowMenuItem>
          <RowMenuItem disabled={following || follow.isPending} onSelect={() => follow.mutate()}>
            <Eye strokeWidth={1.75} aria-hidden />
            {following ? t('wealth.position.following') : t('wealth.position.follow')}
          </RowMenuItem>
          <RowMenuItem onSelect={onSplits}>
            <Split strokeWidth={1.75} aria-hidden />
            {ts('detail.menu.recordSplit')}
          </RowMenuItem>
          <DropdownMenuSeparator />
          <RowMenuItem variant="danger" onSelect={onDelete}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {ts('actions.deletePosition')}
          </RowMenuItem>
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
    </>
  );
}
