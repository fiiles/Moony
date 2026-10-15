/**
 * Renders every page of a PDF to canvas with pdf.js. Loaded lazily by the attachment viewer, so
 * pdf.js (and its worker) only load when a PDF is opened. The bytes come through the
 * `moony-file` scheme; pdf.js's wasm, fonts, CMaps and ICC profiles are served from `/pdfjs/`
 * (vite.config.ts), which keeps everything on 'self' under the app CSP.
 */
import { useEffect, useRef } from 'react';
import { GlobalWorkerOptions, getDocument, type PDFDocumentLoadingTask } from 'pdfjs-dist';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

GlobalWorkerOptions.workerSrc = workerUrl;

const ASSETS = '/pdfjs/';

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
    let cancelled = false;
    let task: PDFDocumentLoadingTask | null = null;

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
        const cssWidth = host.clientWidth || 800;
        for (let n = 1; n <= doc.numPages && !cancelled; n++) {
          const page = await doc.getPage(n);
          const base = page.getViewport({ scale: 1 });
          const viewport = page.getViewport({
            scale: (cssWidth / base.width) * window.devicePixelRatio,
          });
          const canvas = document.createElement('canvas');
          canvas.width = Math.floor(viewport.width);
          canvas.height = Math.floor(viewport.height);
          canvas.className = 'block w-full rounded-r2 border border-line bg-white shadow-e1';
          await page.render({ canvas, viewport }).promise;
          if (cancelled) break;
          host.appendChild(canvas);
          if (n === 1) onReady();
        }
      } catch (error) {
        if (!cancelled) {
          console.error('PDF preview failed', error);
          onError();
        }
      }
    })();

    return () => {
      cancelled = true;
      host.replaceChildren();
      void task?.destroy();
    };
  }, [url, onReady, onError]);

  return <div ref={container} className="flex flex-col gap-3" />;
}
