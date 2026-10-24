import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from 'react';
import { useTranslation } from 'react-i18next';
import { ZoomIn, ZoomOut } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { Sheet } from '@/mobile/components/Sheet';
import { FOOTER_GHOST_WIDE } from '@/mobile/components/import/styles';
import type { DocumentKind } from '@/mobile/components/documents/document-kind';
import { isDoubleTap, scrollAfterZoom, type TapPoint } from '@/mobile/lib/documents';
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

type Status = 'loading' | 'ready' | 'failed';

/**
 * The room the document has inside the tall sheet: 95 % of the screen less the handle, the head and
 * the footer (about 13 rem with the home indicator). The box scrolls inside it — `PdfPages` judges
 * which pages are in view against its parent, which therefore must be a box of this size.
 */
const VIEW_MAX_HEIGHT = 'calc(var(--m-vh, 100dvh) * 0.95 - 13rem)';

/** The zoom step: the content is this many times the box's width. */
const ZOOM = 2;

/** A tap moves its finger less than this many px; a longer one is a drag. */
const TAP_SLOP = 10;

/**
 * The encrypted viewer (spec §8.8 "the encrypted viewer", README §13 "Document viewer"): the name as
 * the title, the document in a full sheet, a zoom button and Zavřít. An image shows contained, a PDF
 * page by page (`PdfPages`, the desktop's), anything else says it opens on the computer. A double
 * tap — or the zoom button, for anyone who cannot double-tap — toggles 1× and 2× (the content twice
 * the box's width, the box scrolling both ways, a new document opening at 1×); the system's pinch
 * (allowed since the viewport is no longer locked to scale 1, README §13 "Dynamic Type") zooms the
 * whole page with the sheet, not the document in its box. The bytes come decrypted through the
 * `moony-file` scheme only while the app is unlocked and nothing is written to disk: the picture
 * and the pages cannot be selected, dragged or long-pressed into the system's Copy / Share / Save
 * menu. There is no "open elsewhere" and no "save a copy" here: both end in a desktop-only command
 * (`open_with_system`, a save dialog).
 */
export function DocumentViewerSheet({
  open,
  onOpenChange,
  kind,
  doc,
  sub,
}: DocumentViewerSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [shown, setShown] = useState(doc);
  const [status, setStatus] = useState<Status>('loading');
  const [zoomed, setZoomed] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);
  const box = useRef<HTMLDivElement>(null);
  // Where the box is to be scrolled once the zoomed content is laid out
  const scrollTarget = useRef<{ left: number; top: number } | null>(null);

  // A new document, and every opening, starts at 1× and loads
  const restart = () => {
    setStatus('loading');
    setZoomed(false);
  };
  if (doc !== null && doc !== shown) {
    setShown(doc);
    restart();
  }
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) restart();
  }

  const view = shown ? viewerKind(shown.mimeType) : 'external';
  // Stable: `PdfPages` starts over whenever one of them changes
  const markReady = useCallback(() => setStatus('ready'), []);
  const markFailed = useCallback(() => setStatus('failed'), []);

  /** 1× ↔ 2×, keeping `at` (a point of the screen; the box's middle without one) where it is. */
  const toggleZoom = (at?: { x: number; y: number }) => {
    const element = box.current;
    if (!element || status !== 'ready') return;
    const rect = element.getBoundingClientRect();
    const pointerX = at ? at.x - rect.left : rect.width / 2;
    const pointerY = at ? at.y - rect.top : rect.height / 2;
    const [from, to] = zoomed ? [ZOOM, 1] : [1, ZOOM];
    scrollTarget.current = {
      left: scrollAfterZoom(element.scrollLeft, pointerX, from, to),
      top: scrollAfterZoom(element.scrollTop, pointerY, from, to),
    };
    setZoomed(!zoomed);
    // A PDF draws its pages at the width it finds, so they are drawn again at the new one
    if (view === 'pdf') setStatus('loading');
  };

  // The scroll waits for the content: an image has its new size at once, a PDF once its first page is drawn
  useLayoutEffect(() => {
    const element = box.current;
    const target = scrollTarget.current;
    if (!element || !target || status !== 'ready') return;
    scrollTarget.current = null;
    element.scrollLeft = target.left;
    element.scrollTop = target.top;
  }, [zoomed, status]);

  // A scroll that was never applied (the sheet closed first) is not for the next document
  useEffect(() => {
    if (!open) scrollTarget.current = null;
  }, [open]);

  const canZoom = view !== 'external' && status !== 'failed';
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      full
      title={shown?.name ?? ''}
      description={sub}
      footer={
        <>
          {canZoom && (
            <Button
              type="button"
              variant="secondary"
              className="size-12 flex-none rounded-[10px] p-0 [&_svg]:size-5"
              aria-label={zoomed ? t('documents.zoomOut') : t('documents.zoomIn')}
              disabled={status !== 'ready'}
              onClick={() => toggleZoom()}
            >
              {zoomed ? (
                <ZoomOut strokeWidth={1.75} aria-hidden />
              ) : (
                <ZoomIn strokeWidth={1.75} aria-hidden />
              )}
            </Button>
          )}
          <Button variant="ghost" className={FOOTER_GHOST_WIDE} onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
        </>
      }
    >
      {shown && (
        <ViewerBody
          key={shown.id}
          kind={kind}
          doc={shown}
          view={view}
          status={status}
          zoomed={zoomed}
          boxRef={box}
          onReady={markReady}
          onFailed={markFailed}
          onDoubleTap={toggleZoom}
        />
      )}
    </Sheet>
  );
}

