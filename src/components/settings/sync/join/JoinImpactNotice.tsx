import type { ReplaceImpactView } from '@shared/schema';
import { ReplaceImpactNotice } from '@/components/settings/sync/ReplaceImpactNotice';

interface JoinImpactNoticeProps {
  /** `JoinPresentation.showImpact`: Settings lists what is at stake, the first run does not. */
  show: boolean;
  impact: ReplaceImpactView | undefined;
  /** Reading the impact failed. */
  error: Error | null;
}

/**
 * What a join puts at stake, in both join flows. In the first run the list is left out, but a
 * failed read still shows its text: the join sends the impact as its consent, so without it the
 * final button stays off and the person would otherwise see no reason.
 */
export function JoinImpactNotice({ show, impact, error }: JoinImpactNoticeProps) {
  if (!show && !error) return null;
  return <ReplaceImpactNotice flow="join" impact={impact} error={error} />;
}
