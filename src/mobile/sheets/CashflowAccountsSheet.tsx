import { useTranslation } from 'react-i18next';
import type { BankAccountWithInstitution } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Sheet } from '@/mobile/components/Sheet';

interface CashflowAccountsSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accounts: BankAccountWithInstitution[];
  /** The accounts in the report; `undefined` is all of them. */
  value: string[] | undefined;
  onChange: (ids: string[] | undefined) => void;
}

/**
 * "Účty v přehledu" (spec §8.7, the `···` menu): which accounts the report counts, as the
 * desktop's account scope. One 52 pt row per account — a checkbox, the name, the bank under it —
 * and "Všechny účty" to go back to all of them. A change applies at once (the report behind the
 * sheet reloads); the one button closes the sheet. The last account in the report cannot be
 * unchecked: a report of no accounts is not a thing, and an empty list would read back as all.
 */
export function CashflowAccountsSheet({
  open,
  onOpenChange,
  accounts,
  value,
  onChange,
}: CashflowAccountsSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const selected = (id: string) => !value || value.includes(id);
  const selectedCount = accounts.filter((account) => selected(account.id)).length;

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('cashflow.accounts.title')}
      description={tr('readiness.accountScopeHelp')}
      footer={
        <Button
          className="h-12 flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {t('cashflow.accounts.done')}
        </Button>
      }
    >
      <Button
        variant="ghost"
        className="relative mb-1 h-[34px] rounded-[9px] px-3 text-m-sub font-semibold after:absolute after:inset-x-0 after:-inset-y-[5px] after:content-['']"
        onClick={() => onChange(undefined)}
      >
        {tr('readiness.allAccounts')}
      </Button>
      <ul className="m-0 list-none p-0">
        {accounts.map((account) => {
          const checked = selected(account.id);
          return (
            <li key={account.id} className="border-t border-line-soft first:border-t-0">
              <label className="flex min-h-[52px] items-center gap-3 px-1 py-2">
                <Checkbox
                  checked={checked}
                  disabled={checked && selectedCount === 1}
                  onCheckedChange={(next) => {
                    const ids = new Set(value ?? accounts.map((a) => a.id));
                    if (next === true) ids.add(account.id);
                    else ids.delete(account.id);
                    onChange([...ids]);
                  }}
                />
                <span className="min-w-0">
                  <span className="block truncate text-m-row text-ink">{account.name}</span>
                  {account.institution && (
                    <span className="block truncate text-m-sub text-ink-4">
                      {account.institution.name}
                    </span>
                  )}
                </span>
              </label>
            </li>
          );
        })}
      </ul>
    </Sheet>
  );
}
