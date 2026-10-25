import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Download, Ellipsis, Landmark, PiggyBank } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { NavIconButton } from '@/mobile/shell/NavBar';

interface CashflowMenuProps {
  /** "Účty (3)": the accounts the report covers, already worded. */
  accountsLabel: string;
  /** Open the account scope sheet. */
  onAccounts: () => void;
  /** The route of Spravovat rozpočty, carrying the period and the scope. */
  budgetsHref: string;
  /** Export has nothing to write yet: the period is loading, or its report is not there. */
  exportDisabled: boolean;
  onExport: () => void;
}

/**
 * The `···` of the Cashflow tab (spec §8.7): the account scope (Účty), Spravovat rozpočty, and,
 * under a separator, Export (the shown period's CSV to the Files picker, as the class pages'
 * `···` do). Items are 44 pt tall with an 18 pt icon (`RowMenuItem`); the trigger is the nav bar's
 * 44 pt icon button.
 */
export function CashflowMenu({
  accountsLabel,
  onAccounts,
  budgetsHref,
  exportDisabled,
  onExport,
}: CashflowMenuProps) {
  const { t: tb } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <NavIconButton aria-label={tc('labels.moreActions')}>
          <Ellipsis strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[220px]">
        <RowMenuItem onSelect={onAccounts}>
          <Landmark strokeWidth={1.75} aria-hidden />
          {accountsLabel}
        </RowMenuItem>
        <RowMenuItem onSelect={() => setLocation(budgetsHref)}>
          <PiggyBank strokeWidth={1.75} aria-hidden />
          {tb('manage.title')}
        </RowMenuItem>
        <DropdownMenuSeparator />
        <RowMenuItem disabled={exportDisabled} onSelect={onExport}>
          <Download strokeWidth={1.75} aria-hidden />
          {tc('buttons.export')}
        </RowMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
