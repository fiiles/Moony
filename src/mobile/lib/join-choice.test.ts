import { describe, expect, it } from 'vitest';
import { joinChoiceNavigation } from './join-choice';
import { readIntent, withoutIntent } from './row-intent';

describe('joinChoiceNavigation', () => {
  it('pushes Nastavení → Sync with the join choice from another screen', () => {
    const nav = joinChoiceNavigation('/', { moonyBack: 'Finance' });
    expect(nav.to).toBe('/settings/sync');
    expect(nav.replace).toBe(false);
    expect(readIntent(nav.state)).toBe('join');
    // The pushed entry is a new one: nothing of the screen it leaves comes along
    expect(withoutIntent(nav.state)).toEqual({});
  });

  it('opens the choice in place on Nastavení → Sync, without a second Sync entry', () => {
    const nav = joinChoiceNavigation('/settings/sync', { moonyBack: 'Přehled' });
    expect(nav.to).toBe('/settings/sync');
    expect(nav.replace).toBe(true);
    expect(readIntent(nav.state)).toBe('join');
    // The entry keeps its back label
    expect(withoutIntent(nav.state)).toEqual({ moonyBack: 'Přehled' });
  });

  it('opens it in place when the Sync entry has no state', () => {
    const nav = joinChoiceNavigation('/settings/sync', null);
    expect(nav.replace).toBe(true);
    expect(readIntent(nav.state)).toBe('join');
  });

  it('pushes from another settings section', () => {
    expect(joinChoiceNavigation('/settings/security', null).replace).toBe(false);
    expect(joinChoiceNavigation('/settings', null).replace).toBe(false);
  });
});
