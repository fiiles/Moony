import { useTranslation } from 'react-i18next';
import { Ellipsis, Lock, LockOpen, Trash2 } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { NavIconButton } from '@/mobile/shell/NavBar';

/** The classes of a menu item on the phone: 44 pt tall, body type, 18 pt icons. */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

interface PolicyMenuProps {
  /** An ended policy offers Obnovit, a running one Ukončit. */
  ended: boolean;
  /** Ukončit: the screen asks for the end day in the desktop's dialog. */
  onEnd: () => void;
  /** Obnovit: the screen reactivates the policy. */
  onReactivate: () => void;
  /** Smazat: the screen asks in its confirm sheet and deletes (`useDeleteAndLeave`). */
  onDelete: () => void;
}

/**
 * The `···` of a policy (spec §8.8 "Pojistka"): Ukončit (or Obnovit for an ended policy) and
 * Smazat. The menu only asks; the dialog, the confirm sheet and the mutations are the screen's, as
 * the loan's delete is: the menu goes away as soon as the policy drops out of the list, taking its
 * callbacks with it. Upravit is not here: it is a full-width button under the facts card.
 */
export function PolicyMenu({ ended, onEnd, onReactivate, onDelete }: PolicyMenuProps) {
  const { t } = useTranslation('insurance');
  const { t: tc } = useTranslation('common');

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <NavIconButton aria-label={tc('labels.moreActions')}>
          <Ellipsis strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[240px]">
        {ended ? (
          <DropdownMenuItem className={MENU_ITEM} onSelect={onReactivate}>
            <LockOpen strokeWidth={1.75} aria-hidden />
            {t('actions.reactivate')}
          </DropdownMenuItem>
        ) : (
          <DropdownMenuItem className={MENU_ITEM} onSelect={onEnd}>
            <Lock strokeWidth={1.75} aria-hidden />
            {t('actions.end')}
          </DropdownMenuItem>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem variant="danger" className={MENU_ITEM} onSelect={onDelete}>
          <Trash2 strokeWidth={1.75} aria-hidden />
          {t('actions.delete')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
