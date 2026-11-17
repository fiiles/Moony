import { useReplaceImpact } from '@/hooks/use-device-sync';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { ReplaceImpactBlock } from '@/mobile/components/sync/ReplaceImpactBlock';
import type { ReplaceFlow } from '@/utils/replace-impact';
import type { ReplaceImpactView } from '@shared/schema';

interface ReplaceConfirmSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** `repair` or `resync` (a join has its own sheets). */
  flow: Exclude<ReplaceFlow, 'join'>;
  title: string;
  description: string;
  confirmLabel: string;
  /** Runs with the typed password and the impact the sheet showed: its consent is what is sent. */
  onConfirm: (password: string, impact: ReplaceImpactView) => Promise<unknown>;
}

/**
 * The repair and re-sync confirmation: what the replace puts at stake (read when the sheet opens,
 * and again each time it opens), then the device password. The confirm stays off until the numbers
 * are in and while they are being read again, so what the person agrees to is what is on screen.
 */
export function ReplaceConfirmSheet({
  open,
  onOpenChange,
  flow,
  title,
  description,
  confirmLabel,
  onConfirm,
}: ReplaceConfirmSheetProps) {
  const impact = useReplaceImpact(open);
  // A read that is running is not shown: the numbers of an earlier opening could be out of date
  const shown = impact.isFetching ? undefined : impact.data;
  return (
    <SyncPasswordSheet
      needsServer
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      description={description}
      extra={
        <ReplaceImpactBlock
          flow={flow}
          impact={shown}
          error={impact.isError ? (impact.error as Error) : null}
        />
      }
      submitDisabled={!shown}
      confirmLabel={confirmLabel}
      destructive
      closeOnSuccess={false}
      onSubmit={async (password) => {
        // The confirm is off until the numbers are in; a missing one never closes the sheet
        if (!shown) throw new Error('The numbers to confirm are not loaded.');
        await onConfirm(password, shown);
      }}
    />
  );
}
