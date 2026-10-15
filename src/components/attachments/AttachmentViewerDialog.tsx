/**
 * In-app viewer for an attached document (D3 of the encrypted-attachments spec): images and PDFs
 * render from the encrypted store through the `moony-file` scheme; nothing is written to disk.
 * "Save a copy" writes a decrypted file where the user chooses; "Open in another app" goes
 * through the warning in `OpenElsewhereDialog`.
 */
import { lazy, Suspense, useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Download, ExternalLink } from 'lucide-react';
import type { AttachmentKind } from '@shared/schema';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { attachmentsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { attachmentUrl } from '@/utils/attachment-url';
import { defaultFileName, viewerKind } from '@/utils/attachment-viewer';

const PdfPages = lazy(() => import('./PdfPages'));

export interface ViewableDocument {
  id: string;
  name: string;
  mimeType: string;
  extension: string | null;
}

interface AttachmentViewerDialogProps {
  kind: Exclude<AttachmentKind, 'photo'>;
  document: ViewableDocument | null;
  onOpenChange: (open: boolean) => void;
  onOpenElsewhere: (document: ViewableDocument) => void;
}

export function AttachmentViewerDialog({
  kind,
  document,
  onOpenChange,
  onOpenElsewhere,
}: AttachmentViewerDialogProps) {
  const { t } = useTranslation('common');
  const [status, setStatus] = useState<'loading' | 'ready' | 'failed'>('loading');
  const [shownId, setShownId] = useState<string | null>(null);
  if (document && document.id !== shownId) {
    setShownId(document.id);
    setStatus('loading');
  }
  const markReady = useCallback(() => setStatus('ready'), []);
  const markFailed = useCallback(() => setStatus('failed'), []);

  const saveCopy = async () => {
    if (!document) return;
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const dest = await save({ defaultPath: defaultFileName(document.name, document.extension) });
      if (!dest) return;
      await attachmentsApi.export(kind, document.id, dest);
      toast(t('attachments.viewer.saved'));
    } catch (error) {
      toast.error(t('status.error'), {
        description: translateApiError(error instanceof Error ? error : String(error), t),
      });
    }
  };

  const url = document ? attachmentUrl(kind, document.id) : '';
  const view = document ? viewerKind(document.mimeType) : 'external';

  return (
    <Dialog open={document !== null} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[88vh] max-w-[920px] flex-col">
        <DialogHeader>
          <DialogTitle className="truncate pr-10">{document?.name}</DialogTitle>
          <DialogDescription className="sr-only">
            {t('attachments.viewer.description')}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto rounded-r3 bg-well p-3">
          {status === 'loading' && (
            <p className="py-10 text-center text-caption text-ink-3">
              {t('attachments.viewer.loading')}
            </p>
          )}
          {status === 'failed' && (
            <p className="py-10 text-center text-caption text-ink-3">
              {t('attachments.viewer.failed')}
            </p>
          )}
          {document && view === 'image' && status !== 'failed' && (
            <img
              src={url}
              alt={document.name}
              onLoad={markReady}
              onError={markFailed}
              className="mx-auto max-h-full max-w-full rounded-r2 object-contain"
            />
          )}
          {document && view === 'pdf' && status !== 'failed' && (
            <Suspense fallback={null}>
              <PdfPages url={url} onReady={markReady} onError={markFailed} />
            </Suspense>
          )}
        </div>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => document && onOpenElsewhere(document)}>
            <ExternalLink /> {t('attachments.viewer.openElsewhere')}
          </Button>
          <Button onClick={saveCopy}>
            <Download /> {t('attachments.viewer.saveCopy')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
