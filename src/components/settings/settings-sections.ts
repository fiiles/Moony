/**
 * Settings sub-pages.
 * Pure data and path resolution; icons and components live in SettingsLayout.
 */

export type SettingsSectionId =
  'general' | 'security' | 'data' | 'integrations' | 'sync' | 'categorization' | 'account';

export interface SettingsSection {
  id: SettingsSectionId;
  path: string;
  /** Key in the `settings` namespace. */
  labelKey: string;
}

export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  { id: 'general', path: '/settings', labelKey: 'sections.general' },
  { id: 'security', path: '/settings/security', labelKey: 'sections.security' },
  { id: 'data', path: '/settings/data', labelKey: 'sections.data' },
  { id: 'integrations', path: '/settings/integrations', labelKey: 'sections.integrations' },
  { id: 'sync', path: '/settings/sync', labelKey: 'sections.sync' },
  { id: 'categorization', path: '/settings/categorization', labelKey: 'sections.categorization' },
  { id: 'account', path: '/settings/account', labelKey: 'sections.account' },
];

/** The sections to list: Sync only while the feature flag is on (it is hidden until release). */
export function visibleSettingsSections(syncFeature: boolean): readonly SettingsSection[] {
  return syncFeature ? SETTINGS_SECTIONS : SETTINGS_SECTIONS.filter((s) => s.id !== 'sync');
}

/**
 * The section a location belongs to. Anything that is not an exact section
 * path (unknown or nested segments, or Sync while its feature is off) falls
 * back to General so a stale link never ends on the 404 page.
 */
export function resolveSettingsSection(location: string, syncFeature = false): SettingsSectionId {
  const pathname = location.split(/[?#]/)[0].replace(/\/+$/, '') || '/';
  return visibleSettingsSections(syncFeature).find((s) => s.path === pathname)?.id ?? 'general';
}
