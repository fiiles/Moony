import { useRef, type ChangeEvent } from 'react';
import { Camera, Upload } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { DocumentSource } from '@/mobile/lib/documents';

interface CaptureButtonsProps {
  /** "Vyfotit". */
  cameraLabel: string;
  /** "Vybrat soubor" for a document, "Vybrat z fotek" for a photo. */
  pickLabel: string;
  /** The picker's `accept`: a document's `DOCUMENT_ACCEPT`, the gallery's `PHOTO_ACCEPT`; none lets any file be chosen. */
  pickAccept?: string;
  /** The picker takes several files (the gallery); the camera always gives one. */
  multiple?: boolean;
  disabled?: boolean;
  /** The chosen files and where they came from. A cancelled camera or picker never calls it. */
  onFiles: (files: File[], source: DocumentSource) => void;
}

/**
 * One of the two buttons, as `ButtonPair`'s: at least 48 pt, the label in the lead role and allowed
 * a second line, so a larger text makes the button taller, never wider.
 */
const CAPTURE_BUTTON =
  'h-auto min-h-12 min-w-0 whitespace-normal rounded-[10px] px-2.5 py-2 text-center text-m-lead font-semibold leading-tight hyphens-auto [&_svg]:size-4';

/** Visually hidden but still in the layout: a file input iOS opens from a click on another element. */
const HIDDEN_INPUT = 'sr-only';

/**
 * "Vyfotit" and "Vybrat soubor" (README §13, `.m-capture`; spec §8.8, §9): two secondary buttons
 * under a list of documents or photos, each driving a hidden file input. The camera's is exactly
 * `accept="image/*" capture="environment"`, which opens the system camera and hands back the
 * photograph as a `File`; the picker's `accept` is the caller's (a document's filter or the
 * gallery's photo types — naming JPEG and not HEIC makes WebKit convert a Photo Library original to
 * JPEG), and a picker with none (a plain file input) would offer every file.
 *
 * The input is opened by `click()` inside the button's own click handler — WKWebView opens a
 * picker only from a user gesture. Both inputs are emptied right after their files have been read,
 * so that choosing the same file again fires `change` (it does not when the value is unchanged),
 * and a cancelled picker fires no `change` at all: nothing is uploaded. Nothing is read from the
 * files here; the caller decides what to do with them.
 */
export function CaptureButtons({
  cameraLabel,
  pickLabel,
  pickAccept,
  multiple = false,
  disabled = false,
  onFiles,
}: CaptureButtonsProps) {
  const cameraInput = useRef<HTMLInputElement>(null);
  const pickInput = useRef<HTMLInputElement>(null);

  const changed = (source: DocumentSource) => (event: ChangeEvent<HTMLInputElement>) => {
    const input = event.target;
    // A copy of the list first: emptying the input empties its FileList too
    const files = Array.from(input.files ?? []);
    input.value = '';
    if (files.length > 0) onFiles(files, source);
  };

  return (
    <div className="mb-3.5 grid grid-cols-2 gap-2.5">
      <Button
        type="button"
        variant="secondary"
        className={CAPTURE_BUTTON}
        disabled={disabled}
        onClick={() => cameraInput.current?.click()}
      >
        <Camera strokeWidth={1.75} aria-hidden />
        {cameraLabel}
      </Button>
      <Button
        type="button"
        variant="secondary"
        className={CAPTURE_BUTTON}
        disabled={disabled}
        onClick={() => pickInput.current?.click()}
      >
        <Upload strokeWidth={1.75} aria-hidden />
        {pickLabel}
      </Button>
      <input
        ref={cameraInput}
        type="file"
        accept="image/*"
        capture="environment"
        tabIndex={-1}
        aria-hidden
        className={HIDDEN_INPUT}
        onChange={changed('camera')}
      />
      <input
        ref={pickInput}
        type="file"
        accept={pickAccept}
        multiple={multiple}
        tabIndex={-1}
        aria-hidden
        className={HIDDEN_INPUT}
        onChange={changed('picker')}
      />
    </div>
  );
}
