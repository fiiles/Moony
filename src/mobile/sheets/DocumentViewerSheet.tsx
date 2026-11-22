import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from 'react';
import { useTranslation } from 'react-i18next';
import { Download, LockKeyhole, ZoomIn, ZoomOut } from 'lucide-react';
import type { AttachmentContent } from '@shared/schema';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { Sheet } from '@/mobile/components/Sheet';
import { FetchingNotice, UnavailableNotice } from '@/mobile/components/documents/AttachmentStates';
import { FOOTER_GHOST_WIDE } from '@/mobile/components/import/styles';
import type { DocumentKind } from '@/mobile/components/documents/document-kind';
import { useAttachmentFetch } from '@/mobile/hooks/use-attachment-fetch';
import { useSaveDocument } from '@/mobile/hooks/use-save-document';
import type { FetchPlan } from '@/mobile/lib/attachment-fetch';
import { isDoubleTap, scrollAfterZoom, type TapPoint } from '@/mobile/lib/documents';
import { field48, fieldMessage } from '@/mobile/pages/first-run/styles';
import { attachmentUrl } from '@/utils/attachment-url';
import { viewerKind } from '@/utils/attachment-viewer';
import type { PdfFailure } from '@/utils/pdf-failure';

// pdf.js and its worker load only when a PDF is opened
const PdfPages = lazy(() => import('@/components/attachments/PdfPages'));

/** What the viewer needs of a document. */
export interface ViewableDocument {
  id: string;
  name: string;
  mimeType: string;
  /** Where the content is with device sync; the viewer follows it while it is open. */
  content: AttachmentContent;
}

interface DocumentViewerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: DocumentKind;
  /** Set together with `open`; the sheet keeps the last one while it slides away. */
  doc: ViewableDocument | null;
  /** The one line under the name ("smlouva · 2,1 MB · 9. 1. 2024"). */
  sub?: string;
  /**
   * When the list the document comes from was last refreshed: a preview that failed to load gets
   * one new attempt per refresh (and per opening), never a timer.
   */
  refreshedAt?: number;
}

// `fetching`: the content is not on the phone yet; nothing is mounted that would load it
type Status = 'loading' | 'ready' | 'failed' | 'fetching';

/** Why a PDF waits for its opening password: none typed yet, or the typed one did not open it. */
type Lock = Exclude<PdfFailure, 'failed'>;

/** The save action's label: the iOS picker saves to the Files app, Android's to the device. */
function saveLabelKey(platform: string): 'documents.saveToFiles' | 'documents.saveToDevice' {
  return platform.startsWith('android') ? 'documents.saveToDevice' : 'documents.saveToFiles';
}

/** The zoom step: the content is this many times the box's width. */
const ZOOM = 2;

/** A tap moves its finger less than this many px; a longer one is a drag. */
const TAP_SLOP = 10;

/**
 * The encrypted viewer (spec §8.8 "the encrypted viewer", README §13 "Document viewer"): the name as
 * the title, the document in a full sheet, a zoom button, "Uložit do Souborů" and Hotovo. An image
 * shows contained, a PDF page by page (`PdfPages`, the desktop's), anything else says the phone
 * cannot show it. A PDF locked with an opening password (insurers send them so) asks for the
 * password in the sheet; it lives in this sheet's state only and is forgotten with the document. A
 * double
 * tap — or the zoom button, for anyone who cannot double-tap — toggles 1× and 2× (the content twice
 * the box's width, the box scrolling both ways, a new document opening at 1×). A pinch over the
 * document does nothing: the viewport is no longer locked to scale 1 (README §13 "Dynamic Type"),
 * so the box allows panning only (`touch-action: pan-x pan-y`), or the pinch would zoom the whole
 * page with the sheet. The box fills the sheet between its head and its footer (`Sheet`'s `fill`),
 * so it is the one scroller at every text size — `PdfPages` judges which pages are in view against
 * its parent, which is this box. The bytes come decrypted through the `moony-file` scheme only
 * while the app is unlocked and nothing is written to disk: the picture and the pages cannot be
 * selected, dragged or long-pressed into the system's Copy / Share / Save menu. The one way out is
 * "Uložit do Souborů" (`useSaveDocument`): a copy Rust stages only for the system's save picker and
 * removes right after, for a document whose content is on the phone — it is also what a file the
 * phone cannot show, or a preview that failed, offers instead. There is no "open elsewhere": that
 * is a desktop-only command (`open_with_system`).
 */
