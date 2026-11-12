import type { ReplaceConsentView, ReplaceImpactView, ReplaceOutcomeView } from '@shared/schema';

export type ReplaceFlow = 'join' | 'repair' | 'resync';
export interface ImpactLine {
  key: string;
  count: number;
}

function present(lines: ImpactLine[]): ImpactLine[] {
  return lines.filter((line) => line.count > 0);
}

/**
 * The lines a confirmation shows, in this order, each only when its count is above 0. Items and
 * batches are separate units and never added together.
 */
export function impactLines(flow: ReplaceFlow, impact: ReplaceImpactView): ImpactLine[] {
  const attachments = [
    { key: 'documents', count: impact.documents },
    { key: 'photos', count: impact.photos },
  ];
  switch (flow) {
    case 'join':
      return present([
        { key: 'accounts', count: impact.accounts },
        { key: 'transactions', count: impact.transactions },
        ...attachments,
      ]);
    case 'resync':
      return present([
        { key: 'items', count: impact.pendingItems },
        { key: 'batches', count: impact.pendingBatches },
        ...attachments,
      ]);
    case 'repair':
      return present(attachments);
  }
}

/** The consent a confirmation sends: the watermark it showed and its attachment count. */
export function consentOf(flow: ReplaceFlow, impact: ReplaceImpactView): ReplaceConsentView {
  return {
    changesThrough: impact.changesThrough,
    attachments: flow === 'resync' ? 0 : impact.documents + impact.photos,
  };
}

/** The toast after a replace: each count that is only in the backup (the file name is shown first). */
export function outcomeLines(outcome: ReplaceOutcomeView): ImpactLine[] {
  return present([
    { key: 'attachmentsInBackupOnly', count: outcome.attachmentsInBackupOnly },
    { key: 'unsentItemsInBackupOnly', count: outcome.unsentItemsInBackupOnly },
    { key: 'unsentBatchesInBackupOnly', count: outcome.unsentBatchesInBackupOnly },
  ]);
}
