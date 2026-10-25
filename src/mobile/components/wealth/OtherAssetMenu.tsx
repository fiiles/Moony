import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, History, Pencil, Trash2 } from 'lucide-react';
import type { AssetValuation, OtherAsset } from '@shared/schema';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { EditOtherAssetModal } from '@/components/other-assets/EditOtherAssetModal';
import { MaskedValuationHistoryDialog } from '@/mobile/components/MaskedValuationHistoryDialog';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { NavIconButton } from '@/mobile/shell/NavBar';

/**
 * The `···` of an other asset (spec §8.8): Upravit opens the desktop's edit dialog (name, currency
 * and yield), Historie přecenění its list of dated estimates, Smazat asks the page to confirm and
 * delete (`onDelete`). The delete is the page's, not this menu's: the menu goes away as soon as the
 * asset drops out of the list the page reads, taking its callbacks with it. Přecenit is a button of
 * the page, not an item here. The other items are wired with their dialogs, so the page only
 * places the button.
 */
export function OtherAssetMenu({
  asset,
  valuations,
  onDelete,
}: {
  asset: OtherAsset;
  /** The asset's estimates, oldest first, for the history dialog. */
  valuations: readonly AssetValuation[];
  /** Smazat: the screen asks in its confirm sheet and deletes (`useDeleteAndLeave`). */
  onDelete: () => void;
}) {
  const { t } = useTranslation('otherAssets');
  const { t: tc } = useTranslation('common');
  const [editOpen, setEditOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <NavIconButton aria-label={tc('labels.moreActions')}>
            <Ellipsis strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[240px]">
          <RowMenuItem onSelect={() => setEditOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {t('actions.edit')}
          </RowMenuItem>
          <RowMenuItem onSelect={() => setHistoryOpen(true)}>
            <History strokeWidth={1.75} aria-hidden />
            {tc('valuationHistory.title')}
          </RowMenuItem>
          <DropdownMenuSeparator />
          <RowMenuItem variant="danger" onSelect={onDelete}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {t('actions.delete')}
          </RowMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <EditOtherAssetModal asset={asset} open={editOpen} onOpenChange={setEditOpen} />
      <MaskedValuationHistoryDialog
        open={historyOpen}
        onOpenChange={setHistoryOpen}
        kind="otherAsset"
        assetId={asset.id}
        valuations={valuations}
      />
    </>
  );
}
