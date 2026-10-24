import { useEffect, useId, useRef, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { DOCUMENT_KINDS, type DocumentKind } from '@/mobile/components/documents/document-kind';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { PickGroup, PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';
import { documentNameFrom, fileSizeText, type DocumentSource } from '@/mobile/lib/documents';

/** A file the person has chosen or photographed, and where it came from. */
export interface PickedDocument {
  /** Tells one choice from the next, so that every opening of the sheet starts over. */
  id: number;
  file: File;
  source: DocumentSource;
}

interface AddDocumentSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: DocumentKind;
  /** The policy or property the document belongs to. */
  ownerId: string;
  /** Set together with `open`; the sheet keeps the last one while it slides away. */
  picked: PickedDocument | null;
  fileTypes: readonly string[];
  typeLabel: (fileType: string) => string;
}

/** The type a new document starts with: the desktop's own default, else the first of the list. */
function defaultType(fileTypes: readonly string[]): string {
  return fileTypes.includes('contract') ? 'contract' : (fileTypes[0] ?? 'other');
}

/**
 * "Přidat dokument" (spec §8.8, the phone counterpart of the desktop's add-document dialog): a half
 * sheet for a file that has already been chosen or photographed — the name it starts with, the type
 * as pick rows, Zrušit · Uložit. Nothing has been read from the file yet: Uložit reads it once and
 * sends the bytes to the encrypted store (`addDocumentBytes`), so cancelling costs nothing and
 * writes nothing. A refusal stays in the sheet as the backend's translated sentence.
 *
 * It keeps the file it was last opened with while it slides away; every new file starts a fresh
 * form (`PickedDocument.id`).
 */
export function AddDocumentSheet({ picked, ...props }: AddDocumentSheetProps) {
  const [shown, setShown] = useState(picked);
  if (picked !== null && picked !== shown) setShown(picked);
  if (shown === null) return null;
  return <AddForm key={shown.id} picked={shown} {...props} />;
}

function AddForm({
  open,
  onOpenChange,
  kind,
  ownerId,
  picked,
  fileTypes,
  typeLabel,
}: Omit<AddDocumentSheetProps, 'picked'> & { picked: PickedDocument }) {
  const config = DOCUMENT_KINDS[kind];
  const { t } = useTranslation('mobile');
  const { t: td } = useTranslation(config.namespace);
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const formId = useId();
  const { file, source } = picked;

  const [name, setName] = useState(() =>
    documentNameFrom(file.name, source, t('documents.photoName', { date: fmt.date(new Date()) }))
  );
  const [fileType, setFileType] = useState(() => defaultType(fileTypes));
  const [error, setError] = useState<string | null>(null);

  // A refusal that comes after the sheet was dismissed has no sheet to stay in: it is a toast
  const openRef = useRef(open);
  useEffect(() => {
    openRef.current = open;
  });

  const trimmed = name.trim();

  const save = useMutation({
    mutationFn: async () => {
      // The bytes are read here, once, after the person has confirmed — never when the file was chosen
      const bytes = new Uint8Array(await file.arrayBuffer());
      return config.add(ownerId, bytes, file.name, { name: trimmed, fileType });
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: config.listKey(ownerId) });
      toast(td('documents.added'));
      onOpenChange(false);
    },
    onError: (failure: Error) => {
      const message = translateApiError(failure, tc);
      if (openRef.current) setError(message);
      else toast.error(tc('status.error'), { description: message });
    },
  });

  const canSave = trimmed !== '' && !save.isPending;
  const submit = () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (!canSave) return;
    setError(null);
    save.mutate();
  };

  const fieldId = `${formId}-name`;
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={td('documents.addTitle')}
      // The size is what the encrypting takes time over; the name is the field below (a long file
      // name without spaces would run past the sheet's edge here)
      description={fileSizeText(file.size, fmt)}
      footer={
        <>
          <Button variant="ghost" className={FOOTER_GHOST} onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={`${formId}-form`}
            className={FOOTER_PRIMARY}
            disabled={!canSave}
            loading={save.isPending}
          >
            {save.isPending ? td('documents.uploading') : tc('buttons.save')}
          </Button>
        </>
      }
    >
      <form
        id={`${formId}-form`}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label
          htmlFor={fieldId}
          className="mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2"
        >
          {tc('labels.name')}
        </label>
        <input
          id={fieldId}
          type="text"
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder={td('documents.namePlaceholder')}
          maxLength={200}
          autoComplete="off"
          autoCorrect="off"
          enterKeyHint="done"
          className="h-12 w-full rounded-[10px] border border-line-strong bg-paper px-3.5 text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none"
        />
        <PickGroup title={td('documents.type')}>
          {fileTypes.map((type) => (
            <PickRow
              key={type}
              label={typeLabel(type)}
              checked={type === fileType}
              onSelect={() => setFileType(type)}
            />
          ))}
        </PickGroup>
        {save.isPending && (
          <p role="status" className="mx-0.5 mt-2 text-m-sub text-ink-3">
            {t('documents.saving')}
          </p>
        )}
        {error && (
          <p role="alert" className="mx-0.5 mt-2 text-m-sub font-semibold text-loss">
            {error}
          </p>
        )}
      </form>
    </Sheet>
  );
}
