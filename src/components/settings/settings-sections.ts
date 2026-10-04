/**
 * Settings sub-pages.
 * Pure data and path resolution; icons and components live in SettingsLayout.
 */

export type SettingsSectionId =
  'general' | 'security' | 'data' | 'integrations' | 'categorization' | 'account';

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
  { id: 'categorization', path: '/settings/categorization', labelKey: 'sections.categorization' },
  { id: 'account', path: '/settings/account', labelKey: 'sections.account' },
];

/**
 * The section a location belongs to. Anything that is not an exact section
 * path (unknown or nested segments) falls back to General so a stale link
 * never ends on the 404 page.
 */
export function resolveSettingsSection(location: string): SettingsSectionId {
  const pathname = location.split(/[?#]/)[0].replace(/\/+$/, '') || '/';
  return SETTINGS_SECTIONS.find((s) => s.path === pathname)?.id ?? 'general';
}