export function DocumentViewerSheet({
  open,
  onOpenChange,
  kind,
  doc,
  sub,
  refreshedAt,
}: DocumentViewerSheetProps) {
  const { t } = useTranslation('mobile');
  const [shown, setShown] = useState(doc);
  const [status, setStatus] = useState<Status>('loading');
  const [zoomed, setZoomed] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);
  // A protected PDF's opening password, as typed; never stored anywhere else
  const [password, setPassword] = useState<string | undefined>(undefined);
  const [lock, setLock] = useState<Lock | null>(null);
  const { save, isPending: saving } = useSaveDocument();
  const box = useRef<HTMLDivElement>(null);
  // Where the box is to be scrolled once the zoomed content is laid out
  const scrollTarget = useRef<{ left: number; top: number } | null>(null);

  // A new document, and every opening, starts at 1×, without a password, and loads
  const restart = () => {
    setStatus('loading');
    setZoomed(false);
    setPassword(undefined);
    setLock(null);
  };
  if (doc !== null && doc !== shown) {
    // A refreshed row of the same document updates it without loading again
    const another = doc.id !== shown?.id;
    setShown(doc);
    if (another) restart();
  }
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) restart();
  }
  // A preview that failed gets one new attempt per refresh of the list
  const [seenRefresh, setSeenRefresh] = useState(refreshedAt);
  if (seenRefresh !== refreshedAt) {
    setSeenRefresh(refreshedAt);
    if (status === 'failed') setStatus('loading');
  }

  const view = shown ? viewerKind(shown.mimeType) : 'external';
  // A file the phone cannot show is still fetched: "Uložit do Souborů" needs its content here
  const { plan, retrying, tryAgain } = useAttachmentFetch({
    open,
    kind,
    id: shown?.id,
    content: shown?.content,
  });
  // The status follows the plan: a spinner while the content comes, a fresh load once it is here
  const planned: Status =
    plan.stage === 'fetching'
      ? 'fetching'
      : status === 'fetching' || plan.stage === 'unavailable'
        ? 'loading'
        : status;
  if (planned !== status) setStatus(planned);
  // Stable: `PdfPages` starts over whenever one of them changes
  const markReady = useCallback(() => setStatus('ready'), []);
  const markFailed = useCallback((failure: PdfFailure = 'failed') => {
    setLock(failure === 'failed' ? null : failure);
    setStatus('failed');
  }, []);
  // The typed password opens the document again: the pages mount anew with it
  const unlock = (typed: string) => {
    setPassword(typed);
    setLock(null);
    setStatus('loading');
  };

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

  // A document the phone cannot show (`external`) or one that failed is a line of text: it gets a
  // normal-height sheet and no zoom, not a tall one that is nearly empty
  const showsDocument = view !== 'external' && status !== 'failed' && plan.stage !== 'unavailable';
  // Saving needs the content on the phone; content on its way or unavailable has nothing to save
  const canSave = shown !== null && plan.stage === 'show' && status !== 'fetching';
  const saveButton = canSave ? (
    <Button
      type="button"
      variant="secondary"
      className={FOOTER_GHOST_WIDE}
      disabled={saving}
      aria-busy={saving}
      onClick={() => shown && void save(kind, shown.id)}
    >
      <Download strokeWidth={1.75} aria-hidden />
      {t(saveLabelKey(import.meta.env.TAURI_ENV_PLATFORM ?? ''))}
    </Button>
  ) : null;
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      full
      fill={showsDocument}
      title={shown?.name ?? ''}
      description={sub}
      dismiss="done"
      footer={
        // "Hotovo" is in the head (C7); the footer keeps the viewer's tools within thumb reach:
        // the zoom while the document shows, "Uložit do Souborů" whenever its content is here
        showsDocument || saveButton ? (
          <>
            {showsDocument && (
              <Button
                type="button"
                variant="secondary"
                className={FOOTER_GHOST_WIDE}
                disabled={status !== 'ready'}
                onClick={() => toggleZoom()}
              >
                {zoomed ? (
                  <ZoomOut strokeWidth={1.75} aria-hidden />
                ) : (
                  <ZoomIn strokeWidth={1.75} aria-hidden />
                )}
                {zoomed ? t('documents.zoomOut') : t('documents.zoomIn')}
              </Button>
            )}
            {saveButton}
          </>
        ) : undefined
      }
    >
      {shown && (
        <ViewerBody
          key={shown.id}
          kind={kind}
          doc={shown}
          view={view}
          plan={plan}
          retrying={retrying}
          onTryAgain={tryAgain}
          status={status}
          zoomed={zoomed}
          password={password}
          lock={lock}
          onUnlock={unlock}
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
  plan: FetchPlan;
  retrying: boolean;
  onTryAgain: () => void;
  status: Status;
  zoomed: boolean;
  password: string | undefined;
  lock: Lock | null;
  onUnlock: (password: string) => void;
  boxRef: RefObject<HTMLDivElement | null>;
  onReady: () => void;
  onFailed: (failure?: PdfFailure) => void;
  onDoubleTap: (at: { x: number; y: number }) => void;
}

function ViewerBody({
  kind,
  doc,
  view,
  plan,
  retrying,
  onTryAgain,
  status,
  zoomed,
  password,
  lock,
  onUnlock,
  boxRef,
  onReady,
  onFailed,
  onDoubleTap,
}: ViewerBodyProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const lastTap = useRef<TapPoint | null>(null);
  const pressedAt = useRef<{ x: number; y: number } | null>(null);

  if (plan.stage === 'unavailable') {
    return (
      <div className="px-1 py-6">
        <UnavailableNotice
          note={plan.note}
          retry={plan.retry}
          busy={retrying}
          onTryAgain={onTryAgain}
        />
      </div>
    );
  }
  if (plan.stage === 'fetching' || status === 'fetching') {
    return (
      // Fills the sheet like the document will; nothing here loads a URL for content that is not here
      <div className="relative flex min-h-0 flex-1 flex-col items-center justify-center rounded-r3 bg-well p-4">
        <FetchingNotice />
      </div>
    );
  }
  if (view === 'external') {
    return (
      <p className="m-0 px-1 py-6 text-center text-m-body text-ink-3">
        {t('documents.cannotShow')}
      </p>
    );
  }
  if (status === 'failed' && lock) {
    return <PasswordPrompt wrong={lock === 'wrongPassword'} onUnlock={onUnlock} />;
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
    // A column that fills the sheet's body: the box takes all of it and is the one scroller
    <div className="relative flex min-h-0 flex-1 flex-col">
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
        // Fills the sheet and scrolls both ways at 2×; a pan scrolls it, a pinch does nothing (the
        // page would zoom with the sheet); no text selection, no callout and no context menu on
        // what is shown
        className={cn(
          'min-h-0 flex-1 select-none overflow-auto overscroll-contain rounded-r3 bg-well p-2 [-webkit-touch-callout:none] [touch-action:pan-x_pan-y] [&_canvas]:pointer-events-none',
          // `PdfPages`' host is the box's one child: at 2× it is twice the box's width
          view === 'pdf' && (zoomed ? '[&>div]:w-[200%]' : '[&>div]:w-full')
        )}
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
            // An image has no password: its failure is a broken preview (not the event as a reason)
            onError={() => onFailed()}
            // At 1× contained in the box (its height is definite: it fills the sheet), at 2× twice its width
            className={cn(
              'pointer-events-none mx-auto block select-none rounded-r2 object-contain [-webkit-touch-callout:none]',
              !zoomed && 'max-h-full max-w-full'
            )}
            style={zoomed ? { width: `${ZOOM * 100}%`, maxWidth: 'none' } : undefined}
          />
        ) : (
          <Suspense fallback={null}>
            {/* Measures its host once per start: a new key starts it again at the other width */}
            <PdfPages
              key={zoomed ? 'zoomed' : 'fit'}
              url={url}
              password={password}
              onReady={onReady}
              onError={onFailed}
            />
          </Suspense>
        )}
      </div>
    </div>
  );
}

