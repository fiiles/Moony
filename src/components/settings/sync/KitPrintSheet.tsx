import { Fragment } from 'react';
import { createPortal } from 'react-dom';
import type { RecoveryKit } from '@shared/schema';
import { recoveryCodeGroups, toBase64 } from '@/utils/recovery-kit';
import { useRecoveryKitCopy } from '@/components/settings/sync/use-recovery-kit-copy';

/**
 * What the print dialog prints for a recovery kit: the title, the intro, the QR and the code. It
 * lives in `document.body` outside the app (`.kit-print-sheet`, shown only in print media) and goes
 * with the component that mounts it, so the kit leaves the page when it leaves the screen.
 */
export function KitPrintSheet({ kit }: { kit: RecoveryKit }) {
  const copy = useRecoveryKitCopy(kit);
  return createPortal(
    <div className="kit-print-sheet" aria-hidden>
      <h1 style={{ fontSize: 22, margin: '0 0 8px' }}>{copy.title}</h1>
      <p style={{ margin: '0 0 16px' }}>{copy.intro}</p>
      <img
        src={`data:image/svg+xml;base64,${toBase64(kit.qrSvg)}`}
        alt={copy.qrLabel}
        style={{ width: 200, height: 200 }}
      />
      <p style={{ margin: '16px 0 4px', fontWeight: 600 }}>{copy.codeLabel}</p>
      <p style={{ margin: 0, fontFamily: 'monospace', fontSize: 15, lineHeight: 1.7 }}>
        {recoveryCodeGroups(kit.recoveryCode).map((group, index) => (
          <Fragment key={index}>
            {index > 0 && '-'}
            <span style={{ whiteSpace: 'nowrap' }}>{group}</span>
          </Fragment>
        ))}
      </p>
      <p style={{ margin: '12px 0 0', fontSize: 13 }}>{copy.codeNote}</p>
      {kit.relayUrl && (
        <p style={{ margin: '12px 0 0', fontSize: 13 }}>
          {copy.relayLabel}: {kit.relayUrl}
        </p>
      )}
      <p style={{ margin: '12px 0 0', fontSize: 13 }}>{copy.created}</p>
    </div>,
    document.body
  );
}
