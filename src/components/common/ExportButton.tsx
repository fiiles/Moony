import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Download, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile } from '@tauri-apps/plugin-fs';
import { useTranslation } from 'react-i18next';

interface ExportButtonProps {
  exportFn: () => Promise<{ csv: string; filename: string; count: number }>;
  /** Tooltip text for the button */
  title?: string;
  /** Visible label ("Export"); without it the button is icon-only. */
  label?: string;
  /** Show only the icon below 1280 px, where a page head with several actions runs out of room. */
  compact?: boolean;
}

/**
 * Reusable export button component that exports data to CSV.
 * Displayed as an icon button matching the import button style.
 */
export function ExportButton({ exportFn, title, label, compact = false }: ExportButtonProps) {
  const [isExporting, setIsExporting] = useState(false);
  const { t } = useTranslation('common');

  const handleExport = async () => {
    try {
      setIsExporting(true);

      // Get CSV data from backend
      const result = await exportFn();

      if (result.count === 0) {
        toast(t('export.noData'), { description: t('export.noDataDescription') });
        return;
      }

      // Open save dialog
      const filePath = await save({
        defaultPath: result.filename,
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });

      if (filePath) {
        // Add UTF-8 BOM for proper encoding detection in Excel
        const BOM = '\uFEFF';
        await writeTextFile(filePath, BOM + result.csv);

        toast(t('export.success'), {
          description: t('export.successDescription', { count: result.count }),
        });
      }
    } catch (error) {
      console.error('Export failed:', error);
      toast.error(t('export.failed'), { description: String(error) });
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <Button
      variant="outline"
      size={label ? 'default' : 'icon'}
      onClick={handleExport}
      disabled={isExporting}
      loading={isExporting}
      title={title || t('export.button')}
      aria-label={label ? undefined : title || t('export.button')}
      className={compact && label ? 'max-xl:px-[11px]' : undefined}
    >
      {isExporting ? <Loader2 /> : <Download />}
      {label && <span className={compact ? 'max-xl:sr-only' : undefined}>{label}</span>}
    </Button>
  );
}