/**
 * A PDF locked with an opening password: the sheet asks for it (insurers and banks send their
 * documents so, often locked with the client's birth number). The typed password goes to pdf.js
 * through the sheet's state and nowhere else; a wrong one is said under the field.
 */
function PasswordPrompt({ wrong, onUnlock }: { wrong: boolean; onUnlock: (p: string) => void }) {
  const { t } = useTranslation('mobile');
  const [typed, setTyped] = useState('');
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (typed !== '') onUnlock(typed);
  };
  return (
    <form onSubmit={submit} noValidate className="space-y-3 px-1 py-4">
      <div className="flex items-start gap-3">
        <LockKeyhole
          className="mt-0.5 size-5 flex-none text-ink-3"
          strokeWidth={1.75}
          aria-hidden
        />
        <div>
          <p className="m-0 text-m-lead font-semibold text-ink">{t('documents.passwordTitle')}</p>
          <p className="m-0 mt-1 text-m-sub text-ink-3">{t('documents.passwordHint')}</p>
        </div>
      </div>
      <label className="block space-y-1.5">
        <span className="text-m-sub font-medium text-ink-2">{t('documents.passwordLabel')}</span>
        <PasswordInput
          touch
          autoComplete="off"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          enterKeyHint="go"
          className={field48}
          value={typed}
          aria-invalid={wrong || undefined}
          onChange={(event) => setTyped(event.target.value)}
        />
      </label>
      {wrong && (
        <p role="alert" className={cn(fieldMessage, 'm-0 text-loss')}>
          {t('documents.passwordWrong')}
        </p>
      )}
      <Button type="submit" className={cn(FOOTER_GHOST_WIDE, 'w-full')} disabled={typed === ''}>
        {t('documents.passwordOpen')}
      </Button>
    </form>
  );
}
