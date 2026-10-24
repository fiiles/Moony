import { useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import {
  DOCUMENT_ACCEPT,
  documentSub,
  pickedFileProblem,
  type DocumentSource,
} from '@/mobile/lib/documents';
import { AddDocumentSheet, type PickedDocument } from '@/mobile/sheets/AddDocumentSheet';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { DocumentViewerSheet } from '@/mobile/sheets/DocumentViewerSheet';
import { CaptureButtons } from './CaptureButtons';
import { DocumentRow } from './DocumentRow';
import { DOCUMENT_KINDS, type DocumentKind, type PhoneDocument } from './document-kind';

interface DocumentsSectionProps {
  kind: DocumentKind;
  /** The policy or property the documents belong to. */
  ownerId: string;
  /** The types the add sheet offers, in order (a policy's: contract, certificate, claim, other). */
  fileTypes: readonly string[];
  /** The type's label in the caller's own namespace ("smlouva"). */
  typeLabel: (fileType: string) => string;
}

/**
 * "Dokumenty" of a policy or a property (spec §8.8, README §13 "Documents section", prototype frame
 * 14): the head with "2 · šifrované", the documents newest first (a tap opens the encrypted viewer,
 * a long press Smazat), a line saying there are none, and the two buttons that bring a document in —
 * Vyfotit and Vybrat soubor. A file the phone cannot keep (HEIC, an empty one, one over 100 MB) is
 * turned away with a toast before anything is read; every other one opens the add sheet, which reads
 * and sends it only when the person confirms. The list is the desktop's (`['insurance-documents',
 * id]`, `['real-estate-documents', id]`), so a background conversion that finishes refreshes it.
 */
export function DocumentsSection({ kind, ownerId, fileTypes, typeLabel }: DocumentsSectionProps) {
  const config = DOCUMENT_KINDS[kind];
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation(config.namespace);
  const { t: tr } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();

  const listKey = config.listKey(ownerId);
  const {
    data: documents = [],
    isLoading,
    error: loadError,
  } = useQuery<PhoneDocument[]>({
    queryKey: listKey,
    queryFn: () => config.list(ownerId),
  });
  const rows = [...documents].sort((a, b) => b.uploadedAt - a.uploadedAt);

  // The sheets keep their document while they slide away: what is shown and whether it is open are apart
  const [picked, setPicked] = useState<PickedDocument | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const pickCount = useRef(0);
  const [viewing, setViewing] = useState<PhoneDocument | null>(null);
  const [viewOpen, setViewOpen] = useState(false);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<PhoneDocument | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const remove = useMutation({
    mutationFn: (documentId: string) => config.remove(documentId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: listKey });
      setConfirmOpen(false);
      toast(td('documents.deleted'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const sub = (doc: PhoneDocument) => documentSub(doc, typeLabel, fmt);

  const chosen = (files: File[], source: DocumentSource) => {
    const file = files[0];
    if (!file) return;
    const problem = pickedFileProblem(file);
    if (problem === 'heic') toast.error(tr('gallery.errors.heicUnsupported', { name: file.name }));
    else if (problem === 'empty') toast.error(tc('validation.fileRequired'));
    else if (problem === 'tooLarge') toast.error(tc('validation.attachmentTooLarge'));
    else {
      pickCount.current += 1;
      setPicked({ id: pickCount.current, file, source });
      setAddOpen(true);
    }
  };

  return (
    <>
      <SectionHead
        title={td(config.headKey)}
        note={rows.length > 0 ? t('documents.count', { count: rows.length }) : undefined}
      />
      {loadError ? (
        <p role="alert" className="mx-0.5 mb-3.5 text-m-sub text-ink-3">
          {translateApiError(loadError, tc)}
        </p>
      ) : isLoading ? (
        <p role="status" className="mx-0.5 mb-3.5 text-m-sub text-ink-3">
          {tc('status.loading')}
        </p>
      ) : rows.length === 0 ? (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{td('documents.noDocuments')}</p>
      ) : (
        <EntityRows>
          {rows.map((doc) => (
            <DocumentRow
              key={doc.id}
              doc={doc}
              sub={sub(doc)}
              menuLabel={t('documents.rowMenu', { name: doc.name })}
              deleteLabel={td('documents.delete')}
              menuOpen={menuFor === doc.id}
              onMenuOpenChange={(next) => setMenuFor(next ? doc.id : null)}
              onOpen={() => {
                setViewing(doc);
                setViewOpen(true);
              }}
              onDelete={() => {
                setPendingDelete(doc);
                setConfirmOpen(true);
              }}
            />
          ))}
        </EntityRows>
      )}
      <CaptureButtons
        cameraLabel={t('documents.takePhoto')}
        pickLabel={t('documents.chooseFile')}
        pickAccept={DOCUMENT_ACCEPT}
        onFiles={chosen}
      />
      <AddDocumentSheet
        open={addOpen}
        onOpenChange={setAddOpen}
        kind={kind}
        ownerId={ownerId}
        picked={picked}
        fileTypes={fileTypes}
        typeLabel={typeLabel}
      />
      <DocumentViewerSheet
        open={viewOpen}
        onOpenChange={setViewOpen}
        kind={kind}
        doc={viewing}
        sub={viewing ? sub(viewing) : undefined}
      />
      <ConfirmSheet
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={td('documents.deleteConfirm.title')}
        description={td('documents.deleteConfirm.description', { name: pendingDelete?.name ?? '' })}
        confirmLabel={td('documents.delete')}
        isPending={remove.isPending}
        onConfirm={() => {
          if (pendingDelete) remove.mutate(pendingDelete.id);
        }}
      />
    </>
  );
}
