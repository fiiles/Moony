import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import type { HistoryRecalculationEvent } from '@shared/schema';
import {
  HISTORY_RECALCULATION_EVENT,
  applyRecalculationEvent,
} from '@/utils/history-recalculation';

/**
 * Tickers whose value history the backend is rebuilding in the background: a stock
 * transaction write returns at once and the rebuild follows. When a rebuild finishes, the charts
 * and analyses derived from the ticker history are refetched.
 */
export function useHistoryRecalculation(): readonly string[] {
  const queryClient = useQueryClient();
  const [running, setRunning] = useState<readonly string[]>([]);

  useEffect(() => {
    const unlisten = listen<HistoryRecalculationEvent>(HISTORY_RECALCULATION_EVENT, (event) => {
      setRunning((current) => applyRecalculationEvent(current, event.payload));
      if (event.payload.status !== 'running') {
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
        queryClient.invalidateQueries({ queryKey: ['ticker-history'] });
        queryClient.invalidateQueries({ queryKey: ['stock-twr'] });
      }
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  return running;
}
