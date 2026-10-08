import { useTranslation } from 'react-i18next';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { Checkbox } from '@/components/ui/checkbox';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';

export function AccountScope({
  value,
  onChange,
}: {
  value: string[] | undefined;
  onChange: (ids: string[] | undefined) => void;
}) {
  const { t } = useTranslation('reports');
  const { accounts } = useBankAccounts();
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm">
          {t('readiness.accounts', { count: value?.length ?? accounts.length })}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-72 space-y-3">
        <p className="text-micro text-ink-4">{t('readiness.accountScopeHelp')}</p>
        <Button variant="ghost" size="sm" onClick={() => onChange(undefined)}>
          {t('readiness.allAccounts')}
        </Button>
        {accounts.map((account) => (
          <label key={account.id} className="flex items-center gap-2 text-table">
            <Checkbox
              checked={!value || value.includes(account.id)}
              onCheckedChange={(checked) => {
                const next = new Set(value ?? accounts.map((a) => a.id));
                if (checked) next.add(account.id);
                else next.delete(account.id);
                onChange([...next]);
              }}
            />
            {account.name}
          </label>
        ))}
      </PopoverContent>
    </Popover>
  );
}
