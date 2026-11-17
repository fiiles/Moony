import { useId } from 'react';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { serverOutOfReach } from '@/utils/sync-state';

export interface ServerGate {
  /** The sync server is out of reach (`serverOutOfReach`): what only it can do waits. */
  blocked: boolean;
  /** The ID of the line that says why ("Server není dostupný…"), for `aria-describedby`. */
  noteId: string;
  /** For a `Button` that needs the server: focusable but inert while blocked, described by the line. */
  props: { 'aria-disabled'?: true; 'aria-describedby'?: string };
}

/**
 * Gates the actions that only the sync server can carry out (adding or removing a device, a new
 * recovery kit, deleting the synced data, a repair or re-sync, a checkout, downloading everything)
 * while it is out of reach. Everything that works on this device alone stays available.
 */
export function useServerGate(): ServerGate {
  const { data: status } = useDeviceSyncStatus();
  const noteId = useId();
  const blocked = serverOutOfReach(status);
  return {
    blocked,
    noteId,
    props: blocked ? { 'aria-disabled': true, 'aria-describedby': noteId } : {},
  };
}
