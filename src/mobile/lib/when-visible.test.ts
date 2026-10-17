import { describe, expect, it, vi } from 'vitest';
import { whenVisible, type VisibilitySource } from './when-visible';

/** A document that can be hidden and shown, with the listeners it holds. */
function fakeDocument(state: DocumentVisibilityState) {
  const listeners = new Set<() => void>();
  const doc: VisibilitySource & { listeners: Set<() => void> } = {
    visibilityState: state,
    listeners,
    addEventListener: (_type, listener) => void listeners.add(listener),
    removeEventListener: (_type, listener) => void listeners.delete(listener),
  };
  const set = (next: DocumentVisibilityState) => {
    (doc as { visibilityState: DocumentVisibilityState }).visibilityState = next;
    [...listeners].forEach((listener) => listener());
  };
  return { doc, set };
}

describe('whenVisible', () => {
  it('runs at once while the app is visible, and holds no listener', () => {
    const { doc } = fakeDocument('visible');
    const callback = vi.fn();
    const stop = whenVisible(callback, doc);
    expect(callback).toHaveBeenCalledTimes(1);
    expect(doc.listeners.size).toBe(0);
    expect(() => stop()).not.toThrow();
  });

  it('waits for the first change to visible while hidden, runs once, and lets go', () => {
    const { doc, set } = fakeDocument('hidden');
    const callback = vi.fn();
    whenVisible(callback, doc);
    expect(callback).not.toHaveBeenCalled();
    expect(doc.listeners.size).toBe(1);

    // a change that is not "visible" does nothing
    set('hidden');
    expect(callback).not.toHaveBeenCalled();

    set('visible');
    expect(callback).toHaveBeenCalledTimes(1);
    expect(doc.listeners.size).toBe(0);

    // a later hide and show does not run it again
    set('hidden');
    set('visible');
    expect(callback).toHaveBeenCalledTimes(1);
  });

  it('never runs when it is stopped before the app is visible', () => {
    const { doc, set } = fakeDocument('hidden');
    const callback = vi.fn();
    const stop = whenVisible(callback, doc);
    stop();
    expect(doc.listeners.size).toBe(0);
    set('visible');
    expect(callback).not.toHaveBeenCalled();
    // stopping twice is fine
    expect(() => stop()).not.toThrow();
  });
});
