/**
 * Renders a PDF with pdf.js, page by page as it scrolls into view. Loaded lazily by the attachment
 * viewer, so pdf.js (and its worker) only load when a PDF is opened. The bytes come through the
 * `moony-file` scheme; pdf.js's wasm, fonts, CMaps and ICC profiles are served from `/pdfjs/`
 * (vite.config.ts), which keeps everything on 'self' under the app CSP.
 *
 * Every page gets a placeholder with its aspect ratio; only pages near the visible part of the
 * viewer's scroll area are drawn, and a page far out of view gives its canvas back, so a long
 * contract never holds hundreds of full-resolution canvases at once.
 */
import { useEffect, useRef } from 'react';
import {
  GlobalWorkerOptions,
  getDocument,
  type PDFDocumentLoadingTask,
  type PDFDocumentProxy,
} from 'pdfjs-dist';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

GlobalWorkerOptions.workerSrc = workerUrl;

const ASSETS = '/pdfjs/';
/** Sharper than 2× costs memory and time without a visible gain at viewer size. */
const MAX_PIXEL_RATIO = 2;
/** Pages within one viewport height above or below the visible part are drawn ahead. */
const RENDER_MARGIN = '100% 0px';

interface PdfPagesProps {
  url: string;
  onReady: () => void;
  onError: () => void;
}

export default function PdfPages({ url, onReady, onError }: PdfPagesProps) {
  const container = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = container.current;
    if (!host) return;
    // The viewer's scroll area: visibility is judged against it, not the window.
    const scrollRoot = host.parentElement;
    let cancelled = false;
    let task: PDFDocumentLoadingTask | null = null;
    let observer: IntersectionObserver | null = null;
    const visible = new Set<number>();
    const inFlight = new Set<number>();

    const fail = (error: unknown) => {
      if (cancelled) return;
      console.error('PDF preview failed', error);
      onError();
    };

    const release = (slot: HTMLElement) => {
      const canvas = slot.querySelector('canvas');
      if (canvas) {
        // Zero-sized canvases give their backing memory back right away.
        canvas.width = 0;
        canvas.height = 0;
        slot.replaceChildren();
      }
    };

    const draw = async (doc: PDFDocumentProxy, slot: HTMLElement, cssWidth: number) => {
      const n = Number(slot.dataset.page);
      if (slot.firstChild || inFlight.has(n)) return;
      inFlight.add(n);
      try {
        const page = await doc.getPage(n);
        const base = page.getViewport({ scale: 1 });
        slot.style.aspectRatio = `${base.width} / ${base.height}`;
        const ratio = Math.min(window.devicePixelRatio || 1, MAX_PIXEL_RATIO);
        const viewport = page.getViewport({ scale: (cssWidth / base.width) * ratio });
        const canvas = document.createElement('canvas');
        canvas.width = Math.floor(viewport.width);
        canvas.height = Math.floor(viewport.height);
        canvas.className = 'block h-full w-full rounded-r2';
        await page.render({ canvas, viewport }).promise;
        if (cancelled || !visible.has(n)) {
          // Scrolled away while it was drawing: do not keep it.
          canvas.width = 0;
          canvas.height = 0;
          return;
        }
        slot.replaceChildren(canvas);
        if (n === 1) onReady();
      } finally {
        inFlight.delete(n);
      }
    };

    (async () => {
      try {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const data = new Uint8Array(await response.arrayBuffer());
        task = getDocument({
          data,
          wasmUrl: `${ASSETS}wasm/`,
          standardFontDataUrl: `${ASSETS}standard_fonts/`,
          cMapUrl: `${ASSETS}cmaps/`,
          cMapPacked: true,
          iccUrl: `${ASSETS}iccs/`,
        });
        const doc = await task.promise;
        if (cancelled) return;
        const cssWidth = host.clientWidth || 800;
        // Placeholders take the first page's proportions until each page is drawn.
        const first = (await doc.getPage(1)).getViewport({ scale: 1 });
        const slots: HTMLElement[] = [];
        for (let n = 1; n <= doc.numPages; n++) {
          const slot = document.createElement('div');
          slot.dataset.page = String(n);
          slot.className = 'w-full rounded-r2 border border-line bg-white shadow-e1';
          slot.style.aspectRatio = `${first.width} / ${first.height}`;
          slots.push(slot);
        }
        host.replaceChildren(...slots);

        observer = new IntersectionObserver(
          (entries) => {
            for (const entry of entries) {
              const slot = entry.target as HTMLElement;
              const n = Number(slot.dataset.page);
              if (entry.isIntersecting) {
                visible.add(n);
                draw(doc, slot, cssWidth).catch(fail);
              } else {
                visible.delete(n);
                release(slot);
              }
            }
          },
          { root: scrollRoot, rootMargin: RENDER_MARGIN }
        );
        for (const slot of slots) observer.observe(slot);
      } catch (error) {
        fail(error);
      }
    })();

    return () => {
      cancelled = true;
      observer?.disconnect();
      for (const canvas of host.querySelectorAll('canvas')) {
        canvas.width = 0;
        canvas.height = 0;
      }
      host.replaceChildren();
      void task?.destroy();
    };
  }, [url, onReady, onError]);

  return <div ref={container} className="flex flex-col gap-3" data-testid="pdf-pages" />;
}
