import { FileText, Image as ImageIcon, Trash2 } from 'lucide-react';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { RowMenu, RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { viewerKind } from '@/utils/attachment-viewer';
import type { PhoneDocument } from './document-kind';

interface DocumentRowProps {
  doc: PhoneDocument;
  /** "smlouva · 2,1 MB · 9. 1. 2024" (`documentSub`). */
  sub: string;
  /** The `···` trigger's accessible name, already worded ("Další akce: Pojistná smlouva"). */
  menuLabel: string;
  /** "Smazat". */
  deleteLabel: string;
  menuOpen: boolean;
  onMenuOpenChange: (open: boolean) => void;
  onOpen: () => void;
  onDelete: () => void;
}

/**
 * A document of a policy or a property (README §13 "Documents section", prototype frame 14): an
 * entity row with a square chip — a page, or a picture for what the viewer shows as one — the
 * name, "typ · velikost · den" and a chevron (no trailing value). A tap opens the viewer; the long
 * press opens the row menu with Smazat, whose `···` trigger is the way in for a screen reader.
 */
export function DocumentRow({
  doc,
  sub,
  menuLabel,
  deleteLabel,
  menuOpen,
  onMenuOpenChange,
  onOpen,
  onDelete,
}: DocumentRowProps) {
  const Icon = viewerKind(doc.mimeType) === 'image' ? ImageIcon : FileText;
  return (
    <EntityRow
      chip={
        <RowChip square>
          <Icon strokeWidth={1.75} />
        </RowChip>
      }
      title={doc.name}
      sub={sub}
      chevron
      onClick={onOpen}
      onLongPress={() => onMenuOpenChange(true)}
      accessory={
        <RowMenu open={menuOpen} onOpenChange={onMenuOpenChange} label={menuLabel}>
          <RowMenuItem variant="danger" onSelect={onDelete}>
            <Trash2 strokeWidth={1.75} aria-hidden />
            {deleteLabel}
          </RowMenuItem>
        </RowMenu>
      }
    />
  );
}
