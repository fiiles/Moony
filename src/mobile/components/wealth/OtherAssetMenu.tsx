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
import { ValuationHistoryDialog } from '@/components/valuations/ValuationHistoryDialog';
import { RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { useOtherAssetMutations } from '@/mobile/hooks/use-other-asset';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { NavIconButton } from '@/mobile/shell/NavBar';

/**
 * The `···` of an other asset (spec §8.8): Upravit opens the desktop's edit dialog (name, currency
 * and yield), Historie přecenění its list of dated estimates, Smazat asks in a confirm sheet and,
 * once the asset is gone and the sheet's history entry with it, tells the page to leave
 * (`onDeleted`). Přecenit is a button of the page, not an item here. Each item is wired with its
 * dialog, so the page only places the button.
 */
export function OtherAssetMenu({
  asset,
  valuations,
  onDeleted,
}: {
  asset: OtherAsset;
  /** The asset's estimates, oldest first, for the history dialog. */
  valuations: readonly AssetValuation[];
  /** The asset is deleted and its confirm sheet has closed: leave the screen. */
  onDeleted: () => void;
}) {
  const { t } = useTranslation('otherAssets');
  const { t: tc } = useTranslation('common');
  const { remove } = useOtherAssetMutations(asset.id);
  const [editOpen, setEditOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
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
          <RowMenuItem onSelect={() => setEditOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {t('actions.edit')}
          </RowMenuItem>
          <RowMenuItem onSelect={() => setHistoryOpen(true)}>
            <History strokeWidth={1.75} aria-hidden />
            {tc('valuationHistory.title')}
          </RowMenuItem>
          <DropdownMenuSeparator />
          <RowMenuItem variant="danger" onSelect={() => setDeleteOpen(true)}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {t('actions.delete')}
          </RowMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <EditOtherAssetModal asset={asset} open={editOpen} onOpenChange={setEditOpen} />
      <ValuationHistoryDialog
        open={historyOpen}
        onOpenChange={setHistoryOpen}
        kind="otherAsset"
        assetId={asset.id}
        valuations={valuations}
      />
      <ConfirmSheet
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: asset.name })}
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