interface ViewerBodyProps {
  kind: DocumentKind;
  doc: ViewableDocument;
  view: ReturnType<typeof viewerKind>;
  status: Status;
  zoomed: boolean;
  boxRef: RefObject<HTMLDivElement | null>;
  onReady: () => void;
  onFailed: () => void;
  onDoubleTap: (at: { x: number; y: number }) => void;
}

function ViewerBody({
  kind,
  doc,
  view,
  status,
  zoomed,
  boxRef,
  onReady,
  onFailed,
  onDoubleTap,
}: ViewerBodyProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const lastTap = useRef<TapPoint | null>(null);
  const pressedAt = useRef<{ x: number; y: number } | null>(null);

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
        {t('documents.previewFailed')}
      </p>
    );
  }

  // A tap is a press and a release that stay put; a drag (the box scrolling) is neither half of a double tap
  const pressed = (event: ReactPointerEvent) => {
    pressedAt.current = { x: event.clientX, y: event.clientY };
  };
  const released = (event: ReactPointerEvent) => {
    const start = pressedAt.current;
    pressedAt.current = null;
    if (!start || Math.hypot(event.clientX - start.x, event.clientY - start.y) > TAP_SLOP) {
      lastTap.current = null;
      return;
    }
    const tap = { time: event.timeStamp, x: event.clientX, y: event.clientY };
    if (isDoubleTap(lastTap.current, tap)) {
      lastTap.current = null;
      onDoubleTap({ x: tap.x, y: tap.y });
    } else {
      lastTap.current = tap;
    }
  };

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
      <div
        ref={boxRef}
        // Scrolls both ways at 2×; no text selection, no callout and no context menu on what is shown
        className={cn(
          'select-none overflow-auto overscroll-contain rounded-r3 bg-well p-2 [-webkit-touch-callout:none] [touch-action:manipulation] [&_canvas]:pointer-events-none',
          // `PdfPages`' host is the box's one child: at 2× it is twice the box's width
          view === 'pdf' && (zoomed ? '[&>div]:w-[200%]' : '[&>div]:w-full')
        )}
        style={{ maxHeight: VIEW_MAX_HEIGHT, minHeight: loading ? '40dvh' : undefined }}
        onPointerDown={pressed}
        onPointerUp={released}
        onPointerCancel={() => {
          pressedAt.current = null;
          lastTap.current = null;
        }}
        onContextMenu={(event) => event.preventDefault()}
      >
        {view === 'image' ? (
          <img
            src={url}
            alt={doc.name}
            draggable={false}
            onLoad={onReady}
            onError={onFailed}
            className="pointer-events-none mx-auto block max-w-full select-none rounded-r2 object-contain [-webkit-touch-callout:none]"
            style={
              zoomed
                ? { width: `${ZOOM * 100}%`, maxWidth: 'none' }
                : { maxHeight: `calc(${VIEW_MAX_HEIGHT} - 1rem)` }
            }
          />
        ) : (
          <Suspense fallback={null}>
            {/* Measures its host once per start: a new key starts it again at the other width */}
            <PdfPages
              key={zoomed ? 'zoomed' : 'fit'}
              url={url}
              onReady={onReady}
              onError={onFailed}
            />
          </Suspense>
        )}
      </div>
    </div>
  );
}
