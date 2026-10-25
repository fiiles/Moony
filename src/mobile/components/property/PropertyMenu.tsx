import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Ellipsis, History, Pencil, Trash2 } from 'lucide-react';
import type { AssetValuation, RealEstate } from '@shared/schema';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { AddRealEstateModal } from '@/components/real-estate/AddRealEstateModal';
import { MaskedValuationHistoryDialog } from '@/mobile/components/MaskedValuationHistoryDialog';
import { NavIconButton } from '@/mobile/shell/NavBar';

/** The classes of a menu item on the phone: 44 pt tall, body type, 18 pt icons. */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

interface PropertyMenuProps {
  property: RealEstate;
  /** The dated estimates, oldest first, for Historie přecenění. */
  valuations: readonly AssetValuation[];
  /** Smazat nemovitost: the screen asks in its confirm sheet and deletes (`useDeleteAndLeave`). */
  onDelete: () => void;
}

/**
 * The `···` of a property (spec §8.8): Upravit opens the desktop's property form (which also
 * keeps the notes and the links of loans and policies, and saves a changed market price as an
 * estimate), Historie přecenění the desktop's list of estimates with its delete, Smazat nemovitost
 * asks the screen to confirm and delete. The delete is the screen's, not this menu's: the menu goes
 * away as soon as the property drops out of the cache, taking its callbacks with it. "Otevřít v
 * kalkulačce" is for the second version (the calculators are not on the phone yet).
 */
export function PropertyMenu({ property, valuations, onDelete }: PropertyMenuProps) {
  const { t } = useTranslation('realEstate');
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
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => setEditOpen(true)}>
            <Pencil strokeWidth={1.75} aria-hidden />
            {t('detail.menu.edit')}
          </DropdownMenuItem>
          <DropdownMenuItem className={MENU_ITEM} onSelect={() => setHistoryOpen(true)}>
            <History strokeWidth={1.75} aria-hidden />
            {t('detail.menu.valuationHistory')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="danger" className={MENU_ITEM} onSelect={onDelete}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {t('detail.menu.delete')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <AddRealEstateModal realEstate={property} open={editOpen} onOpenChange={setEditOpen} />
      <MaskedValuationHistoryDialog
        open={historyOpen}
        onOpenChange={setHistoryOpen}
        kind="realEstate"
        assetId={property.id}
        valuations={valuations}
      />
    </>
  );
}
