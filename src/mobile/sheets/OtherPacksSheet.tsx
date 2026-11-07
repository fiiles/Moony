import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RulePackInfo } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { PackRow } from '@/mobile/components/categorization/PackRow';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SearchField } from '@/mobile/components/SearchField';
import { Sheet } from '@/mobile/components/Sheet';

interface OtherPacksSheetProps {
  open: boolean;
  onClose: () => void;
  /** The packs that are off, in display order. */
  packs: readonly RulePackInfo[];
  label: (pack: RulePackInfo) => string;
  /** A pack write is in flight: the switches wait. */
  disabled: boolean;
  onToggle: (pack: RulePackInfo, enabled: boolean) => void;
  onOpenPack: (packId: string) => void;
}

/**
 * "Další země": every rule pack that is off, searchable by country, each with its switch. A pack
 * turned on here moves to the page's own list; the text of a row opens its rules.
 */
export function OtherPacksSheet({
  open,
  onClose,
  packs,
  label,
  disabled,
  onToggle,
  onOpenPack,
}: OtherPacksSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [query, setQuery] = useState('');
  const [wasOpen, setWasOpen] = useState(open);
  if (wasOpen !== open) {
    setWasOpen(open);
    if (open) setQuery('');
  }

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? packs.filter((p) => label(p).toLowerCase().includes(q)) : packs;
  }, [packs, query, label]);

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={t('settings.categorization.otherPacks')}
      full
      fill
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={onClose}
        >
          {tc('buttons.close')}
        </Button>
      }
    >
      <div className="flex min-h-0 flex-1 flex-col">
        <SearchField
          value={query}
          onChange={setQuery}
          placeholder={t('settings.categorization.searchCountry')}
          clearLabel={t('settings.categorization.clearSearch')}
        />
        <div className="min-h-0 flex-1 overflow-y-auto">
          {filtered.length === 0 ? (
            <p className="m-0 text-m-sub text-ink-3">{t('settings.categorization.noMatching')}</p>
          ) : (
            <EntityRows>
              {filtered.map((pack) => (
                <PackRow
                  key={pack.packId}
                  pack={pack}
                  label={label(pack)}
                  disabled={disabled}
                  onToggle={(enabled) => onToggle(pack, enabled)}
                  onOpen={() => onOpenPack(pack.packId)}
                />
              ))}
            </EntityRows>
          )}
        </div>
      </div>
    </Sheet>
  );
}
