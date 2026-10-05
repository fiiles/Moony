import { useQuery } from '@tanstack/react-query';
import { milestonesApi } from '@/lib/tauri-api';

/**
 * Milestones for the dashboard card and the top-bar indicator. Every successful
 * mutation invalidates the key (query client); the interval catches price
 * refreshes and the change of day.
 */
export function useMilestones() {
  return useQuery({
    queryKey: ['milestones'],
    queryFn: () => milestonesApi.list(),
    staleTime: 0,
    refetchOnMount: 'always',
    refetchInterval: 5 * 60_000,
  });
}
