/**
 * One way to open an attached document everywhere: images and PDFs in the in-app viewer, every
 * other type in its system app after the D3 warning (unless the user opted out on this device).
 */
import { useState, type ReactNode } from 'react';
import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { AttachmentKind } from '@shared/schema';
import { deviceSyncApi, insuranceApi, realEstateApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { readSetting, writeSetting } from '@/utils/device-storage';
import { attachmentView } from '@/utils/attachment-content';
import { OPEN_ELSEWHERE_CONFIRMED_KEY, viewerKind } from '@/utils/attachment-viewer';
import { AttachmentViewerDialog, type ViewableDocument } from './AttachmentViewerDialog';
import { OpenElsewhereDialog } from './OpenElsewhereDialog';

type DocumentKind = Exclude<AttachmentKind, 'photo'>;

const openInSystemApp = (kind: DocumentKind, id: string) =>
  kind === 'insurance-document' ? insuranceApi.openDocument(id) : realEstateApi.openDocument(id);

export function useDocumentOpener(kind: DocumentKind): {
  openDocument: (document: ViewableDocument) => void;
  /** "Try again" on a document whose content failed to move; resolves when the ask is made. */
  retryContent: (id: string) => void;
  /** The row whose "Try again" is in flight, for its busy button. */
  retryingId: string | null;
  dialogs: ReactNode;
} {
  const { t } = useTranslation('common');
  const [viewing, setViewing] = useState<ViewableDocument | null>(null);
  const [confirming, setConfirming] = useState<ViewableDocument | null>(null);

  // The list and the sync status refresh through the `sync-attachments-changed` event; this
  // changes no financial data, so no snapshot and no cache writes by hand.
  const retryMutation = useMutation({
    mutationFn: (id: string) => deviceSyncApi.retryAttachment(kind, id),
    onError: (error: Error) => {
      toast.error(t('status.error'), { description: translateApiError(error, t) });
    },
  });

  const openElsewhere = async (document: ViewableDocument) => {
    try {
      await openInSystemApp(kind, document.id);
    } catch (error) {
      toast.error(t('status.error'), {
        description: translateApiError(error instanceof Error ? error : String(error), t),
      });
    }
  };

  const requestOpenElsewhere = (document: ViewableDocument) => {
    if (readSetting(OPEN_ELSEWHERE_CONFIRMED_KEY) === '1') void openElsewhere(document);
    else setConfirming(document);
  };

  const openDocument = (document: ViewableDocument) => {
    // Content that is not here has no `moony-file` URL to build: the protocol would answer 425/404.
    if (!attachmentView(document.content, 'desktop').openable) return;
    if (viewerKind(document.mimeType) === 'external') requestOpenElsewhere(document);
    else setViewing(document);
  };

  const dialogs = (
    <>
      <AttachmentViewerDialog
        kind={kind}
        document={viewing}
        onOpenChange={(open) => !open && setViewing(null)}
        onOpenElsewhere={requestOpenElsewhere}
      />
      <OpenElsewhereDialog
        open={confirming !== null}
        onOpenChange={(open) => !open && setConfirming(null)}
        onConfirm={(dontAskAgain) => {
          if (dontAskAgain) writeSetting(OPEN_ELSEWHERE_CONFIRMED_KEY, '1');
          const document = confirming;
          setConfirming(null);
          if (document) void openElsewhere(document);
        }}
      />
    </>
  );

  return {
    openDocument,
    retryContent: (id: string) => retryMutation.mutate(id),
    retryingId: retryMutation.isPending ? (retryMutation.variables ?? null) : null,
    dialogs,
  };
}
