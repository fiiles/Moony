import { describe, expect, it } from 'vitest';
import { BOM } from './use-csv-export';

describe('BOM', () => {
  it('is exactly one U+FEFF', () => {
    expect(BOM).toBe('﻿');
    expect(BOM.length).toBe(1);
    expect(BOM.charCodeAt(0)).toBe(0xfeff);
  });
});
