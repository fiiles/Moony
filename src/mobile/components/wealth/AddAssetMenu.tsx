import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Bitcoin, FileText, Gem, House, TrendingUp, type LucideIcon } from 'lucide-react';
import { AddCryptoModal } from '@/components/crypto/AddCryptoModal';
import { AddOtherAssetModal } from '@/components/other-assets/AddOtherAssetModal';
import { AddRealEstateModal } from '@/components/real-estate/AddRealEstateModal';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { BondsFormDialog } from '@/components/bonds/BondsFormDialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useBondMutations } from '@/hooks/use-bond-mutations';

/** What the `+` can add: the five kinds of asset Majetek lists (spec §6). */
export type AddKind = 'investment' | 'crypto' | 'bond' | 'property' | 'asset';

const ITEMS: { kind: AddKind; icon: LucideIcon }[] = [
  { kind: 'investment', icon: TrendingUp },
  { kind: 'crypto', icon: Bitcoin },
  { kind: 'bond', icon: FileText },
  { kind: 'property', icon: House },
  { kind: 'asset', icon: Gem },
];

interface AddAssetMenuProps {
  /** The button that opens the menu (the nav bar's `+`, the empty state's button). */
  trigger: ReactNode;
  /**
   * The dialog that is open. Given, the page controls it — so a row (an empty class) can open a
   * dialog without the menu; left out, the menu keeps its own state.
   */
  open?: AddKind | null;
  onOpenChange?: (kind: AddKind | null) => void;
}

/**
 * The typed `+` of Majetek (spec §6): a menu of the five kinds of asset — investici, kryptoměnu,
 * dluhopis, nemovitost, aktivum — and the five desktop add dialogs it opens, mounted once here
 * (the phone has no add forms of its own yet). The dialogs write through the desktop's own
 * mutations, so the mutation contract and the snapshot are theirs.
 */
export function AddAssetMenu({ trigger, open, onOpenChange }: AddAssetMenuProps) {
  const { t } = useTranslation('mobile');
  const { createMutation: createBond } = useBondMutations();
  const [inner, setInner] = useState<AddKind | null>(null);
  const kind = open !== undefined ? open : inner;
  const setKind = (next: AddKind | null) => {
    if (open === undefined) setInner(next);
    onOpenChange?.(next);
  };
  const close = () => setKind(null);
  const closeWhenDismissed = (isOpen: boolean) => {
    if (!isOpen) close();
  };

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[220px]">
          <DropdownMenuLabel>{t('wealth.add.label')}</DropdownMenuLabel>
          {ITEMS.map(({ kind: itemKind, icon: Icon }) => (
            <DropdownMenuItem
              key={itemKind}
              className="min-h-11 text-m-body [&_svg]:size-[18px]"
              onSelect={() => setKind(itemKind)}
            >
              <Icon strokeWidth={1.75} aria-hidden />
              {t(`wealth.add.${itemKind}`)}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      <AddInvestmentModal open={kind === 'investment'} onOpenChange={closeWhenDismissed} />
      <AddCryptoModal open={kind === 'crypto'} onOpenChange={closeWhenDismissed} />
      <BondsFormDialog
        open={kind === 'bond'}
        onOpenChange={closeWhenDismissed}
        onSubmit={(data) => createBond.mutate(data, { onSuccess: close })}
        isLoading={createBond.isPending}
      />
      <AddRealEstateModal open={kind === 'property'} onOpenChange={closeWhenDismissed} />
      <AddOtherAssetModal open={kind === 'asset'} onOpenChange={closeWhenDismissed} />
    </>
  );
}
