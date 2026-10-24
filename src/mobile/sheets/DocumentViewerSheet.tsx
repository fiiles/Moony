import { lazy, Suspense, useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Sheet } from '@/mobile/components/Sheet';
import { FOOTER_GHOST_WIDE } from '@/mobile/components/import/styles';
import type { DocumentKind } from '@/mobile/components/documents/document-kind';
import { attachmentUrl } from '@/utils/attachment-url';
import { viewerKind } from '@/utils/attachment-viewer';

// pdf.js and its worker load only when a PDF is opened
const PdfPages = lazy(() => import('@/components/attachments/PdfPages'));

/** What the viewer needs of a document. */
export interface ViewableDocument {
  id: string;
  name: string;
  mimeType: string;
}

interface DocumentViewerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: DocumentKind;
  /** Set together with `open`; the sheet keeps the last one while it slides away. */
  doc: ViewableDocument | null;
  /** The one line under the name ("smlouva · 2,1 MB · 9. 1. 2024"). */
  sub?: string;
}

/**
 * The room the document has inside the tall sheet: 95 % of the screen less the handle, the head and
 * the footer (about 13 rem with the home indicator). The PDF scrolls inside it — `PdfPages` judges
 * which pages are in view against its parent, which therefore must be a box of this size.
 */
const VIEW_MAX_HEIGHT = 'calc(var(--m-vh, 100dvh) * 0.95 - 13rem)';

/**
 * The encrypted viewer (spec §8.8 "the encrypted viewer", README §13 "Document viewer"): the name as
 * the title, the document in a full sheet and Zavřít. An image shows contained (the system pinches
 * it), a PDF page by page (`PdfPages`, the desktop's), anything else says it opens on the
 * computer. The bytes come decrypted through the `moony-file` scheme only while the app is unlocked
 * and nothing is written to disk. There is no "open elsewhere" and no "save a copy" here: both end
 * in a desktop-only command (`open_with_system`, a save dialog).
 */
export function DocumentViewerSheet({
  open,
  onOpenChange,
  kind,
  doc,
  sub,
}: DocumentViewerSheetProps) {
  const { t: tc } = useTranslation('common');
  const [shown, setShown] = useState(doc);
  if (doc !== null && doc !== shown) setShown(doc);

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      full
      title={shown?.name ?? ''}
      description={sub}
      footer={
        <Button variant="ghost" className={FOOTER_GHOST_WIDE} onClick={() => onOpenChange(false)}>
          {tc('buttons.close')}
        </Button>
      }
    >
      {shown && <ViewerBody key={shown.id} kind={kind} doc={shown} />}
    </Sheet>
  );
}

function ViewerBody({ kind, doc }: { kind: DocumentKind; doc: ViewableDocument }) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const view = viewerKind(doc.mimeType);
  const [status, setStatus] = useState<'loading' | 'ready' | 'failed'>('loading');
  // Stable: `PdfPages` starts over whenever one of them changes
  const markReady = useCallback(() => setStatus('ready'), []);
  const markFailed = useCallback(() => setStatus('failed'), []);

  if (view === 'external') {
    return (
      <p className="m-0 px-1 py-6 text-center text-m-body text-ink-3">
        {t('documents.openOnComputer')}
      </p>
    );
  }
  if (status === 'failed') {
    return (
      <p role="alert" className="m-0 px-1 py-6 text-center text-m-body text-ink-3">
        {tc('attachments.viewer.failed')}
      </p>
    );
  }

  const url = attachmentUrl(kind, doc.id);
  const loading = status === 'loading';
  return (
    <div className="relative">
      {loading && (
        <>
          <Skeleton className="absolute inset-0 z-[1] rounded-r3" />
          <span role="status" className="sr-only">
            {tc('attachments.viewer.loading')}
          </span>
        </>
      )}
      {view === 'image' ? (
        <img
          src={url}
          alt={doc.name}
          onLoad={markReady}
          onError={markFailed}
          className="mx-auto block max-w-full rounded-r2 object-contain"
          style={{ maxHeight: VIEW_MAX_HEIGHT, minHeight: loading ? '40dvh' : undefined }}
        />
      ) : (
        <div
          className="overflow-y-auto overscroll-contain rounded-r3 bg-well p-2"
          style={{ maxHeight: VIEW_MAX_HEIGHT, minHeight: loading ? '40dvh' : undefined }}
        >
          <Suspense fallback={null}>
            <PdfPages url={url} onReady={markReady} onError={markFailed} />
          </Suspense>
        </div>
      )}
    </div>
  );
}
