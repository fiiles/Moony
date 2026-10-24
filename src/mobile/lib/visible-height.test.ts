import { describe, expect, it } from 'vitest';
import { visibleHeightFor } from './visible-height';

describe('visibleHeightFor', () => {
  it('rounds the visual viewport height at the normal zoom', () => {
    expect(visibleHeightFor(778.4, 1)).toBe(778);
  });

  it('treats a hair of overshoot as no zoom', () => {
    expect(visibleHeightFor(500, 1.005)).toBe(500);
  });

  it('is null while the page is pinch-zoomed, so the app box keeps its height', () => {
    expect(visibleHeightFor(300, 2)).toBeNull();
  });
});
