import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { bankAccountsApi } from '@/lib/tauri-api';
import { BankGuides } from '@/mobile/components/import/ExportGuide';
import { Sheet } from '@/mobile/components/Sheet';
import { guidesList } from '@/mobile/lib/export-guide';

interface ExportGuidesSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Jak získat výpis z banky" from Nastavení → Import (spec §8.6, rung 2): the banks that have a
 * guide (`BankGuides`, the list the import sheet offers for a file it cannot read) in a tall
 * sheet, the tapped bank's three steps in place. The presets are the ones the import reads
 * (`getCsvPresets`, shipped with the app); nothing goes to the network until a guide's own
 * "Otevřít internetové bankovnictví" is tapped.
 */
export function ExportGuidesSheet({ open, onOpenChange }: ExportGuidesSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { data: presets = [] } = useQuery({
    queryKey: ['csv-presets'],
    queryFn: () => bankAccountsApi.getCsvPresets(),
    staleTime: Infinity,
    enabled: open,
  });
  const guides = useMemo(() => guidesList(presets), [presets]);

  return (
    <Sheet
      full
      open={open}
      onOpenChange={onOpenChange}
      title={t('settings.import.guidesRow')}
      footer={
        <Button
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.close')}
        </Button>
      }
    >
      <BankGuides presets={guides} />
    </Sheet>
  );
}
