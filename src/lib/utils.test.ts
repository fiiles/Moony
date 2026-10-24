import { describe, expect, it } from 'vitest';
import { cn } from './utils';

describe('cn (tailwind-merge with the design tokens)', () => {
  it('keeps a type role and a text color together', () => {
    expect(cn('text-micro', 'text-ink-inverse')).toBe('text-micro text-ink-inverse');
    expect(cn('text-ink-inverse', 'text-micro')).toBe('text-ink-inverse text-micro');
  });

  it('merges two type roles to the last one', () => {
    expect(cn('text-caption', 'text-micro')).toBe('text-micro');
  });

  it('knows the phone type roles', () => {
    expect(cn('text-m-title', 'text-ink')).toBe('text-m-title text-ink');
    expect(cn('text-m-title-lg', 'text-ink-3')).toBe('text-m-title-lg text-ink-3');
    expect(cn('text-m-body', 'text-m-sub')).toBe('text-m-sub');
  });

  it('knows the Dynamic Type roles: lead, note and field', () => {
    expect(cn('text-m-lead', 'text-ink-3')).toBe('text-m-lead text-ink-3');
    expect(cn('text-m-note', 'text-ink-4')).toBe('text-m-note text-ink-4');
    expect(cn('text-m-field', 'text-ink')).toBe('text-m-field text-ink');
    expect(cn('text-m-body', 'text-m-field')).toBe('text-m-field');
  });

  it('treats the numeric weights as weights, not families', () => {
    expect(cn('font-600', 'font-650')).toBe('font-650');
    expect(cn('font-650', 'font-mono')).toBe('font-650 font-mono');
  });

  it('merges token shadows, radii and control sizes', () => {
    expect(cn('shadow-e1', 'shadow-e3')).toBe('shadow-e3');
    expect(cn('rounded-r2', 'rounded-full')).toBe('rounded-full');
    expect(cn('h-control', 'h-8')).toBe('h-8');
  });
});
