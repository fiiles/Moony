import { describe, expect, it, vi } from 'vitest';
import { navigateFromMenu } from './menu-navigate';

describe('navigateFromMenu', () => {
  it('goes at once where menus hold no history entry', () => {
    const navigate = vi.fn();
    navigateFromMenu(navigate, '/budgets', { TAURI_ENV_PLATFORM: 'ios' }, new EventTarget());
    expect(navigate).toHaveBeenCalledWith('/budgets');
  });

  it('waits on Android until the closing menu popped its entry', () => {
    const navigate = vi.fn();
    const target = new EventTarget();
    navigateFromMenu(navigate, '/budgets', { TAURI_ENV_PLATFORM: 'android' }, target);
    expect(navigate).not.toHaveBeenCalled();
    target.dispatchEvent(new Event('popstate'));
    expect(navigate).toHaveBeenCalledWith('/budgets');
  });
});
