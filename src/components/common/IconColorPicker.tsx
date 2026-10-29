/**
 * IconColorPicker - icon grid, color presets, custom color and a preview chip.
 * Shared by the category dialog and the spending-plan dialog. Renders a fragment
 * of form rows, so place it inside a `space-y-*` container.
 */
import { useTranslation } from 'react-i18next';
import { Check } from 'lucide-react';
import { Label } from '@/components/ui/label';
import { CategoryIcon, CategoryIconChip } from '@/components/common/CategoryIcon';
import { ICON_MAP } from '@/components/common/category-icons-map';
import { cn } from '@/lib/utils';

export const DEFAULT_COLOR = '#6366F1';

/** Distinct, readable on light and dark backgrounds. */
// eslint-disable-next-line react-refresh/only-export-components
export const COLOR_PRESETS = [
  '#EF4444',
  '#F97316',
  '#F59E0B',
  '#84CC16',
  '#22C55E',
  '#14B8A6',
  '#06B6D4',
  '#3B82F6',
  '#6366F1',
  '#8B5CF6',
  '#D946EF',
  '#EC4899',
  '#64748B',
  '#9E9E9E',
];

const ICON_NAMES = Object.keys(ICON_MAP);

interface IconColorPickerProps {
  icon: string;
  color: string;
  onIconChange: (icon: string) => void;
  onColorChange: (color: string) => void;
  previewLabel: string;
  previewShape?: 'round' | 'square';
}

export function IconColorPicker({
  icon,
  color,
  onIconChange,
  onColorChange,
  previewLabel,
  previewShape = 'round',
}: IconColorPickerProps) {
  const { t } = useTranslation('bank_accounts');
  return (
    <>
      <div className="space-y-2">
        <Label>{t('categoryManagement.icon')}</Label>
        <div className="grid grid-cols-9 gap-1" role="radiogroup">
          {ICON_NAMES.map((iconName) => (
            <button
              key={iconName}
              type="button"
              role="radio"
              aria-checked={icon === iconName}
              aria-label={iconName}
              onClick={() => onIconChange(iconName)}
              className={cn(
                'flex h-8 w-8 items-center justify-center rounded-r1 border transition-colors',
                icon === iconName
                  ? 'border-dark bg-well-2 text-ink'
                  : 'border-transparent text-ink-3 hover:bg-well'
              )}
            >
              <CategoryIcon iconName={iconName} className="h-4 w-4" />
            </button>
          ))}
        </div>
      </div>

      <div className="space-y-2">
        <Label>{t('categoryManagement.color')}</Label>
        <div className="flex flex-wrap items-center gap-2" role="radiogroup">
          {COLOR_PRESETS.map((preset) => (
            <button
              key={preset}
              type="button"
              role="radio"
              aria-checked={color.toLowerCase() === preset.toLowerCase()}
              aria-label={preset}
              onClick={() => onColorChange(preset)}
              className="flex h-6 w-6 items-center justify-center rounded-full border border-line"
              style={{ backgroundColor: preset }}
            >
              {color.toLowerCase() === preset.toLowerCase() && (
                <Check className="h-3.5 w-3.5 text-white" />
              )}
            </button>
          ))}
          <label className="ml-1 flex items-center gap-2 text-xs text-ink-3">
            <input
              type="color"
              value={color}
              aria-label={t('categoryManagement.customColor')}
              onChange={(e) => onColorChange(e.target.value.toUpperCase())}
              className="h-6 w-8 cursor-pointer rounded border border-line bg-transparent p-0"
            />
            {t('categoryManagement.customColor')}
          </label>
        </div>
      </div>

      <div className="flex items-center gap-2 rounded-r1 border bg-well px-3 py-2">
        <CategoryIconChip category={{ icon, color }} shape={previewShape} />
        <span className="truncate text-sm font-medium">{previewLabel}</span>
      </div>
    </>
  );
}
