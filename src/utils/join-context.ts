/**
 * Where a join runs: Settings (the profile may hold data) or the first run (a brand-new profile;
 * the password typed a step earlier).
 */
export type JoinContext = { kind: 'settings' } | { kind: 'firstRun'; password: string };

/** What a join flow asks and shows, by where it runs (spec 2026-11-14 §5.1, §5.3). */
export interface JoinPresentation {
  /** The device password field: Settings asks for it; the first run already has it. */
  askPassword: boolean;
  /** "This device's data is replaced …": a brand-new profile has nothing to replace. */
  showReplaceWarning: boolean;
  /** The list of what is at stake; the impact is still read for the consent either way. */
  showImpact: boolean;
  /** The `common` key of the flow's way out. */
  cancelKey: 'buttons.cancel' | 'buttons.back';
}

export function joinPresentation(context: JoinContext): JoinPresentation {
  const settings = context.kind === 'settings';
  return {
    askPassword: settings,
    showReplaceWarning: settings,
    showImpact: settings,
    cancelKey: settings ? 'buttons.cancel' : 'buttons.back',
  };
}

/** The device password a join sends: the typed one in Settings, the first run's otherwise. */
export function joinPassword(context: JoinContext, typed: string): string {
  return context.kind === 'firstRun' ? context.password : typed;
}
